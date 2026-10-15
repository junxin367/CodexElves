import { useEffect, useRef, useState } from "react";
import { Copy, Eye, EyeOff, Smartphone, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { remoteAccessValidation, type QuickTunnelAccessTest, type RemoteAccessCredentials, type RemoteAccessSettings, type RemoteAccessStatus } from "@/remote-access-types";

export function RemoteAccessScreen({
  settings,
  onChange,
  onSave,
  command,
  onNotice,
  onSuccess,
}: {
  settings: RemoteAccessSettings;
  onChange: (value: RemoteAccessSettings) => void;
  onSave: (next: RemoteAccessSettings) => Promise<void>;
  command: <T>(action: string, payload?: Record<string, unknown>) => Promise<T>;
  onNotice: (message: string) => void;
  onSuccess: (message: string) => void;
}) {
  const [status, setStatus] = useState<RemoteAccessStatus | null>(null);
  const [credentials, setCredentials] = useState<RemoteAccessCredentials | null>(null);
  const [keyValue, setKeyValue] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [publicTest, setPublicTest] = useState<QuickTunnelAccessTest | null>(null);
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
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    const interval = status?.cloudflareQuickTunnel?.state === "downloading" ? 500 : 5000;
    const timer = window.setInterval(() => void refresh(), interval);
    return () => clearInterval(timer);
  }, [status?.cloudflareQuickTunnel?.state]);
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
  const quickTunnel = status?.cloudflareQuickTunnel;
  const publicUrl = quickTunnel?.publicUrl || "";
  const quickTunnelTransitioning = quickTunnel
    ? ["downloading", "starting", "connecting", "reconnecting", "retry_wait"].includes(quickTunnel.state)
    : false;
  const quickTunnelGenerating = quickTunnel
    ? ["downloading", "starting", "connecting"].includes(quickTunnel.state)
    : false;
  const quickTunnelDownloading = Boolean(
    settings.cloudflareQuickTunnelEnabled
      && quickTunnel?.state === "downloading"
      && !quickTunnel.installed,
  );
  const quickTunnelDownloadPercent = Math.max(
    0,
    Math.min(100, Math.round(quickTunnel?.downloadPercent || 0)),
  );
  useEffect(() => { setPublicTest(null); }, [publicUrl, settings.cloudflareQuickTunnelEnabled]);
  const runApply = (operation: () => Promise<void>) => run(async () => {
    try { await operation(); }
    catch (error) {
      onNotice(error instanceof Error ? error.message : String(error));
    }
  });
  const toggleWebService = () => runApply(() => applySettings(settings.enabled
    ? { ...settings, enabled: false, cloudflareQuickTunnelEnabled: false }
    : { ...settings, enabled: true }));
  const toggleQuickTunnel = async () => {
    if (settings.cloudflareQuickTunnelEnabled) {
      await applySettings({ ...settings, cloudflareQuickTunnelEnabled: false });
      return;
    }
    const reason = !settings.enabled
      ? "请先启用远程会话网页。"
      : remoteAccessValidation({ ...settings, cloudflareQuickTunnelEnabled: true });
    if (reason) throw new Error(reason);
    await applySettings({ ...settings, cloudflareQuickTunnelEnabled: true });
  };
  const handleToggleQuickTunnel = () => runApply(toggleQuickTunnel);
  const renderQuickTunnelBadge = () => {
    if (!settings.cloudflareQuickTunnelEnabled) {
      return null;
    }
    switch (quickTunnel?.state) {
      case "ready":
        return <span className={`quick-tunnel-status-badge ${publicTest?.status === "ok" ? "connected" : ""}`}><span className="quick-tunnel-status-dot" />{publicTest?.status === "ok" ? "可访问" : "已就绪"}</span>;
      case "downloading":
        return <span className="quick-tunnel-status-badge connecting"><span className="quick-tunnel-status-dot" />正在准备组件</span>;
      case "starting":
        return <span className="quick-tunnel-status-badge connecting"><span className="quick-tunnel-status-dot" />正在启动</span>;
      case "connecting":
        return <span className="quick-tunnel-status-badge connecting"><span className="quick-tunnel-status-dot" />正在连接</span>;
      case "reconnecting":
        return <span className="quick-tunnel-status-badge connecting"><span className="quick-tunnel-status-dot" />正在重连</span>;
      case "retry_wait":
        return <span className="quick-tunnel-status-badge connecting"><span className="quick-tunnel-status-dot" />等待重试</span>;
      case "error":
        return <span className="quick-tunnel-status-badge error" title={quickTunnel.error || "连接异常"}><span className="quick-tunnel-status-dot" />异常</span>;
      default:
        return <span className="quick-tunnel-status-badge"><span className="quick-tunnel-status-dot" />已停用</span>;
    }
  };
  const testPublicUrl = () => run(async () => {
    if (!publicUrl) return;
    setPublicTest(null);
    setMessage("正在验证临时公网地址与网页回源…");
    const result = await command<QuickTunnelAccessTest>("testQuickTunnel");
    if (!mounted.current) return;
    if (result.status !== "ok") throw new Error(result.message || "公网访问验证失败，请检查网络连接后重试。");
    setPublicTest(result);
    setMessage(result.message || "公网访问验证通过。");
  });
  const confirmKeyChange = () => credentials?.accessKeyConfigured === false || window.confirm("更换访问密钥会使所有已授权设备失效，需要重新输入新密钥。继续？");
  const applySettings = async (next: RemoteAccessSettings) => {
    const error = remoteAccessValidation(next);
    if (error) throw new Error(error);
    onChange(next);
    try {
      await onSave(next);
    } catch (error) {
      onChange({ ...latestSettings.current, enabled: settings.enabled, cloudflareQuickTunnelEnabled: settings.cloudflareQuickTunnelEnabled });
      throw error;
    }
    const result = await command<RemoteAccessStatus>("apply");
    setStatus(result);
    if (result.status !== "ok") throw new Error(result.message || "配置未应用");
    if (result.error) throw new Error(result.error);
    if (result.settings && Object.keys(next).some(key => result.settings?.[key as keyof RemoteAccessSettings] !== next[key as keyof RemoteAccessSettings])) {
      setMessage("请核对保存结果。服务正在使用已保存的配置。");
      return;
    }
    const tunnelState = result.cloudflareQuickTunnel?.state;
    if (next.enabled && next.cloudflareQuickTunnelEnabled && tunnelState && tunnelState !== "ready") {
      setMessage(result.cloudflareQuickTunnel?.error || "设置已保存，Cloudflare Quick Tunnel 正在生成临时公网地址。");
    }
    const tunnelSuccess = next.cloudflareQuickTunnelEnabled
      ? tunnelState === "ready" ? "Quick Tunnel 已就绪。" : "Quick Tunnel 正在连接。"
      : "";
    const keyConfigured = result.accessKeyConfigured ?? credentials?.accessKeyConfigured ?? false;
    const authorizationNotice = keyConfigured
      ? ""
      : "访问密钥尚未设置，网页只显示登录页，无法进入会话。";
    onSuccess(result.running
      ? `设置已保存，远程网页服务已启动。${tunnelSuccess}${authorizationNotice}`
      : "设置已保存，远程访问已关闭。");
  };
  const save = () => runApply(() => applySettings(settings));
  const generateQuickTunnelAddress = () => runApply(async () => {
    if (!settings.cloudflareQuickTunnelEnabled) {
      await toggleQuickTunnel();
      return;
    }
    if (publicUrl && !window.confirm("重新生成会立即停用当前临时公网地址，已打开的手机页面将断开。继续？")) return;
    const result = await command<RemoteAccessStatus>("restartQuickTunnel");
    if (!mounted.current) return;
    setStatus(result);
    setPublicTest(null);
    if (result.status !== "ok") throw new Error(result.message || "临时公网地址重新生成失败");
    setMessage("正在重新生成临时公网地址，请稍候。");
  });

  return (
    <>
      <Card className="panel remote-access-combined-panel">
        <CardHeader className="panel-head pb-[10px]">
          <div className="panel-head-copy">
            <CardTitle>远程会话网页</CardTitle>
            <CardDescription>通过手机浏览器安全访问本机会话；修改监听地址或端口会断开当前连接。</CardDescription>
          </div>
          <div className="panel-head-actions">
            <button type="button" role="switch" aria-label="启用远程会话网页" aria-checked={settings.enabled} disabled={busy} className={`context-enabled-switch checkpoint-enabled-switch ${settings.enabled ? "active" : ""}`} onClick={() => void toggleWebService()}>
              <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
              <span>{settings.enabled ? "已启用" : "已停用"}</span>
            </button>
            <Button size="sm" disabled={busy} onClick={save}>{busy ? "处理中…" : "保存并应用"}</Button>
          </div>
        </CardHeader>
        <CardContent>
          {status?.error ? <p className="remote-access-error" role="alert">{status.error}</p> : null}
          <div className="remote-access-fields remote-access-fields-top remote-access-service-fields">
          <div><Label htmlFor="remote-bind">监听 IP 地址</Label><Input id="remote-bind" value={settings.bindHost} onChange={event => patch({ bindHost: event.target.value.trim() })} /><small>仅支持本机回环地址；公网访问请使用 Cloudflare Quick Tunnel。</small></div>
          <div><Label htmlFor="remote-port">独立端口</Label><Input id="remote-port" type="number" min={1} max={65535} value={settings.port} onChange={event => patch({ port: Number(event.target.value) })} /></div>
          <div>
            <div className="remote-access-expiry-heading">
              <Label htmlFor="remote-expiry">设备授权有效期（小时）</Label>
              <button
                type="button"
                role="switch"
                aria-label="限制设备授权有效期"
                aria-checked={settings.deviceSessionHours > 0}
                disabled={busy}
                className={`context-enabled-switch remote-access-expiry-switch ${settings.deviceSessionHours > 0 ? "active" : ""}`}
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
        </CardContent>
        <CardHeader
          aria-disabled={!settings.enabled}
          className={`panel-head border-t border-border pt-6 pb-[10px] ${!settings.enabled ? "remote-access-section-disabled" : ""}`}
        >
          <div className="panel-head-copy">
            <CardTitle>公网入口 · Cloudflare Quick Tunnel</CardTitle>
            <CardDescription>无需域名、Cloudflare 账号或额外客户端，自动生成临时 HTTPS 公网地址。</CardDescription>
          </div>
          <div className="panel-head-actions flex items-center gap-3">
            {renderQuickTunnelBadge()}
            <button
              type="button"
              role="switch"
              aria-label="启用 Cloudflare Quick Tunnel 公网入口"
              aria-checked={settings.cloudflareQuickTunnelEnabled}
              disabled={busy || !settings.enabled}
              className={`context-enabled-switch checkpoint-enabled-switch ${settings.cloudflareQuickTunnelEnabled ? "active" : ""}`}
              onClick={() => void handleToggleQuickTunnel()}
            >
              <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
              <span>{settings.cloudflareQuickTunnelEnabled ? "已启用" : "已停用"}</span>
            </button>
          </div>
        </CardHeader>
        <CardContent
          aria-disabled={!settings.enabled}
          className={!settings.enabled ? "remote-access-section-disabled" : undefined}
        >
        <div className="remote-access-fields">
          <div className="remote-access-wide">
            <div className="remote-access-field-heading">
              <Label htmlFor="remote-quick-tunnel-url">临时公网地址</Label>
              {quickTunnelDownloading ? (
                <div className="quick-tunnel-download-progress">
                  <span>{quickTunnel?.downloadTotalBytes ? `下载组件 ${quickTunnelDownloadPercent}%` : "正在下载组件"}</span>
                  <div
                    className="quick-tunnel-download-track"
                    role="progressbar"
                    aria-label="下载 Cloudflare Quick Tunnel 组件"
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={quickTunnel?.downloadTotalBytes ? quickTunnelDownloadPercent : undefined}
                  >
                    <div
                      className={`quick-tunnel-download-fill ${quickTunnel?.downloadTotalBytes ? "" : "indeterminate"}`}
                      style={quickTunnel?.downloadTotalBytes ? { width: `${quickTunnelDownloadPercent}%` } : undefined}
                    />
                  </div>
                </div>
              ) : null}
            </div>
            <div className="remote-access-input-row remote-access-public-row">
              <div className="remote-access-key-input remote-access-public-input">
                <Input id="remote-quick-tunnel-url" readOnly disabled={!settings.enabled} placeholder={quickTunnelTransitioning ? "正在生成 trycloudflare.com 临时地址…" : "启用后自动生成 trycloudflare.com 临时地址"} value={publicUrl} />
                <div className="remote-access-key-actions">
                  <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={!settings.enabled || !publicUrl} aria-label="复制临时公网地址" onClick={() => void copy(publicUrl)}><Copy className="h-4 w-4" /></Button>
                </div>
              </div>
              <Button variant="secondary" size="sm" className="h-9" disabled={busy || quickTunnelGenerating || !settings.enabled} onClick={() => void generateQuickTunnelAddress()}>{publicUrl ? "重新生成" : "生成"}</Button>
              <Button variant="secondary" size="sm" className="h-9" disabled={busy || !settings.enabled || !publicUrl} onClick={() => void testPublicUrl()}>{busy ? "处理中…" : "测试访问"}</Button>
            </div>
            <small>地址由 trycloudflare.com 临时分配，进程或应用重启、手动重新生成后会变化。公网访问仍需输入本机保存的访问密钥。</small>
          </div>
        </div>
        {["error", "retry_wait"].includes(quickTunnel?.state || "") && quickTunnel?.error ? <p className="remote-access-error" role="alert">{quickTunnel.error}</p> : null}
        {quickTunnel?.state === "retry_wait" && quickTunnel.retryAtMs ? <p className="remote-access-hint">将在 {new Date(quickTunnel.retryAtMs).toLocaleTimeString()} 自动重试。</p> : null}
        </CardContent>
        <CardHeader className="panel-head border-t border-border pt-6 pb-[10px]">
          <div className="panel-head-copy">
            <CardTitle>访问密钥与设备授权</CardTitle>
            <CardDescription>
              {credentials === null
                ? "访问密钥状态尚未确认。"
                : credentials.accessKeyConfigured ? "访问密钥已设置。" : "尚未设置访问密钥。"}{" "}
              手机端输入此密钥即可连接，更换后旧设备需重新登录。
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent>
          <div className="remote-access-fields remote-access-fields-top">
          <div className="remote-access-wide"><Label htmlFor="remote-access-key">设置访问密钥</Label><div className="remote-access-input-row remote-access-key-row"><div className="remote-access-key-input">
                <Input id="remote-access-key" type={showKey ? "text" : "password"} spellCheck={false} disabled={busy} autoComplete="off" minLength={32} maxLength={64} placeholder={credentials?.accessKeyConfigured ? "已设置；输入新密钥替换" : "32–64 个非空格 ASCII 字符，建议随机生成"} value={keyValue} onChange={event => setKeyValue(event.target.value)} />
                <div className="remote-access-key-actions">
                  <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={busy || !keyValue} aria-label={showKey ? "隐藏访问密钥" : "显示访问密钥"} onClick={() => setShowKey(value => !value)}>{showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button>
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
