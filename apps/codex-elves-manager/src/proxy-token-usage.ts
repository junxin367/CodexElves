type JsonObject = Record<string, unknown>;

export type ProxyTokenUsage = {
  input: number | null;
  output: number | null;
  total: number | null;
  cached: number | null;
  cacheWrite: number | null;
  reasoning: number | null;
  cacheHitRate: number | null;
};

function object(value: unknown): JsonObject {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as JsonObject
    : {};
}

function tokens(...values: unknown[]): number | null {
  return values.find(
    (value): value is number =>
      typeof value === "number" && Number.isSafeInteger(value) && value >= 0,
  ) ?? null;
}

// 输入是详情页格式化后的 JSON：单个响应，或 SSE / WebSocket 事件数组。
// 流中的 usage 是累计快照，按字段更新，不能把每条事件相加。
export function readProxyTokenUsage(formattedBody: string): ProxyTokenUsage {
  let parsed: unknown;
  try {
    parsed = JSON.parse(formattedBody);
  } catch {
    parsed = null;
  }
  let usage: JsonObject = {};
  for (const item of Array.isArray(parsed) ? parsed : [parsed]) {
    const event = object(item);
    const response = object(event.response ?? event.message ?? event);
    if (event.type === "message_start" || event.type === "response.created") {
      usage = {};
    }
    const next = object(response.usage);
    const merged = { ...usage, ...next };
    for (const key of [
      "input_tokens_details", "prompt_tokens_details",
      "output_tokens_details", "completion_tokens_details", "cache_creation",
    ]) {
      if (next[key] != null) merged[key] = { ...object(usage[key]), ...object(next[key]) };
    }
    usage = merged;
  }

  const inputDetails = object(usage.input_tokens_details);
  const promptDetails = object(usage.prompt_tokens_details);
  const cacheCreation = object(usage.cache_creation);
  const cached = tokens(
    inputDetails.cached_tokens, promptDetails.cached_tokens,
    usage.cache_read_input_tokens, usage.prompt_cache_hit_tokens,
  );
  const write5m = tokens(cacheCreation.ephemeral_5m_input_tokens, usage.cache_creation_5m_input_tokens);
  const write1h = tokens(cacheCreation.ephemeral_1h_input_tokens, usage.cache_creation_1h_input_tokens);
  const writeBreakdown = write5m !== null || write1h !== null
    ? (write5m ?? 0) + (write1h ?? 0)
    : null;
  const reportedWrite = tokens(usage.cache_creation_input_tokens);
  const cacheWrite = tokens(inputDetails.cache_write_tokens, promptDetails.cache_write_tokens)
    ?? (reportedWrite !== null || writeBreakdown !== null
      ? Math.max(reportedWrite ?? 0, writeBreakdown ?? 0)
      : null);
  let input = tokens(usage.input_tokens, usage.prompt_tokens);
  // 代理已转换的 Responses usage 包含 input_tokens_details，input 已含缓存。
  // Anthropic 原始 usage 的 input_tokens 仅表示未命中缓存的输入。
  const rawAnthropic = usage.input_tokens_details == null
    && usage.prompt_tokens == null
    && ["cache_read_input_tokens", "cache_creation_input_tokens", "cache_creation",
      "cache_creation_5m_input_tokens", "cache_creation_1h_input_tokens"]
      .some((key) => usage[key] != null);
  if (rawAnthropic && input !== null) input += (cached ?? 0) + (cacheWrite ?? 0);
  const output = tokens(usage.output_tokens, usage.completion_tokens);
  const total = tokens(usage.total_tokens)
    ?? (input !== null && output !== null ? input + output : null);
  const reasoning = tokens(
    object(usage.output_tokens_details).reasoning_tokens,
    object(usage.completion_tokens_details).reasoning_tokens,
    usage.reasoning_tokens, usage.thinking_tokens,
  );
  return {
    input, output, total, cached, cacheWrite, reasoning,
    cacheHitRate: input !== null && input > 0 && cached !== null && cached <= input
      ? cached / input
      : null,
  };
}
