import { useEffect, useRef, useState, type ReactNode } from "react";
import { Copy, Eye, EyeOff, Link2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { defaultFrpConfig, type FrpConfig, type PrimaryProvider, type RemoteAccessStatus } from "@/remote-access-types";

type Result = { status: string; message?: string; httpsReady?: boolean };
type Feedback = { text: string; kind: "success" | "warning" | "error" };
function parsePublicOrigin(value: string): URL | null {
  try {
    const url = new URL(value.trim());
    return url.protocol === "https:" && url.pathname === "/" && !url.search && !url.hash && !url.username && !url.password ? url : null;
  } catch { return null; }
}
export function PrimaryRouteTabs({ value, onChange, disabled }: {
  value: PrimaryProvider;
  onChange: (provider: PrimaryProvider) => void;
  disabled: boolean;
}) {
  return <div className="remote-primary-breadcrumbs" role="tablist" aria-label="主链路配置">
    {(["zrok", "frp"] as const).map((provider, index) => (
      <span className="remote-primary-crumb" key={provider}>
        {index > 0 ? <span aria-hidden="true">/</span> : null}
        <button id={`remote-primary-${provider}`} type="button" role="tab"
          aria-selected={value === provider} aria-controls={`remote-primary-panel-${provider}`}
          tabIndex={value === provider ? 0 : -1} disabled={disabled}
          onClick={() => onChange(provider)}
          onKeyDown={event => {
            if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === "Home" ? "zrok" : event.key === "End" ? "frp" : provider === "zrok" ? "frp" : "zrok";
            onChange(next);
            document.getElementById(`remote-primary-${next}`)?.focus();
          }}>{provider}</button>
      </span>
    ))}
  </div>;
}

export function PrimaryRouteControls({
  status, token, busy, enabled, editing, target, command, run, refresh, children,
}: {
  status: RemoteAccessStatus | null;
  token?: string;
  busy: boolean;
  enabled: boolean;
  editing: PrimaryProvider;
  target: string;
  command: <T>(action: string, payload?: Record<string, unknown>) => Promise<T>;
  run: (operation: () => Promise<void>) => Promise<void>;
  refresh: () => Promise<void>;
  children: ReactNode;
}) {
  const primary = status?.primary;
  const [config, setConfig] = useState<FrpConfig>({ ...defaultFrpConfig });
  const [frpToken, setFrpToken] = useState("");
  const [visible, setVisible] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [operation, setOperation] = useState<"test" | "apply" | "testCurrent" | null>(null);
  const initialized = useRef(false);
  const tokenDirty = useRef(false);
  const loadedToken = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!primary || initialized.current) return;
    initialized.current = true;
    setConfig({ ...defaultFrpConfig, ...primary.frp.config, httpsMode: "provider" });
  }, [primary]);
  useEffect(() => { setFeedback(null); }, [editing, enabled]);
  useEffect(() => {
    if (!tokenDirty.current && token !== undefined && token !== loadedToken.current) {
      loadedToken.current = token;
      setFrpToken(token);
      setVisible(false);
    }
  }, [token]);
  const update = (patch: Partial<FrpConfig>) => {
    setConfig(value => ({ ...value, ...patch }));
    setFeedback(null);
  };
  const publicOrigin = parsePublicOrigin(config.publicOrigin);
  const invalidOrigin = Boolean(config.publicOrigin.trim()) && !publicOrigin;
  const current = primary?.provider || "zrok";
  const pending = busy || Boolean(operation) || Boolean(primary?.switching);
  const known = Boolean(primary);
  const canUse = enabled && status?.running && status?.nativeConnected;
  const actionFeedback: Feedback | null = feedback || (operation
    ? { text: operation === "apply" ? "正在验证并切换主链路…" : operation === "testCurrent" ? "正在测试当前公网地址…" : enabled ? "正在保存并测试入口…" : "正在保存配置…", kind: "success" }
    : editing === "frp" && primary?.frp.error ? { text: primary.frp.error, kind: "error" } : null);
  const saveFrp = async () => {
    const normalized: FrpConfig = {
      ...config, serverAddr: config.serverAddr.trim(), publicOrigin: config.publicOrigin.trim(), httpsMode: "provider",
    };
    if (!normalized.serverAddr || !frpToken.trim()) throw new Error("请填写 FRP 服务器地址和 Token。");
    if (![normalized.serverPort, normalized.remotePort].every(value => Number.isInteger(value) && value >= 1 && value <= 65535))
      throw new Error("服务端口与公网映射端口必须在 1–65535 之间。");
    if (normalized.publicOrigin) {
      if (!publicOrigin) throw new Error("固定公网地址需为 HTTPS 根地址，可包含端口，不能包含路径、账号或参数。");
      normalized.publicOrigin = publicOrigin.origin;
    }
    const result = await command<Result>("saveFrp", {
      config: normalized, token: frpToken.trim(),
    });
    if (result.status !== "ok") throw new Error(result.message || "FRP 配置保存失败");
    setConfig(normalized);
    // Polling that started before this save must not restore an older credential.
    tokenDirty.current = true;
    loadedToken.current = frpToken.trim();
    setFrpToken(frpToken.trim());
    setVisible(false);
  };
  const execute = (action: "test" | "apply" | "testCurrent") => run(async () => {
    setOperation(action);
    setFeedback(null);
    try {
      if (action !== "testCurrent" && editing === "frp") await saveFrp();
      if (!enabled && action === "test") {
        setFeedback({ text: "FRP 候选配置已保存；启用远程会话网页后可测试入口。", kind: "success" });
        return;
      }
      const result = await command<Result>(action === "apply" ? "applyPrimaryRoute" : "testPrimaryRoute", { provider: action === "testCurrent" ? current : editing });
      if (result.status !== "ok") throw new Error(result.message || "主入口验证失败");
      setFeedback({ text: result.message || "操作完成。", kind: result.httpsReady === false ? "warning" : "success" });
    } catch (error) {
      setFeedback({ text: error instanceof Error ? error.message : String(error), kind: "error" });
    } finally {
      try { await refresh(); } finally { setOperation(null); }
    }
  });
  const copy = async (value: string) => {
    try { await navigator.clipboard.writeText(value); setFeedback({ text: "已复制到剪贴板。", kind: "success" }); }
    catch { setFeedback({ text: "复制失败，请手动选择文本复制。", kind: "error" }); }
  };
  return (
    <>
      {!known && status?.status === "ok" ? <p className="remote-access-hint">当前运行的远程后台尚不支持主链路切换，请更新并重新启动后台。</p> : null}
      <div id="remote-primary-panel-zrok" role="tabpanel" aria-labelledby="remote-primary-zrok" hidden={editing !== "zrok"}>{children}</div>
      <div id="remote-primary-panel-frp" role="tabpanel" aria-labelledby="remote-primary-frp" hidden={editing !== "frp"}>
        <div className="remote-frp-fields">
          <div className="remote-frp-server"><Label htmlFor="remote-frp-server">服务器地址</Label><Input id="remote-frp-server" value={config.serverAddr} disabled={pending} spellCheck={false} placeholder="商家提供的 server_addr" onChange={e => update({ serverAddr: e.target.value })} /></div>
          <div className="remote-frp-token"><Label htmlFor="remote-frp-token">Token</Label>
            <div className="remote-access-key-input"><Input id="remote-frp-token" type={visible ? "text" : "password"} autoComplete="off" spellCheck={false} disabled={pending}
              value={frpToken} onChange={e => { tokenDirty.current = true; setFrpToken(e.target.value); setFeedback(null); }} />
              <div className="remote-access-key-actions"><Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={pending || !frpToken}
                aria-label={`${visible ? "隐藏" : "显示"} FRP Token`} aria-pressed={visible} onClick={() => setVisible(v => !v)}>{visible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button>
                <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={pending || !frpToken} aria-label="复制 FRP Token" onClick={() => void copy(frpToken)}><Copy className="h-4 w-4" /></Button></div>
            </div>
          </div>
          <div className="remote-frp-port"><Label htmlFor="remote-frp-server-port">服务端口</Label><Input id="remote-frp-server-port" type="number" min={1} max={65535} disabled={pending} value={config.serverPort || ""} onChange={e => update({ serverPort: Number(e.target.value) })} /></div>
          <div className="remote-frp-port"><Label htmlFor="remote-frp-remote-port">公网映射端口</Label><Input id="remote-frp-remote-port" type="number" min={1} max={65535} disabled={pending} value={config.remotePort || ""} onChange={e => update({ remotePort: Number(e.target.value) })} /></div>
          <div className="remote-access-wide"><Label htmlFor="remote-frp-url">固定公网地址</Label><Input id="remote-frp-url" type="url" autoComplete="off" aria-invalid={invalidOrigin || undefined} aria-describedby="remote-frp-url-hint" disabled={pending} value={config.publicOrigin} spellCheck={false}
            placeholder="https://服务商提供的访问地址:端口" onChange={e => update({ publicOrigin: e.target.value })} />
            <small id="remote-frp-url-hint">填写手机访问的完整地址；可稍后填写，先保存并测试隧道。</small>
          </div>
        </div>
      </div>
      {known ? <>
        <div className="remote-primary-actions"><p className={actionFeedback ? `remote-primary-feedback ${actionFeedback.kind}` : invalidOrigin && editing === "frp" ? "remote-primary-feedback error" : "remote-access-hint"} role="status" aria-live="polite" aria-atomic="true">
          {actionFeedback?.text || (editing === "frp" && invalidOrigin ? "固定公网地址需为 HTTPS 根地址，可包含端口，不能包含路径、账号或参数。"
            : current === editing ? (editing === "frp" ? "修改正在使用的 FRP 配置后，请先切回 zrok，再验证并应用新配置。" : "当前主链路的配置已保留，可查看另一份配置。") : editing === "frp" && !publicOrigin
            ? "HTTPS 入口验证通过后，才能应用为会话主链路。" : "切换公网地址后，远程设备需用新地址重新登录。")}
        </p><div className="remote-primary-buttons">
          <Button variant="secondary" size="sm" disabled={pending || !enabled && editing !== "frp"} onClick={() => void execute("test")}>{operation === "test" ? "正在处理…" : enabled ? "保存并测试" : "保存配置"}</Button>
          <Button size="sm" disabled={pending || enabled && !canUse || current === editing || editing === "frp" && !publicOrigin}
            onClick={() => void execute("apply")}>{operation === "apply" ? "正在切换…" : current === editing ? "当前主链路" : enabled ? "应用为主链路" : "保存为主链路"}</Button>
        </div></div>
        <div className="remote-primary-current remote-access-fields"><div className="remote-access-wide"><Label htmlFor="remote-primary-url">当前公网地址 <span className="remote-primary-provider-label">{current === "frp" ? "FRP" : "zrok"}</span></Label>
          <div className="remote-access-input-row"><div className="remote-access-key-input remote-access-public-input"><Input id="remote-primary-url" readOnly value={primary?.publicUrl || ""} placeholder="主入口启动后显示" /><div className="remote-access-key-actions"><Button variant="ghost" size="sm" className="h-7 w-7 p-0" aria-label="复制当前公网地址" disabled={pending || !primary?.publicUrl} onClick={() => void copy(primary?.publicUrl || "")}><Copy className="h-4 w-4" /></Button></div></div>
            <Button variant="secondary" size="sm" disabled={pending || !enabled || !primary?.publicUrl}
              onClick={() => void execute("testCurrent")}>{operation === "testCurrent" ? "正在测试…" : "测试访问"}</Button></div>
        </div></div>
      </> : null}
    </>
  );
}
