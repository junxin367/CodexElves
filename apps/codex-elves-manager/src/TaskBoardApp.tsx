import { invoke } from "@tauri-apps/api/core";
import {
  LoaderCircle,
  MessageSquare,
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
  type DragEvent,
} from "react";
import "./task-board.css";

type TaskStatus = "new" | "planning" | "executing" | "review" | "done";

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
};

type DropdownOption = {
  value: string;
  label: string;
  description?: string;
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
  tasks?: Task[];
  projects?: CatalogProject[];
  sessions?: CatalogSession[];
  warnings?: Array<{ code: string; count: number }>;
  sessionId?: string;
  canStart?: boolean;
  modelId?: string;
  effortId?: string;
  models?: CreateModelOption[];
};

type BoardSnapshot = {
  schemaVersion: number;
  revision: number;
  tasks: Task[];
};

type Catalog = {
  projects: CatalogProject[];
  sessions: CatalogSession[];
  warnings: Array<{ code: string; count: number }>;
};

type EditorState = {
  targetTask: Task | null;
  mode: "existing" | "new";
  title: string;
  projectCwd: string;
  initialStatus: TaskStatus;
  selectedSessionIds: string[];
  instruction: string;
  busy: boolean;
  feedback: string;
  nativeCreateAvailable: boolean;
  modelId: string;
  effortId: string;
  modelOptions: CreateModelOption[];
  modelSelectionTouched: boolean;
};

const statuses: Array<{ id: TaskStatus; label: string; color: string }> = [
  { id: "new", label: "新任务", color: "#94a3b8" },
  { id: "planning", label: "规划中", color: "#60a5fa" },
  { id: "executing", label: "执行中", color: "#c084fc" },
  { id: "review", label: "验收中", color: "#fbbf24" },
  { id: "done", label: "已完成", color: "#34d399" },
];

const emptySnapshot: BoardSnapshot = {
  schemaVersion: 1,
  revision: 0,
  tasks: [],
};

const emptyCatalog: Catalog = {
  projects: [],
  sessions: [],
  warnings: [],
};

const newSessionRetryDelaysMs = [250, 750, 1500, 2500, 5000];

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
  align = "end",
  placement = "auto",
  disabled = false,
  modalFocusTrap = false,
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
  align?: "start" | "end";
  placement?: "auto" | "top" | "bottom";
  disabled?: boolean;
  modalFocusTrap?: boolean;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const selectedOption = options.find((option) => option.value === value);
  const triggerLabel = selectedOption?.label || placeholder || options[0]?.label || "";

  const closeDropdown = useCallback((restoreFocus: boolean) => {
    setOpen(false);
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
    const preferredLeft =
      align === "start" ? triggerRect.left : triggerRect.right - renderedWidth;
    const left = Math.min(
      viewportWidth - renderedWidth - 8,
      Math.max(8, preferredLeft),
    );
    const fitsBelow = triggerRect.bottom + gap + renderedHeight <= viewportHeight - 8;
    const top =
      placement === "top"
        ? Math.max(8, triggerRect.top - gap - renderedHeight)
        : placement === "bottom" || fitsBelow
          ? triggerRect.bottom + gap
          : Math.max(8, triggerRect.top - gap - renderedHeight);
    menu.style.left = `${Math.max(8, left)}px`;
    menu.style.top = `${top}px`;
    menu.style.visibility = "visible";
  }, [
    align,
    fixedWidth,
    matchTriggerWidth,
    minWidth,
    open,
    options,
    placement,
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
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
        closeDropdown(true);
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
  }, [closeDropdown, modalFocusTrap, open]);

  const menu =
    open && document.body
      ? createPortal(
          <div
            className={`task-board-dropdown-menu ${menuClassName}`.trim()}
            ref={menuRef}
            role="listbox"
            aria-label={ariaLabel}
            style={{ left: 8, top: 8, visibility: "hidden" }}
          >
            {options.map((option) => {
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
                    <span className="task-board-dropdown-option-title">
                      {option.label}
                    </span>
                    {option.description ? (
                      <span className="task-board-dropdown-option-description">
                        {option.description}
                      </span>
                    ) : null}
                  </span>
                  <span className="task-board-dropdown-option-marker">
                    {selected ? (
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
                          d="m3.5 8.2 2.8 2.8 6.2-6.2"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        />
                      </svg>
                    ) : null}
                  </span>
                </button>
              );
            })}
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
        onClick={() => setOpen((current) => !current)}
      >
        <span className="task-board-dropdown-label">
          {triggerLabel}
        </span>
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
    const left = Math.min(
      viewportWidth - width - 8,
      Math.max(8, triggerRect.right - width),
    );
    const top = Math.max(8, triggerRect.top - 6 - height);
    menu.style.left = `${Math.max(8, left)}px`;
    menu.style.top = `${Math.min(viewportHeight - height - 8, top)}px`;
    menu.style.visibility = "visible";
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !submenuKind) return;
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    const submenu = submenuRef.current;
    const parentButton = menu?.querySelector<HTMLButtonElement>(
      `[data-settings-kind="${submenuKind}"]`,
    );
    if (!trigger || !menu || !submenu || !parentButton) return;
    submenu.style.minWidth = "220px";
    const menuRect = menu.getBoundingClientRect();
    const parentRect = parentButton.getBoundingClientRect();
    const submenuRect = submenu.getBoundingClientRect();
    const triggerRect = trigger.getBoundingClientRect();
    const viewportWidth = window.innerWidth || 1024;
    const viewportHeight = window.innerHeight || 768;
    const width = submenuRect.width || 220;
    const height = submenuRect.height || 0;
    const gap = 6;
    const fitsRight = menuRect.right + gap + width <= viewportWidth - 8;
    const left = fitsRight
      ? menuRect.right + gap
      : Math.max(8, menuRect.left - gap - width);
    const opensAbove = menuRect.bottom <= triggerRect.top;
    const preferredTop = opensAbove
      ? menuRect.bottom - height
      : parentRect.top - 5;
    const top = Math.min(
      viewportHeight - height - 8,
      Math.max(8, preferredTop),
    );
    submenu.style.left = `${Math.max(8, left)}px`;
    submenu.style.top = `${Math.max(8, top)}px`;
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
                      <span className="task-board-dropdown-option-marker">
                        {selected ? (
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
                              d="m3.5 8.2 2.8 2.8 6.2-6.2"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            />
                          </svg>
                        ) : null}
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
  const [query, setQuery] = useState("");
  const [projectFilter, setProjectFilter] = useState("");
  const [loading, setLoading] = useState(true);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [dragTaskId, setDragTaskId] = useState("");
  const [dropStatus, setDropStatus] = useState<TaskStatus | "">("");
  const [toast, setToast] = useState("");

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

  const refresh = useCallback(
    async () => {
      try {
        const [board, sessions] = await Promise.all([
          invoke<BoardResponse>("task_board_load_snapshot"),
          invoke<BoardResponse>("task_board_load_catalog"),
        ]);
        applySnapshotResponse(board, "任务看板读取失败");
        if (sessions.status === "ok") {
          setCatalog({
            projects: sessions.projects ?? [],
            sessions: sessions.sessions ?? [],
            warnings: sessions.warnings ?? [],
          });
        } else {
          showToast(sessions.message || "会话目录读取失败");
        }
      } catch (error) {
        showToast(messageFromError(error, "任务看板读取失败"));
      } finally {
        setLoading(false);
      }
    },
    [applySnapshotResponse, showToast],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

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
    void invoke<BoardResponse>("task_board_probe_host", { project })
      .then((result) => {
        if (cancelled) return;
        setEditor((current) => {
          if (!current || current.projectCwd !== projectCwd) return current;
          const nativeCreateAvailable =
            result.status === "ok" && result.canStart === true;
          return {
            ...current,
            mode:
              !nativeCreateAvailable && current.mode === "new"
                ? "existing"
                : current.mode,
            nativeCreateAvailable,
          };
        });
      })
      .catch(() => {
        if (cancelled) return;
        setEditor((current) => {
          if (!current || current.projectCwd !== projectCwd) return current;
          return { ...current, nativeCreateAvailable: false };
        });
      });
    return () => {
      cancelled = true;
    };
  }, [catalog.projects, editor?.projectCwd, editor?.targetTask?.id]);

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

  const tasksWithCatalogTitles = useMemo(
    () =>
      snapshot.tasks.map((task) => {
        let changed = false;
        const conversations = task.conversations.map((conversation) => {
          const title =
            catalogSessionTitles.get(normalizeSessionId(conversation.sessionId)) ||
            conversation.title;
          if (title === conversation.title) return conversation;
          changed = true;
          return { ...conversation, title };
        });
        return changed ? { ...task, conversations } : task;
      }),
    [catalogSessionTitles, snapshot.tasks],
  );

  const visibleTasks = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase("zh-CN");
    return tasksWithCatalogTitles.filter((task) => {
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
  }, [projectFilter, query, tasksWithCatalogTitles]);

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

  const openConversation = useCallback(
    async (conversation: TaskConversation) => {
      try {
        const result = await invoke<BoardResponse>("task_board_open_session", {
          request: conversation,
        });
        if (result.status !== "ok") {
          showToast(result.message || "无法打开关联会话");
        }
      } catch (error) {
        showToast(messageFromError(error, "无法打开关联会话"));
      }
    },
    [showToast],
  );

  const closeEditor = useCallback(() => {
    setEditor((current) => (current?.busy ? current : null));
  }, []);

  const detachConversation = useCallback(
    async (task: Task, conversation: TaskConversation) => {
      if (!window.confirm(`从任务“${task.title}”移除会话“${conversation.title}”？`)) {
        return;
      }
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
        }
      } catch (error) {
        showToast(messageFromError(error, "移除会话失败"));
      }
    },
    [applySnapshotResponse, showToast, snapshot.revision],
  );

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
      setEditor({
        targetTask: task,
        mode: "existing",
        title: task?.title || "",
        projectCwd: defaultProject,
        initialStatus: "new",
        selectedSessionIds: defaultSession ? [defaultSession.sessionId] : [],
        instruction: "",
        busy: false,
        feedback: "",
        nativeCreateAvailable: false,
        modelId: "",
        effortId: "medium",
        modelOptions: fallbackModelOptions,
        modelSelectionTouched: false,
      });
    },
    [catalog.projects, catalog.sessions, projectFilter],
  );

  const submitEditor = useCallback(async () => {
    if (!editor) return;
    const project = catalog.projects.find(
      (candidate) => candidate.cwd === editor.projectCwd,
    );
    if (!project) {
      setEditor({ ...editor, feedback: "请选择项目" });
      return;
    }
    if (!editor.targetTask && !editor.title.trim()) {
      setEditor({ ...editor, feedback: "请输入任务名称" });
      return;
    }
    if (editor.mode === "existing" && editor.selectedSessionIds.length === 0) {
      setEditor({ ...editor, feedback: "至少选择一个会话" });
      return;
    }
    if (editor.mode === "new" && !editor.instruction.trim()) {
      setEditor({ ...editor, feedback: "请输入首条指令" });
      return;
    }

    const busyEditor = { ...editor, busy: true, feedback: "" };
    setEditor(busyEditor);
    try {
      let sessionIds = editor.selectedSessionIds;
      if (editor.mode === "new") {
        const probe = await invoke<BoardResponse>("task_board_probe_host", {
          project,
        });
        if (probe.status !== "ok") {
          setEditor({
            ...busyEditor,
            busy: false,
            feedback: probe.message || "当前项目暂不支持新建会话",
          });
          return;
        }
        const started = await invoke<BoardResponse>("task_board_start_conversation", {
          request: {
            project,
            firstInstruction: editor.instruction.trim(),
            modelId: editor.modelId,
            effortId: editor.effortId,
          },
        });
        if (started.status !== "ok" || !started.sessionId) {
          setEditor({
            ...busyEditor,
            busy: false,
            feedback: started.message || "新建 Codex 会话失败",
          });
          return;
        }
        sessionIds = [started.sessionId];
      }

      const command = editor.targetTask
        ? "task_board_attach_conversations"
        : "task_board_create_task";
      const request = editor.targetTask
        ? {
            taskId: editor.targetTask.id,
            expectedRevision: snapshot.revision,
            sessionIds,
          }
        : {
            taskId: crypto.randomUUID(),
            expectedRevision: snapshot.revision,
            title: editor.title.trim(),
            project: { cwd: project.cwd, label: project.label },
            sessionIds,
          };
      let result = await invoke<BoardResponse>(command, { request });
      if (editor.mode === "new") {
        for (const delayMs of newSessionRetryDelaysMs) {
          if (result.code !== "session_not_found") break;
          await wait(delayMs);
          const sessions = await invoke<BoardResponse>("task_board_load_catalog");
          if (sessions.status === "ok") {
            setCatalog({
              projects: sessions.projects ?? [],
              sessions: sessions.sessions ?? [],
              warnings: sessions.warnings ?? [],
            });
          }
          result = await invoke<BoardResponse>(command, { request });
        }
      }
      if (!applySnapshotResponse(result, editor.targetTask ? "关联会话失败" : "创建任务失败")) {
        setEditor({
          ...busyEditor,
          busy: false,
          feedback: result.message || "操作失败",
        });
        return;
      }

      if (!editor.targetTask && editor.initialStatus !== "new" && Array.isArray(result.tasks)) {
        const created = result.tasks.find(
          (task) =>
            task.title === editor.title.trim() &&
            task.project.cwd === project.cwd &&
            sessionIds.some((sessionId) =>
              task.conversations.some(
                (conversation) => conversation.sessionId === sessionId,
              ),
            ),
        );
        if (created) {
          const targetCount = result.tasks.filter(
            (task) => task.status === editor.initialStatus,
          ).length;
          const moved = await invoke<BoardResponse>("task_board_move_task", {
            request: {
              taskId: created.id,
              toStatus: editor.initialStatus,
              targetIndex: targetCount,
              expectedRevision: result.revision ?? snapshot.revision,
            },
          });
          applySnapshotResponse(moved, "任务已创建，但初始状态设置失败");
        }
      }

      setEditor(null);
      showToast(editor.targetTask ? "会话已关联到任务" : "任务已创建");
      void refresh();
    } catch (error) {
      setEditor({
        ...busyEditor,
        busy: false,
        feedback: messageFromError(error, "操作失败"),
      });
    }
  }, [applySnapshotResponse, catalog.projects, editor, refresh, showToast, snapshot.revision]);

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

  return (
    <main className="task-board-app">
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
            align="start"
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
          <div className="task-board-columns" aria-label="任务状态列">
            {statuses.map((status) => {
              const tasks = columnTasks(status.id);
              return (
                <section
                  className="task-board-column"
                  key={status.id}
                    style={
                      {
                        "--task-board-status-color": status.color,
                      } as React.CSSProperties
                    }
                  >
                    <header className="task-board-column-head">
                    <div className="task-board-column-title">
                      <span className="task-board-status-dot" aria-hidden="true" />
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
                      event.preventDefault();
                      setDropStatus("");
                      const task = snapshot.tasks.find(
                        (candidate) => candidate.id === dragTaskId,
                      );
                      setDragTaskId("");
                      if (task && task.status !== status.id) {
                        void moveTask(task, status.id);
                      }
                    }}
                  >
                    {tasks.map((task) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        onDragStart={(event) => {
                          setDragTaskId(task.id);
                          event.dataTransfer.effectAllowed = "move";
                        }}
                        onDragEnd={() => {
                          setDragTaskId("");
                          setDropStatus("");
                        }}
                        dragging={dragTaskId === task.id}
                        onOpenConversation={openConversation}
                        onDetachConversation={detachConversation}
                        onAttach={() => openEditor(task)}
                        onMoveStatus={(status) => void moveTask(task, status)}
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
  );
}

function TaskCard({
  task,
  dragging,
  onDragStart,
  onDragEnd,
  onOpenConversation,
  onDetachConversation,
  onAttach,
  onMoveStatus,
}: {
  task: Task;
  dragging: boolean;
  onDragStart: (event: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onOpenConversation: (conversation: TaskConversation) => void;
  onDetachConversation: (task: Task, conversation: TaskConversation) => void;
  onAttach: () => void;
  onMoveStatus: (status: TaskStatus) => void;
}) {
  return (
    <article
      className={`task-board-card ${dragging ? "dragging" : ""}`}
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
    >
      <div className="task-board-project">{task.project.label}</div>
      <h2 className="task-board-card-title">{task.title}</h2>
      <div className="task-board-conversations">
        {task.conversations.length ? (
          task.conversations.map((conversation) => (
            <div className="task-board-conversation-row" key={conversation.sessionId}>
              <button
                className="task-board-conversation"
                type="button"
                onClick={() => onOpenConversation(conversation)}
                title={conversation.title || conversation.sessionId}
              >
                <span className="task-board-conversation-icon" aria-hidden="true">
                  <MessageSquare size={14} strokeWidth={1.2} />
                </span>
                <span className="task-board-conversation-title">
                  {conversation.title || "未命名会话"}
                </span>
              </button>
              <button
                className="task-board-conversation-remove"
                type="button"
                aria-label={`移除会话 ${conversation.title || conversation.sessionId}`}
                onClick={() => onDetachConversation(task, conversation)}
              >
                <X size={13} strokeWidth={1.35} aria-hidden="true" />
              </button>
            </div>
          ))
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
          options={statuses.map((status) => ({
            value: status.id,
            label: status.label,
          }))}
          minWidth={150}
          onChange={(status) => onMoveStatus(status as TaskStatus)}
        />
      </footer>
    </article>
  );
}

function TaskEditor({
  editor,
  projects,
  sessions,
  onChange,
  onClose,
  onSubmit,
}: {
  editor: EditorState;
  projects: CatalogProject[];
  sessions: CatalogSession[];
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
                  align="start"
                  modalFocusTrap
                  onChange={(projectCwd) =>
                    onChange({
                      ...editor,
                      projectCwd,
                      selectedSessionIds: [],
                      feedback: "",
                      nativeCreateAvailable: false,
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
                  options={statuses.map((status) => ({
                    value: status.id,
                    label: status.label,
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
                disabled={
                  editor.busy ||
                  !editor.projectCwd ||
                  !editor.nativeCreateAvailable
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
                          <span
                            className="task-board-session-title"
                            title={session.title || "未命名会话"}
                          >
                            {session.title || "未命名会话"}
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
                      disabled={editor.busy || !editor.nativeCreateAvailable}
                    />
                    <TaskBoardCreateSettings
                      modelId={editor.modelId}
                      effortId={editor.effortId}
                      modelOptions={editor.modelOptions}
                      disabled={editor.busy || !editor.nativeCreateAvailable}
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

function messageFromError(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) return error.message;
  const message = String(error || "").trim();
  return message || fallback;
}

function wait(delay: number) {
  return new Promise((resolve) => window.setTimeout(resolve, delay));
}
