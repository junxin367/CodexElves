import { useEffect, useRef, useState, type ReactNode } from "react";
import { Copy, Eye, EyeOff, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { defaultFrpConfig, frpPublicOrigin, normalizeFrpServerAddress, type FrpConfig, type PrimaryProvider, type RemoteAccessStatus } from "@/remote-access-types";

type Result = { status: string; message?: string; httpsReady?: boolean };
type Feedback = { text: string; kind: "success" | "warning" | "error" };
export function PrimaryRouteTabs({ value, onChange, disabled }: {
  value: PrimaryProvider;
  onChange: (provider: PrimaryProvider) => void;
  disabled: boolean;
}) {
  const labels: Record<PrimaryProvider, string> = {
    zrok: "免费",
    frp: "推荐速度快",
  };
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
        <span className="remote-primary-label">{labels[provider]}</span>
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
  const [operation, setOperation] = useState<"test" | "apply" | null>(null);
  const [openingDirectory, setOpeningDirectory] = useState(false);
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
  const publicOrigin = frpPublicOrigin(config);
  const downloading = primary?.frp.state === "downloading";
  const downloadTotal = primary?.frp.downloadTotalBytes || 0;
  const downloaded = primary?.frp.downloadedBytes || 0;
  const downloadPercent = downloadTotal > 0
    ? Math.max(0, Math.min(100, Math.floor(downloaded / downloadTotal * 100))) : null;
  const downloadSize = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  const current = primary?.provider || "zrok";
  const pending = busy || Boolean(operation) || Boolean(primary?.switching) || downloading;
  const known = Boolean(primary);
  const canUse = enabled && status?.running && status?.nativeConnected;
  const providerError = editing === "frp" ? primary?.frp.error
    : ["connecting", "retry_wait", "error"].includes(status?.zrok?.state || "") ? status?.zrok?.error : null;
  const actionFeedback: Feedback | null = feedback || (operation
    ? { text: operation === "apply" ? "正在验证并切换主链路…" : enabled ? "正在保存并测试入口…" : "正在保存配置…", kind: "success" }
    : providerError ? { text: providerError, kind: "error" } : null);
  const saveFrp = async () => {
    const normalized: FrpConfig = {
      ...config, serverAddr: normalizeFrpServerAddress(config.serverAddr), publicOrigin, httpsMode: "provider",
    };
    if (!normalized.serverAddr || !frpToken.trim()) throw new Error("请填写 FRP 服务器地址和 Token。");
    if (![normalized.serverPort, normalized.remotePort].every(value => Number.isInteger(value) && value >= 1 && value <= 65535))
      throw new Error("服务端口与公网映射端口必须在 1–65535 之间。");
    if (!publicOrigin) throw new Error("请填写有效的服务器地址和公网映射端口。");
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
  const execute = (action: "test" | "apply") => run(async () => {
    setOperation(action);
    setFeedback(null);
    try {
      if (editing === "frp") await saveFrp();
      if (!enabled && action === "test") {
        setFeedback({ text: "FRP 候选配置已保存；启用远程会话网页后可测试入口。", kind: "success" });
        return;
      }
      const result = await command<Result>(action === "apply" ? "applyPrimaryRoute" : "testPrimaryRoute", { provider: editing });
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
  const openProgramDirectory = async () => {
    setOpeningDirectory(true);
    try {
      const result = await command<Result>("openFrpDirectory");
      if (result.status !== "ok") throw new Error(result.message || "打开 frp 程序目录失败");
      if (!pending && result.message) setFeedback({ text: result.message, kind: "success" });
    } catch (error) {
      setFeedback({ text: error instanceof Error ? error.message : String(error), kind: "error" });
    } finally {
      setOpeningDirectory(false);
    }
  };
  return (
    <>
      {!known && status?.status === "ok" ? <p className="remote-access-hint">当前运行的远程后台尚不支持主链路切换，请更新并重新启动后台。</p> : null}
      <div id="remote-primary-panel-zrok" role="tabpanel" aria-labelledby="remote-primary-zrok" hidden={editing !== "zrok"}>{children}</div>
      <div id="remote-primary-panel-frp" role="tabpanel" aria-labelledby="remote-primary-frp" hidden={editing !== "frp"}>
        <div className="remote-frp-fields">
          <div className="remote-frp-server"><Label htmlFor="remote-frp-server">服务器地址</Label><Input id="remote-frp-server" value={config.serverAddr} disabled={pending} spellCheck={false} placeholder="商家提供的 server_addr"
            onChange={e => update({ serverAddr: e.target.value, publicOrigin: "" })}
            onBlur={() => update({ serverAddr: normalizeFrpServerAddress(config.serverAddr) })} /></div>
          <div className="remote-frp-token"><Label htmlFor="remote-frp-token">Token</Label>
            <div className="remote-access-key-input"><Input id="remote-frp-token" type={visible ? "text" : "password"} autoComplete="off" spellCheck={false} disabled={pending}
              value={frpToken} onChange={e => { tokenDirty.current = true; setFrpToken(e.target.value); setFeedback(null); }} />
              <div className="remote-access-key-actions"><Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={pending || !frpToken}
                aria-label={`${visible ? "隐藏" : "显示"} FRP Token`} aria-pressed={visible} onClick={() => setVisible(v => !v)}>{visible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button>
                <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={pending || !frpToken} aria-label="复制 FRP Token" onClick={() => void copy(frpToken)}><Copy className="h-4 w-4" /></Button></div>
            </div>
          </div>
          <div className="remote-frp-port"><Label htmlFor="remote-frp-server-port">服务端口</Label><Input id="remote-frp-server-port" type="number" min={1} max={65535} disabled={pending} value={config.serverPort || ""} onChange={e => update({ serverPort: Number(e.target.value) })} /></div>
          <div className="remote-frp-port"><Label htmlFor="remote-frp-remote-port">公网映射端口</Label><Input id="remote-frp-remote-port" type="number" min={1} max={65535} disabled={pending} value={config.remotePort || ""} onChange={e => update({ remotePort: Number(e.target.value), publicOrigin: "" })} /></div>
          <div className="remote-access-wide"><Label htmlFor="remote-frp-url">固定公网地址</Label>
            <div className="remote-access-key-input remote-access-public-input"><Input id="remote-frp-url" readOnly aria-describedby="remote-frp-url-hint" value={publicOrigin}
              placeholder="填写服务器地址和公网映射端口后自动生成" />
              <div className="remote-access-key-actions"><Button variant="ghost" size="sm" className="h-7 w-7 p-0" aria-label="复制固定公网地址" disabled={pending || !publicOrigin} onClick={() => void copy(publicOrigin)}><Copy className="h-4 w-4" /></Button></div>
            </div>
            <small id="remote-frp-url-hint">由服务器地址和公网映射端口自动生成，无需重复填写。</small>
          </div>
        </div>
      </div>
      {known ? <>
        <div className="remote-primary-actions">{downloading ? <div className="remote-frp-download">
          <span>下载 frp{downloadPercent !== null ? ` ${downloadPercent}%` : "…"}</span>
          <span>{downloadTotal > 0 ? `${downloadSize(downloaded)} / ${downloadSize(downloadTotal)}` : downloaded > 0 ? downloadSize(downloaded) : "正在连接…"}</span>
          <div className="remote-frp-download-track" role="progressbar" aria-label="下载 frp 程序" aria-valuemin={0} aria-valuemax={100} aria-valuenow={downloadPercent ?? undefined}>
            <div className={`remote-frp-download-fill${downloadPercent === null ? " indeterminate" : ""}`}
              style={downloadPercent !== null ? { width: `${downloadPercent}%` } : undefined} />
          </div>
        </div> : <p className={actionFeedback ? `remote-primary-feedback ${actionFeedback.kind}` : "remote-access-hint"} role="status" aria-live="polite" aria-atomic="true">
          {actionFeedback?.text || (editing === "frp" && publicOrigin.startsWith("http:") ? "HTTP 公网传输不加密；验证通过后可应用为主链路。"
            : current === editing ? (editing === "frp" ? "修改正在使用的 frp 配置后，请先切回 zrok，再验证并应用新配置。" : "当前主链路的配置已保留，可查看另一份配置。")
            : "切换公网地址后，远程设备需用新地址重新登录。")}
        </p>}<div className="remote-primary-buttons">
          {editing === "frp" ? <Button type="button" variant="secondary" size="sm" disabled={openingDirectory}
            onClick={() => void openProgramDirectory()}><FolderOpen className="h-4 w-4" />{openingDirectory ? "正在打开…" : "打开程序目录"}</Button> : null}
          <Button variant="secondary" size="sm" disabled={pending || !enabled && editing !== "frp"} onClick={() => void execute("test")}>{downloading ? "下载中…" : operation === "test" ? "正在处理…" : enabled ? "保存并测试" : "保存配置"}</Button>
          <Button size="sm" disabled={pending || enabled && !canUse || current === editing || editing === "frp" && !publicOrigin}
            onClick={() => void execute("apply")}>{operation === "apply" ? "正在切换…" : current === editing ? "当前主链路" : enabled ? "应用为主链路" : "保存为主链路"}</Button>
        </div></div>
      </> : null}
    </>
  );
}
