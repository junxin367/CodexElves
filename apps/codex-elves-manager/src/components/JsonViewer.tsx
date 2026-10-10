import { useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
const PAGE_SIZE = 100;
const STRING_PREVIEW_LENGTH = 160;
const IMPORTANT_JSON_KEYS = [
  "type",
  "id",
  "object",
  "status",
  "error",
  "incomplete_details",
  "model",
  "created_at",
  "role",
  "name",
  "description",
  "index",
  "sequence_number",
  "output_index",
  "content_index",
  "item_id",
  "call_id",
  "stream",
  "previous_response_id",
  "prompt_cache_key",
  "prompt_cache_retention",
  "instructions",
  "system",
  "messages",
  "input",
  "response",
  "item",
  "part",
  "output",
  "choices",
  "message",
  "content",
  "output_text",
  "text",
  "delta",
  "arguments",
  "parameters",
  "strict",
  "tools",
  "tool_choice",
  "parallel_tool_calls",
  "reasoning",
  "thinking",
  "effort",
  "summary",
  "finish_reason",
  "stop_reason",
  "stop_sequence",
  "usage",
  "input_tokens",
  "input_tokens_details",
  "cached_tokens",
  "cache_write_tokens",
  "output_tokens",
  "output_tokens_details",
  "reasoning_tokens",
  "total_tokens",
  "attribution",
  "request_fields",
  "service_tier",
  "safety_identifier",
  "temperature",
  "top_p",
  "top_k",
  "presence_penalty",
  "frequency_penalty",
  "max_tokens",
  "max_completion_tokens",
  "max_output_tokens",
  "stop",
  "stop_sequences",
  "response_format",
  "output_config",
  "stream_options",
  "include",
  "store",
  "metadata",
  "client_metadata",
  "annotations",
  "encrypted_content",
] as const;
const JSON_KEY_PRIORITY = new Map<string, number>(
  IMPORTANT_JSON_KEYS.map((key, index) => [key, index]),
);

export function JsonViewer({ text, label, onCopy }: {
  text: string;
  label: string;
  onCopy: (text: string) => Promise<void>;
}) {
  const parsed = useMemo(() => {
    try {
      return { valid: true as const, value: JSON.parse(text) as JsonValue };
    } catch {
      return { valid: false as const };
    }
  }, [text]);
  const [display, setDisplay] = useState({ revision: 0, open: true });
  const [prioritizeKeys, setPrioritizeKeys] = useState(true);
  useEffect(() => {
    setDisplay((previous) => ({ revision: previous.revision + 1, open: true }));
  }, [text]);

  return (
    <section className="json-viewer" aria-label={label}>
      <div className="json-viewer-toolbar">
        <span>{parsed.valid
          ? `JSON · ${prioritizeKeys ? "重要字段优先" : "原始顺序"}`
          : text.trim() ? "非 JSON 内容 · 原文" : "暂无内容"}</span>
        {parsed.valid && parsed.value !== null && typeof parsed.value === "object" ? (
          <div>
            <button
              aria-pressed={prioritizeKeys}
              title="仅切换展示顺序；复制内容仍保持原始 JSON 顺序"
              type="button"
              onClick={() => setPrioritizeKeys((current) => !current)}
            >
              {prioritizeKeys ? "原始顺序" : "重要字段优先"}
            </button>
            <button type="button" onClick={() => setDisplay((previous) => ({
              revision: previous.revision + 1, open: true,
            }))}>展开首层</button>
            <button type="button" onClick={() => setDisplay((previous) => ({
              revision: previous.revision + 1, open: false,
            }))}>收起全部</button>
          </div>
        ) : null}
      </div>
      <div className="json-viewer-content" tabIndex={0} aria-label={`${label}内容`}>
        {parsed.valid ? (
          <JsonNode
            key={display.revision}
            value={parsed.value}
            path="$"
            initiallyOpen={display.open}
            prioritizeKeys={prioritizeKeys}
            onCopy={onCopy}
          />
        ) : (
          <pre className="json-viewer-raw">{text || "暂无内容"}</pre>
        )}
      </div>
    </section>
  );
}

function JsonNode({
  value, name, path, initiallyOpen = false, prioritizeKeys, onCopy,
}: {
  value: JsonValue;
  name?: string;
  path: string;
  initiallyOpen?: boolean;
  prioritizeKeys: boolean;
  onCopy: (text: string) => Promise<void>;
}) {
  const [open, setOpen] = useState(initiallyOpen && value !== null && typeof value === "object");
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const container = value !== null && typeof value === "object";
  const array = Array.isArray(value);
  // 收起的节点不挂载后代；大数组/对象按批次展示，避免大型请求一次创建全部 DOM。
  const keys = useMemo(() => {
    if (!container || array) return [];
    const objectKeys = Object.keys(value);
    return prioritizeKeys ? objectKeys.sort(compareJsonKeys) : objectKeys;
  }, [value, container, array, prioritizeKeys]);
  const count = array ? value.length : keys.length;
  const start = array ? "[" : "{";
  const end = array ? "]" : "}";
  const longString = typeof value === "string" && value.length > STRING_PREVIEW_LENGTH;
  const expandable = container ? count > 0 : longString;
  const keyLabel = name === undefined ? null : <span className="json-viewer-key">{JSON.stringify(name)}: </span>;
  const actionLabel = `${open ? "收起" : "展开"} ${path}`;

  return (
    <div className="json-viewer-node">
      <div className="json-viewer-row">
        {expandable ? (
          <button
            className="json-viewer-toggle"
            type="button"
            aria-label={actionLabel}
            aria-expanded={open}
            onClick={() => setOpen(!open)}
          >
            {open ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
          </button>
        ) : <span className="json-viewer-toggle-spacer" />}
        <div
          className="json-viewer-value"
          title="右键复制完整值"
          onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
            void onCopy(typeof value === "string" ? value : JSON.stringify(value, null, 2));
          }}
        >
          {keyLabel}
          {container ? (
            <>
              {expandable ? (
                <button
                  className="json-viewer-value-toggle"
                  type="button"
                  aria-label={`${actionLabel} 的内容`}
                  aria-expanded={open}
                  onClick={() => setOpen(!open)}
                >
                  {start}{!open ? ` … ${end}` : ""}
                </button>
              ) : <span>{start}{end}</span>}
              <span className="json-viewer-count">{count.toLocaleString("zh-CN")} {array ? "项" : "个字段"}</span>
            </>
          ) : (
            <>
              {longString && !open ? (
                <button
                  className="json-viewer-string json-viewer-value-toggle"
                  type="button"
                  aria-label={`展开 ${path} 的全文`}
                  aria-expanded={false}
                  onClick={() => setOpen(true)}
                >
                  {JSON.stringify(value.slice(0, STRING_PREVIEW_LENGTH))}
                </button>
              ) : (
                <span className={`json-viewer-${value === null ? "null" : typeof value}`}>
                  {JSON.stringify(value)}
                </span>
              )}
              {longString ? (
                <button className="json-viewer-string-toggle" type="button" aria-expanded={open} onClick={() => setOpen(!open)}>
                  {open ? "收起长文本" : `… 展开全文（${value.length.toLocaleString("zh-CN")} 字符）`}
                </button>
              ) : null}
            </>
          )}
        </div>
      </div>
      {container && open && count > 0 ? (
        <>
          <div className="json-viewer-children">
            {Array.from({ length: Math.min(count, visibleCount) }, (_, index) => {
              const key = array ? String(index) : keys[index];
              return (
                <JsonNode
                  key={key}
                  name={key}
                  path={array ? `${path}[${index}]` : `${path}[${JSON.stringify(key)}]`}
                  value={array ? value[index] : (value as Record<string, JsonValue>)[key]}
                  prioritizeKeys={prioritizeKeys}
                  onCopy={onCopy}
                />
              );
            })}
            {visibleCount < count ? (
              <button className="json-viewer-more" type="button" onClick={() => setVisibleCount(visibleCount + PAGE_SIZE)}>
                再显示 {Math.min(PAGE_SIZE, count - visibleCount)} 项（剩余 {(count - visibleCount).toLocaleString("zh-CN")} 项）
              </button>
            ) : null}
          </div>
          <div className="json-viewer-closing">{end}</div>
        </>
      ) : null}
    </div>
  );
}

function compareJsonKeys(left: string, right: string) {
  const leftPriority = JSON_KEY_PRIORITY.get(left) ?? Number.MAX_SAFE_INTEGER;
  const rightPriority = JSON_KEY_PRIORITY.get(right) ?? Number.MAX_SAFE_INTEGER;
  if (leftPriority !== rightPriority) return leftPriority - rightPriority;
  if (left === right) return 0;
  return left < right ? -1 : 1;
}
