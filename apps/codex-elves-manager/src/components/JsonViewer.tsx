import { useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
const PAGE_SIZE = 100;
const STRING_PREVIEW_LENGTH = 160;

export function JsonViewer({ text, label }: { text: string; label: string }) {
  const parsed = useMemo(() => {
    try {
      return { valid: true as const, value: JSON.parse(text) as JsonValue };
    } catch {
      return { valid: false as const };
    }
  }, [text]);
  const [display, setDisplay] = useState({ revision: 0, open: true });
  useEffect(() => {
    setDisplay((previous) => ({ revision: previous.revision + 1, open: true }));
  }, [text]);

  return (
    <section className="json-viewer" aria-label={label}>
      <div className="json-viewer-toolbar">
        <span>{parsed.valid ? "JSON" : text.trim() ? "非 JSON 内容 · 原文" : "暂无内容"}</span>
        {parsed.valid && parsed.value !== null && typeof parsed.value === "object" ? (
          <div>
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
          <JsonNode key={display.revision} value={parsed.value} path="$" initiallyOpen={display.open} />
        ) : (
          <pre className="json-viewer-raw">{text || "暂无内容"}</pre>
        )}
      </div>
    </section>
  );
}

function JsonNode({
  value, name, path, initiallyOpen = false,
}: {
  value: JsonValue;
  name?: string;
  path: string;
  initiallyOpen?: boolean;
}) {
  const [open, setOpen] = useState(initiallyOpen && value !== null && typeof value === "object");
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const container = value !== null && typeof value === "object";
  const array = Array.isArray(value);
  // 收起的节点不挂载后代；大数组/对象按批次展示，避免大型请求一次创建全部 DOM。
  const keys = useMemo(() => container && !array ? Object.keys(value) : [], [value, container, array]);
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
        <div className="json-viewer-value">
          {keyLabel}
          {container ? (
            <>
              <span>{start}{!open && count > 0 ? " … " : ""}{!open || count === 0 ? end : ""}</span>
              <span className="json-viewer-count">{count.toLocaleString("zh-CN")} {array ? "项" : "个字段"}</span>
            </>
          ) : (
            <>
              <span className={`json-viewer-${value === null ? "null" : typeof value}`}>
                {JSON.stringify(longString && !open ? value.slice(0, STRING_PREVIEW_LENGTH) : value)}
              </span>
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
