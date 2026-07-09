const COMMON_MODEL_CONTEXT_WINDOWS: Record<string, string> = {
  "codex-auto-review": "272000",
  "gpt-4.1": "1047576",
  "gpt-4.1-mini": "1047576",
  "gpt-4.1-nano": "1047576",
  "gpt-4o": "128000",
  "gpt-4o-mini": "128000",
  "gpt-5": "272000",
  "gpt-5-mini": "272000",
  "gpt-5-nano": "272000",
  "gpt-5.2": "272000",
  "gpt-5.3-codex": "272000",
  "gpt-5.4": "1000000",
  "gpt-5.4-mini": "272000",
  "gpt-5.5": "272000",
  "gpt-5.6": "1000000",
  o3: "200000",
  "o3-mini": "200000",
  o4: "200000",
  "o4-mini": "200000",
  "claude-opus-4-8": "1000000",
  "claude-opus-4-7": "1000000",
  "claude-opus-4-6": "1000000",
  "claude-sonnet-4-6": "1000000",
  "claude-opus-4-5": "200000",
  "claude-opus-4-1": "200000",
  "claude-opus-4": "200000",
  "claude-sonnet-4-5": "200000",
  "claude-sonnet-4": "200000",
  "claude-3-7-sonnet": "200000",
  "claude-3-5-sonnet": "200000",
  "claude-3-opus": "200000",
  "claude-3-haiku": "200000",
  "deepseek-v4-flash": "1000000",
  "deepseek-v4-pro": "1000000",
  "deepseek-chat": "128000",
  "deepseek-reasoner": "128000",
  "deepseek-coder": "128000",
  "deepseek-r1": "128000",
  "deepseek-v3": "128000",
  "qwen3-coder-plus": "1000000",
  "qwen3-coder-plus-2025-09-23": "1000000",
  "qwen3-coder-plus-2025-07-22": "1000000",
  "qwen3-coder-flash": "1000000",
  "qwen3-coder-flash-2025-07-28": "1000000",
  "qwen3.7-max": "1000000",
  "qwen3.6-plus": "1000000",
  "qwen3.5-plus": "1000000",
  "qwen-plus": "1000000",
  "qwen-flash": "1000000",
  "qwen3-max": "262144",
  "qwen3-max-2026-01-23": "262144",
  "qwen3.6-max-preview": "262144",
};

const COMMON_MODEL_CONTEXT_PATTERNS: Array<{ pattern: RegExp; contextWindow: string }> = [
  { pattern: /^gpt-5\.6(?:[.-]|$)/, contextWindow: "1000000" },
  { pattern: /^gpt-5(?:[.-]|$)/, contextWindow: "272000" },
  { pattern: /^gpt-4\.1(?:[.-]|$)/, contextWindow: "1047576" },
  { pattern: /^gpt-4o(?:[.-]|$)/, contextWindow: "128000" },
  { pattern: /^o[34](?:[.-]|$)/, contextWindow: "200000" },
  { pattern: /^claude-(?:opus-4-[678]|sonnet-4-6)(?:[.-]|$)/, contextWindow: "1000000" },
  { pattern: /^claude-/, contextWindow: "200000" },
  { pattern: /^deepseek-v4-/, contextWindow: "1000000" },
  { pattern: /^deepseek-/, contextWindow: "128000" },
  { pattern: /^qwen3-coder-(?:plus|flash)(?:[.-]|$)/, contextWindow: "1000000" },
  { pattern: /^qwen3\.7-max(?:[.-]|$)/, contextWindow: "1000000" },
  { pattern: /^qwen(?:3\.[56]-)?(?:plus|flash)(?:[.-]|$)/, contextWindow: "1000000" },
  { pattern: /^qwen3(?:\.6)?-?max(?:[.-]|$)/, contextWindow: "262144" },
];

export function knownModelContextWindow(model: string): string {
  const normalized = model.trim().toLowerCase();
  if (!normalized) return "";
  const lastPathSegment = normalized.split("/").filter(Boolean).pop() || normalized;
  const candidates = uniqueStrings([normalized, lastPathSegment]);
  for (const candidate of candidates) {
    const exact = COMMON_MODEL_CONTEXT_WINDOWS[candidate];
    if (exact) return exact;
  }
  for (const candidate of candidates) {
    const match = COMMON_MODEL_CONTEXT_PATTERNS.find((item) => item.pattern.test(candidate));
    if (match) return match.contextWindow;
  }
  return "";
}

function uniqueStrings(values: string[]): string[] {
  return Array.from(new Set(values));
}
