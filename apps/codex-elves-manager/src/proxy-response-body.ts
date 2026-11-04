type JsonObject = Record<string, unknown>;

function object(value: unknown): JsonObject | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as JsonObject
    : null;
}

function mergeSnapshot(previous: unknown, next: unknown): JsonObject {
  const left = object(previous) ?? {};
  return Object.fromEntries([
    ...Object.entries(left),
    ...Object.entries(object(next) ?? {}).map(([key, value]) => [
      key, object(value) ? mergeSnapshot(left[key], value) : value,
    ]),
  ]);
}

export function formatProxyBody(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text.trim() ? text : "";
  }
}

function streamEvents(text: string): unknown[] | null {
  const normalized = text.trim().replace(/\r\n?/g, "\n");
  if (/^\s*data:/m.test(normalized)) {
    const events: unknown[] = [];
    for (const block of normalized.split(/\n\s*\n/)) {
      const data = block.split("\n").map(line => line.trim()).filter(line => line.startsWith("data:"))
        .map(line => line.slice(5).trimStart()).join("\n").trim();
      if (!data || data === "[DONE]") continue;
      try { events.push(JSON.parse(data)); }
      catch { events.push(data); }
    }
    return events.length ? events : null;
  }
  const lines = normalized.split("\n").filter(line => line.trim());
  if (lines.length < 2) return null;
  try { return lines.map(line => JSON.parse(line)); }
  catch { return null; }
}

function appendText(target: JsonObject, source: JsonObject, key: string) {
  if (typeof source[key] === "string") {
    target[key] = (typeof target[key] === "string" ? target[key] : "") + source[key];
  }
}

function summarizeChat(events: JsonObject[]): JsonObject | null {
  if (!events.some(event => Array.isArray(event.choices)
    && event.choices.some(choice => object(object(choice)?.delta)))) return null;
  let response: JsonObject = {};
  let usage: JsonObject = {};
  const choices = new Map<number, {
    value: JsonObject; message: JsonObject; tools: Map<number, JsonObject>;
  }>();
  for (const event of events) {
    if (!Array.isArray(event.choices)) return null;
    if (response.id && event.id && response.id !== event.id) return null;
    const { choices: _choices, usage: nextUsage, ...metadata } = event;
    response = { ...response, ...metadata };
    usage = mergeSnapshot(usage, nextUsage);
    for (const [position, value] of event.choices.entries()) {
      const choice = object(value);
      if (!choice) return null;
      const index = Number.isInteger(choice.index) ? choice.index as number : position;
      const state = choices.get(index) ?? {
        value: { index, finish_reason: null }, message: {}, tools: new Map<number, JsonObject>(),
      };
      choices.set(index, state);
      const delta = object(choice.delta) ?? {};
      if (object(choice.message)) state.message = { ...object(choice.message) };
      if (typeof delta.role === "string") state.message.role = delta.role;
      for (const key of ["content", "reasoning_content", "reasoning", "thought", "refusal"]) {
        // 未知的多模态结构保留原始事件，不能将非字符串正文悄悄丢掉。
        if (delta[key] != null && typeof delta[key] !== "string") return null;
        appendText(state.message, delta, key);
      }
      if (delta.audio != null) return null;
      if (Array.isArray(delta.reasoning_details)) {
        state.message.reasoning_details = [
          ...(Array.isArray(state.message.reasoning_details) ? state.message.reasoning_details : []),
          ...delta.reasoning_details,
        ];
      }
      if (object(delta.function_call)) {
        const call = { ...object(state.message.function_call) };
        for (const key of ["name", "arguments"]) appendText(call, object(delta.function_call)!, key);
        state.message.function_call = call;
      }
      if (Array.isArray(delta.tool_calls)) {
        for (const value of delta.tool_calls) {
          const tool = object(value);
          if (!tool || !Number.isInteger(tool.index)) return null;
          const index = tool.index as number;
          const previous = state.tools.get(index) ?? {};
          const { index: _index, function: nextFunction, ...fields } = tool;
          const call = { ...object(previous.function) };
          if (object(nextFunction)) {
            for (const key of ["name", "arguments"]) appendText(call, object(nextFunction)!, key);
          }
          state.tools.set(index, { ...previous, ...fields, ...(nextFunction ? { function: call } : {}) });
        }
      }
      if (choice.finish_reason != null) state.value.finish_reason = choice.finish_reason;
      if (object(choice.logprobs)) {
        const logprobs = mergeSnapshot(state.value.logprobs, choice.logprobs);
        for (const key of ["content", "refusal"]) {
          const previous = object(state.value.logprobs)?.[key], next = object(choice.logprobs)?.[key];
          if (Array.isArray(next)) logprobs[key] = [...(Array.isArray(previous) ? previous : []), ...next];
        }
        state.value.logprobs = logprobs;
      }
    }
  }
  return {
    ...response,
    object: "chat.completion",
    choices: [...choices].sort(([left], [right]) => left - right).map(([, state]) => ({
      ...state.value,
      message: {
        ...state.message,
        ...(state.tools.size ? {
          tool_calls: [...state.tools].sort(([left], [right]) => left - right).map(([, tool]) => tool),
        } : {}),
      },
    })),
    ...(Object.keys(usage).length ? { usage } : {}),
  };
}

function summarizeAnthropic(events: JsonObject[]): JsonObject | null {
  const starts = events.filter(event => event.type === "message_start");
  if (starts.length !== 1 || !object(starts[0].message)) return null;
  let message = { ...object(starts[0].message) };
  const blocks = new Map<number, JsonObject>();
  const inputs = new Map<number, string>();
  if (Array.isArray(message.content)) {
    for (const [index, value] of message.content.entries()) {
      if (!object(value)) return null;
      blocks.set(index, { ...object(value) });
    }
  }
  for (const event of events) {
    const index = event.index as number;
    if (event.type === "content_block_start") {
      if (!Number.isInteger(index) || !object(event.content_block)) return null;
      blocks.set(index, { ...object(event.content_block) });
    } else if (event.type === "content_block_delta") {
      const block = blocks.get(index), delta = object(event.delta);
      if (!block || !delta) return null;
      if (delta.type === "text_delta") appendText(block, delta, "text");
      else if (delta.type === "thinking_delta") appendText(block, delta, "thinking");
      else if (delta.type === "signature_delta") appendText(block, delta, "signature");
      else if (delta.type === "input_json_delta" && typeof delta.partial_json === "string") {
        inputs.set(index, (inputs.get(index) ?? "") + delta.partial_json);
      } else if (delta.type === "citations_delta" && object(delta.citation)) {
        block.citations = [...(Array.isArray(block.citations) ? block.citations : []), delta.citation];
      } else return null;
    } else if (event.type === "message_delta") {
      message = { ...message, ...object(event.delta), usage: mergeSnapshot(message.usage, event.usage) };
    } else if (!["message_start", "message_stop", "content_block_stop", "ping"].includes(String(event.type))) {
      return null;
    }
  }
  for (const [index, text] of inputs) {
    try { blocks.get(index)!.input = JSON.parse(text); }
    catch { return null; }
  }
  return { ...message, content: [...blocks].sort(([left], [right]) => left - right).map(([, block]) => block) };
}

export function readProxyResponseBody(text: string): {
  formatted: string; summary: string | null;
} {
  // 用户要求各协议默认展示汇总 JSON，并能查看原始事件；只处理展示，不修改请求、计时或 usage 统计来源。
  // 依据实际事件结构识别协议，不能依据供应商默认协议：同一供应商的模型可能使用不同协议。
  let parsed: unknown;
  try { parsed = JSON.parse(text); } catch { /* SSE / JSONL 在下面解析。 */ }
  const events = Array.isArray(parsed) ? parsed : parsed !== undefined ? null : streamEvents(text);
  if (!events) return { formatted: formatProxyBody(text), summary: null };
  const objects = events.map(object);
  // 错误和无法解析的片段不能被汇总隐藏；截断流不补造 finish_reason / stop_reason。
  const canSummarize = objects.every(event => event && !event.error && event.type !== "error");
  const terminal = canSummarize ? objects.filter(event => event && [
    "response.completed", "response.incomplete", "response.failed",
  ].includes(String(event.type)) && event.response).at(-1)?.response : null;
  const body = terminal ?? (events.length === 1 ? events[0] : events);
  const formatted = JSON.stringify(body, null, 2);
  const summary = canSummarize
    ? terminal ?? summarizeChat(objects as JsonObject[]) ?? summarizeAnthropic(objects as JsonObject[])
    : null;
  return { formatted, summary: summary ? JSON.stringify(summary, null, 2) : null };
}
