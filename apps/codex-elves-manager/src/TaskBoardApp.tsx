import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  GripVertical,
  LoaderCircle,
  MessageSquare,
  Pencil,
  Plus,
  Search,
  X,
} from "lucide-react";
import { createPortal } from "react-dom";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
  type ReactNode,
} from "react";
import "./task-board.css";

type TaskStatus = string;

type BoardDefinition = {
  id: TaskStatus;
  label: string;
  color: string;
};

type TaskProject = {
  cwd: string;
  label: string;
};

type TaskConversation = {
  sessionId: string;
  title: string;
  cwd: string;
  updatedAtMs?: number | null;
};

type Task = {
  id: string;
  title: string;
  project: TaskProject;
  status: TaskStatus;
  order: number;
  conversations: TaskConversation[];
  createdAtMs: number;
  updatedAtMs: number;
};

type CatalogProject = TaskProject & {
  sessionCount: number;
};

type CatalogSession = {
  sessionId: string;
  title: string;
  cwd: string;
  updatedAtMs?: number | null;
  sessionAliases?: string[];
};

type DropdownOption = {
  value: string;
  label: string;
  description?: string;
  iconId?: string;
  disabled?: boolean;
};

type CreateModelOption = DropdownOption & {
  efforts: DropdownOption[];
};

type BoardResponse = {
  status: string;
  code?: string;
  message?: string;
  schemaVersion?: number;
  revision?: number;
  boards?: BoardDefinition[];
  tasks?: Task[];
  projects?: CatalogProject[];
  sessions?: CatalogSession[];
  warnings?: Array<{ code: string; count: number }>;
  sessionId?: string;
  canStart?: boolean;
  modelId?: string;
  effortId?: string;
  models?: CreateModelOption[];
  appearance?: TaskBoardAppearance;
  statuses?: ConversationRuntimeStatus[];
};

type BoardSnapshot = {
  schemaVersion: number;
  revision: number;
  boards: BoardDefinition[];
  tasks: Task[];
};

type Catalog = {
  projects: CatalogProject[];
  sessions: CatalogSession[];
  warnings: Array<{ code: string; count: number }>;
};

type EditorState = {
  targetTask: Task | null;
  taskId: string;
  semanticKey: string;
  mode: "existing" | "new";
  title: string;
  projectCwd: string;
  initialStatus: TaskStatus;
  selectedSessionIds: string[];
  instruction: string;
  busy: boolean;
  feedback: string;
  nativeCreateAvailable: boolean | null;
  nativeCreateMessage: string;
  modelId: string;
  effortId: string;
  modelOptions: CreateModelOption[];
  modelSelectionTouched: boolean;
};

type NativeCreateRecoveryKind = "create-task" | "attach-conversation";

type NativeCreateRecoveryRecord = {
  kind: NativeCreateRecoveryKind;
  taskId: string;
  title: string;
  project: TaskProject;
  sessionId: string;
  initialStatus: TaskStatus;
  targetTaskId?: string;
  createdAtMs: number;
  semanticKey: string;
};

type TaskBoardAppearanceOverlay = {
  enabled: boolean;
  kind: "image" | "color" | "gradient";
  imageUrl: string;
  opacity: number;
  fit: "cover" | "contain";
  backgroundColor: string;
  gradientFrom: string;
  gradientTo: string;
  gradientAngle: number;
};

type TaskBoardAppearance = {
  version?: number;
  signature?: string;
  background: string;
  foreground: string;
  panelBackground: string;
  cardBackground: string;
  cardBackgroundHover: string;
  border: string;
  borderSoft: string;
  textSecondary: string;
  textTertiary: string;
  accent: string;
  actionBackground: string;
  actionBackgroundHover: string;
  actionBackgroundActive: string;
  actionForeground: string;
  actionBorder: string;
  modalBackground: string;
  modalForeground: string;
  modalBorder: string;
  fieldBackground: string;
  menuBackground: string;
  rootFontFamily: string;
  modalFontFamily: string;
  overlay?: TaskBoardAppearanceOverlay;
};

type DetachConfirmationState = {
  task: Task;
  conversation: TaskConversation;
  busy: boolean;
  feedback: string;
};

type DeleteTaskConfirmationState = {
  task: Task;
  busy: boolean;
  feedback: string;
};

type BoardManagerState = {
  label: string;
  busy: boolean;
  busyKind: "" | "create" | "delete" | "rename" | "move";
  busyBoardId: string;
  feedback: string;
  pendingDeleteId: string;
};

type ConversationRuntimeStatus = {
  sessionId: string;
  known: boolean;
  checking: boolean;
  isRunning: boolean;
  unread: boolean;
};

type ConversationStatusPresentation = {
  id: "running" | "unread" | "completed" | "checking" | "unknown" | "unavailable";
  label: string;
};

const unassignedStatus: BoardDefinition = {
  id: "new",
  label: "未分配",
  color: "#94a3b8",
};

const defaultBoards: BoardDefinition[] = [
  { id: "planning", label: "规划", color: "#60a5fa" },
  { id: "executing", label: "执行", color: "#c084fc" },
  { id: "review", label: "验收", color: "#fbbf24" },
  { id: "done", label: "完成", color: "#34d399" },
];

const taskBoardStatusIconPaths = [
  ["M3 3.5h10l-1 8H4l-1-8Z", "M3.6 8.5h2.5l1 1h1.8l1-1h2.5"],
  ["M6 4h7", "M6 8h7", "M6 12h7", "M3 4h.01", "M3 8h.01", "M3 12h.01"],
  ["m5 3.5 7 4.5-7 4.5Z"],
  [
    "M2.5 8s2-3.5 5.5-3.5S13.5 8 13.5 8 11.5 11.5 8 11.5 2.5 8 2.5 8Z",
    "M8 6.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 1 0 0-3Z",
  ],
  ["M8 3a5 5 0 1 0 0 10A5 5 0 0 0 8 3Z", "m5.5 8 1.6 1.6 3.5-3.5"],
  ["M4 13V3.5", "M4 4h7l-1.2 2L11 8H4"],
  ["M4.5 2.5h7v11L8 11.5l-3.5 2Z"],
  ["m8 2.8 5 2.7-5 2.7-5-2.7Z", "m3 8 5 2.7L13 8", "m3 10.5 5 2.5 5-2.5"],
  [
    "M8 3a5 5 0 1 0 0 10A5 5 0 0 0 8 3Z",
    "M8 6a2 2 0 1 0 0 4 2 2 0 1 0 0-4Z",
  ],
  [
    "M8 2.5 9 5l2.5 1L9 7l-1 2.5L7 7 4.5 6 7 5Z",
    "m12 10 .5 1.2 1.2.5-1.2.5-.5 1.3-.5-1.3-1.2-.5 1.2-.5Z",
  ],
] as const;

const taskBoardBuiltInStatusIconIndexes: Record<string, number> = {
  new: 0,
  planning: 1,
  executing: 2,
  review: 3,
  done: 4,
};

function taskBoardStatusIconIndex(statusId: string) {
  const normalized = statusId.trim().toLocaleLowerCase("en-US");
  const builtInIndex = taskBoardBuiltInStatusIconIndexes[normalized];
  if (builtInIndex !== undefined) return builtInIndex;
  let hash = 2166136261;
  for (let index = 0; index < normalized.length; index += 1) {
    hash ^= normalized.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) % taskBoardStatusIconPaths.length;
}

function TaskBoardStatusIcon({
  statusId,
  className = "task-board-status-icon",
}: {
  statusId: string;
  className?: string;
}) {
  const iconIndex = taskBoardStatusIconIndex(statusId);
  return (
    <svg
      className={className}
      data-task-board-status-icon={iconIndex}
      aria-hidden="true"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.35"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {taskBoardStatusIconPaths[iconIndex].map((path) => (
        <path d={path} key={path} />
      ))}
    </svg>
  );
}

const emptySnapshot: BoardSnapshot = {
  schemaVersion: 1,
  revision: 0,
  boards: defaultBoards,
  tasks: [],
};

const emptyCatalog: Catalog = {
  projects: [],
  sessions: [],
  warnings: [],
};

const newSessionRetryDelaysMs = [250, 750, 1500, 2500, 5000];
const appearanceRefreshIntervalMs = 20_000;
const nativeCreateRecoveryKey =
  "codexElvesTaskBoardStandaloneNativeCreateRecoveryV1";
const nativeCreateRecoveryTtlMs = 24 * 60 * 60 * 1000;
const nativeCreateRecoveryClockSkewMs = 5 * 60 * 1000;

const defaultEffortOptions: DropdownOption[] = [
  { value: "low", label: "轻度" },
  { value: "medium", label: "中" },
  { value: "high", label: "高" },
  { value: "xhigh", label: "极高" },
  { value: "max", label: "最高" },
];

const fallbackModelOptions: CreateModelOption[] = [
  {
    value: "",
    label: "使用当前默认模型",
    description: "沿用 Codex 当前会话的默认模型",
    efforts: defaultEffortOptions,
  },
];

function taskBoardManagedBoards(value: unknown): BoardDefinition[] {
  const source = Array.isArray(value) ? value : defaultBoards;
  const seenIds = new Set<string>();
  const seenLabels = new Set<string>();
  return source.flatMap((entry) => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return [];
    const candidate = entry as Record<string, unknown>;
    const id = String(candidate.id || "").trim().toLocaleLowerCase("en-US");
    const label = String(candidate.label || "").trim();
    const color = String(candidate.color || "").trim();
    const labelKey = label.toLocaleLowerCase("zh-CN");
    if (
      id === "new" ||
      !taskBoardStatusIdIsValid(id) ||
      !label ||
      Array.from(label).length > 40 ||
      !/^#[0-9a-f]{6}$/i.test(color) ||
      seenIds.has(id) ||
      seenLabels.has(labelKey)
    ) {
      return [];
    }
    seenIds.add(id);
    seenLabels.add(labelKey);
    return [{ id, label, color }];
  });
}

function taskBoardStatusIdIsValid(value: string) {
  const status = value.trim().toLocaleLowerCase("en-US");
  return (
    ["new", "planning", "executing", "review", "done"].includes(status) ||
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      status,
    )
  );
}

function taskBoardStatusDefinitions(snapshot: BoardSnapshot) {
  return [unassignedStatus, ...snapshot.boards];
}

function taskBoardColumnsStyle(columnCount: number): CSSProperties {
  const count = Math.max(1, Math.trunc(columnCount) || 1);
  return {
    gridTemplateColumns: `repeat(${count}, minmax(292px, 1fr))`,
    minWidth: `${count * 304 + Math.max(0, count - 1) * 12}px`,
  };
}

function taskBoardVisibleStatusDefinitions(
  statusDefinitions: BoardDefinition[],
  visibleTasks: Task[],
) {
  const unassignedHasTasks = visibleTasks.some((task) => task.status === "new");
  return statusDefinitions.filter(
    (status) => status.id !== "new" || unassignedHasTasks,
  );
}

function taskBoardMoveBoardDefinitions(
  boards: BoardDefinition[],
  boardId: string,
  targetIndex: number,
) {
  const sourceIndex = boards.findIndex((board) => board.id === boardId);
  if (sourceIndex < 0) return boards;
  const next = boards.slice();
  const [board] = next.splice(sourceIndex, 1);
  const index = Math.max(0, Math.min(Math.trunc(targetIndex), next.length));
  next.splice(index, 0, board);
  return next;
}

function taskBoardMoveBoardTargetIndex(
  boards: BoardDefinition[],
  sourceBoardId: string,
  targetBoardId: string,
  placeAfter: boolean,
) {
  const sourceIndex = boards.findIndex((board) => board.id === sourceBoardId);
  if (sourceIndex < 0 || sourceBoardId === targetBoardId) {
    return Math.max(0, sourceIndex);
  }
  const remaining = boards.filter((board) => board.id !== sourceBoardId);
  const targetIndex = remaining.findIndex((board) => board.id === targetBoardId);
  if (targetIndex < 0) return Math.max(0, sourceIndex);
  return targetIndex + (placeAfter ? 1 : 0);
}

function normalizeProjectCwd(value: string) {
  return value
    .trim()
    .replace(/\\/g, "/")
    .replace(/\/+$/, "")
    .toLocaleLowerCase("en-US");
}

function taskBoardCreateTaskId() {
  return crypto.randomUUID();
}

function taskBoardCreateTaskIdIsValid(value: string) {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
    value.trim(),
  );
}

function taskBoardRecoverySemanticKey(
  kind: NativeCreateRecoveryKind,
  title: string,
  project: TaskProject,
  initialStatus: TaskStatus,
  targetTaskId = "",
) {
  return JSON.stringify([
    kind,
    targetTaskId.trim().toLocaleLowerCase("en-US"),
    title.trim(),
    normalizeProjectCwd(project.cwd),
    initialStatus,
  ]);
}

function taskBoardNativeCreateRecoveryRecord(
  value: unknown,
): NativeCreateRecoveryRecord | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const candidate = value as Record<string, unknown>;
  const allowedKeys = new Set([
    "kind",
    "taskId",
    "title",
    "project",
    "sessionId",
    "initialStatus",
    "targetTaskId",
    "createdAtMs",
    "semanticKey",
  ]);
  if (Object.keys(candidate).some((key) => !allowedKeys.has(key))) return null;
  if (
    !candidate.project ||
    typeof candidate.project !== "object" ||
    Array.isArray(candidate.project)
  ) {
    return null;
  }
  const projectValue = candidate.project as Record<string, unknown>;
  if (
    Object.keys(projectValue).some(
      (key) => key !== "cwd" && key !== "label",
    )
  ) {
    return null;
  }
  const kind =
    candidate.kind === "create-task" ||
    candidate.kind === "attach-conversation"
      ? candidate.kind
      : null;
  const taskId = String(candidate.taskId || "").trim();
  const title = String(candidate.title || "").trim();
  const project = {
    cwd: String(projectValue.cwd || "").trim(),
    label: String(projectValue.label || "").trim(),
  };
  const sessionId = String(candidate.sessionId || "").trim();
  const initialStatus = String(candidate.initialStatus || "") as TaskStatus;
  const targetTaskId = String(candidate.targetTaskId || "").trim();
  const createdAtMs = Number(candidate.createdAtMs || 0);
  const semanticKey = String(candidate.semanticKey || "");
  const now = Date.now();
  if (
    !kind ||
    !taskBoardCreateTaskIdIsValid(taskId) ||
    !title ||
    !project.cwd ||
    !project.label ||
    !sessionId ||
    /(^|:)(client-)?new-thread:/i.test(sessionId) ||
    !taskBoardStatusIdIsValid(initialStatus) ||
    !Number.isFinite(createdAtMs) ||
    createdAtMs <= 0 ||
    now - createdAtMs > nativeCreateRecoveryTtlMs ||
    createdAtMs - now > nativeCreateRecoveryClockSkewMs
  ) {
    return null;
  }
  if (
    (kind === "attach-conversation" &&
      (!taskBoardCreateTaskIdIsValid(targetTaskId) ||
        targetTaskId !== taskId)) ||
    (kind === "create-task" && targetTaskId)
  ) {
    return null;
  }
  const expectedSemanticKey = taskBoardRecoverySemanticKey(
    kind,
    title,
    project,
    initialStatus,
    targetTaskId,
  );
  if (semanticKey !== expectedSemanticKey) return null;
  return {
    kind,
    taskId,
    title,
    project,
    sessionId,
    initialStatus,
    ...(targetTaskId ? { targetTaskId } : {}),
    createdAtMs,
    semanticKey,
  };
}

function readNativeCreateRecovery() {
  try {
    const parsed = JSON.parse(
      sessionStorage.getItem(nativeCreateRecoveryKey) || "null",
    );
    const record = taskBoardNativeCreateRecoveryRecord(parsed);
    if (!record) sessionStorage.removeItem(nativeCreateRecoveryKey);
    return record;
  } catch {
    try {
      sessionStorage.removeItem(nativeCreateRecoveryKey);
    } catch {
      // Ignore unavailable session storage.
    }
    return null;
  }
}

function saveNativeCreateRecovery(record: NativeCreateRecoveryRecord) {
  const validated = taskBoardNativeCreateRecoveryRecord(record);
  if (!validated) return false;
  const payload =
    validated.kind === "attach-conversation"
      ? {
          kind: validated.kind,
          taskId: validated.taskId,
          title: validated.title,
          project: validated.project,
          sessionId: validated.sessionId,
          initialStatus: validated.initialStatus,
          targetTaskId: validated.targetTaskId,
          createdAtMs: validated.createdAtMs,
          semanticKey: validated.semanticKey,
        }
      : {
          kind: validated.kind,
          taskId: validated.taskId,
          title: validated.title,
          project: validated.project,
          sessionId: validated.sessionId,
          initialStatus: validated.initialStatus,
          createdAtMs: validated.createdAtMs,
          semanticKey: validated.semanticKey,
        };
  try {
    sessionStorage.setItem(nativeCreateRecoveryKey, JSON.stringify(payload));
    return true;
  } catch {
    return false;
  }
}

function clearNativeCreateRecovery() {
  try {
    sessionStorage.removeItem(nativeCreateRecoveryKey);
  } catch {
    // Ignore unavailable session storage.
  }
}

function taskBoardRecoveryAlreadyApplied(
  record: NativeCreateRecoveryRecord,
  snapshot: BoardSnapshot,
) {
  const taskId =
    record.kind === "attach-conversation"
      ? record.targetTaskId || record.taskId
      : record.taskId;
  const task = snapshot.tasks.find((candidate) => candidate.id === taskId);
  if (!task) return false;
  if (
    normalizeProjectCwd(task.project.cwd) !==
    normalizeProjectCwd(record.project.cwd)
  ) {
    return false;
  }
  if (record.kind === "create-task" && task.title.trim() !== record.title) {
    return false;
  }
  const expectedSessionId = normalizeSessionId(record.sessionId);
  return task.conversations.some(
    (conversation) =>
      normalizeSessionId(conversation.sessionId) === expectedSessionId,
  );
}

function taskBoardSnapshotFromResponse(
  response: BoardResponse,
): BoardSnapshot | null {
  if (!Array.isArray(response.tasks)) return null;
  return {
    schemaVersion: response.schemaVersion ?? 1,
    revision: response.revision ?? 0,
    boards: taskBoardManagedBoards(response.boards),
    tasks: response.tasks,
  };
}

async function invokeSessionMutationWithRetry(
  command: "task_board_create_task" | "task_board_attach_conversations",
  request: Record<string, unknown>,
  onCatalog: (catalog: Catalog) => void,
) {
  let result = await invoke<BoardResponse>(command, { request });
  for (const delayMs of newSessionRetryDelaysMs) {
    if (result.code !== "session_not_found") break;
    await wait(delayMs);
    const sessions = await invoke<BoardResponse>("task_board_load_catalog");
    if (sessions.status === "ok") {
      onCatalog({
        projects: sessions.projects ?? [],
        sessions: sessions.sessions ?? [],
        warnings: sessions.warnings ?? [],
      });
    }
    result = await invoke<BoardResponse>(command, { request });
  }
  return result;
}

function formatSessionUpdatedTime(value?: number | null) {
  const timestamp = Number(value || 0);
  if (!Number.isFinite(timestamp) || timestamp <= 0) return "时间未知";
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return "时间未知";
  const pad = (part: number) => String(part).padStart(2, "0");
  return [
    `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())}`,
    `${pad(date.getHours())}:${pad(date.getMinutes())}`,
  ].join(" ");
}

function taskBoardAppearanceStyle(
  appearance: TaskBoardAppearance | null,
): CSSProperties | undefined {
  if (!appearance) return undefined;
  return {
    "--task-board-background": appearance.background,
    "--task-board-foreground": appearance.foreground,
    "--task-board-panel-background": appearance.panelBackground,
    "--task-board-card-background": appearance.cardBackground,
    "--task-board-card-background-hover": appearance.cardBackgroundHover,
    "--task-board-border": appearance.border,
    "--task-board-border-soft": appearance.borderSoft,
    "--task-board-text-secondary": appearance.textSecondary,
    "--task-board-text-tertiary": appearance.textTertiary,
    "--task-board-accent": appearance.accent,
    "--task-board-action-background": appearance.actionBackground,
    "--task-board-action-background-hover": appearance.actionBackgroundHover,
    "--task-board-action-background-active": appearance.actionBackgroundActive,
    "--task-board-action-foreground": appearance.actionForeground,
    "--task-board-action-border": appearance.actionBorder,
    "--task-board-modal-background": appearance.modalBackground,
    "--task-board-modal-foreground": appearance.modalForeground,
    "--task-board-modal-border": appearance.modalBorder,
    "--task-board-field-background": appearance.fieldBackground,
    "--task-board-menu-background": appearance.menuBackground,
    "--task-board-root-font-family": appearance.rootFontFamily,
    "--task-board-modal-font-family": appearance.modalFontFamily,
  } as CSSProperties;
}

function taskBoardAppearanceOverlay(
  appearance: TaskBoardAppearance | null,
): TaskBoardAppearanceOverlay | null {
  const overlay = appearance?.overlay;
  if (!overlay?.enabled) return null;
  const kind = ["image", "color", "gradient"].includes(overlay.kind)
    ? overlay.kind
    : "image";
  const imageUrl = String(overlay.imageUrl || "").trim();
  if (kind === "image" && (!imageUrl || /^data:/i.test(imageUrl))) return null;
  return {
    enabled: true,
    kind,
    imageUrl,
    opacity: Math.min(1, Math.max(0.01, Number(overlay.opacity) || 0.35)),
    fit: overlay.fit === "cover" ? "cover" : "contain",
    backgroundColor: String(overlay.backgroundColor || "#1e293b"),
    gradientFrom: String(overlay.gradientFrom || "#4338ca"),
    gradientTo: String(overlay.gradientTo || "#0ea5e9"),
    gradientAngle: Number.isFinite(Number(overlay.gradientAngle))
      ? Number(overlay.gradientAngle)
      : 135,
  };
}

function taskBoardAppearanceImageUrl(
  imageUrl: string,
  appearanceSignature = "",
) {
  const rawImageUrl = imageUrl.trim();
  const signature = appearanceSignature.trim();
  if (!rawImageUrl || !signature) return rawImageUrl;
  const separator = rawImageUrl.includes("?") ? "&" : "?";
  return `${rawImageUrl}${separator}codexElvesAppearance=${encodeURIComponent(signature)}`;
}

function taskProjectRef(project: TaskProject): TaskProject {
  return {
    cwd: project.cwd,
    label: project.label,
  };
}

function taskBoardDropdownLeft(
  triggerLeft: number,
  menuWidth: number,
  viewportWidth: number,
) {
  const viewportRight = Math.max(8, viewportWidth - menuWidth - 8);
  return Math.max(8, Math.min(viewportRight, triggerLeft));
}

function taskBoardCreateSubmenuLeft(
  menuLeft: number,
  menuRight: number,
  submenuWidth: number,
  viewportWidth: number,
) {
  const edge = 8;
  const gap = 6;
  const rightLeft = menuRight + gap;
  if (rightLeft + submenuWidth <= viewportWidth - edge) return rightLeft;
  const viewportRight = Math.max(edge, viewportWidth - submenuWidth - edge);
  return Math.max(
    edge,
    Math.min(viewportRight, menuLeft - gap - submenuWidth),
  );
}

function taskBoardCreateSubmenuTop(
  anchorTop: number,
  anchorHeight: number,
  submenuHeight: number,
  viewportHeight: number,
) {
  const edge = 8;
  const viewportBottom = Math.max(
    edge,
    viewportHeight - submenuHeight - edge,
  );
  const centeredTop = anchorTop + (anchorHeight - submenuHeight) / 2;
  return Math.max(edge, Math.min(viewportBottom, centeredTop));
}

function conversationStatusPresentation(
  runtimeStatus: ConversationRuntimeStatus | undefined,
  available: boolean,
): ConversationStatusPresentation {
  if (!available) return { id: "unavailable", label: "不可用" };
  if (runtimeStatus?.isRunning) return { id: "running", label: "运行中" };
  if (!runtimeStatus || runtimeStatus.checking) {
    return { id: "checking", label: "检查中" };
  }
  if (!runtimeStatus.known) return { id: "unknown", label: "状态未知" };
  if (runtimeStatus.unread) {
    return { id: "unread", label: "未读" };
  }
  return { id: "completed", label: "已完成" };
}

function taskBoardModalFocusableElements(modal: HTMLElement) {
  return [
    modal.querySelector<HTMLElement>(".task-board-icon-button"),
    ...Array.from(
      modal.querySelectorAll<HTMLElement>(".task-board-create-mode"),
    ),
    modal.querySelector<HTMLElement>("[data-task-board-modal-autofocus]"),
    ...Array.from(
      modal.querySelectorAll<HTMLElement>(".task-board-create-select"),
    ),
    modal.querySelector<HTMLElement>(".task-board-instruction textarea"),
    modal.querySelector<HTMLElement>(".task-board-create-model-trigger"),
    ...Array.from(
      modal.querySelectorAll<HTMLElement>(
        '.task-board-session-picker input[type="checkbox"]',
      ),
    ),
    modal.querySelector<HTMLElement>(".task-board-button:not(.primary)"),
    modal.querySelector<HTMLElement>(".task-board-button.primary"),
  ].filter(
    (element): element is HTMLElement =>
      Boolean(
        element &&
          !("disabled" in element && element.disabled) &&
          !element.hidden &&
          element.getAttribute("aria-hidden") !== "true" &&
          element.getClientRects().length > 0,
      ),
  );
}

function TaskBoardDropdown({
  value,
  options,
  ariaLabel,
  placeholder = "",
  className = "",
  menuClassName = "",
  minWidth = 180,
  fixedWidth = 0,
  matchTriggerWidth = false,
  placement = "auto",
  disabled = false,
  modalFocusTrap = false,
  showChevron = true,
  searchable = false,
  searchPlaceholder = "搜索选项",
  onChange,
}: {
  value: string;
  options: DropdownOption[];
  ariaLabel: string;
  placeholder?: string;
  className?: string;
  menuClassName?: string;
  minWidth?: number;
  fixedWidth?: number;
  matchTriggerWidth?: boolean;
  placement?: "auto" | "top" | "bottom";
  disabled?: boolean;
  modalFocusTrap?: boolean;
  showChevron?: boolean;
  searchable?: boolean;
  searchPlaceholder?: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const selectedOption = options.find((option) => option.value === value);
  const triggerLabel = selectedOption?.label || placeholder || options[0]?.label || "";
  const visibleOptions = useMemo(() => {
    const normalizedQuery = searchQuery.trim().toLocaleLowerCase("zh-CN");
    if (!searchable || !normalizedQuery) return options;
    return options.filter((option) =>
      [option.label, option.description]
        .filter(Boolean)
        .join("\n")
        .toLocaleLowerCase("zh-CN")
        .includes(normalizedQuery),
    );
  }, [options, searchQuery, searchable]);

  const closeDropdown = useCallback((restoreFocus: boolean) => {
    setOpen(false);
    setSearchQuery("");
    if (restoreFocus) {
      window.requestAnimationFrame(() => {
        if (triggerRef.current?.isConnected) triggerRef.current.focus();
      });
    }
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    if (!trigger || !menu) return;
    const triggerRect = trigger.getBoundingClientRect();
    const viewportWidth = window.innerWidth || 1024;
    const viewportHeight = window.innerHeight || 768;
    const triggerWidth = Math.max(0, Math.round(triggerRect.width));
    const requestedFixedWidth = Math.max(0, Math.round(fixedWidth));
    const constrainedFixedWidth = requestedFixedWidth
      ? Math.min(requestedFixedWidth, Math.max(0, viewportWidth - 16))
      : 0;
    const menuWidth = constrainedFixedWidth || Math.max(minWidth, triggerWidth);
    menu.style.minWidth = `${menuWidth}px`;
    menu.style.width = constrainedFixedWidth
      ? `${constrainedFixedWidth}px`
      : matchTriggerWidth && triggerWidth
        ? `${triggerWidth}px`
        : "";
    const menuRect = menu.getBoundingClientRect();
    const renderedWidth = menuRect.width || menuWidth;
    const renderedHeight = menuRect.height || 0;
    const gap = 6;
    const left = taskBoardDropdownLeft(
      triggerRect.left,
      renderedWidth,
      viewportWidth,
    );
    const fitsBelow = triggerRect.bottom + gap + renderedHeight <= viewportHeight - 8;
    const top =
      placement === "top"
        ? Math.max(8, triggerRect.top - gap - renderedHeight)
        : placement === "bottom" || fitsBelow
          ? triggerRect.bottom + gap
          : Math.max(8, triggerRect.top - gap - renderedHeight);
    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;
    menu.style.visibility = "visible";
  }, [
    fixedWidth,
    matchTriggerWidth,
    minWidth,
    open,
    placement,
    visibleOptions,
  ]);

  useEffect(() => {
    if (!open) return;
    const menu = menuRef.current;
    const trigger = triggerRef.current;
    if (!menu || !trigger) return;

    const enabledButtons = () =>
      Array.from(menu.querySelectorAll<HTMLButtonElement>("button")).filter(
        (button) => !button.disabled,
      );
    const moveModalFocusFromTrigger = (shiftKey: boolean) => {
      const modal = trigger.closest(".task-board-modal");
      if (!(modal instanceof HTMLElement)) return;
      const focusable = taskBoardModalFocusableElements(modal);
      const triggerIndex = focusable.indexOf(trigger);
      const nextIndex = shiftKey
        ? triggerIndex <= 0
          ? focusable.length - 1
          : triggerIndex - 1
        : triggerIndex === focusable.length - 1
          ? 0
          : triggerIndex + 1;
      setOpen(false);
      window.requestAnimationFrame(() => focusable[nextIndex]?.focus());
    };
    const handleKeydown = (event: KeyboardEvent) => {
      const buttons = enabledButtons();
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const searchActive =
        searchable && document.activeElement === searchInputRef.current;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
        closeDropdown(true);
      } else if (searchActive) {
        if (
          (event.key === "ArrowDown" || event.key === "ArrowUp") &&
          buttons.length
        ) {
          event.preventDefault();
          buttons[event.key === "ArrowDown" ? 0 : buttons.length - 1]?.focus();
        } else if (event.key === "Tab" && modalFocusTrap) {
          if (!event.shiftKey && buttons.length) {
            event.preventDefault();
            event.stopImmediatePropagation();
            buttons[0]?.focus();
          } else {
            event.preventDefault();
            event.stopImmediatePropagation();
            moveModalFocusFromTrigger(event.shiftKey);
          }
        }
      } else if (event.key === "Tab" && modalFocusTrap) {
        event.preventDefault();
        event.stopImmediatePropagation();
        moveModalFocusFromTrigger(event.shiftKey);
      } else if (
        event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "Home" ||
        event.key === "End"
      ) {
        if (!buttons.length) return;
        event.preventDefault();
        const next =
          event.key === "Home"
            ? 0
            : event.key === "End"
              ? buttons.length - 1
              : event.key === "ArrowDown"
                ? (current + 1 + buttons.length) % buttons.length
                : (current - 1 + buttons.length) % buttons.length;
        buttons[next]?.focus();
      } else if (event.key === "Enter" || event.key === " ") {
        const target = buttons[current >= 0 ? current : 0];
        if (!target) return;
        event.preventDefault();
        target.click();
      }
    };
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (menu.contains(target) || trigger.contains(target)) return;
      closeDropdown(false);
    };
    const handleViewportChange = (event: Event) => {
      const target = event.target;
      if (target instanceof Node && menu.contains(target)) return;
      closeDropdown(false);
    };

    document.addEventListener("keydown", handleKeydown, true);
    document.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    window.requestAnimationFrame(() => {
      if (searchable && searchInputRef.current?.isConnected) {
        searchInputRef.current.focus({ preventScroll: true });
        return;
      }
      const selected = menu.querySelector<HTMLButtonElement>(
        'button[aria-selected="true"]:not(:disabled)',
      );
      (selected ?? enabledButtons()[0])?.focus({ preventScroll: true });
    });
    return () => {
      document.removeEventListener("keydown", handleKeydown, true);
      document.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
    };
  }, [closeDropdown, modalFocusTrap, open, searchable]);

  const menu =
    open && document.body
      ? createPortal(
          <div
            className={`task-board-dropdown-menu ${menuClassName}`.trim()}
            ref={menuRef}
            role="listbox"
            aria-label={ariaLabel}
            data-searchable={searchable || undefined}
            style={{ left: 8, top: 8, visibility: "hidden" }}
          >
            {searchable ? (
              <label className="task-board-dropdown-search">
                <Search size={14} strokeWidth={1.4} aria-hidden="true" />
                <input
                  ref={searchInputRef}
                  type="search"
                  value={searchQuery}
                  placeholder={searchPlaceholder}
                  aria-label={searchPlaceholder}
                  onChange={(event) => setSearchQuery(event.target.value)}
                />
              </label>
            ) : null}
            <div className="task-board-dropdown-options">
              {visibleOptions.map((option) => {
                const selected = option.value === value;
                return (
                  <button
                    type="button"
                    role="option"
                    aria-selected={selected}
                    data-value={option.value}
                    disabled={option.disabled}
                    title={
                      option.description
                        ? `${option.label}\n${option.description}`
                        : option.label
                    }
                    key={option.value}
                    onClick={() => {
                      if (option.disabled) return;
                      closeDropdown(true);
                      onChange(option.value);
                    }}
                  >
                    <span className="task-board-dropdown-option-copy">
                      <span className="task-board-dropdown-option-title-row">
                        {option.iconId ? (
                          <TaskBoardStatusIcon
                            className="task-board-dropdown-status-icon"
                            statusId={option.iconId}
                          />
                        ) : null}
                        <span className="task-board-dropdown-option-title">
                          {option.label}
                        </span>
                      </span>
                      {option.description ? (
                        <span className="task-board-dropdown-option-description">
                          {option.description}
                        </span>
                      ) : null}
                    </span>
                  </button>
                );
              })}
              {!visibleOptions.length ? (
                <div className="task-board-dropdown-empty" role="status">
                  没有匹配的项目
                </div>
              ) : null}
            </div>
          </div>,
          document.body,
        )
      : null;

  return (
    <>
      <button
        className={`task-board-dropdown-trigger ${className}`.trim()}
        ref={triggerRef}
        type="button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        title={triggerLabel}
        onClick={() => {
          if (open) {
            closeDropdown(false);
            return;
          }
          setSearchQuery("");
          setOpen(true);
        }}
      >
        <span className="task-board-dropdown-trigger-copy">
          {selectedOption?.iconId ? (
            <TaskBoardStatusIcon
              className="task-board-dropdown-status-icon"
              statusId={selectedOption.iconId}
            />
          ) : null}
          <span className="task-board-dropdown-label">{triggerLabel}</span>
        </span>
        {showChevron ? (
          <span className="task-board-dropdown-chevron">
            <svg
              aria-hidden="true"
              viewBox="0 0 16 16"
              width="14"
              height="14"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
            >
              <path
                d="m5 6 3 3 3-3"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </span>
        ) : null}
      </button>
      {menu}
    </>
  );
}

function TaskBoardCreateSettings({
  modelId,
  effortId,
  modelOptions,
  disabled,
  onModelChange,
  onEffortChange,
}: {
  modelId: string;
  effortId: string;
  modelOptions: CreateModelOption[];
  disabled: boolean;
  onModelChange: (modelId: string, effortId: string) => void;
  onEffortChange: (effortId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [submenuKind, setSubmenuKind] = useState<"model" | "effort" | "">("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const submenuRef = useRef<HTMLDivElement>(null);
  const submenuFocusRequestedRef = useRef(false);
  const selectedModel = modelOptions.find((option) => option.value === modelId);
  const effortOptions = selectedModel?.efforts?.length
    ? selectedModel.efforts
    : defaultEffortOptions;
  const selectedEffort = effortOptions.find(
    (option) => option.value === effortId,
  );
  const modelLabel = selectedModel?.label || modelId || "默认模型";
  const effortLabel = selectedEffort?.label || effortId || "中";

  const closeSettings = useCallback((restoreFocus: boolean) => {
    setSubmenuKind("");
    setOpen(false);
    if (restoreFocus) {
      window.requestAnimationFrame(() => {
        if (triggerRef.current?.isConnected) triggerRef.current.focus();
      });
    }
  }, []);

  const submenuOptions = useMemo<DropdownOption[]>(() => {
    if (submenuKind === "model") {
      const options = modelOptions.filter((option) => option.value.trim());
      return options.length
        ? options
        : [{ value: "", label: "暂无可用模型", disabled: true }];
    }
    if (submenuKind === "effort") {
      return effortOptions.length
        ? effortOptions
        : [{ value: "", label: "暂无可用强度", disabled: true }];
    }
    return [];
  }, [effortOptions, modelOptions, submenuKind]);

  useLayoutEffect(() => {
    if (!open) return;
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    if (!trigger || !menu) return;
    const triggerRect = trigger.getBoundingClientRect();
    menu.style.minWidth = "220px";
    const menuRect = menu.getBoundingClientRect();
    const viewportWidth = window.innerWidth || 1024;
    const viewportHeight = window.innerHeight || 768;
    const width = menuRect.width || 220;
    const height = menuRect.height || 0;
    const left = taskBoardDropdownLeft(
      triggerRect.left,
      width,
      viewportWidth,
    );
    const top = Math.max(8, triggerRect.top - 6 - height);
    menu.style.left = `${left}px`;
    menu.style.top = `${Math.min(viewportHeight - height - 8, top)}px`;
    menu.style.visibility = "visible";
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !submenuKind) return;
    const menu = menuRef.current;
    const submenu = submenuRef.current;
    if (!menu || !submenu) return;
    submenu.style.minWidth = "220px";
    const menuRect = menu.getBoundingClientRect();
    const parentButton = menu.querySelector<HTMLElement>(
      `[data-settings-kind="${submenuKind}"]`,
    );
    const parentRect = parentButton?.getBoundingClientRect() ?? menuRect;
    const submenuRect = submenu.getBoundingClientRect();
    const viewportWidth = window.innerWidth || 1024;
    const viewportHeight = window.innerHeight || 768;
    const width = submenuRect.width || 220;
    const height = submenuRect.height || 0;
    submenu.style.left = `${taskBoardCreateSubmenuLeft(
      menuRect.left,
      menuRect.right,
      width,
      viewportWidth,
    )}px`;
    submenu.style.top = `${taskBoardCreateSubmenuTop(
      parentRect.top,
      parentRect.height,
      height,
      viewportHeight,
    )}px`;
    submenu.style.visibility = "visible";
  }, [open, submenuKind, submenuOptions]);

  useEffect(() => {
    if (!open) return;
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    if (!trigger || !menu) return;

    const focusButtons = (container: HTMLElement | null) =>
      Array.from(container?.querySelectorAll<HTMLButtonElement>("button") ?? []).filter(
        (button) => !button.disabled,
      );
    const focusSelectedSubmenuOption = () => {
      const submenu = submenuRef.current;
      const selected = submenu?.querySelector<HTMLButtonElement>(
        'button[aria-checked="true"]:not(:disabled)',
      );
      (selected ?? focusButtons(submenu)[0])?.focus({ preventScroll: true });
    };
    const openSubmenu = (
      kind: "model" | "effort",
      focus: boolean,
    ) => {
      submenuFocusRequestedRef.current = focus;
      setSubmenuKind(kind);
    };
    const moveModalFocusFromTrigger = (shiftKey: boolean) => {
      const modal = trigger.closest(".task-board-modal");
      if (!(modal instanceof HTMLElement)) return;
      const focusable = taskBoardModalFocusableElements(modal);
      const triggerIndex = focusable.indexOf(trigger);
      const nextIndex = shiftKey
        ? triggerIndex <= 0
          ? focusable.length - 1
          : triggerIndex - 1
        : triggerIndex === focusable.length - 1
          ? 0
          : triggerIndex + 1;
      setSubmenuKind("");
      setOpen(false);
      window.requestAnimationFrame(() => focusable[nextIndex]?.focus());
    };
    const handleKeydown = (event: KeyboardEvent) => {
      const submenu = submenuRef.current;
      if (submenuKind && submenu) {
        const buttons = focusButtons(submenu);
        const current = buttons.indexOf(
          document.activeElement as HTMLButtonElement,
        );
        if (event.key === "Escape" || event.key === "ArrowLeft") {
          event.preventDefault();
          event.stopImmediatePropagation();
          setSubmenuKind("");
          window.requestAnimationFrame(() => {
            menu
              .querySelector<HTMLButtonElement>(
                `[data-settings-kind="${submenuKind}"]`,
              )
              ?.focus({ preventScroll: true });
          });
        } else if (
          event.key === "ArrowDown" ||
          event.key === "ArrowUp" ||
          event.key === "Home" ||
          event.key === "End"
        ) {
          if (!buttons.length) return;
          event.preventDefault();
          const next =
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? buttons.length - 1
                : event.key === "ArrowDown"
                  ? (current + 1 + buttons.length) % buttons.length
                  : (current - 1 + buttons.length) % buttons.length;
          buttons[next]?.focus({ preventScroll: true });
        } else if (event.key === "Enter" || event.key === " ") {
          const target = buttons[current >= 0 ? current : 0];
          if (!target) return;
          event.preventDefault();
          target.click();
        } else if (event.key === "Tab") {
          event.preventDefault();
          event.stopImmediatePropagation();
          moveModalFocusFromTrigger(event.shiftKey);
        }
        return;
      }

      const buttons = focusButtons(menu);
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
        closeSettings(true);
      } else if (event.key === "Tab") {
        event.preventDefault();
        event.stopImmediatePropagation();
        moveModalFocusFromTrigger(event.shiftKey);
      } else if (
        event.key === "ArrowDown" ||
        event.key === "ArrowUp" ||
        event.key === "Home" ||
        event.key === "End"
      ) {
        event.preventDefault();
        const next =
          event.key === "Home"
            ? 0
            : event.key === "End"
              ? buttons.length - 1
              : event.key === "ArrowDown"
                ? (current + 1 + buttons.length) % buttons.length
                : (current - 1 + buttons.length) % buttons.length;
        buttons[next]?.focus({ preventScroll: true });
      } else if (
        event.key === "Enter" ||
        event.key === " " ||
        event.key === "ArrowRight"
      ) {
        const target = buttons[current >= 0 ? current : 0];
        const kind = target?.dataset.settingsKind;
        if (kind !== "model" && kind !== "effort") return;
        event.preventDefault();
        openSubmenu(kind, true);
      }
    };
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (
        menu.contains(target) ||
        submenuRef.current?.contains(target) ||
        trigger.contains(target)
      ) {
        return;
      }
      closeSettings(false);
    };
    const handleViewportChange = (event: Event) => {
      const target = event.target;
      if (
        target instanceof Node &&
        (menu.contains(target) || submenuRef.current?.contains(target))
      ) {
        return;
      }
      closeSettings(false);
    };

    document.addEventListener("keydown", handleKeydown, true);
    document.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);
    if (!submenuKind) {
      window.requestAnimationFrame(() =>
        focusButtons(menu)[0]?.focus({ preventScroll: true }),
      );
    } else if (submenuFocusRequestedRef.current) {
      submenuFocusRequestedRef.current = false;
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(focusSelectedSubmenuOption);
      });
    }
    return () => {
      document.removeEventListener("keydown", handleKeydown, true);
      document.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
    };
  }, [
    closeSettings,
    modelId,
    effortId,
    open,
    onEffortChange,
    onModelChange,
    submenuKind,
  ]);

  const selectSubmenuOption = (value: string) => {
    if (submenuKind === "model") {
      const nextModel =
        modelOptions.find((option) => option.value === value) ?? modelOptions[0];
      const nextEfforts = nextModel?.efforts?.length
        ? nextModel.efforts
        : defaultEffortOptions;
      const nextEffort = nextEfforts.some((option) => option.value === effortId)
        ? effortId
        : nextEfforts.find((option) => option.value === "medium")?.value ||
          nextEfforts[0]?.value ||
          "";
      closeSettings(true);
      onModelChange(value, nextEffort);
    } else if (submenuKind === "effort") {
      closeSettings(true);
      onEffortChange(value);
    }
  };

  const menu =
    open && document.body
      ? createPortal(
          <>
            <div
              className="task-board-dropdown-menu task-board-create-settings-menu"
              ref={menuRef}
              role="menu"
              aria-label="新会话模型设置"
              style={{ left: 8, top: 8, visibility: "hidden" }}
            >
              {[
                { kind: "model" as const, label: "模型", value: modelLabel },
                {
                  kind: "effort" as const,
                  label: "推理强度",
                  value: effortLabel,
                },
              ].map((item) => (
                <button
                  type="button"
                  role="menuitem"
                  aria-haspopup="menu"
                  aria-expanded={submenuKind === item.kind}
                  aria-label={`${item.label} ${item.value}`}
                  data-settings-kind={item.kind}
                  key={item.kind}
                  onClick={() => {
                    submenuFocusRequestedRef.current = true;
                    setSubmenuKind(item.kind);
                  }}
                  onPointerEnter={() => {
                    submenuFocusRequestedRef.current = false;
                    setSubmenuKind(item.kind);
                  }}
                >
                  <span className="task-board-create-settings-label">
                    {item.label}
                  </span>
                  <span className="task-board-create-settings-value">
                    {item.value}
                  </span>
                  <span className="task-board-create-settings-chevron">
                    <svg
                      aria-hidden="true"
                      viewBox="0 0 16 16"
                      width="14"
                      height="14"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.4"
                    >
                      <path
                        d="m6 4 4 4-4 4"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </span>
                </button>
              ))}
            </div>
            {submenuKind ? (
              <div
                className={`task-board-dropdown-menu task-board-create-${submenuKind}-menu`}
                ref={submenuRef}
                role="menu"
                aria-label={
                  submenuKind === "model"
                    ? "选择新会话模型"
                    : "选择新会话推理强度"
                }
                style={{ left: 8, top: 8, visibility: "hidden" }}
              >
                {submenuOptions.map((option) => {
                  const currentValue =
                    submenuKind === "model" ? modelId : effortId;
                  const selected = option.value === currentValue;
                  return (
                    <button
                      type="button"
                      role="menuitemradio"
                      aria-checked={selected}
                      aria-label={option.label}
                      data-value={option.value}
                      disabled={option.disabled}
                      key={option.value}
                      onClick={() => {
                        if (!option.disabled) selectSubmenuOption(option.value);
                      }}
                    >
                      <span className="task-board-dropdown-option-title">
                        {option.label}
                      </span>
                    </button>
                  );
                })}
              </div>
            ) : null}
          </>,
          document.body,
        )
      : null;

  return (
    <>
      <button
        className="task-board-create-model-trigger"
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`选择新会话模型与推理强度，当前 ${modelLabel}，${effortLabel}`}
        title={`模型 ${modelLabel}；推理强度 ${effortLabel}`}
        disabled={disabled}
        onClick={() => {
          setSubmenuKind("");
          setOpen((current) => !current);
        }}
      >
        <span className="task-board-create-model-trigger-label">
          {modelLabel}
        </span>
        <span className="task-board-create-effort-trigger-label">
          {effortLabel}
        </span>
      </button>
      {menu}
    </>
  );
}

export function TaskBoardApp() {
  const [snapshot, setSnapshot] = useState<BoardSnapshot>(emptySnapshot);
  const [catalog, setCatalog] = useState<Catalog>(emptyCatalog);
  const [catalogReady, setCatalogReady] = useState(false);
  const [appearance, setAppearance] = useState<TaskBoardAppearance | null>(null);
  const [detachConfirmation, setDetachConfirmation] =
    useState<DetachConfirmationState | null>(null);
  const [deleteTaskConfirmation, setDeleteTaskConfirmation] =
    useState<DeleteTaskConfirmationState | null>(null);
  const [boardManager, setBoardManager] = useState<BoardManagerState | null>(
    null,
  );
  const [conversationStatuses, setConversationStatuses] = useState<
    Map<string, ConversationRuntimeStatus>
  >(new Map());
  const conversationReadSuppressionsRef = useRef(new Set<string>());
  const [query, setQuery] = useState("");
  const [projectFilter, setProjectFilter] = useState("");
  const [loading, setLoading] = useState(true);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [dragTaskId, setDragTaskId] = useState("");
  const [dropStatus, setDropStatus] = useState<TaskStatus | "">("");
  const [toast, setToast] = useState("");
  const appearanceRequestRef = useRef<Promise<void> | null>(null);
  const appearanceMountedRef = useRef(false);
  const submitEditorBusyRef = useRef(false);
  const nativeCreateRecoveryAttemptedRef = useRef(false);
  const statusDefinitions = useMemo(
    () => taskBoardStatusDefinitions(snapshot),
    [snapshot],
  );

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.setTimeout(() => {
      setToast((current) => (current === message ? "" : current));
    }, 3600);
  }, []);

  const applySnapshotResponse = useCallback(
    (response: BoardResponse, fallback: string) => {
      if (
        (response.status === "ok" || response.status === "conflict") &&
        Array.isArray(response.tasks)
      ) {
        setSnapshot({
          schemaVersion: response.schemaVersion ?? 1,
          revision: response.revision ?? 0,
          boards: taskBoardManagedBoards(response.boards),
          tasks: response.tasks,
        });
      }
      if (response.status !== "ok") {
        showToast(response.message || fallback);
        return false;
      }
      return true;
    },
    [showToast],
  );

  const applyTaskInitialStatus = useCallback(
    async (
      taskId: string,
      initialStatus: TaskStatus,
      currentSnapshot: BoardSnapshot,
    ) => {
      const task = currentSnapshot.tasks.find(
        (candidate) => candidate.id === taskId,
      );
      if (!task || initialStatus === "new" || task.status === initialStatus) {
        return currentSnapshot;
      }
      if (
        !taskBoardStatusDefinitions(currentSnapshot).some(
          (status) => status.id === initialStatus,
        )
      ) {
        showToast("初始看板已不存在，任务已保留在未分配");
        return currentSnapshot;
      }
      const targetCount = currentSnapshot.tasks.filter(
        (candidate) => candidate.status === initialStatus,
      ).length;
      const moved = await invoke<BoardResponse>("task_board_move_task", {
        request: {
          taskId,
          toStatus: initialStatus,
          targetIndex: targetCount,
          expectedRevision: currentSnapshot.revision,
        },
      });
      const movedSnapshot = taskBoardSnapshotFromResponse(moved);
      if (movedSnapshot) setSnapshot(movedSnapshot);
      if (moved.status !== "ok") {
        showToast("任务已创建，但初始状态设置失败，可在看板中手动移动");
        return movedSnapshot ?? currentSnapshot;
      }
      return movedSnapshot ?? currentSnapshot;
    },
    [showToast],
  );

  const attemptNativeCreateRecovery = useCallback(
    async (loadedSnapshot: BoardSnapshot) => {
      if (nativeCreateRecoveryAttemptedRef.current) return;
      nativeCreateRecoveryAttemptedRef.current = true;
      const record = readNativeCreateRecovery();
      if (!record) return;

      let effectiveSnapshot = loadedSnapshot;
      if (!taskBoardRecoveryAlreadyApplied(record, effectiveSnapshot)) {
        const command =
          record.kind === "attach-conversation"
            ? "task_board_attach_conversations"
            : "task_board_create_task";
        const request =
          record.kind === "attach-conversation"
            ? {
                taskId: record.targetTaskId || record.taskId,
                expectedRevision: effectiveSnapshot.revision,
                sessionIds: [record.sessionId],
              }
            : {
                taskId: record.taskId,
                expectedRevision: effectiveSnapshot.revision,
                title: record.title,
                project: taskProjectRef(record.project),
                sessionIds: [record.sessionId],
              };
        let result: BoardResponse;
        try {
          result = await invokeSessionMutationWithRetry(
            command,
            request,
            setCatalog,
          );
        } catch (error) {
          showToast(
            messageFromError(
              error,
              record.kind === "attach-conversation"
                ? "恢复关联会话失败，将在下次打开任务看板时重试"
                : "恢复任务失败，将在下次打开任务看板时重试",
            ),
          );
          return;
        }
        const responseSnapshot = taskBoardSnapshotFromResponse(result);
        if (responseSnapshot) {
          effectiveSnapshot = responseSnapshot;
          setSnapshot(responseSnapshot);
        }
        if (
          result.status !== "ok" &&
          !taskBoardRecoveryAlreadyApplied(record, effectiveSnapshot)
        ) {
          showToast(
            result.message ||
              (record.kind === "attach-conversation"
                ? "会话尚未恢复到任务，将在下次打开任务看板时重试"
                : "会话对应任务尚未恢复，将在下次打开任务看板时重试"),
          );
          return;
        }
      }

      clearNativeCreateRecovery();
      if (record.kind === "create-task") {
        effectiveSnapshot = await applyTaskInitialStatus(
          record.taskId,
          record.initialStatus,
          effectiveSnapshot,
        );
        setSnapshot(effectiveSnapshot);
      }
      showToast(
        record.kind === "attach-conversation"
          ? "已恢复新会话与任务的关联"
          : "已恢复新会话对应的任务",
      );
    },
    [applyTaskInitialStatus, showToast],
  );

  const refresh = useCallback(
    async () => {
      try {
        const [board, sessions] = await Promise.all([
          invoke<BoardResponse>("task_board_load_snapshot"),
          invoke<BoardResponse>("task_board_load_catalog"),
        ]);
        applySnapshotResponse(board, "任务看板读取失败");
        const loadedSnapshot = taskBoardSnapshotFromResponse(board);
        if (sessions.status === "ok") {
          setCatalog({
            projects: sessions.projects ?? [],
            sessions: sessions.sessions ?? [],
            warnings: sessions.warnings ?? [],
          });
          setCatalogReady(true);
        } else {
          setCatalogReady(false);
          showToast(sessions.message || "会话目录读取失败");
        }
        if (board.status === "ok" && loadedSnapshot && sessions.status === "ok") {
          await attemptNativeCreateRecovery(loadedSnapshot);
        }
      } catch (error) {
        setCatalogReady(false);
        showToast(messageFromError(error, "任务看板读取失败"));
      } finally {
        setLoading(false);
      }
    },
    [applySnapshotResponse, attemptNativeCreateRecovery, showToast],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const refreshAppearance = useCallback(() => {
    if (appearanceRequestRef.current) return appearanceRequestRef.current;
    const request = invoke<BoardResponse>("task_board_load_host_appearance")
      .then((result) => {
        if (
          !appearanceMountedRef.current ||
          result.status !== "ok" ||
          !result.appearance
        ) {
          return;
        }
        const signature = String(result.appearance.signature || "");
        setAppearance((current) =>
          signature && current?.signature === signature
            ? current
            : result.appearance ?? current,
        );
      })
      .catch(() => undefined);
    appearanceRequestRef.current = request;
    void request.finally(() => {
      if (appearanceRequestRef.current === request) {
        appearanceRequestRef.current = null;
      }
    });
    return request;
  }, []);

  useEffect(() => {
    appearanceMountedRef.current = true;
    const refreshWhenVisible = () => {
      if (!document.hidden) void refreshAppearance();
    };
    void refreshAppearance();
    window.addEventListener("focus", refreshWhenVisible);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    const timer = window.setInterval(refreshWhenVisible, appearanceRefreshIntervalMs);
    return () => {
      appearanceMountedRef.current = false;
      window.removeEventListener("focus", refreshWhenVisible);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.clearInterval(timer);
    };
  }, [refreshAppearance]);

  useEffect(() => {
    const styles = taskBoardAppearanceStyle(appearance);
    if (!styles) return undefined;
    const root = document.documentElement;
    const previous = new Map<string, string>();
    Object.entries(styles).forEach(([name, value]) => {
      if (!name.startsWith("--") || typeof value !== "string") return;
      previous.set(name, root.style.getPropertyValue(name));
      root.style.setProperty(name, value);
    });
    return () => {
      previous.forEach((value, name) => {
        if (value) {
          root.style.setProperty(name, value);
        } else {
          root.style.removeProperty(name);
        }
      });
    };
  }, [appearance]);

  useEffect(() => {
    const refreshWhenActive = () => {
      if (!document.hidden) void refresh();
    };
    window.addEventListener("focus", refreshWhenActive);
    document.addEventListener("visibilitychange", refreshWhenActive);
    return () => {
      window.removeEventListener("focus", refreshWhenActive);
      document.removeEventListener("visibilitychange", refreshWhenActive);
    };
  }, [refresh]);

  useEffect(() => {
    const projectCwd = editor?.projectCwd || "";
    const project = catalog.projects.find((candidate) => candidate.cwd === projectCwd);
    if (!editor || !project) return;
    let cancelled = false;
    setEditor((current) => {
      if (!current || current.projectCwd !== projectCwd) return current;
      return {
        ...current,
        nativeCreateAvailable: null,
        nativeCreateMessage: "正在确认 Codex 新会话能力…",
      };
    });
    void invoke<BoardResponse>("task_board_probe_host", {
      project: taskProjectRef(project),
    })
      .then((result) => {
        if (cancelled) return;
        setEditor((current) => {
          if (!current || current.projectCwd !== projectCwd) return current;
          const nativeCreateAvailable =
            result.status === "ok" && result.canStart === true;
          return {
            ...current,
            nativeCreateAvailable,
            nativeCreateMessage: nativeCreateAvailable
              ? ""
              : result.message || "当前项目暂不支持新建关联会话",
          };
        });
      })
      .catch(() => {
        if (cancelled) return;
        setEditor((current) => {
          if (!current || current.projectCwd !== projectCwd) return current;
          return {
            ...current,
            nativeCreateAvailable: false,
            nativeCreateMessage: "暂时无法连接 Codex，新建时将再次检查",
          };
        });
      });
    return () => {
      cancelled = true;
    };
  }, [catalog.projects, editor?.projectCwd, editor?.targetTask?.id]);

  const catalogSessionIds = useMemo(() => {
    const sessionIds = new Set<string>();
    catalog.sessions.forEach((session) => {
      const key = normalizeSessionId(session.sessionId);
      if (key) sessionIds.add(key);
    });
    return sessionIds;
  }, [catalog.sessions]);

  const catalogSessionAliases = useMemo(() => {
    const aliases = new Map<string, string[]>();
    catalog.sessions.forEach((session) => {
      const sessionId = normalizeSessionId(session.sessionId);
      if (!sessionId || !Array.isArray(session.sessionAliases)) return;
      const values = Array.from(
        new Set(
          session.sessionAliases
            .map((alias) => String(alias || "").trim())
            .filter(Boolean),
        ),
      );
      if (values.length) aliases.set(sessionId, values);
    });
    return aliases;
  }, [catalog.sessions]);

  const linkedConversations = useMemo(() => {
    const conversations = new Map<string, TaskConversation>();
    snapshot.tasks.forEach((task) => {
      task.conversations.forEach((conversation) => {
        const key = normalizeSessionId(conversation.sessionId);
        if (
          key &&
          taskBoardConversationShouldDisplay(
            conversation,
            catalogSessionIds,
            catalogReady,
          ) &&
          !conversations.has(key)
        ) {
          conversations.set(key, conversation);
        }
      });
    });
    return Array.from(conversations.values());
  }, [catalogReady, catalogSessionIds, snapshot.tasks]);

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    const refreshStatuses = async () => {
      const activeKeys = new Set(
        linkedConversations.map((conversation) =>
          normalizeSessionId(conversation.sessionId),
        ),
      );
      conversationReadSuppressionsRef.current.forEach((key) => {
        if (!activeKeys.has(key)) {
          conversationReadSuppressionsRef.current.delete(key);
        }
      });
      setConversationStatuses((current) => {
        const next = new Map<string, ConversationRuntimeStatus>();
        linkedConversations.forEach((conversation) => {
          const key = normalizeSessionId(conversation.sessionId);
          const existing = current.get(key);
          next.set(
            key,
            existing ?? {
              sessionId: conversation.sessionId,
              known: false,
              checking: true,
              isRunning: false,
              unread: false,
            },
          );
        });
        return next;
      });
      if (!linkedConversations.length) return;
      let nextDelay = 10_000;
      try {
        const result = await invoke<BoardResponse>(
          "task_board_load_conversation_statuses",
          {
            request: {
              conversations: linkedConversations.map((conversation) => {
                const sessionKey = normalizeSessionId(conversation.sessionId);
                return {
                  sessionId: conversation.sessionId,
                  title: conversation.title,
                  sessionAliases: catalogSessionAliases.get(sessionKey) ?? [],
                };
              }),
            },
          },
        );
        if (cancelled) return;
        if (result.status === "ok" && Array.isArray(result.statuses)) {
          const statuses = new Map<string, ConversationRuntimeStatus>();
          result.statuses.forEach((status) => {
            const key = normalizeSessionId(status.sessionId);
            if (!key || !activeKeys.has(key)) return;
            const readSuppressed = conversationReadSuppressionsRef.current.has(key);
            if (readSuppressed && !status.unread) {
              conversationReadSuppressionsRef.current.delete(key);
            }
            statuses.set(
              key,
              readSuppressed && status.unread ? { ...status, unread: false } : status,
            );
          });
          linkedConversations.forEach((conversation) => {
            const key = normalizeSessionId(conversation.sessionId);
            if (!statuses.has(key)) {
              statuses.set(key, {
                sessionId: conversation.sessionId,
                known: false,
                checking: false,
                isRunning: false,
                unread: false,
              });
            }
          });
          setConversationStatuses(statuses);
          if (result.statuses.some((status) => status.isRunning)) {
            nextDelay = 2_500;
          }
        } else {
          setConversationStatuses((current) => {
            const next = new Map(current);
            next.forEach((status, key) => {
              if (!activeKeys.has(key)) {
                next.delete(key);
                return;
              }
              next.set(key, { ...status, known: false, checking: false });
            });
            return next;
          });
        }
      } catch {
        if (cancelled) return;
        setConversationStatuses((current) => {
          const next = new Map(current);
          next.forEach((status, key) => {
            if (!activeKeys.has(key)) {
              next.delete(key);
              return;
            }
            next.set(key, { ...status, known: false, checking: false });
          });
          return next;
        });
      }
      if (!cancelled) {
        timer = window.setTimeout(refreshStatuses, nextDelay);
      }
    };
    void refreshStatuses();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [catalogSessionAliases, linkedConversations]);

  useEffect(() => {
    if (!editor) return;
    let cancelled = false;
    void invoke<BoardResponse>("task_board_load_create_options")
      .then((result) => {
        if (cancelled || result.status !== "ok") return;
        setEditor((current) => {
          if (!current) return current;
          const modelOptions = result.models?.length
            ? result.models
            : fallbackModelOptions;
          const requestedModelId = String(result.modelId || "");
          const modelId =
            !current.modelSelectionTouched &&
            modelOptions.some((option) => option.value === requestedModelId)
              ? requestedModelId
              : modelOptions.some((option) => option.value === current.modelId)
                ? current.modelId
                : modelOptions[0]?.value || "";
          const effortOptions =
            modelOptions.find((option) => option.value === modelId)?.efforts ??
            defaultEffortOptions;
          const requestedEffortId = String(result.effortId || "");
          const effortId =
            !current.modelSelectionTouched &&
            effortOptions.some((option) => option.value === requestedEffortId)
              ? requestedEffortId
              : effortOptions.some(
                    (option) => option.value === current.effortId,
                  )
                ? current.effortId
                : effortOptions.find((option) => option.value === "medium")?.value ||
                  effortOptions[0]?.value ||
                  "";
          return { ...current, modelId, effortId, modelOptions };
        });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [editor?.targetTask?.id, Boolean(editor)]);

  const catalogSessionTitles = useMemo(() => {
    const titles = new Map<string, string>();
    catalog.sessions.forEach((session) => {
      const sessionId = normalizeSessionId(session.sessionId);
      const title = session.title.trim();
      if (sessionId && title) titles.set(sessionId, title);
    });
    return titles;
  }, [catalog.sessions]);

  const catalogProjectsByCwd = useMemo(() => {
    const projects = new Map<string, CatalogProject>();
    catalog.projects.forEach((project) => projects.set(project.cwd, project));
    return projects;
  }, [catalog.projects]);

  const tasksWithCatalogMetadata = useMemo(
    () =>
      snapshot.tasks.map((task) => {
        let changed = false;
        const catalogProject = catalogProjectsByCwd.get(task.project.cwd);
        const project =
          catalogProject && catalogProject.label !== task.project.label
            ? {
                cwd: task.project.cwd,
                label: catalogProject.label,
              }
            : task.project;
        if (project !== task.project) changed = true;
        const visibleConversations = task.conversations.filter((conversation) =>
          taskBoardConversationShouldDisplay(
            conversation,
            catalogSessionIds,
            catalogReady,
          ),
        );
        if (visibleConversations.length !== task.conversations.length) {
          changed = true;
        }
        const conversations = visibleConversations.map((conversation) => {
          const title =
            catalogSessionTitles.get(normalizeSessionId(conversation.sessionId)) ||
            conversation.title;
          if (title === conversation.title) return conversation;
          changed = true;
          return { ...conversation, title };
        });
        return changed ? { ...task, project, conversations } : task;
      }),
    [
      catalogProjectsByCwd,
      catalogReady,
      catalogSessionIds,
      catalogSessionTitles,
      snapshot.tasks,
    ],
  );

  const visibleTasks = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase("zh-CN");
    return tasksWithCatalogMetadata.filter((task) => {
      if (projectFilter && task.project.cwd !== projectFilter) return false;
      if (!normalizedQuery) return true;
      const searchable = [
        task.title,
        task.project.label,
        task.project.cwd,
        ...task.conversations.flatMap((conversation) => [
          conversation.title,
          conversation.sessionId,
        ]),
      ]
        .join("\n")
        .toLocaleLowerCase("zh-CN");
      return searchable.includes(normalizedQuery);
    });
  }, [projectFilter, query, tasksWithCatalogMetadata]);

  const visibleStatusDefinitions = useMemo(
    () =>
      taskBoardVisibleStatusDefinitions(
        statusDefinitions,
        visibleTasks,
      ),
    [statusDefinitions, visibleTasks],
  );
  const unassignedDropActive =
    Boolean(dragTaskId) &&
    !visibleStatusDefinitions.some((status) => status.id === "new");

  const columnTasks = useCallback(
    (status: TaskStatus) =>
      visibleTasks
        .filter((task) => task.status === status)
        .sort((left, right) => left.order - right.order),
    [visibleTasks],
  );

  const moveTask = useCallback(
    async (task: Task, status: TaskStatus, targetIndex?: number) => {
      const targetTasks = snapshot.tasks
        .filter((candidate) => candidate.status === status && candidate.id !== task.id)
        .sort((left, right) => left.order - right.order);
      const index = targetIndex ?? targetTasks.length;
      try {
        const result = await invoke<BoardResponse>("task_board_move_task", {
          request: {
            taskId: task.id,
            toStatus: status,
            targetIndex: index,
            expectedRevision: snapshot.revision,
          },
        });
        applySnapshotResponse(result, "任务移动失败");
      } catch (error) {
        showToast(messageFromError(error, "任务移动失败"));
      }
    },
    [applySnapshotResponse, showToast, snapshot.revision, snapshot.tasks],
  );

  const dropTaskIntoStatus = useCallback(
    (event: DragEvent<HTMLElement>, status: TaskStatus) => {
      event.preventDefault();
      setDropStatus("");
      const taskId =
        dragTaskId || event.dataTransfer.getData("text/plain").trim();
      const task = snapshot.tasks.find((candidate) => candidate.id === taskId);
      setDragTaskId("");
      if (task && task.status !== status) {
        void moveTask(task, status);
      }
    },
    [dragTaskId, moveTask, snapshot.tasks],
  );

  const openConversation = useCallback(
    async (conversation: TaskConversation, project: TaskProject) => {
      const sessionKey = normalizeSessionId(conversation.sessionId);
      const wasUnread = conversationStatuses.get(sessionKey)?.unread === true;
      if (wasUnread) {
        conversationReadSuppressionsRef.current.add(sessionKey);
        setConversationStatuses((current) => {
          const status = current.get(sessionKey);
          if (!status?.unread) return current;
          const next = new Map(current);
          next.set(sessionKey, { ...status, unread: false });
          return next;
        });
      }
      const restoreUnread = () => {
        if (!wasUnread) return;
        conversationReadSuppressionsRef.current.delete(sessionKey);
        setConversationStatuses((current) => {
          const status = current.get(sessionKey);
          if (!status || status.unread) return current;
          const next = new Map(current);
          next.set(sessionKey, { ...status, unread: true });
          return next;
        });
      };
      try {
        const result = await invoke<BoardResponse>("task_board_open_session", {
          request: {
            ...conversation,
            projectLabel: project.label,
            sessionAliases: catalogSessionAliases.get(sessionKey) ?? [],
          },
        });
        if (result.status !== "ok") {
          restoreUnread();
          showToast(result.message || "无法打开关联会话");
        }
      } catch (error) {
        restoreUnread();
        showToast(messageFromError(error, "无法打开关联会话"));
      }
    },
    [catalogSessionAliases, conversationStatuses, showToast],
  );

  const closeEditor = useCallback(() => {
    setEditor((current) => (current?.busy ? current : null));
  }, []);

  const requestDetachConfirmation = useCallback(
    (task: Task, conversation: TaskConversation) => {
      setDeleteTaskConfirmation(null);
      setDetachConfirmation({
        task,
        conversation,
        busy: false,
        feedback: "",
      });
    },
    [],
  );

  const closeDetachConfirmation = useCallback(() => {
    setDetachConfirmation((current) => (current?.busy ? current : null));
  }, []);

  const requestDeleteTaskConfirmation = useCallback((task: Task) => {
    setDetachConfirmation(null);
    setDeleteTaskConfirmation({
      task,
      busy: false,
      feedback: "",
    });
  }, []);

  const closeDeleteTaskConfirmation = useCallback(() => {
    setDeleteTaskConfirmation((current) => (current?.busy ? current : null));
  }, []);

  const openBoardManager = useCallback(() => {
    setDetachConfirmation(null);
    setDeleteTaskConfirmation(null);
    setEditor(null);
    setBoardManager({
      label: "",
      busy: false,
      busyKind: "",
      busyBoardId: "",
      feedback: "",
      pendingDeleteId: "",
    });
  }, []);

  const closeBoardManager = useCallback(() => {
    setBoardManager((current) => (current?.busy ? current : null));
  }, []);

  const createManagedBoard = useCallback(async () => {
    const current = boardManager;
    if (!current || current.busy) return;
    const label = current.label.trim();
    const labelLength = Array.from(label).length;
    if (!label || labelLength > 40) {
      setBoardManager({
        ...current,
        feedback: "看板名称必须为 1 到 40 个字符",
      });
      return;
    }
    const boardId = crypto.randomUUID();
    setBoardManager({
      ...current,
      busy: true,
      busyKind: "create",
      busyBoardId: "",
      feedback: "",
    });
    let expectedRevision = snapshot.revision;
    try {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await invoke<BoardResponse>("task_board_create_board", {
          request: {
            boardId,
            expectedRevision,
            label,
          },
        });
        const nextSnapshot = taskBoardSnapshotFromResponse(result);
        if (nextSnapshot) setSnapshot(nextSnapshot);
        if (result.status === "ok") {
          setBoardManager({
            label: "",
            busy: false,
            busyKind: "",
            busyBoardId: "",
            feedback: "",
            pendingDeleteId: "",
          });
          showToast(`已添加看板“${label}”`);
          return;
        }
        if (
          (result.status === "conflict" || result.code === "revision_conflict") &&
          nextSnapshot &&
          attempt === 0
        ) {
          expectedRevision = nextSnapshot.revision;
          continue;
        }
        setBoardManager((state) =>
          state
            ? {
                ...state,
                busy: false,
                busyKind: "",
                busyBoardId: "",
                feedback: taskBoardBoardFailureMessage(result, "添加看板失败"),
              }
            : state,
        );
        return;
      }
    } catch (error) {
      setBoardManager((state) =>
        state
          ? {
              ...state,
              busy: false,
              busyKind: "",
              busyBoardId: "",
              feedback: messageFromError(error, "添加看板失败"),
            }
          : state,
      );
    }
  }, [boardManager, showToast, snapshot.revision]);

  const confirmDeleteBoard = useCallback(async () => {
    const current = boardManager;
    const boardId = current?.pendingDeleteId || "";
    if (!current || current.busy || !boardId) return;
    const board = snapshot.boards.find((candidate) => candidate.id === boardId);
    setBoardManager({
      ...current,
      busy: true,
      busyKind: "delete",
      busyBoardId: boardId,
      feedback: "",
    });
    let expectedRevision = snapshot.revision;
    try {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await invoke<BoardResponse>("task_board_delete_board", {
          request: {
            boardId,
            expectedRevision,
          },
        });
        const nextSnapshot = taskBoardSnapshotFromResponse(result);
        if (nextSnapshot) setSnapshot(nextSnapshot);
        const boardStillExists =
          nextSnapshot?.boards.some((candidate) => candidate.id === boardId) ??
          true;
        if (result.status === "ok" || !boardStillExists) {
          setBoardManager({
            ...current,
            busy: false,
            busyKind: "",
            busyBoardId: "",
            feedback: "",
            pendingDeleteId: "",
          });
          showToast(
            board
              ? `已删除看板“${board.label}”，原任务已移至未分配`
              : "看板已删除",
          );
          return;
        }
        if (
          (result.status === "conflict" || result.code === "revision_conflict") &&
          nextSnapshot &&
          attempt === 0
        ) {
          expectedRevision = nextSnapshot.revision;
          continue;
        }
        setBoardManager((state) =>
          state
            ? {
                ...state,
                busy: false,
                busyKind: "",
                busyBoardId: "",
                feedback: taskBoardBoardFailureMessage(result, "删除看板失败"),
              }
            : state,
        );
        return;
      }
    } catch (error) {
      setBoardManager((state) =>
        state
          ? {
              ...state,
              busy: false,
              busyKind: "",
              busyBoardId: "",
              feedback: messageFromError(error, "删除看板失败"),
            }
          : state,
      );
    }
  }, [boardManager, showToast, snapshot.boards, snapshot.revision]);

  const renameManagedBoard = useCallback(
    async (boardId: string, rawLabel: string) => {
      const current = boardManager;
      if (!current || current.busy) return false;
      const board = snapshot.boards.find((candidate) => candidate.id === boardId);
      if (!board) {
        setBoardManager({
          ...current,
          feedback: "看板不存在或已被删除",
        });
        return false;
      }
      const label = rawLabel.trim();
      const labelLength = Array.from(label).length;
      if (!label || labelLength > 40) {
        setBoardManager({
          ...current,
          feedback: "看板名称必须为 1 到 40 个字符",
        });
        return false;
      }
      if (
        snapshot.boards.some(
          (candidate) =>
            candidate.id !== boardId &&
            candidate.label.toLocaleLowerCase("zh-CN") ===
              label.toLocaleLowerCase("zh-CN"),
        )
      ) {
        setBoardManager({
          ...current,
          feedback: "看板名称已存在",
        });
        return false;
      }
      if (board.label === label) return true;

      setBoardManager({
        ...current,
        busy: true,
        busyKind: "rename",
        busyBoardId: boardId,
        feedback: "",
        pendingDeleteId: "",
      });
      let expectedRevision = snapshot.revision;
      try {
        for (let attempt = 0; attempt < 2; attempt += 1) {
          const result = await invoke<BoardResponse>("task_board_rename_board", {
            request: {
              boardId,
              expectedRevision,
              label,
            },
          });
          const nextSnapshot = taskBoardSnapshotFromResponse(result);
          if (nextSnapshot) setSnapshot(nextSnapshot);
          const renamedBoard = nextSnapshot?.boards.find(
            (candidate) => candidate.id === boardId,
          );
          if (
            (result.status === "ok" && nextSnapshot) ||
            renamedBoard?.label === label
          ) {
            showToast(`已将看板“${board.label}”改名为“${label}”`);
            return true;
          }
          if (
            (result.status === "conflict" ||
              result.code === "revision_conflict") &&
            nextSnapshot &&
            attempt === 0
          ) {
            expectedRevision = nextSnapshot.revision;
            continue;
          }
          setBoardManager((state) =>
            state
              ? {
                  ...state,
                  feedback: taskBoardBoardFailureMessage(
                    result,
                    "看板改名失败",
                  ),
                }
              : state,
          );
          return false;
        }
      } catch (error) {
        setBoardManager((state) =>
          state
            ? {
                ...state,
                feedback: messageFromError(error, "看板改名失败"),
              }
            : state,
        );
      } finally {
        setBoardManager((state) =>
          state
            ? {
                ...state,
                busy: false,
                busyKind: "",
                busyBoardId: "",
              }
            : state,
        );
      }
      return false;
    },
    [boardManager, showToast, snapshot.boards, snapshot.revision],
  );

  const moveManagedBoard = useCallback(
    async (boardId: string, targetIndex: number) => {
      const current = boardManager;
      if (!current || current.busy) return false;
      const currentIndex = snapshot.boards.findIndex(
        (candidate) => candidate.id === boardId,
      );
      if (currentIndex < 0) {
        setBoardManager({
          ...current,
          feedback: "看板不存在或已被删除",
        });
        return false;
      }
      const reorderedBoards = taskBoardMoveBoardDefinitions(
        snapshot.boards,
        boardId,
        targetIndex,
      );
      const normalizedTargetIndex = reorderedBoards.findIndex(
        (candidate) => candidate.id === boardId,
      );
      if (currentIndex === normalizedTargetIndex) return true;

      setBoardManager({
        ...current,
        busy: true,
        busyKind: "move",
        busyBoardId: boardId,
        feedback: "",
        pendingDeleteId: "",
      });
      let expectedRevision = snapshot.revision;
      try {
        for (let attempt = 0; attempt < 2; attempt += 1) {
          const result = await invoke<BoardResponse>("task_board_move_board", {
            request: {
              boardId,
              targetIndex: normalizedTargetIndex,
              expectedRevision,
            },
          });
          const nextSnapshot = taskBoardSnapshotFromResponse(result);
          if (nextSnapshot) setSnapshot(nextSnapshot);
          const appliedIndex =
            nextSnapshot?.boards.findIndex(
              (candidate) => candidate.id === boardId,
            ) ?? -1;
          if (
            (result.status === "ok" && nextSnapshot) ||
            appliedIndex === normalizedTargetIndex
          ) {
            showToast("已调整看板排序");
            return true;
          }
          if (
            (result.status === "conflict" ||
              result.code === "revision_conflict") &&
            nextSnapshot &&
            attempt === 0
          ) {
            expectedRevision = nextSnapshot.revision;
            continue;
          }
          setBoardManager((state) =>
            state
              ? {
                  ...state,
                  feedback: taskBoardBoardFailureMessage(
                    result,
                    "调整看板排序失败",
                  ),
                }
              : state,
          );
          return false;
        }
      } catch (error) {
        setBoardManager((state) =>
          state
            ? {
                ...state,
                feedback: messageFromError(error, "调整看板排序失败"),
              }
            : state,
        );
      } finally {
        setBoardManager((state) =>
          state
            ? {
                ...state,
                busy: false,
                busyKind: "",
                busyBoardId: "",
              }
            : state,
        );
      }
      return false;
    },
    [boardManager, showToast, snapshot.boards, snapshot.revision],
  );

  const detachConversation = useCallback(
    async (task: Task, conversation: TaskConversation) => {
      try {
        const result = await invoke<BoardResponse>("task_board_detach_conversations", {
          request: {
            taskId: task.id,
            expectedRevision: snapshot.revision,
            sessionIds: [conversation.sessionId],
          },
        });
        if (applySnapshotResponse(result, "移除会话失败")) {
          showToast("已从任务中移除会话");
          return "";
        }
        return result.message || "移除会话失败";
      } catch (error) {
        return messageFromError(error, "移除会话失败");
      }
    },
    [applySnapshotResponse, showToast, snapshot.revision],
  );

  const confirmDetachConversation = useCallback(async () => {
    const pending = detachConfirmation;
    if (!pending || pending.busy) return;
    setDetachConfirmation({ ...pending, busy: true, feedback: "" });
    const feedback = await detachConversation(pending.task, pending.conversation);
    if (!feedback) {
      setDetachConfirmation(null);
      return;
    }
    setDetachConfirmation((current) =>
      current?.task.id === pending.task.id &&
      current.conversation.sessionId === pending.conversation.sessionId
        ? { ...current, busy: false, feedback }
        : current,
    );
  }, [detachConfirmation, detachConversation]);

  const confirmDeleteTask = useCallback(async () => {
    const pending = deleteTaskConfirmation;
    if (!pending || pending.busy) return;
    setDeleteTaskConfirmation({ ...pending, busy: true, feedback: "" });
    let expectedRevision = snapshot.revision;
    try {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await invoke<BoardResponse>("task_board_delete_task", {
          request: {
            taskId: pending.task.id,
            expectedRevision,
          },
        });
        const nextSnapshot = taskBoardSnapshotFromResponse(result);
        if (nextSnapshot) setSnapshot(nextSnapshot);
        const taskStillExists =
          nextSnapshot?.tasks.some((task) => task.id === pending.task.id) ?? true;
        if (result.status === "ok" || !taskStillExists) {
          setDeleteTaskConfirmation(null);
          showToast("任务已从看板删除");
          window.requestAnimationFrame(() => {
            document.querySelector<HTMLElement>(".task-board-scroll")?.focus();
          });
          return;
        }
        if (
          (result.status === "conflict" || result.code === "revision_conflict") &&
          nextSnapshot &&
          attempt === 0
        ) {
          expectedRevision = nextSnapshot.revision;
          continue;
        }
        setDeleteTaskConfirmation((current) =>
          current?.task.id === pending.task.id
            ? {
                ...current,
                busy: false,
                feedback: taskBoardDeleteFailureMessage(result),
              }
            : current,
        );
        return;
      }
    } catch (error) {
      setDeleteTaskConfirmation((current) =>
        current?.task.id === pending.task.id
          ? {
              ...current,
              busy: false,
              feedback: messageFromError(error, "删除任务失败"),
            }
          : current,
      );
    }
  }, [deleteTaskConfirmation, showToast, snapshot.revision]);

  const openEditor = useCallback(
    (task: Task | null = null) => {
      const defaultProject =
        task?.project.cwd || projectFilter || catalog.projects[0]?.cwd || "";
      const attachedSessionIds = new Set(
        task?.conversations.map((conversation) =>
          normalizeSessionId(conversation.sessionId),
        ) ?? [],
      );
      const defaultSession = catalog.sessions
        .filter(
          (session) =>
            session.cwd === defaultProject &&
            !attachedSessionIds.has(normalizeSessionId(session.sessionId)),
        )
        .sort(
          (left, right) =>
            Number(right.updatedAtMs || 0) - Number(left.updatedAtMs || 0),
        )[0];
      const title = task?.title || "";
      const project =
        task?.project ||
        catalog.projects.find((candidate) => candidate.cwd === defaultProject) || {
          cwd: defaultProject,
          label: defaultProject,
        };
      const initialStatus: TaskStatus = "new";
      const kind: NativeCreateRecoveryKind = task
        ? "attach-conversation"
        : "create-task";
      const taskId = task?.id || taskBoardCreateTaskId();
      setEditor({
        targetTask: task,
        taskId,
        semanticKey: taskBoardRecoverySemanticKey(
          kind,
          title,
          project,
          initialStatus,
          task?.id || "",
        ),
        mode: "existing",
        title,
        projectCwd: defaultProject,
        initialStatus,
        selectedSessionIds: defaultSession ? [defaultSession.sessionId] : [],
        instruction: "",
        busy: false,
        feedback: "",
        nativeCreateAvailable: null,
        nativeCreateMessage: "正在确认 Codex 新会话能力…",
        modelId: "",
        effortId: "medium",
        modelOptions: fallbackModelOptions,
        modelSelectionTouched: false,
      });
    },
    [catalog.projects, catalog.sessions, projectFilter],
  );

  const submitEditor = useCallback(async () => {
    if (submitEditorBusyRef.current) return;
    submitEditorBusyRef.current = true;
    try {
      if (!editor || editor.busy) return;
      const project = catalog.projects.find(
        (candidate) => candidate.cwd === editor.projectCwd,
      );
      if (!project) {
        setEditor({ ...editor, feedback: "请选择项目" });
        return;
      }
      const title = editor.title.trim();
      if (!editor.targetTask && !title) {
        setEditor({ ...editor, feedback: "请输入任务名称" });
        return;
      }
      if (
        editor.mode === "existing" &&
        editor.selectedSessionIds.length === 0
      ) {
        setEditor({ ...editor, feedback: "至少选择一个会话" });
        return;
      }
      if (editor.mode === "new" && !editor.instruction.trim()) {
        setEditor({ ...editor, feedback: "请输入首条指令" });
        return;
      }

      const kind: NativeCreateRecoveryKind = editor.targetTask
        ? "attach-conversation"
        : "create-task";
      const targetTaskId = editor.targetTask?.id || "";
      const initialStatus = editor.targetTask ? "new" : editor.initialStatus;
      const semanticKey = taskBoardRecoverySemanticKey(
        kind,
        title,
        project,
        initialStatus,
        targetTaskId,
      );
      let taskId = editor.targetTask?.id || editor.taskId;
      if (
        !editor.targetTask &&
        (editor.semanticKey !== semanticKey ||
          !taskBoardCreateTaskIdIsValid(taskId))
      ) {
        taskId = taskBoardCreateTaskId();
      }

      let recovery =
        editor.mode === "new" ? readNativeCreateRecovery() : null;
      const matchingRecovery =
        recovery?.kind === kind &&
        recovery.semanticKey === semanticKey &&
        (kind !== "attach-conversation" ||
          recovery.targetTaskId === targetTaskId);
      if (!matchingRecovery) recovery = null;
      if (recovery) taskId = recovery.taskId;

      let busyEditor: EditorState = {
        ...editor,
        taskId,
        semanticKey,
        busy: true,
        feedback: "",
      };
      setEditor(busyEditor);

      let sessionIds = editor.selectedSessionIds;
      if (editor.mode === "new" && recovery) {
        sessionIds = [recovery.sessionId];
      } else if (editor.mode === "new") {
        const hostProject = taskProjectRef(project);
        const probe = await invoke<BoardResponse>("task_board_probe_host", {
          project: hostProject,
        });
        if (probe.status !== "ok" || probe.canStart !== true) {
          setEditor({
            ...busyEditor,
            busy: false,
            nativeCreateAvailable: false,
            nativeCreateMessage:
              probe.message || "当前项目暂不支持新建关联会话",
            feedback: probe.message || "当前项目暂不支持新建会话",
          });
          return;
        }
        const started = await invoke<BoardResponse>(
          "task_board_start_conversation",
          {
            request: {
              project: hostProject,
              firstInstruction: editor.instruction.trim(),
              modelId: editor.modelId,
              effortId: editor.effortId,
            },
          },
        );
        const sessionId = String(started.sessionId || "").trim();
        if (
          started.status !== "ok" ||
          !sessionId ||
          /(^|:)(client-)?new-thread:/i.test(sessionId)
        ) {
          setEditor({
            ...busyEditor,
            busy: false,
            feedback: started.message || "新建 Codex 会话失败",
          });
          return;
        }
        sessionIds = [sessionId];
        recovery = {
          kind,
          taskId,
          title,
          project: taskProjectRef(project),
          sessionId,
          initialStatus,
          ...(targetTaskId ? { targetTaskId } : {}),
          createdAtMs: Date.now(),
          semanticKey,
        };
        saveNativeCreateRecovery(recovery);
      }

      if (
        recovery &&
        taskBoardRecoveryAlreadyApplied(recovery, snapshot)
      ) {
        clearNativeCreateRecovery();
        let recoveredSnapshot = snapshot;
        if (recovery.kind === "create-task") {
          recoveredSnapshot = await applyTaskInitialStatus(
            recovery.taskId,
            recovery.initialStatus,
            recoveredSnapshot,
          );
          setSnapshot(recoveredSnapshot);
        }
        setEditor(null);
        showToast(
          recovery.kind === "attach-conversation"
            ? "会话已关联到任务"
            : "任务已创建",
        );
        return;
      }

      const command = editor.targetTask
        ? "task_board_attach_conversations"
        : "task_board_create_task";
      const request = editor.targetTask
        ? {
            taskId,
            expectedRevision: snapshot.revision,
            sessionIds,
          }
        : {
            taskId,
            expectedRevision: snapshot.revision,
            title,
            project: taskProjectRef(project),
            sessionIds,
          };
      const result =
        editor.mode === "new"
          ? await invokeSessionMutationWithRetry(command, request, setCatalog)
          : await invoke<BoardResponse>(command, { request });
      const responseSnapshot = taskBoardSnapshotFromResponse(result);
      if (responseSnapshot) setSnapshot(responseSnapshot);
      const idempotentSuccess =
        Boolean(recovery) &&
        taskBoardRecoveryAlreadyApplied(
          recovery as NativeCreateRecoveryRecord,
          responseSnapshot ?? snapshot,
        );
      if (result.status !== "ok" && !idempotentSuccess) {
        setEditor({
          ...busyEditor,
          busy: false,
          feedback: result.message || "操作失败",
        });
        return;
      }

      if (editor.mode === "new") clearNativeCreateRecovery();
      if (!editor.targetTask) {
        const currentSnapshot = responseSnapshot ?? snapshot;
        const finalSnapshot = await applyTaskInitialStatus(
          taskId,
          editor.initialStatus,
          currentSnapshot,
        );
        setSnapshot(finalSnapshot);
      }

      setEditor(null);
      showToast(editor.targetTask ? "会话已关联到任务" : "任务已创建");
      void refresh();
    } catch (error) {
      setEditor((current) =>
        current
          ? {
              ...current,
              busy: false,
              feedback: messageFromError(error, "操作失败"),
            }
          : current,
      );
    } finally {
      submitEditorBusyRef.current = false;
    }
  }, [
    applyTaskInitialStatus,
    catalog.projects,
    editor,
    refresh,
    showToast,
    snapshot,
  ]);

  const sessionsForEditor = useMemo(() => {
    if (!editor) return [];
    const attached = new Set(
      editor.targetTask?.conversations.map((conversation) =>
        normalizeSessionId(conversation.sessionId),
      ) ?? [],
    );
    return catalog.sessions
      .filter(
        (session) =>
          session.cwd === editor.projectCwd &&
          !attached.has(normalizeSessionId(session.sessionId)),
      )
      .sort(
        (left, right) =>
          Number(right.updatedAtMs || 0) - Number(left.updatedAtMs || 0),
      );
  }, [catalog.sessions, editor]);

  const availableSessionIds = catalogSessionIds;

  return (
    <>
      <main
        className="task-board-app"
        style={taskBoardAppearanceStyle(appearance)}
      >
      <section className="task-board-page" aria-label="任务看板">
        <div className="task-board-heading">
          <h1>任务看板</h1>
        </div>
        <p className="task-board-description">
          跨项目观察任务状态，并集中关联项目下的多个会话
        </p>
        <div className="task-board-toolbar">
          <label className="task-board-search">
            <span className="sr-only">搜索任务或会话</span>
            <Search size={17} aria-hidden="true" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索任务、项目或关联会话"
              type="search"
            />
          </label>
          <TaskBoardDropdown
            className="task-board-select"
            value={projectFilter}
            options={[
              { value: "", label: "全部项目" },
              ...catalog.projects.map((project) => ({
                value: project.cwd,
                label: project.label,
                description: project.cwd,
              })),
            ]}
            ariaLabel="筛选项目"
            fixedWidth={320}
            searchable
            searchPlaceholder="搜索项目名称或路径"
            onChange={setProjectFilter}
          />
          <button
            className="task-board-create"
            type="button"
            onClick={() => openEditor()}
            disabled={catalog.projects.length === 0}
          >
            <Plus size={17} aria-hidden="true" />
            新建任务
          </button>
          <button
            className="task-board-manage"
            type="button"
            onClick={openBoardManager}
          >
            <Plus size={17} aria-hidden="true" />
            管理看板
          </button>
          <span
            className="task-board-hint"
            data-status={catalog.warnings.length ? "warning" : loading ? "loading" : "ok"}
          >
            {loading
              ? "正在加载任务与会话目录…"
              : catalog.warnings.length
                ? "目录部分不可用"
                : "拖动任务卡片可切换状态"}
          </span>
          <div
            className="task-board-unassigned-drop-zone"
            data-active={unassignedDropActive || undefined}
            data-drop-active={
              unassignedDropActive && dropStatus === "new" ? true : undefined
            }
            aria-hidden={!unassignedDropActive}
            onDragOver={(event) => {
              if (!unassignedDropActive) return;
              event.preventDefault();
              event.dataTransfer.dropEffect = "move";
              setDropStatus("new");
            }}
            onDragLeave={(event) => {
              if (
                !event.currentTarget.contains(event.relatedTarget as Node | null)
              ) {
                setDropStatus("");
              }
            }}
            onDrop={(event) => dropTaskIntoStatus(event, "new")}
          >
            <TaskBoardStatusIcon statusId="new" />
            <span>
              <strong>未分配</strong>
              <small>拖到这里移除当前状态</small>
            </span>
          </div>
        </div>

        <div
          className="task-board-scroll"
          tabIndex={0}
          aria-label="任务看板列，可横向和纵向滚动"
        >
          {loading ? (
            <div className="task-board-loading">
              <LoaderCircle className="spinning" aria-hidden="true" />
              正在读取任务看板
            </div>
          ) : (
          <div
            className="task-board-columns"
            aria-label="任务状态列"
            style={taskBoardColumnsStyle(visibleStatusDefinitions.length)}
          >
            {visibleStatusDefinitions.map((status) => {
              const tasks = columnTasks(status.id);
              return (
                <section className="task-board-column" key={status.id}>
                    <header className="task-board-column-head">
                    <div className="task-board-column-title">
                      <TaskBoardStatusIcon statusId={status.id} />
                      {status.label}
                    </div>
                    <span className="task-board-count">{tasks.length}</span>
                  </header>
                  <div
                    className={`task-board-card-list ${
                      dropStatus === status.id ? "drag-over" : ""
                    }`}
                    onDragOver={(event) => {
                      event.preventDefault();
                      setDropStatus(status.id);
                    }}
                    onDragLeave={(event) => {
                      if (!event.currentTarget.contains(event.relatedTarget as Node)) {
                        setDropStatus("");
                      }
                    }}
                    onDrop={(event) => {
                      dropTaskIntoStatus(event, status.id);
                    }}
                  >
                    {tasks.map((task) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        onDragStart={(event) => {
                          event.dataTransfer.effectAllowed = "move";
                          event.dataTransfer.setData("text/plain", task.id);
                          setDragTaskId(task.id);
                        }}
                        onDragEnd={() => {
                          setDragTaskId("");
                          setDropStatus("");
                        }}
                        dragging={dragTaskId === task.id}
                        onOpenConversation={openConversation}
                        onDetachConversation={requestDetachConfirmation}
                        onDeleteTask={() => requestDeleteTaskConfirmation(task)}
                        onAttach={() => openEditor(task)}
                        onMoveStatus={(status) => void moveTask(task, status)}
                        statusDefinitions={statusDefinitions}
                        conversationStatuses={conversationStatuses}
                        availableSessionIds={availableSessionIds}
                        catalogPartiallyUnavailable={catalog.warnings.length > 0}
                      />
                    ))}
                    {tasks.length === 0 ? (
                      <div className="task-board-empty-column">暂无任务</div>
                    ) : null}
                  </div>
                </section>
              );
            })}
          </div>
          )}
        </div>
      </section>

      {editor ? (
        <TaskEditor
          editor={editor}
          projects={catalog.projects}
          sessions={sessionsForEditor}
          statusDefinitions={statusDefinitions}
          onChange={setEditor}
          onClose={closeEditor}
          onSubmit={() => void submitEditor()}
        />
      ) : null}

        <div
          className={`task-board-toast ${toast ? "visible" : ""}`}
          role="status"
          aria-live="polite"
        >
          {toast}
        </div>
      </main>
      <TaskBoardAppearanceOverlay appearance={appearance} />
      {boardManager ? (
        <TaskBoardManager
          manager={boardManager}
          boards={snapshot.boards}
          tasks={snapshot.tasks}
          onChange={setBoardManager}
          onClose={closeBoardManager}
          onCreate={() => void createManagedBoard()}
          onConfirmDelete={() => void confirmDeleteBoard()}
          onRename={renameManagedBoard}
          onMove={moveManagedBoard}
        />
      ) : null}
      {detachConfirmation ? (
        <TaskBoardDetachConfirmation
          confirmation={detachConfirmation}
          onClose={closeDetachConfirmation}
          onConfirm={() => void confirmDetachConversation()}
        />
      ) : null}
      {deleteTaskConfirmation ? (
        <TaskBoardDeleteConfirmation
          confirmation={deleteTaskConfirmation}
          onClose={closeDeleteTaskConfirmation}
          onConfirm={() => void confirmDeleteTask()}
        />
      ) : null}
    </>
  );
}

function TaskCard({
  task,
  dragging,
  onDragStart,
  onDragEnd,
  onOpenConversation,
  onDetachConversation,
  onDeleteTask,
  onAttach,
  onMoveStatus,
  statusDefinitions,
  conversationStatuses,
  availableSessionIds,
  catalogPartiallyUnavailable,
}: {
  task: Task;
  dragging: boolean;
  onDragStart: (event: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onOpenConversation: (
    conversation: TaskConversation,
    project: TaskProject,
  ) => void;
  onDetachConversation: (task: Task, conversation: TaskConversation) => void;
  onDeleteTask: () => void;
  onAttach: () => void;
  onMoveStatus: (status: TaskStatus) => void;
  statusDefinitions: BoardDefinition[];
  conversationStatuses: Map<string, ConversationRuntimeStatus>;
  availableSessionIds: Set<string>;
  catalogPartiallyUnavailable: boolean;
}) {
  return (
    <article
      className={`task-board-card ${dragging ? "dragging" : ""}`}
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
    >
      <button
        className="task-board-card-delete"
        type="button"
        draggable={false}
        aria-label={`删除任务 ${task.title}`}
        title="删除任务"
        onPointerDown={(event) => event.stopPropagation()}
        onDragStart={(event) => event.preventDefault()}
        onClick={(event) => {
          event.stopPropagation();
          onDeleteTask();
        }}
      >
        <X size={13} strokeWidth={1.35} aria-hidden="true" />
      </button>
      <div className="task-board-project">{task.project.label}</div>
      <h2 className="task-board-card-title">{task.title}</h2>
      <div className="task-board-conversations">
        {task.conversations.length ? (
          task.conversations.map((conversation) => {
            const sessionKey = normalizeSessionId(conversation.sessionId);
            const available =
              availableSessionIds.has(sessionKey) ||
              catalogPartiallyUnavailable;
            const status = conversationStatusPresentation(
              conversationStatuses.get(sessionKey),
              available,
            );
            const title = conversation.title || "未命名会话";
            const showStatus = status.id !== "completed";
            const iconOnlyStatus =
              status.id === "running" || status.id === "unread";
            return (
              <div
                className="task-board-conversation-row"
                key={conversation.sessionId}
              >
                <button
                  className="task-board-conversation"
                  type="button"
                  onClick={() => onOpenConversation(conversation, task.project)}
                  disabled={!available}
                  title={showStatus ? `${title}\n${status.label}` : title}
                  aria-label={`${title}${showStatus ? `，${status.label}` : ""}，${
                    available ? "打开会话" : "会话不可用"
                  }`}
                >
                  <span
                    className="task-board-conversation-icon"
                    aria-hidden="true"
                  >
                    <MessageSquare size={14} strokeWidth={1.2} />
                  </span>
                  <span className="task-board-conversation-title">{title}</span>
                  {showStatus ? (
                    <span
                      className="task-board-conversation-state"
                      data-conversation-status={status.id}
                      aria-hidden="true"
                    >
                      <span className="task-board-conversation-status-indicator" />
                      {iconOnlyStatus ? null : status.label}
                    </span>
                  ) : null}
                </button>
                <button
                  className="task-board-conversation-remove"
                  type="button"
                  aria-label={`移除会话 ${title}`}
                  onClick={() => onDetachConversation(task, conversation)}
                >
                  <X size={13} strokeWidth={1.35} aria-hidden="true" />
                </button>
              </div>
            );
          })
        ) : (
          <div className="task-board-empty task-board-card-empty">未关联会话</div>
        )}
      </div>
      <footer className="task-board-card-footer">
        <button
          className="task-board-card-add"
          type="button"
          aria-label={`为 ${task.title} 关联会话`}
          onClick={onAttach}
        >
          <Plus size={13} strokeWidth={1.35} aria-hidden="true" />
          <span>添加会话</span>
        </button>
        <TaskBoardDropdown
          className="task-board-card-move"
          value={task.status}
          ariaLabel={`移动任务 ${task.title} 的状态`}
          options={statusDefinitions.map((status) => ({
            value: status.id,
            label: status.label,
            iconId: status.id,
          }))}
          minWidth={150}
          showChevron={false}
          onChange={(status) => onMoveStatus(status as TaskStatus)}
        />
      </footer>
    </article>
  );
}

function TaskBoardManager({
  manager,
  boards,
  tasks,
  onChange,
  onClose,
  onCreate,
  onConfirmDelete,
  onRename,
  onMove,
}: {
  manager: BoardManagerState;
  boards: BoardDefinition[];
  tasks: Task[];
  onChange: (manager: BoardManagerState) => void;
  onClose: () => void;
  onCreate: () => void;
  onConfirmDelete: () => void;
  onRename: (boardId: string, label: string) => Promise<boolean>;
  onMove: (boardId: string, targetIndex: number) => Promise<boolean>;
}) {
  const dialogRef = useRef<HTMLElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const dragHandleRefs = useRef(new Map<string, HTMLButtonElement>());
  const previousFocusRef = useRef<HTMLElement | null>(
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const busyRef = useRef(manager.busy);
  const backdropPressStartedRef = useRef(false);
  const backdropPressCompletedRef = useRef(false);
  const [editingBoardId, setEditingBoardId] = useState("");
  const [editingLabel, setEditingLabel] = useState("");
  const [dragBoardId, setDragBoardId] = useState("");
  const [dropTarget, setDropTarget] = useState<{
    boardId: string;
    position: "before" | "after";
    targetIndex: number;
  } | null>(null);
  busyRef.current = manager.busy;
  const pendingBoard = boards.find(
    (board) => board.id === manager.pendingDeleteId,
  );
  const editingBoard = boards.find((board) => board.id === editingBoardId);
  const pendingTaskCount = pendingBoard
    ? tasks.filter((task) => task.status === pendingBoard.id).length
    : 0;

  useEffect(() => {
    window.requestAnimationFrame(() => inputRef.current?.focus());
    return () => {
      window.requestAnimationFrame(() => {
        if (previousFocusRef.current?.isConnected) {
          previousFocusRef.current.focus();
        }
      });
    };
  }, []);

  useEffect(() => {
    if (!editingBoardId || manager.busy) return;
    window.requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
  }, [editingBoardId, manager.busy]);

  useEffect(() => {
    if (
      editingBoardId &&
      !boards.some((board) => board.id === editingBoardId)
    ) {
      setEditingBoardId("");
      setEditingLabel("");
    }
  }, [boards, editingBoardId]);

  useEffect(() => {
    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (busyRef.current) return;
        if (editingBoardId) {
          setEditingBoardId("");
          setEditingLabel("");
          return;
        }
        if (dragBoardId) {
          setDragBoardId("");
          setDropTarget(null);
          return;
        }
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const dialog = dialogRef.current;
      if (!dialog) return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          "button:not(:disabled), input:not(:disabled)",
        ),
      );
      if (!focusable.length) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const currentIndex = focusable.indexOf(
        document.activeElement as HTMLElement,
      );
      const nextIndex = event.shiftKey
        ? currentIndex <= 0
          ? focusable.length - 1
          : currentIndex - 1
        : currentIndex === focusable.length - 1
          ? 0
          : currentIndex + 1;
      event.preventDefault();
      focusable[nextIndex]?.focus();
    };
    document.addEventListener("keydown", handleKeydown, true);
    return () => {
      document.removeEventListener("keydown", handleKeydown, true);
    };
  }, [dragBoardId, editingBoardId, onClose]);

  const beginBoardEdit = (board: BoardDefinition) => {
    if (manager.busy) return;
    setEditingBoardId(board.id);
    setEditingLabel(board.label);
    setDragBoardId("");
    setDropTarget(null);
    onChange({
      ...manager,
      feedback: "",
      pendingDeleteId: "",
    });
  };

  const submitBoardRename = async () => {
    if (!editingBoardId || manager.busy) return;
    if (await onRename(editingBoardId, editingLabel)) {
      setEditingBoardId("");
      setEditingLabel("");
    }
  };

  const clearBoardDrag = () => {
    setDragBoardId("");
    setDropTarget(null);
  };

  return (
    <div
      className="task-board-modal-backdrop"
      role="presentation"
      onPointerDown={(event) => {
        backdropPressStartedRef.current = event.target === event.currentTarget;
        backdropPressCompletedRef.current = false;
      }}
      onPointerUp={(event) => {
        backdropPressCompletedRef.current =
          backdropPressStartedRef.current && event.target === event.currentTarget;
      }}
      onPointerCancel={() => {
        backdropPressStartedRef.current = false;
        backdropPressCompletedRef.current = false;
      }}
      onClick={(event) => {
        const shouldClose =
          event.target === event.currentTarget &&
          backdropPressCompletedRef.current;
        backdropPressStartedRef.current = false;
        backdropPressCompletedRef.current = false;
        if (shouldClose && !manager.busy) onClose();
      }}
    >
      <section
        className="task-board-modal task-board-board-manager"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="task-board-manager-title"
        tabIndex={-1}
      >
        <header className="task-board-modal-head">
          <div>
            <h2 id="task-board-manager-title">管理看板</h2>
            <p>添加、改名、排序或删除任务状态列；“未分配”是固定的系统列。</p>
          </div>
          <button
            className="task-board-icon-button"
            type="button"
            aria-label="关闭看板管理"
            onClick={onClose}
            disabled={manager.busy}
          >
            <X size={16} strokeWidth={1.4} aria-hidden="true" />
          </button>
        </header>
        <div className="task-board-board-manager-body">
          <form
            className="task-board-board-add"
            onSubmit={(event) => {
              event.preventDefault();
              if (editingBoardId) {
                void submitBoardRename();
              } else {
                onCreate();
              }
            }}
          >
            <label className="task-board-field">
              <span>新看板名称</span>
              <input
                ref={inputRef}
                value={editingBoardId ? editingLabel : manager.label}
                maxLength={40}
                placeholder="例如：待发布"
                aria-label={
                  editingBoard
                    ? `看板 ${editingBoard.label} 的新名称`
                    : "新看板名称"
                }
                disabled={manager.busy}
                onChange={(event) => {
                  if (editingBoardId) {
                    setEditingLabel(event.target.value);
                    onChange({
                      ...manager,
                      feedback: "",
                      pendingDeleteId: "",
                    });
                    return;
                  }
                  onChange({
                    ...manager,
                    label: event.target.value,
                    feedback: "",
                    pendingDeleteId: "",
                  });
                }}
              />
            </label>
            <div className="task-board-board-action-slot">
              {editingBoardId ? (
                <>
                  <button
                    className="task-board-button primary task-board-board-mode-action"
                    type="submit"
                    aria-label={`保存看板 ${editingBoard?.label || ""} 的名称`}
                    title="保存名称"
                    disabled={manager.busy || !editingLabel.trim()}
                  >
                    {manager.busyKind === "rename" &&
                    manager.busyBoardId === editingBoardId ? (
                      <LoaderCircle
                        className="spinning"
                        size={14}
                        aria-hidden="true"
                      />
                    ) : (
                      <Check size={14} strokeWidth={1.5} aria-hidden="true" />
                    )}
                  </button>
                  <button
                    className="task-board-button task-board-board-mode-action"
                    type="button"
                    aria-label={`取消编辑看板 ${editingBoard?.label || ""}`}
                    title="取消编辑"
                    disabled={manager.busy}
                    onClick={() => {
                      setEditingBoardId("");
                      setEditingLabel("");
                    }}
                  >
                    <X size={14} strokeWidth={1.4} aria-hidden="true" />
                  </button>
                </>
              ) : (
                <button
                  className="task-board-button primary task-board-board-add-button"
                  type="submit"
                  disabled={manager.busy || !manager.label.trim()}
                >
                  {manager.busyKind === "create" ? (
                    <LoaderCircle
                      className="spinning"
                      size={14}
                      aria-hidden="true"
                    />
                  ) : (
                    <Plus size={14} strokeWidth={1.3} aria-hidden="true" />
                  )}
                  添加看板
                </button>
              )}
            </div>
          </form>

          <div className="task-board-board-list" aria-label="现有看板">
            {boards.length ? (
              boards.map((board, boardIndex) => {
                const taskCount = tasks.filter(
                  (task) => task.status === board.id,
                ).length;
                const editing = editingBoardId === board.id;
                const moving =
                  manager.busyKind === "move" &&
                  manager.busyBoardId === board.id;
                const dropPosition =
                  dropTarget?.boardId === board.id
                    ? dropTarget.position
                    : undefined;
                return (
                  <div
                    className="task-board-board-item"
                    key={board.id}
                    data-dragging={dragBoardId === board.id || undefined}
                    data-drop-position={dropPosition}
                    aria-busy={
                      manager.busyBoardId === board.id && manager.busy
                    }
                    onDragOver={(event) => {
                      if (!dragBoardId || manager.busy) return;
                      event.preventDefault();
                      event.dataTransfer.dropEffect = "move";
                      const bounds = event.currentTarget.getBoundingClientRect();
                      const placeAfter =
                        event.clientY >= bounds.top + bounds.height / 2;
                      setDropTarget({
                        boardId: board.id,
                        position: placeAfter ? "after" : "before",
                        targetIndex: taskBoardMoveBoardTargetIndex(
                          boards,
                          dragBoardId,
                          board.id,
                          placeAfter,
                        ),
                      });
                    }}
                    onDragLeave={(event) => {
                      if (
                        event.currentTarget.contains(
                          event.relatedTarget as Node | null,
                        )
                      ) {
                        return;
                      }
                      setDropTarget((current) =>
                        current?.boardId === board.id ? null : current,
                      );
                    }}
                    onDrop={(event) => {
                      event.preventDefault();
                      const sourceBoardId =
                        dragBoardId ||
                        event.dataTransfer.getData("text/plain").trim();
                      const targetIndex =
                        dropTarget?.boardId === board.id
                          ? dropTarget.targetIndex
                          : taskBoardMoveBoardTargetIndex(
                              boards,
                              sourceBoardId,
                              board.id,
                              false,
                            );
                      clearBoardDrag();
                      if (sourceBoardId) {
                        void onMove(sourceBoardId, targetIndex);
                      }
                    }}
                  >
                    <button
                      className="task-board-board-drag-handle"
                      type="button"
                      ref={(node) => {
                        if (node) {
                          dragHandleRefs.current.set(board.id, node);
                        } else {
                          dragHandleRefs.current.delete(board.id);
                        }
                      }}
                      draggable={!manager.busy && !editing}
                      aria-label={`拖动看板 ${board.label} 调整排序`}
                      aria-keyshortcuts="ArrowUp ArrowDown"
                      title="拖动排序；也可使用上下方向键"
                      disabled={manager.busy || editing}
                      onDragStart={(event) => {
                        if (manager.busy || editing) {
                          event.preventDefault();
                          return;
                        }
                        setDragBoardId(board.id);
                        setDropTarget(null);
                        event.dataTransfer.effectAllowed = "move";
                        event.dataTransfer.setData("text/plain", board.id);
                        const row =
                          event.currentTarget.closest<HTMLElement>(
                            ".task-board-board-item",
                          );
                        if (row) {
                          const bounds = row.getBoundingClientRect();
                          event.dataTransfer.setDragImage(
                            row,
                            Math.max(
                              0,
                              Math.min(bounds.width, event.clientX - bounds.left),
                            ),
                            Math.max(
                              0,
                              Math.min(bounds.height, event.clientY - bounds.top),
                            ),
                          );
                        }
                      }}
                      onDragEnd={clearBoardDrag}
                      onKeyDown={(event) => {
                        if (
                          event.key !== "ArrowUp" &&
                          event.key !== "ArrowDown"
                        ) {
                          return;
                        }
                        const targetIndex =
                          boardIndex + (event.key === "ArrowUp" ? -1 : 1);
                        if (
                          targetIndex < 0 ||
                          targetIndex >= boards.length ||
                          manager.busy
                        ) {
                          return;
                        }
                        event.preventDefault();
                        void onMove(board.id, targetIndex).finally(() => {
                          window.requestAnimationFrame(() => {
                            const handle = dragHandleRefs.current.get(board.id);
                            if (handle?.isConnected) handle.focus();
                          });
                        });
                      }}
                    >
                      {moving ? (
                        <LoaderCircle
                          className="spinning"
                          size={15}
                          aria-hidden="true"
                        />
                      ) : (
                        <GripVertical
                          size={15}
                          strokeWidth={1.5}
                          aria-hidden="true"
                        />
                      )}
                    </button>
                    <TaskBoardStatusIcon statusId={board.id} />
                    <span className="task-board-board-name" title={board.label}>
                      {board.label}
                    </span>
                    <span className="task-board-board-task-count">
                      {taskCount} 个任务
                    </span>
                    <button
                      className="task-board-board-edit"
                      type="button"
                      aria-label={`编辑看板 ${board.label}`}
                      title="编辑看板名称"
                      disabled={manager.busy || editing}
                      onClick={() => beginBoardEdit(board)}
                    >
                      <Pencil
                        size={14}
                        strokeWidth={1.4}
                        aria-hidden="true"
                      />
                    </button>
                    <button
                      className="task-board-board-delete"
                      type="button"
                      aria-label={`删除看板 ${board.label}`}
                      title="删除看板"
                      disabled={manager.busy}
                      onClick={() =>
                        onChange({
                          ...manager,
                          feedback: "",
                          pendingDeleteId: board.id,
                        })
                      }
                    >
                      <X size={14} strokeWidth={1.4} aria-hidden="true" />
                    </button>
                  </div>
                );
              })
            ) : (
              <p className="task-board-board-empty">
                当前没有可管理看板，任务都会显示在“未分配”。
              </p>
            )}
          </div>

          {pendingBoard ? (
            <div className="task-board-board-delete-confirm" role="alert">
              <p>
                删除“{pendingBoard.label}”后，其中 {pendingTaskCount} 个任务将移至
                “未分配”。
              </p>
              <div>
                <button
                  type="button"
                  disabled={manager.busy}
                  onClick={() =>
                    onChange({ ...manager, pendingDeleteId: "", feedback: "" })
                  }
                >
                  取消
                </button>
                <button
                  className="danger"
                  type="button"
                  disabled={manager.busy}
                  onClick={onConfirmDelete}
                >
                  {manager.busyKind === "delete" ? "正在删除…" : "确认删除"}
                </button>
              </div>
            </div>
          ) : null}

          {manager.feedback ? (
            <p className="task-board-feedback" role="alert">
              {manager.feedback}
            </p>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function TaskBoardAppearanceOverlay({
  appearance,
}: {
  appearance: TaskBoardAppearance | null;
}) {
  const overlay = taskBoardAppearanceOverlay(appearance);
  if (!overlay || !document.body) return null;
  const imageUrl = taskBoardAppearanceImageUrl(
    overlay.imageUrl,
    appearance?.signature,
  );
  const layer =
    overlay.kind === "image" ? (
      <img
        key={appearance?.signature || imageUrl}
        className="task-board-appearance-overlay"
        src={imageUrl}
        alt=""
        aria-hidden="true"
        draggable={false}
        style={{
          opacity: overlay.opacity,
          objectFit: overlay.fit,
          objectPosition: "50% 50%",
        }}
      />
    ) : (
      <div
        className="task-board-appearance-overlay"
        aria-hidden="true"
        style={{
          opacity: overlay.opacity,
          background:
            overlay.kind === "color"
              ? overlay.backgroundColor
              : `linear-gradient(${overlay.gradientAngle}deg, ${overlay.gradientFrom}, ${overlay.gradientTo})`,
        }}
      />
    );
  return createPortal(layer, document.body);
}

function TaskBoardDetachConfirmation({
  confirmation,
  onClose,
  onConfirm,
}: {
  confirmation: DetachConfirmationState;
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <TaskBoardConfirmationDialog
      idPrefix="task-board-detach"
      title="移除关联会话？"
      description={
        <>
          仅解除与任务“{confirmation.task.title || "未命名任务"}”的关联，不会删除
          Codex 中的原始会话。
        </>
      }
      busy={confirmation.busy}
      feedback={confirmation.feedback}
      confirmLabel="移除"
      busyLabel="正在移除…"
      onClose={onClose}
      onConfirm={onConfirm}
    />
  );
}

function TaskBoardDeleteConfirmation({
  confirmation,
  onClose,
  onConfirm,
}: {
  confirmation: DeleteTaskConfirmationState;
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <TaskBoardConfirmationDialog
      idPrefix="task-board-delete-task"
      title="删除任务？"
      description={
        <>
          将从任务看板删除“{confirmation.task.title || "未命名任务"}”及其所有关联。
          不会删除 Codex 中的原始会话。
        </>
      }
      busy={confirmation.busy}
      feedback={confirmation.feedback}
      confirmLabel="删除任务"
      busyLabel="正在删除…"
      onClose={onClose}
      onConfirm={onConfirm}
    />
  );
}

function TaskBoardConfirmationDialog({
  idPrefix,
  title,
  description,
  busy,
  feedback,
  confirmLabel,
  busyLabel,
  onClose,
  onConfirm,
}: {
  idPrefix: string;
  title: string;
  description: ReactNode;
  busy: boolean;
  feedback: string;
  confirmLabel: string;
  busyLabel: string;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const dialogRef = useRef<HTMLElement>(null);
  const cancelButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const busyRef = useRef(busy);
  const backdropPressStartedRef = useRef(false);
  const backdropPressCompletedRef = useRef(false);
  busyRef.current = busy;

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!busyRef.current) onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>("button:not(:disabled)"),
      );
      if (!focusable.length) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
      const nextIndex = event.shiftKey
        ? currentIndex <= 0
          ? focusable.length - 1
          : currentIndex - 1
        : currentIndex === focusable.length - 1
          ? 0
          : currentIndex + 1;
      event.preventDefault();
      focusable[nextIndex]?.focus();
    };
    document.addEventListener("keydown", handleKeydown, true);
    window.requestAnimationFrame(() => cancelButtonRef.current?.focus());
    return () => {
      document.removeEventListener("keydown", handleKeydown, true);
    };
  }, [onClose]);

  useEffect(
    () => () => {
      window.requestAnimationFrame(() => {
        if (previousFocusRef.current?.isConnected) {
          previousFocusRef.current.focus();
        }
      });
    },
    [],
  );

  if (!document.body) return null;
  return createPortal(
    <div
      className="task-board-confirm-backdrop"
      role="presentation"
      onPointerDown={(event) => {
        backdropPressStartedRef.current = event.target === event.currentTarget;
        backdropPressCompletedRef.current = false;
      }}
      onPointerUp={(event) => {
        backdropPressCompletedRef.current =
          backdropPressStartedRef.current && event.target === event.currentTarget;
      }}
      onPointerCancel={() => {
        backdropPressStartedRef.current = false;
        backdropPressCompletedRef.current = false;
      }}
      onClick={(event) => {
        const shouldClose =
          event.target === event.currentTarget && backdropPressCompletedRef.current;
        backdropPressStartedRef.current = false;
        backdropPressCompletedRef.current = false;
        if (shouldClose && !busy) onClose();
      }}
    >
      <section
        className="task-board-confirm-dialog"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={`${idPrefix}-title`}
        aria-describedby={`${idPrefix}-description`}
        tabIndex={-1}
      >
        <h2 id={`${idPrefix}-title`}>{title}</h2>
        <p id={`${idPrefix}-description`}>{description}</p>
        {feedback ? (
          <p className="task-board-confirm-feedback" role="alert">
            {feedback}
          </p>
        ) : null}
        <div className="task-board-confirm-actions">
          <button
            ref={cancelButtonRef}
            type="button"
            onClick={onClose}
            disabled={busy}
          >
            取消
          </button>
          <button
            className="danger"
            type="button"
            onClick={onConfirm}
            disabled={busy}
          >
            {busy ? busyLabel : confirmLabel}
          </button>
        </div>
      </section>
    </div>,
    document.body,
  );
}

function TaskEditor({
  editor,
  projects,
  sessions,
  statusDefinitions,
  onChange,
  onClose,
  onSubmit,
}: {
  editor: EditorState;
  projects: CatalogProject[];
  sessions: CatalogSession[];
  statusDefinitions: BoardDefinition[];
  onChange: (editor: EditorState) => void;
  onClose: () => void;
  onSubmit: () => void;
}) {
  const attaching = Boolean(editor.targetTask);
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const backdropPressStartedRef = useRef(false);
  const backdropPressCompletedRef = useRef(false);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const handleKeydown = (event: KeyboardEvent) => {
      if (document.querySelector(".task-board-dropdown-menu")) return;
      if (event.key === "Escape") {
        event.preventDefault();
        if (!editor.busy) onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = taskBoardModalFocusableElements(dialog);
      if (!focusable.length) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
      const nextIndex = event.shiftKey
        ? currentIndex <= 0
          ? focusable.length - 1
          : currentIndex - 1
        : currentIndex === focusable.length - 1
          ? 0
          : currentIndex + 1;
      event.preventDefault();
      focusable[nextIndex]?.focus();
    };
    document.addEventListener("keydown", handleKeydown, true);
    window.requestAnimationFrame(() => {
      const autofocus = attaching
        ? dialog.querySelector<HTMLElement>(".task-board-create-mode")
        : dialog.querySelector<HTMLElement>("[data-task-board-modal-autofocus]");
      autofocus?.focus();
    });
    return () => {
      document.removeEventListener("keydown", handleKeydown, true);
      window.requestAnimationFrame(() => {
        if (previousFocusRef.current?.isConnected) previousFocusRef.current.focus();
      });
    };
  }, [attaching, editor.busy, onClose]);

  const submitLabel = attaching
    ? editor.mode === "new"
      ? "创建并添加"
      : "添加会话"
    : "创建任务";

  return (
    <div
      className="task-board-modal-backdrop"
      role="presentation"
      onPointerDown={(event) => {
        backdropPressStartedRef.current = event.target === event.currentTarget;
        backdropPressCompletedRef.current = false;
      }}
      onPointerUp={(event) => {
        backdropPressCompletedRef.current =
          backdropPressStartedRef.current && event.target === event.currentTarget;
      }}
      onPointerCancel={() => {
        backdropPressStartedRef.current = false;
        backdropPressCompletedRef.current = false;
      }}
      onClick={(event) => {
        const shouldClose =
          event.target === event.currentTarget && backdropPressCompletedRef.current;
        backdropPressStartedRef.current = false;
        backdropPressCompletedRef.current = false;
        if (shouldClose && !editor.busy) onClose();
      }}
    >
      <section
        className="task-board-modal"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="task-editor-title"
        tabIndex={-1}
      >
        <header className="task-board-modal-head">
          <div>
            <h2 id="task-editor-title">{attaching ? "添加会话" : "新建任务"}</h2>
            <p>
              {attaching
                ? `为“${editor.targetTask?.title || "未命名任务"}”关联已有会话，或创建一个新会话`
                : "将 Codex 会话组织到跨项目任务看板中"}
            </p>
          </div>
          <button
            className="task-board-icon-button"
            type="button"
            aria-label="关闭"
            onClick={onClose}
            disabled={editor.busy}
          >
            <X size={16} strokeWidth={1.4} aria-hidden="true" />
          </button>
        </header>

        <div className="task-board-modal-body">
          {!attaching ? (
            <label className="task-board-field">
              <span>任务名称</span>
              <input
                value={editor.title}
                onChange={(event) =>
                  onChange({ ...editor, title: event.target.value, feedback: "" })
                }
                maxLength={120}
                placeholder="输入一个清晰、可跟进的任务名称"
                aria-label="任务名称"
                data-task-board-modal-autofocus="true"
                disabled={editor.busy}
              />
            </label>
          ) : null}

          {!attaching ? (
            <div className="task-board-field-row">
              <label className="task-board-field">
                <span>所属项目</span>
                <TaskBoardDropdown
                  className="task-board-create-select"
                  value={editor.projectCwd}
                  options={
                    projects.length
                      ? projects.map((project) => ({
                        value: project.cwd,
                        label: project.label,
                        description: project.cwd,
                        }))
                      : [
                          {
                            value: "",
                            label: "暂无可用项目",
                            disabled: true,
                          },
                        ]
                  }
                  ariaLabel="选择所属项目"
                  placeholder="请选择项目"
                  fixedWidth={320}
                  modalFocusTrap
                  searchable
                  searchPlaceholder="搜索项目名称或路径"
                  onChange={(projectCwd) =>
                    onChange({
                      ...editor,
                      projectCwd,
                      selectedSessionIds: [],
                      feedback: "",
                      nativeCreateAvailable: null,
                      nativeCreateMessage: "正在确认 Codex 新会话能力…",
                    })
                  }
                  disabled={editor.busy}
                />
              </label>
              <label className="task-board-field">
                <span>初始状态</span>
                <TaskBoardDropdown
                  className="task-board-create-select"
                  value={editor.initialStatus}
                  options={statusDefinitions.map((status) => ({
                    value: status.id,
                    label: status.label,
                    iconId: status.id,
                  }))}
                  ariaLabel="选择初始状态"
                  minWidth={160}
                  matchTriggerWidth
                  modalFocusTrap
                  onChange={(initialStatus) =>
                    onChange({
                      ...editor,
                      initialStatus: initialStatus as TaskStatus,
                    })
                  }
                  disabled={editor.busy}
                />
              </label>
            </div>
          ) : null}

          <div className="task-board-field">
            <span>会话关联方式</span>
            <div
              className="task-board-mode-tabs"
              role="group"
              aria-label="会话关联方式"
            >
              <button
                type="button"
                aria-pressed={editor.mode === "existing"}
                className="task-board-create-mode"
                onClick={() =>
                  onChange({ ...editor, mode: "existing", feedback: "" })
                }
                disabled={editor.busy}
              >
                <svg
                  aria-hidden="true"
                  viewBox="0 0 16 16"
                  width="15"
                  height="15"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.3"
                >
                  <path
                    d="M6.1 9.9 9.9 6.1M5.2 11.7l-1 .9a2.55 2.55 0 0 1-3.6-3.6l2.1-2.1a2.55 2.55 0 0 1 3.6 0M10.8 4.3l1-.9a2.55 2.55 0 1 1 3.6 3.6l-2.1 2.1a2.55 2.55 0 0 1-3.6 0"
                    strokeLinecap="round"
                  />
                </svg>
                <span>绑定已有会话</span>
              </button>
              <button
                type="button"
                aria-pressed={editor.mode === "new"}
                className="task-board-create-mode"
                onClick={() => onChange({ ...editor, mode: "new", feedback: "" })}
                disabled={editor.busy || !editor.projectCwd}
                data-availability={
                  editor.nativeCreateAvailable === null
                    ? "checking"
                    : editor.nativeCreateAvailable
                      ? "available"
                      : "unavailable"
                }
                title={
                  editor.nativeCreateAvailable === false
                    ? `${editor.nativeCreateMessage}；提交时会重新检查`
                    : undefined
                }
              >
                <svg
                  aria-hidden="true"
                  viewBox="0 0 16 16"
                  width="15"
                  height="15"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.25"
                >
                  <path
                    d="M8 1.8c.35 2.6 1.6 3.85 4.2 4.2C9.6 6.35 8.35 7.6 8 10.2 7.65 7.6 6.4 6.35 3.8 6 6.4 5.65 7.65 4.4 8 1.8ZM12.2 10c.2 1.45.9 2.15 2.35 2.35-1.45.2-2.15.9-2.35 2.35-.2-1.45-.9-2.15-2.35-2.35 1.45-.2 2.15-.9 2.35-2.35Z"
                    strokeLinejoin="round"
                  />
                </svg>
                <span>创建新会话</span>
              </button>
            </div>
          </div>

          <div className="task-board-mode-content">
            {editor.mode === "existing" ? (
              <div className="task-board-session-panel">
                <div className="task-board-session-picker-head">
                  <span>选择已有会话</span>
                  <span className="task-board-session-picker-count">
                    已选 {editor.selectedSessionIds.length} 个
                  </span>
                </div>
                <div
                  className="task-board-session-picker"
                  role="group"
                  aria-label="选择同项目下的已有会话"
                >
                  {sessions.length ? (
                    sessions.map((session) => {
                      const selected = editor.selectedSessionIds.includes(
                        session.sessionId,
                      );
                      return (
                        <label
                          className="task-board-session-option"
                          key={session.sessionId}
                        >
                          <input
                            type="checkbox"
                            checked={selected}
                            onChange={() => {
                              const selectedSessionIds = selected
                                ? editor.selectedSessionIds.filter(
                                    (sessionId) => sessionId !== session.sessionId,
                                  )
                                : [...editor.selectedSessionIds, session.sessionId];
                              onChange({
                                ...editor,
                                selectedSessionIds,
                                feedback: "",
                              });
                            }}
                            disabled={editor.busy}
                          />
                          <span className="task-board-session-icon">
                            <svg
                              aria-hidden="true"
                              viewBox="0 0 16 16"
                              width="14"
                              height="14"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="1.2"
                            >
                              <path
                                d="M3.25 3.25h9.5v7H7.4l-3.15 2.5v-2.5h-1z"
                                strokeLinejoin="round"
                              />
                            </svg>
                          </span>
                          <span className="task-board-session-copy">
                            <span
                              className="task-board-session-title"
                              title={session.title || "未命名会话"}
                            >
                              {session.title || "未命名会话"}
                            </span>
                            <span
                              className="task-board-session-time"
                              title={`更新时间：${formatSessionUpdatedTime(
                                session.updatedAtMs,
                              )}`}
                            >
                              {formatSessionUpdatedTime(session.updatedAtMs)}
                            </span>
                          </span>
                        </label>
                      );
                    })
                  ) : (
                    <p className="task-board-picker-empty">
                      {editor.projectCwd
                        ? "该项目暂无可关联会话。"
                        : "请先选择项目。"}
                    </p>
                  )}
                </div>
              </div>
            ) : (
              <div className="task-board-new-session">
                {editor.nativeCreateAvailable !== true ? (
                  <p
                    className="task-board-create-availability"
                    data-status={
                      editor.nativeCreateAvailable === null
                        ? "checking"
                        : "unavailable"
                    }
                  >
                    {editor.nativeCreateMessage ||
                      "提交时会重新检查 Codex 新会话能力"}
                  </p>
                ) : null}
                <label className="task-board-field task-board-instruction">
                  <span>新会话首条指令</span>
                  <div className="task-board-create-composer">
                    <textarea
                      value={editor.instruction}
                      rows={4}
                      maxLength={4000}
                      onChange={(event) =>
                        onChange({
                          ...editor,
                          instruction: event.target.value,
                          feedback: "",
                        })
                      }
                      placeholder="例如：梳理任务看板的数据模型，并输出可执行方案"
                      aria-label="新会话首条指令"
                      disabled={editor.busy}
                    />
                    <TaskBoardCreateSettings
                      modelId={editor.modelId}
                      effortId={editor.effortId}
                      modelOptions={editor.modelOptions}
                      disabled={editor.busy}
                      onModelChange={(modelId, effortId) =>
                        onChange({
                          ...editor,
                          modelId,
                          effortId,
                          feedback: "",
                          modelSelectionTouched: true,
                        })
                      }
                      onEffortChange={(effortId) =>
                        onChange({
                          ...editor,
                          effortId,
                          feedback: "",
                          modelSelectionTouched: true,
                        })
                      }
                    />
                  </div>
                </label>
              </div>
            )}
          </div>

          {editor.feedback ? (
            <p className="task-board-feedback" role="alert">
              {editor.feedback}
            </p>
          ) : null}
        </div>

        <footer
          className={`task-board-modal-footer ${
            attaching ? "" : "actions-only"
          }`.trim()}
        >
          {attaching ? (
            <span className="task-board-create-note">
              <svg
                aria-hidden="true"
                viewBox="0 0 16 16"
                width="13"
                height="13"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.2"
              >
                <path
                  d="M8 2c.3 2.2 1.4 3.3 3.6 3.6C9.4 5.9 8.3 7 8 9.2 7.7 7 6.6 5.9 4.4 5.6 6.6 5.3 7.7 4.2 8 2Z"
                  strokeLinejoin="round"
                />
              </svg>
              <span>只可追加当前任务所属项目中的会话。</span>
            </span>
          ) : null}
          <div className="task-board-modal-actions">
            <button
              className="task-board-button primary"
              type="button"
              onClick={onSubmit}
              disabled={editor.busy}
            >
              {editor.busy ? (
                <LoaderCircle className="spinning" size={14} aria-hidden="true" />
              ) : (
                <Plus size={14} strokeWidth={1.3} aria-hidden="true" />
              )}
              <span>{submitLabel}</span>
            </button>
            <button
              className="task-board-button"
              type="button"
              onClick={onClose}
              disabled={editor.busy}
            >
              取消
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}

function normalizeSessionId(sessionId: string) {
  return sessionId.trim().replace(/^local:/i, "").toLocaleLowerCase();
}

function taskBoardConversationShouldDisplay(
  conversation: TaskConversation,
  activeSessionIds: Set<string>,
  catalogReady: boolean,
) {
  const sessionId = normalizeSessionId(conversation.sessionId);
  if (!sessionId) return false;
  return !catalogReady || activeSessionIds.has(sessionId);
}

function messageFromError(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) return error.message;
  const message = String(error || "").trim();
  return message || fallback;
}

function taskBoardBoardFailureMessage(
  result: BoardResponse,
  fallback: string,
) {
  if (result.code === "board_id_conflict") return "看板标识冲突，请重试";
  if (result.code === "board_not_found") return "看板不存在或已被删除";
  if (result.code === "revision_conflict") return "看板已被其他更改更新，请重试";
  if (result.code === "task_board_busy") return "任务看板正忙，请稍后重试";
  if (result.code === "task_file_invalid") return "任务文件无效，请检查后重试";
  if (result.code === "invalid_input") {
    return result.message?.includes("label")
      ? "看板名称已存在或格式无效"
      : "看板信息无效，请刷新后重试";
  }
  if (result.code === "task_board_unavailable") return "任务看板暂不可用，请稍后重试";
  return result.message || fallback;
}

function taskBoardDeleteFailureMessage(result: BoardResponse) {
  if (result.code === "invalid_input") return "任务信息无效，请刷新任务看板后重试";
  if (result.code === "task_not_found") return "任务不存在或已被删除";
  if (result.code === "revision_conflict") return "任务已被其他更改更新，请确认后重试";
  if (result.code === "task_board_busy") return "任务看板正忙，请稍后重试";
  if (result.code === "task_file_invalid") return "任务文件无效，请检查后重试";
  if (result.code === "task_board_unavailable") return "任务看板暂不可用，请稍后重试";
  return result.message || "删除任务失败";
}

function wait(delay: number) {
  return new Promise((resolve) => window.setTimeout(resolve, delay));
}
