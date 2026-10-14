import { useEffect, useRef, useState } from "react";
import { Copy, Download, ExternalLink, Eye, EyeOff, Smartphone, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { remoteAccessValidation, type RemoteAccessCredentials, type RemoteAccessSettings, type RemoteAccessStatus, type TailscaleInstall, type TailscaleInstallProgress, type TailscaleProbe } from "@/remote-access-types";

export function RemoteAccessScreen({
  settings,
  onChange,
  onSave,
  command,
  openExternalUrl,
  onNotice,
  onSuccess,
}: {
  settings: RemoteAccessSettings;
  onChange: (value: RemoteAccessSettings) => void;
  onSave: (next: RemoteAccessSettings) => Promise<void>;
  command: <T>(action: string, payload?: Record<string, unknown>) => Promise<T>;
  openExternalUrl: (url: string) => Promise<void>;
  onNotice: (message: string) => void;
  onSuccess: (message: string) => void;
}) {
  const [status, setStatus] = useState<RemoteAccessStatus | null>(null);
  const [credentials, setCredentials] = useState<RemoteAccessCredentials | null>(null);
  const [keyValue, setKeyValue] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installPercent, setInstallPercent] = useState(0);
  const [installPhase, setInstallPhase] = useState<"downloading" | "verifying">("downloading");
  const [installError, setInstallError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const mounted = useRef(true);
  const refreshing = useRef(false);
  const latestSettings = useRef(settings);
  latestSettings.current = settings;
  const patch = (value: Partial<RemoteAccessSettings>) => onChange({ ...latestSettings.current, ...value });
  const refresh = async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      const saved = await command<RemoteAccessCredentials>("credentials");
      if (!mounted.current) return;
      if (saved.status === "ok") setCredentials(saved);
      else { setCredentials(null); setMessage(saved.message || "凭据状态读取失败"); }
      const result = await command<RemoteAccessStatus>("status");
      if (!mounted.current) return;
      setStatus(result);
      if (result.status !== "ok") setMessage(result.message || "本机服务未连接");
    } catch (error) {
      if (mounted.current) setMessage(String(error));
    } finally { refreshing.current = false; }
  };
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => { mounted.current = false; clearInterval(timer); };
  }, []);
  const run = async (operation: () => Promise<void>) => {
    setBusy(true);
    setMessage("");
    try { await operation(); } catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const copy = async (value: string) => {
    try { await navigator.clipboard.writeText(value); setMessage("已复制到剪贴板"); }
    catch { setMessage("复制失败，请手动选择文本复制"); }
  };
  const funnel = status?.funnel;
  const publicUrl = funnel?.publicUrl || "";
  const actionUrl = funnel?.actionUrl || "";
  const funnelReady = funnel?.state === "ready";
  const tailscaleInstalled = Boolean(funnel?.installed);
  const tailscaleRunning = funnel?.backendState === "Running";
  const runApply = (operation: () => Promise<void>) => run(async () => {
    try { await operation(); }
    catch (error) {
      onNotice(error instanceof Error ? error.message : String(error));
    }
  });
  const toggleWebService = () => runApply(() => applySettings({ ...settings, enabled: !settings.enabled }));
  const handleToggleFunnel = () => runApply(async () => {
    if (settings.funnelEnabled) {
      await applySettings({ ...settings, funnelEnabled: false });
      return;
    }
    const reason = installing ? "Tailscale 安装包正在下载，请稍候。"
      : !settings.enabled ? "请先启用远程会话网页。"
      : remoteAccessValidation({ ...settings, funnelEnabled: true });
    if (reason) throw new Error(reason);
    const probe = await command<TailscaleProbe>("tailscaleStatus");
    if (!probe.installed) throw new Error("请先点击「安装程序」安装 Tailscale。");
    if (probe.backendState !== "Running") throw new Error(probe.message || "Tailscale 尚未连接，请先在客户端中登录。");
    await applySettings({ ...settings, funnelEnabled: true });
  });
  const installBarPercent = installPhase === "verifying" ? 100 : installPercent;
  const installProgressView = (
    <div className="remote-access-step-progress">
      <small>{installPhase === "verifying" ? "正在校验安装包签名…" : `正在下载官方安装包 ${installPercent}%`}</small>
      <div className="download-progress-bar" role="progressbar" aria-label="下载 Tailscale 安装包" aria-valuemin={0} aria-valuemax={100} aria-valuenow={installBarPercent}>
        <div className="download-progress-fill determinate" style={{ width: `${installBarPercent}%` }} />
      </div>
    </div>
  );
  const installTailscale = async () => {
    if (installing) return;
    setInstalling(true);
    setInstallError(null);
    setInstallPercent(0);
    setInstallPhase("downloading");
    try {
      const started = await command<TailscaleInstall>("installTailscale");
      if (started.status !== "ok") throw new Error(started.message || "安装包准备失败，请重试。");
      if (started.installed) {
        setMessage(started.message || "Tailscale 已安装，请点击「检查状态」。");
        return;
      }
      for (;;) {
        await new Promise(resolve => window.setTimeout(resolve, 300));
        if (!mounted.current) return;
        const progress = await command<TailscaleInstallProgress>("tailscaleInstallProgress");
        if (!mounted.current) return;
        if (progress.phase === "failed") throw new Error(progress.message || "安装包下载失败，请重试。");
        if (progress.phase === "launched") {
          setMessage(progress.message || "已启动 Tailscale 官方安装向导，完成后点击「检查状态」。");
          return;
        }
        if (progress.phase === "idle") throw new Error("安装任务未启动，请重试。");
        setInstallPercent(progress.percent);
        setInstallPhase(progress.phase === "verifying" ? "verifying" : "downloading");
      }
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error);
      setInstallError(text);
      setMessage("Tailscale 安装失败：" + text);
    } finally {
      if (mounted.current) setInstalling(false);
    }
  };  const checkStatus = () => run(async () => {
    const result = await command<RemoteAccessStatus>("status", { check: true });
    if (!mounted.current) return;
    setStatus(result);
    setMessage(result.funnel?.state === "ready" ? "Funnel 公网入口已就绪。" : "已检查，请按上方步骤完成未就绪的配置。");
  });
  const renderFunnelBadge = () => {
    if (!settings.funnelEnabled) {
      return null;
    }
    switch (funnel?.state) {
      case "ready":
        return <span className="funnel-status-badge connected"><span className="funnel-status-dot" />已就绪</span>;
      case "action_required":
        return <span className="funnel-status-badge connecting"><span className="funnel-status-dot" />待授权</span>;
      case "not_ready":
        return <span className="funnel-status-badge connecting"><span className="funnel-status-dot" />待登录</span>;
      case "not_installed":
        return <span className="funnel-status-badge"><span className="funnel-status-dot" />未安装</span>;
      case "waiting":
        return <span className="funnel-status-badge"><span className="funnel-status-dot" />等待网页服务</span>;
      case "error":
        return <span className="funnel-status-badge error" title={funnel.error || "连接异常"}><span className="funnel-status-dot" />异常</span>;
      default:
        return <span className="funnel-status-badge"><span className="funnel-status-dot" />检查中</span>;
    }
  };
  const testPublicUrl = async () => {
    if (!publicUrl) return;
    setMessage("正在测试公网连通性…");
    try {
      await fetch(publicUrl, { method: "HEAD", mode: "no-cors" });
      setMessage("公网探测完成：已向固定地址发送连通请求。若无法打开，请确认 Funnel 已就绪且网页服务正在运行。");
    } catch (err: any) {
      setMessage("公网探测失败：" + (err?.message || "网络请求异常，请确认 Funnel 已就绪"));
    }
  };
  const confirmKeyChange = () => credentials?.accessKeyConfigured === false || window.confirm("更换访问密钥会使所有已授权设备失效，需要重新输入新密钥。继续？");
  const applySettings = async (next: RemoteAccessSettings) => {
    const error = remoteAccessValidation(next);
    if (error) throw new Error(error);
    if (next.enabled && !credentials?.accessKeyConfigured) throw new Error("请先设置访问密钥，再启用远程网页。");
    onChange(next);
    try {
      await onSave(next);
    } catch (error) {
      onChange({ ...latestSettings.current, enabled: settings.enabled, funnelEnabled: settings.funnelEnabled });
      throw error;
    }
    const result = await command<RemoteAccessStatus>("apply");
    setStatus(result);
    if (result.status !== "ok") throw new Error(result.message || "配置未应用");
    if (result.error) throw new Error(result.error);
    if (next.enabled && next.funnelEnabled && result.funnel?.state !== "ready") {
      throw new Error(`设置已保存，但公网入口未就绪：${result.funnel?.error || "请按上方步骤完成 Tailscale 配置。"}`);
    }
    if (result.settings && Object.keys(next).some(key => result.settings?.[key as keyof RemoteAccessSettings] !== next[key as keyof RemoteAccessSettings])) {
      setMessage("请核对保存结果。服务正在使用已保存的配置。");
      return;
    }
    onSuccess(result.running ? `设置已保存，远程网页服务已启动。${next.funnelEnabled ? "Funnel 公网地址已就绪。" : ""}` : "设置已保存，远程访问已关闭。");
  };
  const save = () => runApply(() => applySettings(settings));
  const generateFunnelAddress = () => runApply(async () => {
    if (!settings.funnelEnabled) throw new Error("请先开启公网入口，再生成公网地址。");
    await applySettings(settings);
  });
  const steps = [
    {
      key: "install",
      title: "安装 Tailscale 客户端",
      done: tailscaleInstalled,
      detail: tailscaleInstalled ? "已检测到官方客户端。" : "未检测到客户端，点击右侧安装程序。",
      action: tailscaleInstalled ? null : (
        <Button variant="secondary" size="sm" disabled={busy || installing} onClick={() => void installTailscale()}>
          <Download className="h-4 w-4" />安装程序
        </Button>
      ),
    },    {
      key: "login",
      title: "登录并连接 Tailscale",
      done: tailscaleRunning,
      detail: tailscaleRunning ? "客户端已连接到 Tailscale 网络。" : tailscaleInstalled ? (funnel?.error || "请在 Tailscale 客户端中登录账号。") : "安装完成后登录账号。",
      action: tailscaleInstalled ? (
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => void checkStatus()}>检查状态</Button>
      ) : null,
    },
    {
      key: "funnel",
      title: "开启 Funnel 公网入口",
      done: funnelReady,
      detail: funnelReady ? `已就绪：${publicUrl}` : actionUrl ? (funnel?.error || "请在打开的 Tailscale 页面完成授权。") : "开启后自动生成固定公网地址。",
      action: (
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" disabled={busy || installing} onClick={() => void generateFunnelAddress()}>生成公网地址</Button>
          {actionUrl ? (
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void openExternalUrl(actionUrl)}>
              <ExternalLink className="h-4 w-4" />打开授权页面
            </Button>
          ) : null}
        </div>
      ),
    },
  ];

  return (
    <>
      <Card className="panel">
        <CardHeader className="panel-head pb-[10px]">
          <div className="panel-head-copy">
            <CardTitle>远程会话网页</CardTitle>
            <CardDescription>开启后可通过手机浏览器安全查看与交互本机会话。</CardDescription>
          </div>
          <div className="panel-head-actions">
            <button type="button" role="switch" aria-label="启用远程会话网页" aria-checked={settings.enabled} disabled={busy} className={`context-enabled-switch checkpoint-enabled-switch ${settings.enabled ? "active" : ""}`} onClick={() => void toggleWebService()}>
              <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
              <span>{settings.enabled ? "已启用" : "已停用"}</span>
            </button>
          </div>
        </CardHeader>
        <CardContent>
          {status?.error ? <p className="remote-access-error" role="alert">{status.error}</p> : null}
          <div className="remote-access-fields remote-access-fields-top">
          <div><Label htmlFor="remote-bind">监听 IP 地址</Label><Input id="remote-bind" value={settings.bindHost} onChange={event => patch({ bindHost: event.target.value.trim() })} /><small>127.0.0.1 仅限本机；0.0.0.0 允许局域网设备连接（需配置 HTTPS）。</small></div>
          <div><Label htmlFor="remote-port">独立端口</Label><Input id="remote-port" type="number" min={1} max={65535} value={settings.port} onChange={event => patch({ port: Number(event.target.value) })} /></div>
          <div><Label htmlFor="remote-certificate">HTTPS 证书路径（PEM）</Label><Input id="remote-certificate" placeholder="C:\certs\codex.pem" value={settings.tlsCertificatePath} onChange={event => patch({ tlsCertificatePath: event.target.value.trim() })} /></div>
          <div><Label htmlFor="remote-key">HTTPS 私钥路径（PEM）</Label><Input id="remote-key" placeholder="C:\certs\codex-key.pem" value={settings.tlsPrivateKeyPath} onChange={event => patch({ tlsPrivateKeyPath: event.target.value.trim() })} /></div>
          <div>
            <div className="flex items-center justify-between gap-2 mb-1.5">
              <Label htmlFor="remote-expiry">设备授权有效期（小时）</Label>
              <button
                type="button"
                role="switch"
                aria-label="限制设备授权有效期"
                aria-checked={settings.deviceSessionHours > 0}
                disabled={busy}
                className={`context-enabled-switch checkpoint-enabled-switch ${settings.deviceSessionHours > 0 ? "active" : ""}`}
                style={{ transform: "scale(0.85)", transformOrigin: "right center" }}
                onClick={() => {
                  if (settings.deviceSessionHours > 0) {
                    patch({ deviceSessionHours: 0 });
                  } else {
                    patch({ deviceSessionHours: 168 });
                  }
                }}
              >
                <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
                <span>{settings.deviceSessionHours > 0 ? "已开启" : "已关闭"}</span>
              </button>
            </div>
            {settings.deviceSessionHours > 0 ? (
              <Input
                id="remote-expiry"
                type="number"
                min={1}
                max={720}
                disabled={busy}
                value={settings.deviceSessionHours}
                onChange={event => {
                  const val = Number(event.target.value);
                  patch({ deviceSessionHours: Number.isNaN(val) ? 1 : Math.max(1, Math.min(720, val)) });
                }}
              />
            ) : (
              <Input
                id="remote-expiry"
                disabled
                value="未开启有效期限制（长期有效）"
              />
            )}
            <small>{settings.deviceSessionHours > 0 ? "对新登录设备生效，到期需重新输入密钥。" : "默认关闭，已授权设备长期有效。"}</small>
          </div>
        </div>
        <div className="remote-access-actions"><Button disabled={busy || installing} onClick={save}>{busy ? "处理中…" : "保存并应用"}</Button><span className="remote-access-hint">修改端口或证书会断开当前网页连接。</span></div>
        </CardContent>
      </Card>
      <Card className="panel">
        <CardHeader className="panel-head pb-[10px]">
          <div className="panel-head-copy">
            <CardTitle>公网入口 · Tailscale Funnel</CardTitle>
            <CardDescription>通过 Tailscale Funnel 发布固定 HTTPS 地址，重启后保持不变。</CardDescription>
          </div>
          <div className="panel-head-actions flex items-center gap-3">
            {renderFunnelBadge()}
            <button
              type="button"
              role="switch"
              aria-label="启用 Tailscale Funnel 公网入口"
              aria-checked={settings.funnelEnabled}
              disabled={busy}
              className={`context-enabled-switch checkpoint-enabled-switch ${settings.funnelEnabled ? "active" : ""}`}
              onClick={() => void handleToggleFunnel()}
            >
              <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
              <span>{settings.funnelEnabled ? "已启用" : "已停用"}</span>
            </button>
          </div>
        </CardHeader>
        <CardContent>
        <div className="remote-access-steps">
          {steps.map((step, index) => (
            <div className="remote-access-step" key={step.key}>
              <span className={`remote-access-step-mark ${step.done ? "done" : ""}`}>{step.done ? "✓" : index + 1}</span>
              <div className="remote-access-step-body">
                <strong>{step.title}</strong>
                {step.key === "install" && installing ? installProgressView : <small>{step.detail}</small>}
              </div>
              {step.action ? <div className="remote-access-step-action">{step.action}</div> : null}
            </div>
          ))}
        </div>
        {installError ? (
          <small className="remote-access-error block mt-1" role="alert">安装失败：{installError}</small>
        ) : null}
        <div className="remote-access-fields">
          <div className="remote-access-wide">
            <Label htmlFor="remote-funnel-url">公网地址</Label>
            <div className="remote-access-input-row">
              <div className="remote-access-key-input remote-access-public-input">
                <Input id="remote-funnel-url" readOnly placeholder="启用并应用后自动生成" value={publicUrl} />
                <div className="remote-access-key-actions">
                  <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={!publicUrl} aria-label="复制固定公网地址" onClick={() => void copy(publicUrl)}><Copy className="h-4 w-4" /></Button>
                </div>
              </div>
              <Button variant="secondary" size="sm" className="h-9" disabled={busy || !publicUrl} onClick={() => void testPublicUrl()}>测试访问</Button>
            </div>
            <small>地址由 Tailscale 分配为 *.ts.net，证书自动签发，机器名不变时地址保持不变。</small>
          </div>
        </div>
        {funnel?.state === "error" && funnel.error ? <p className="remote-access-error" role="alert">{funnel.error}</p> : null}
        <div className="remote-access-actions">
          <Button variant="secondary" disabled={busy} onClick={() => void openExternalUrl("https://login.tailscale.com/admin")}><ExternalLink className="h-4 w-4" />打开 Tailscale 管理后台</Button>
          <Button variant="secondary" disabled={busy} onClick={() => void openExternalUrl("https://tailscale.com/docs/features/tailscale-funnel")}><ExternalLink className="h-4 w-4" />Funnel 配置指南</Button>
        </div>
        </CardContent>
      </Card>
      <Card className="panel">
        <CardHeader className="panel-head pb-[10px]">
          <div className="panel-head-copy">
            <CardTitle>访问密钥与设备授权</CardTitle>
            <CardDescription>{credentials?.accessKeyConfigured ? "访问密钥已设置。" : "尚未设置访问密钥。"} 手机端输入此密钥即可连接，更换后旧设备需重新登录。</CardDescription>
          </div>
        </CardHeader>
        <CardContent>
          <div className="remote-access-fields remote-access-fields-top">
          <div className="remote-access-wide"><Label htmlFor="remote-access-key">设置访问密钥</Label><div className="remote-access-input-row remote-access-key-row"><div className="remote-access-key-input">
                <Input id="remote-access-key" type="text" className={showKey ? undefined : "remote-access-masked"} spellCheck={false} disabled={busy} autoComplete="off" minLength={32} maxLength={64} placeholder={credentials?.accessKeyConfigured ? "已设置；输入新密钥替换" : "32–64 个非空格 ASCII 字符，建议随机生成"} value={keyValue} onChange={event => setKeyValue(event.target.value)} />
                <div className="remote-access-key-actions">
                  <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={busy || !keyValue} aria-label={showKey ? "隐藏访问密钥" : "显示访问密钥"} onClick={() => setShowKey(!showKey)}>{showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button>
                  <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={busy || !keyValue} aria-label="复制访问密钥" onClick={() => void copy(keyValue)}><Copy className="h-4 w-4" /></Button>
                </div>
              </div><Button variant="secondary" disabled={busy} onClick={() => void run(async () => {
            const result = await command<{ status: string; key?: string; message?: string }>("generateKey");
            if (result.status !== "ok" || !result.key) throw new Error(result.message || "密钥生成失败");
            setKeyValue(result.key); setShowKey(false);
            setMessage("随机密钥已生成，尚未保存。点击右侧「保存」后生效，当前设备授权不受影响。");
          })}>生成</Button><Button disabled={busy || !keyValue.trim()} onClick={() => void run(async () => {
            if (!/^[\x21-\x7e]{32,64}$/.test(keyValue.trim())) throw new Error("密钥需为 32–64 个非空格 ASCII 字符。");
            if (!confirmKeyChange()) return;
            const result = await command<RemoteAccessCredentials>("setKey", { key: keyValue.trim() });
            if (result.status !== "ok") throw new Error(result.message || "访问密钥保存失败");
            setCredentials(result);
            onSuccess("访问密钥已保存，请复制妥善保管。原有设备需重新输入新密钥连接。");
          })}>保存</Button></div><small>点击「生成」后需点击「保存」生效，请妥善保存密钥。</small></div>
        </div>
        <div className="remote-access-devices">
          {(status?.devices || []).map(device => <div className="remote-access-device" key={device.id}>
            <Smartphone className="h-4 w-4" /><div><strong>{device.name}</strong><small>到期：{device.expiresAtMs > Date.now() + 365 * 24 * 3600 * 1000 ? "长期有效" : new Date(device.expiresAtMs).toLocaleString()} · 最近访问：{new Date(device.lastSeenAtMs).toLocaleString()}</small></div>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => void run(async () => {
              if (!window.confirm(`撤销「${device.name}」的授权？此设备将立即失去会话访问权限。`)) return;
              const result = await command<{ status: string; message?: string }>("revoke", { deviceId: device.id });
              if (result.status !== "ok") throw new Error(result.message || "撤销失败");
              await refresh();
              setMessage("设备授权已撤销。");
            })}><Trash2 className="h-4 w-4" />撤销</Button>
          </div>)}
          {!status?.devices?.length ? <p className="remote-access-hint">尚无已授权设备。</p> : null}
        </div>
        </CardContent>
      </Card>
      {message ? <p className="remote-access-message" role="status" aria-live="polite">{message}</p> : null}
    </>
  );
}
