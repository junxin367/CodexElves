import { useEffect, useRef, useState } from "react";
import { Cloud, Copy, Download, ExternalLink, Eye, EyeOff, RefreshCw, ShieldCheck, Smartphone, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { remoteAccessValidation, type RemoteAccessCredentials, type RemoteAccessProgram, type RemoteAccessSettings, type RemoteAccessStatus } from "@/remote-access-types";

export function RemoteAccessScreen({
  settings,
  onChange,
  onSave,
  command,
  openExternalUrl,
}: {
  settings: RemoteAccessSettings;
  onChange: (value: RemoteAccessSettings) => void;
  onSave: () => Promise<void>;
  command: <T>(action: string, payload?: Record<string, unknown>) => Promise<T>;
  openExternalUrl: (url: string) => Promise<void>;
}) {
  const [status, setStatus] = useState<RemoteAccessStatus | null>(null);
  const [credentials, setCredentials] = useState<RemoteAccessCredentials | null>(null);
  const [keyValue, setKeyValue] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [tokenValue, setTokenValue] = useState("");
  const [version, setVersion] = useState("");
  const [program, setProgram] = useState<RemoteAccessProgram | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const mounted = useRef(true);
  const refreshing = useRef(false);
  const patch = (value: Partial<RemoteAccessSettings>) => onChange({ ...settings, ...value });
  const refresh = async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      const saved = await command<RemoteAccessCredentials>("credentials");
      if (!mounted.current) return;
      if (saved.status === "ok") setCredentials(saved);
      else { setCredentials(null); setMessage(saved.message || "凭据状态读取失败"); }
      const installed = await command<RemoteAccessProgram>("cloudflaredStatus");
      if (!mounted.current) return;
      if (installed.status === "ok") setProgram(installed);
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
  const listening = status?.status === "ok" && status.running;
  const origin = `http://${settings.bindHost.includes(":") ? `[${settings.bindHost}]` : settings.bindHost}:${settings.port}`;
  const automaticProgram = !settings.cloudflaredPath.trim() || settings.cloudflaredPath.trim() === "cloudflared";
  const downloadProgram = async () => {
    setMessage("正在从 Cloudflare 官方发布页下载并校验 cloudflared，首次下载可能需要一段时间…");
    const result = await command<RemoteAccessProgram>("downloadCloudflared");
    if (result.status !== "ok" || !result.installed) throw new Error(result.message || "官方程序下载失败，请检查网络后重试。");
    setProgram(result);
    setMessage("官方 cloudflared 已下载并通过校验，无需手动安装或设置 PATH。");
  };
  const confirmKeyChange = () => credentials?.accessKeyConfigured === false || window.confirm("更换访问密钥会使所有已授权设备失效，需要重新输入新密钥。继续？");
  const save = () => run(async () => {
    const error = remoteAccessValidation(settings);
    if (error) throw new Error(error);
    if (settings.enabled && !credentials?.accessKeyConfigured) throw new Error("请先设置访问密钥，再启用远程网页。");
    if (settings.enabled && settings.cloudflareTunnelEnabled && !credentials?.tunnelTokenConfigured) throw new Error("请先保存 Cloudflare 隧道令牌。");
    if (settings.enabled && settings.cloudflareTunnelEnabled && automaticProgram && !program?.installed) await downloadProgram();
    await onSave();
    const result = await command<RemoteAccessStatus>("apply");
    setStatus(result);
    if (result.status !== "ok") throw new Error(result.message || "配置未应用");
    if (result.error) throw new Error(result.error);
    if (result.tunnel?.error) throw new Error(`设置已保存，但公网隧道未启动：${result.tunnel.error}`);
    if (result.settings && Object.keys(settings).some(key => result.settings?.[key as keyof RemoteAccessSettings] !== settings[key as keyof RemoteAccessSettings])) {
      setMessage("请核对保存结果。服务正在使用已保存的配置。");
      return;
    }
    setMessage(result.running ? `设置已保存，远程网页服务已启动。${settings.cloudflareTunnelEnabled ? "固定域名不变，隧道连接状态请查看下方。" : ""}` : "设置已保存，远程访问已关闭。");
  });

  return (
    <div className="remote-access-screen">
      <div className="remote-access-heading">
        <div><h2>远程会话</h2><p>通过手机网页查看和继续本机 Codex 会话，历史、项目和执行状态与桌面共享。</p></div>
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => void refresh()}><RefreshCw className="h-4 w-4" />刷新状态</Button>
      </div>
      <section className="remote-access-card">
        <div className="remote-access-card-heading">
          <h3><Smartphone className="h-4 w-4" />远程会话网页</h3>
          <button type="button" role="switch" aria-label="启用远程会话网页" aria-checked={settings.enabled} disabled={busy} className={`context-enabled-switch checkpoint-enabled-switch ${settings.enabled ? "active" : ""}`} onClick={() => patch({ enabled: !settings.enabled })}>
            <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
            <span>{settings.enabled ? "已启用" : "已停用"}</span>
          </button>
        </div>
        <div className="remote-access-status">
          <Smartphone className="h-5 w-5" />
          <strong>{listening ? "网页服务运行中" : "网页服务未启动"}</strong>
          <span className={status?.nativeConnected ? "remote-access-connected" : "remote-access-muted"}>
            {status?.nativeConnected ? "Codex 原生会话已连接" : "Codex 原生会话未连接"}
          </span>
        </div>
        <p className="remote-access-hint">请先设置下方的访问密钥。后台已连接时保存后立即启停；后台未启动时需先重启新版 launcher。不要求开启其他增强或模型代理。</p>
        {status?.error ? <p className="remote-access-error" role="alert">{status.error}</p> : null}
        <div className="remote-access-fields">
          <div><Label htmlFor="remote-bind">监听 IP 地址</Label><Input id="remote-bind" value={settings.bindHost} onChange={event => patch({ bindHost: event.target.value.trim() })} /><small>127.0.0.1 仅本机；0.0.0.0 允许网络设备连接，必须配置 HTTPS。</small></div>
          <div><Label htmlFor="remote-port">独立端口</Label><Input id="remote-port" type="number" min={1} max={65535} value={settings.port} onChange={event => patch({ port: Number(event.target.value) })} /><small>不与本地模型代理共用端口。</small></div>
          <div><Label htmlFor="remote-certificate">HTTPS 证书路径（PEM）</Label><Input id="remote-certificate" placeholder="C:\certs\codex.pem" value={settings.tlsCertificatePath} onChange={event => patch({ tlsCertificatePath: event.target.value.trim() })} /></div>
          <div><Label htmlFor="remote-key">HTTPS 私钥路径（PEM）</Label><Input id="remote-key" placeholder="C:\certs\codex-key.pem" value={settings.tlsPrivateKeyPath} onChange={event => patch({ tlsPrivateKeyPath: event.target.value.trim() })} /><small>私钥仅由本机读取，不会发送到网页。</small></div>
          <div><Label htmlFor="remote-expiry">设备授权有效期（小时）</Label><Input id="remote-expiry" type="number" min={1} max={720} value={settings.deviceSessionHours} onChange={event => patch({ deviceSessionHours: Number(event.target.value) })} /><small>对新登录设备生效；到期后重新输入访问密钥。</small></div>
          <div className="remote-access-permissions">
            <label className="remote-access-toggle"><input type="checkbox" checked={settings.allowApprovals} disabled={settings.readOnly} onChange={event => patch({ allowApprovals: event.target.checked })} /><span>允许远程审批及回答 Codex 询问</span></label>
          </div>
        </div>
        <div className="remote-access-actions"><Button disabled={busy} onClick={save}>{busy ? "处理中…" : "保存并应用"}</Button><span className="remote-access-hint">更改监听地址、证书或访问权限会断开现有网页连接。</span></div>
      </section>
      <section className="remote-access-card">
        <div className="remote-access-card-heading">
          <h3><Cloud className="h-4 w-4" />固定公网入口 · Cloudflare Named Tunnel</h3>
          <button type="button" role="switch" aria-label="由 CodexElves 管理 Cloudflare 隧道" aria-checked={settings.cloudflareTunnelEnabled} disabled={busy} className={`context-enabled-switch checkpoint-enabled-switch ${settings.cloudflareTunnelEnabled ? "active" : ""}`} onClick={() => patch({ cloudflareTunnelEnabled: !settings.cloudflareTunnelEnabled })}>
            <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
            <span>{settings.cloudflareTunnelEnabled ? "已启用" : "已停用"}</span>
          </button>
        </div>
        <p className="remote-access-hint">使用 Cloudflare 账号和已接入的自有域名。CodexElves 自动下载并管理官方 cloudflared，无需手动安装或配置 PATH；复用你创建的固定隧道，不安装 Windows 系统服务，重启后访问地址不变。</p>
        <div className="remote-access-guides">
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => void openExternalUrl("https://dash.cloudflare.com/sign-up")}><ExternalLink className="h-4 w-4" />注册 Cloudflare</Button>
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => void openExternalUrl("https://dash.cloudflare.com/")}><ExternalLink className="h-4 w-4" />打开 Cloudflare 控制台</Button>
          <Button variant="ghost" size="sm" disabled={busy} onClick={() => void openExternalUrl("https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/get-started/create-remote-tunnel/")}><ExternalLink className="h-4 w-4" />固定域名配置指南</Button>
        </div>
        <div className="remote-access-fields">
          <div className="remote-access-wide"><Label htmlFor="remote-public">固定 HTTPS 访问地址</Label><div className="remote-access-input-row"><Input id="remote-public" disabled={busy} placeholder="https://codex.example.com" value={settings.publicUrl} onChange={event => patch({ publicUrl: event.target.value.trim() })} /><Button variant="ghost" size="sm" disabled={!settings.publicUrl} aria-label="复制固定公网地址" onClick={() => void copy(settings.publicUrl)}><Copy className="h-4 w-4" /></Button></div><small>填写你在 Cloudflare 中绑定的域名。关闭隧道管理时，此地址也可用于已有 HTTPS 反向代理；仅允许网站根地址。</small></div>
          <div className="remote-access-wide">
            <Label>Cloudflare 程序</Label>
            <div className="remote-access-input-row"><span className="remote-access-program">{automaticProgram ? program?.installed ? program.version : "首次启用时自动下载官方 cloudflared" : "当前使用自定义程序路径"}</span><Button variant="secondary" disabled={busy} onClick={() => void run(downloadProgram)}><Download className="h-4 w-4" />{program?.installed ? "检查托管程序" : "下载官方程序"}</Button></div>
            <small>{program?.message || "从 Cloudflare 官方 GitHub 发布页获取，核对 SHA-256 后才保存运行；已下载的程序离线复用，不覆盖其它 cloudflared。"}</small>
            <details className="remote-access-program-options"><summary>高级设置：使用自定义程序</summary><Label htmlFor="remote-cloudflared">自定义 cloudflared 程序路径（可选）</Label><div className="remote-access-input-row"><Input id="remote-cloudflared" disabled={busy} placeholder="留空使用自动下载的程序" value={automaticProgram ? "" : settings.cloudflaredPath} onChange={event => { patch({ cloudflaredPath: event.target.value }); setVersion(""); }} /><Button variant="secondary" disabled={busy} onClick={() => void run(async () => {
            if (settings.cloudflaredPath.includes("\0")) throw new Error("程序路径不能包含空字符。");
            const result = await command<{ status: string; version?: string; message?: string }>("checkCloudflared", { path: settings.cloudflaredPath });
            if (result.status !== "ok") throw new Error(result.message || "程序检查失败");
            setVersion(result.version || "");
          })}>检查程序</Button></div><small>{version || "留空时使用 CodexElves 托管程序；自定义路径不要包含启动参数。"}</small></details></div>
          <div className="remote-access-wide"><Label htmlFor="remote-tunnel-token">Cloudflare 隧道令牌</Label><div className="remote-access-input-row"><Input id="remote-tunnel-token" type="password" disabled={busy} autoComplete="off" maxLength={8192} placeholder={credentials?.tunnelTokenConfigured ? "已保存；输入新令牌可替换" : "只粘贴令牌，不要粘贴完整安装命令"} value={tokenValue} onChange={event => setTokenValue(event.target.value)} /><Button variant="secondary" disabled={busy || !tokenValue.trim()} onClick={() => void run(async () => {
            if (!/^[A-Za-z0-9+\/_=-]{20,8192}$/.test(tokenValue.trim())) throw new Error("请只粘贴 Cloudflare 隧道令牌，不要粘贴完整安装命令。");
            const result = await command<RemoteAccessCredentials>("setTunnelToken", { token: tokenValue.trim() });
            if (result.status !== "ok") throw new Error(result.message || "隧道令牌保存失败");
            setCredentials(result); setTokenValue("");
            setMessage("隧道令牌已保存。启用后由管理器连接同一固定隧道。");
          })}>保存令牌</Button></div><small>此令牌与网页访问密钥不同，仅用于本机连接 Cloudflare；Windows 使用当前用户的 DPAPI 加密保存，不返回网页，也不放入进程命令行。</small></div>
        </div>
        <ol className="remote-access-steps">
          <li>没有账号时先点击「注册 Cloudflare」，将自有域名接入 Cloudflare；在控制台的 Networking → Tunnels 中创建固定隧道，并添加 Published application 域名路由。</li>
          <li>将该域名的 Service 设置为 <code>{origin}</code>；保持原始 Host，不要配置额外的 HTTP Host Header 覆盖。</li>
          <li>只复制连接器安装命令中的令牌，在「Cloudflare 隧道令牌」中保存；填写固定地址，启用两个开关后「保存并应用」。程序自动下载，无需执行安装命令。修改监听端口时，同步修改 Cloudflare 的 Service。</li>
        </ol>
        <div className="remote-access-status"><strong>{!settings.enabled || !settings.cloudflareTunnelEnabled ? "隧道管理未启用" : status?.tunnel?.state === "downloading" ? "正在准备官方 cloudflared 程序…" : status?.tunnel?.connected ? "Cloudflare 隧道已连接" : status?.tunnel?.running ? "Cloudflare 隧道连接中" : "Cloudflare 隧道未启动"}</strong><span className="remote-access-muted">网络恢复后由 cloudflared 重连；进程意外退出后 30 秒重试。</span></div>
        {status?.tunnel?.error ? <p className="remote-access-error" role="alert">{status.tunnel.error}</p> : null}
        <div className="remote-access-actions"><Button disabled={busy} onClick={save}>保存并应用</Button><span className="remote-access-hint">关闭隧道管理后保存，只停止 CodexElves 自己启动的隧道进程；本机网页仍可使用。</span></div>
      </section>
      <section className="remote-access-card">
        <h3><ShieldCheck className="h-4 w-4" />访问密钥与设备授权</h3>
        <p className="remote-access-hint">{credentials?.accessKeyConfigured ? "访问密钥已设置。" : "尚未设置访问密钥。"} 手机网页只需输入这个长期密钥即可进入，无需填写设备名称。密钥不会随隧道重启过期；更换后所有旧设备授权失效。已撤销设备若仍知道密钥，可以重新登录；如需阻止其再次登录，请同时更换密钥。</p>
        <div className="remote-access-fields">
          <div className="remote-access-wide"><Label htmlFor="remote-access-key">设置访问密钥</Label><div className="remote-access-input-row remote-access-key-row"><Button variant="secondary" disabled={busy} onClick={() => void run(async () => {
            const result = await command<{ status: string; key?: string; message?: string }>("generateKey");
            if (result.status !== "ok" || !result.key) throw new Error(result.message || "密钥生成失败");
            setKeyValue(result.key); setShowKey(false);
            setMessage("随机密钥已生成，尚未保存。点击右侧「保存」后生效，当前设备授权不受影响。");
          })}>生成</Button><div className="remote-access-key-input"><Input id="remote-access-key" type={showKey ? "text" : "password"} disabled={busy} autoComplete="off" minLength={32} maxLength={256} placeholder={credentials?.accessKeyConfigured ? "已设置；输入新密钥替换" : "32–256 个非空格 ASCII 字符，建议随机生成"} value={keyValue} onChange={event => setKeyValue(event.target.value)} /><Button variant="ghost" size="sm" disabled={busy || !keyValue} aria-label={showKey ? "隐藏访问密钥" : "显示访问密钥"} onClick={() => setShowKey(!showKey)}>{showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button><Button variant="ghost" size="sm" disabled={busy || !keyValue} aria-label="复制访问密钥" onClick={() => void copy(keyValue)}><Copy className="h-4 w-4" /></Button></div><Button disabled={busy || !keyValue.trim()} onClick={() => void run(async () => {
            if (!/^[\x21-\x7e]{32,256}$/.test(keyValue.trim())) throw new Error("密钥需为 32–256 个非空格 ASCII 字符。");
            if (!confirmKeyChange()) return;
            const result = await command<RemoteAccessCredentials>("setKey", { key: keyValue.trim() });
            if (result.status !== "ok") throw new Error(result.message || "访问密钥保存失败");
            setCredentials(result);
            setMessage("访问密钥已保存，请复制妥善保管。原有设备需重新输入新密钥连接。");
          })}>保存</Button></div><small>生成仅填入当前输入框，保存后才生效。本机仅保存加盐哈希，不保存明文；离开页面后无法恢复明文。更换密钥不会停止已在 Codex 中执行的任务。</small></div>
        </div>
        <div className="remote-access-devices">
          {(status?.devices || []).map(device => <div className="remote-access-device" key={device.id}>
            <Smartphone className="h-4 w-4" /><div><strong>{device.name}</strong><small>到期：{new Date(device.expiresAtMs).toLocaleString()} · 最近访问：{new Date(device.lastSeenAtMs).toLocaleString()}</small></div>
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
      </section>
      <p className="remote-access-message" role="status" aria-live="polite">{message}</p>
    </div>
  );
}
