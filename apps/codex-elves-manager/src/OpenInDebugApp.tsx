import {
  ChevronDown,
  Folder,
  Github,
  PanelLeft,
  Play,
  Search,
  SquarePen,
  Terminal,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";

type OpenTargetId =
  | "vscode"
  | "visualStudio"
  | "sublimeText"
  | "githubDesktop"
  | "fileManager"
  | "gitBash"
  | "cmder"
  | "rider";

type OpenTarget = {
  id: OpenTargetId;
  label: string;
};

const targets: OpenTarget[] = [
  { id: "vscode", label: "VS Code" },
  { id: "visualStudio", label: "Visual Studio" },
  { id: "sublimeText", label: "Sublime Text" },
  { id: "githubDesktop", label: "GitHub Desktop" },
  { id: "fileManager", label: "File Explorer" },
  { id: "gitBash", label: "Git Bash" },
  { id: "cmder", label: "Cmder" },
  { id: "rider", label: "Rider" },
];

function TargetIcon({ target }: { target: OpenTarget }) {
  if (target.id === "vscode") {
    return (
      <svg
        aria-hidden="true"
        className="open-in-product-glyph open-in-vscode-glyph"
        viewBox="0 0 24 24"
      >
        <path d="M17.25 2.8 7.57 11.6 3.4 8.43 1.5 9.5v5l1.9 1.08 4.17-3.18 9.68 8.8L22.5 19V5l-5.25-2.2Zm-.2 5.08v8.24L11.75 12l5.3-4.12Z" />
      </svg>
    );
  }
  if (target.id === "visualStudio") {
    return (
      <svg
        aria-hidden="true"
        className="open-in-product-glyph open-in-visual-studio-glyph"
        viewBox="0 0 24 24"
      >
        <path d="m17.34 2.5-8.8 6.7-4.42-3.42L1.5 7.1v9.8l2.62 1.32 4.42-3.42 8.8 6.7 5.16-2.06V4.56L17.34 2.5ZM4.65 9.35 7.7 12l-3.05 2.65v-5.3Zm12.5-1.82v8.94L11.28 12l5.87-4.47Z" />
      </svg>
    );
  }
  if (target.id === "sublimeText") {
    return (
      <span className="open-in-sublime-glyph" aria-hidden="true">
        S
      </span>
    );
  }
  if (target.id === "fileManager") {
    return <Folder className="open-in-app-glyph open-in-folder-glyph" fill="currentColor" />;
  }
  if (target.id === "githubDesktop") {
    return <Github className="open-in-app-glyph open-in-github-glyph" fill="currentColor" />;
  }
  if (target.id === "cmder") {
    return <Terminal className="open-in-app-glyph open-in-terminal-glyph" />;
  }
  if (target.id === "gitBash") {
    return <Terminal className="open-in-app-glyph open-in-git-bash-glyph" />;
  }
  if (target.id === "rider") {
    return (
      <span className="open-in-rider-glyph" aria-hidden="true">
        RD
      </span>
    );
  }
  return null;
}

function OpenInControl({
  selected,
  onSelect,
  onOpen,
}: {
  selected: OpenTarget;
  onSelect: (target: OpenTarget) => void;
  onOpen: (target: OpenTarget) => void;
}) {
  const [menuOpen, setMenuOpen] = useState(true);
  const groupRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const dismiss = (event: PointerEvent) => {
      if (!groupRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    const dismissWithKeyboard = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenuOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", dismissWithKeyboard);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", dismissWithKeyboard);
    };
  }, []);

  return (
    <div className="legacy-open-in" ref={groupRef}>
      <div className="legacy-open-in-trigger">
        <button
          className="legacy-open-in-primary"
          onClick={() => onOpen(selected)}
          title={`Open in ${selected.label}`}
          type="button"
        >
          <TargetIcon target={selected} />
        </button>
        <button
          aria-expanded={menuOpen}
          aria-haspopup="menu"
          aria-label="Choose application"
          className="legacy-open-in-chevron"
          onClick={() => setMenuOpen((current) => !current)}
          title="Choose application"
          type="button"
        >
          <ChevronDown aria-hidden="true" />
        </button>
      </div>
      {menuOpen ? (
        <div aria-label="Open in" className="legacy-open-in-menu" role="menu">
          <div className="legacy-open-in-menu-heading">Open in</div>
          {targets.map((target) => (
            <button
              className={target.id === selected.id ? "is-selected" : undefined}
              key={target.id}
              onClick={() => {
                onSelect(target);
                onOpen(target);
                setMenuOpen(false);
              }}
              role="menuitem"
              type="button"
            >
              <span className="legacy-open-in-menu-icon">
                <TargetIcon target={target} />
              </span>
              <span>{target.label}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function OpenInDebugApp() {
  const [selectedTarget, setSelectedTarget] = useState<OpenTarget>(
    () => targets.find((target) => target.id === "visualStudio") ?? targets[0],
  );
  const [lastAction, setLastAction] = useState("Ready");

  const openTarget = (target: OpenTarget) => {
    setLastAction(`Opened workspace in ${target.label}`);
  };

  return (
    <main className="open-in-debug-root">
      <section className="open-in-debug-workbench" aria-label="Legacy Codex Open in preview">
        <div className="open-in-debug-caption">
          <div>
            <span className="open-in-debug-kicker">DEBUG PREVIEW</span>
            <strong>Legacy Codex Open in</strong>
          </div>
          <span className="open-in-debug-status">{lastAction}</span>
        </div>

        <div className="open-in-debug-window">
          <header className="open-in-debug-titlebar">
            <div className="open-in-debug-titlebar-start">
              <button aria-label="Toggle sidebar" className="open-in-debug-icon-button" type="button">
                <PanelLeft aria-hidden="true" />
              </button>
              <button aria-label="New thread" className="open-in-debug-icon-button" type="button">
                <SquarePen aria-hidden="true" />
              </button>
            </div>
            <div className="open-in-debug-project">
              <strong>CodexElves</strong>
              <span>main</span>
            </div>
            <div className="open-in-debug-titlebar-actions">
              <button aria-label="Run" className="open-in-debug-icon-button" type="button">
                <Play aria-hidden="true" />
              </button>
              <OpenInControl
                selected={selectedTarget}
                onOpen={openTarget}
                onSelect={setSelectedTarget}
              />
            </div>
          </header>

          <div className="open-in-debug-body">
            <aside className="open-in-debug-sidebar">
              <button className="open-in-debug-search" type="button">
                <Search aria-hidden="true" />
                <span>Search</span>
              </button>
              <div className="open-in-debug-sidebar-heading">Threads</div>
              <div className="open-in-debug-thread is-active">Open In titlebar control</div>
              <div className="open-in-debug-thread">Task board polish</div>
              <div className="open-in-debug-thread">Windows installer</div>
            </aside>
            <section className="open-in-debug-conversation">
              <div className="open-in-debug-conversation-title">Open In titlebar control</div>
              <div className="open-in-debug-copy">
                <span />
                <span />
                <span />
              </div>
              <div className="open-in-debug-copy is-short">
                <span />
                <span />
              </div>
            </section>
          </div>
        </div>
      </section>
    </main>
  );
}
