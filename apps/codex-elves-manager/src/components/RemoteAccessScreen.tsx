import { useEffect, useRef, useState } from "react";
import { Copy, ExternalLink, Eye, EyeOff, Smartphone, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PrimaryRouteControls, PrimaryRouteTabs } from "@/components/PrimaryRouteControls";
import { remoteAccessValidation, type PrimaryProvider, type RemoteAccessCredentials, type RemoteAccessSettings, type RemoteAccessStatus, type ZrokAccessTest } from "@/remote-access-types";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const FIXED_NAME_PATTERN = /^[a-z0-9](?:[a-z0-9-]{1,61}[a-z0-9])?$/;
const DESKTOP_WAITING_GUIDANCE = "Codex 桌面当前未连接；公网入口仍可测试和切换，远程网页会等待桌面恢复后就绪。";

export function RemoteAccessScreen({
  settings,
  onChange,
  onSave,
  command,
  onOpenExternalUrl,
  onNotice,
  onSuccess,
}: {
  settings: RemoteAccessSettings;
  onChange: (value: RemoteAccessSettings) => void;
  onSave: (next: RemoteAccessSettings) => Promise<void>;
  command: <T>(action: string, payload?: Record<string, unknown>) => Promise<T>;
  onOpenExternalUrl?: (url: string) => Promise<void>;
  onNotice: (message: string) => void;
  onSuccess: (message: string) => void;
}) {
  const [status, setStatus] = useState<RemoteAccessStatus | null>(null);
  const [editingPrimary, setEditingPrimary] = useState<PrimaryProvider>("zrok");
  const initializedPrimary = useRef(false);
  const [credentials, setCredentials] = useState<RemoteAccessCredentials | null>(null);
  const [loginLocked, setLoginLocked] = useState(false);
  const [zrokValue, setZrokValue] = useState(settings.zrokSetupValue);
  const [fixedNameValue, setFixedNameValue] = useState("");
  const [ngrokToken, setNgrokToken] = useState("");
  const [ngrokTokenConfigured, setNgrokTokenConfigured] = useState(false);
  const [showNgrokToken, setShowNgrokToken] = useState(false);
  const [keyValue, setKeyValue] = useState("");
  const [keyDirty, setKeyDirty] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const mounted = useRef(true);
  const refreshing = useRef(false);
  const loadedZrokSetupValue = useRef(settings.zrokSetupValue);
  const fixedNameDirtyRef = useRef(false);
  const loadedKeyValue = useRef<string | null>(null);
  const loadedNgrokToken = useRef<string | null>(null);
  const loginLockRevision = useRef(0);
  const ngrokTokenRevision = useRef(0);
  const keyDirtyRef = useRef(false);
  const ngrokTokenDirtyRef = useRef(false);
  const latestSettings = useRef(settings);
  latestSettings.current = settings;
  const patch = (value: Partial<RemoteAccessSettings>) => onChange({ ...latestSettings.current, ...value });
  const refresh = async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    const loginLockReadRevision = loginLockRevision.current;
    const ngrokTokenReadRevision = ngrokTokenRevision.current;
    try {
      const saved = await command<RemoteAccessCredentials>("credentials");
      if (!mounted.current || loginLockReadRevision !== loginLockRevision.current) return;
      if (saved.status === "ok") {
        setCredentials(saved);
        if (loginLockReadRevision === loginLockRevision.current && typeof saved.loginLocked === "boolean")
          setLoginLocked(saved.loginLocked);
        const savedKey = saved.key || "";
        if (!keyDirtyRef.current && loadedKeyValue.current !== savedKey) {
          loadedKeyValue.current = savedKey;
          setKeyValue(savedKey);
          setShowKey(false);
        }
        if (ngrokTokenReadRevision === ngrokTokenRevision.current && typeof saved.ngrokTokenConfigured === "boolean") {
          setNgrokTokenConfigured(saved.ngrokTokenConfigured);
          const savedToken = saved.ngrokToken || "";
          if (!ngrokTokenDirtyRef.current && loadedNgrokToken.current !== savedToken) {
            loadedNgrokToken.current = savedToken;
            setNgrokToken(savedToken);
            setShowNgrokToken(false);
          }
        }
      }
      else { setCredentials(null); setMessage(saved.message || "凭据状态读取失败"); }
      const result = await command<RemoteAccessStatus>("status");
      if (!mounted.current) return;
      setStatus(result);
      if (ngrokTokenReadRevision === ngrokTokenRevision.current && typeof result.ngrok?.configured === "boolean")
        setNgrokTokenConfigured(result.ngrok.configured);
      if (loginLockReadRevision === loginLockRevision.current && result.status === "ok" && typeof result.loginLocked === "boolean")
        setLoginLocked(result.loginLocked);
      if (result.status !== "ok") setMessage(result.message || "本机服务未连接");
    } catch (error) {
      if (mounted.current) { setStatus(null); setMessage(String(error)); }
    } finally { refreshing.current = false; }
  };
  useEffect(() => {
    mounted.current = true;
    void refresh();
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    if (!status?.primary || initializedPrimary.current) return;
    initializedPrimary.current = true;
    setEditingPrimary(status.primary.provider);
  }, [status?.primary]);
  useEffect(() => {
    const active = busy || status?.primary?.switching || ["starting", "downloading"].includes(status?.primary?.frp.state || "")
      || ["checking", "downloading", "starting", "connecting"].includes(status?.zrok?.state || "");
    const timer = window.setInterval(() => void refresh(), active ? 500 : 5000);
    return () => clearInterval(timer);
  }, [busy, status?.zrok?.state, status?.primary?.switching, status?.primary?.frp.state]);
  useEffect(() => {
    if (settings.zrokSetupValue === loadedZrokSetupValue.current) return;
    loadedZrokSetupValue.current = settings.zrokSetupValue;
    setZrokValue(settings.zrokSetupValue);
  }, [settings.zrokSetupValue]);
  useEffect(() => {
    if (!fixedNameDirtyRef.current && status?.zrok?.fixedName) setFixedNameValue(status.zrok.fixedName);
  }, [status?.zrok?.fixedName]);
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
  const zrok = status?.zrok;
  const desktopReady = status?.status === "ok" && (status.nativeConnected === true || status.serviceConnected === true);
  const desktopGuidance = !settings.enabled
    ? "请先启用上方「远程会话网页」，再重启 Codex 并等待主界面就绪，最后启动公网。"
    : desktopReady
      ? status.nativeConnected === true
        ? "Codex 桌面已连接，可以启动公网。后续重启 Codex 时，请等待主界面恢复后再刷新远程网页。"
        : "Codex 只读服务已连接，可以启动公网；发送、审批和侧边会话等原生写入功能将在注入恢复后可用。"
      : DESKTOP_WAITING_GUIDANCE;
  const publicUrl = zrok?.publicUrl || "";
  const zrokInput = zrokValue.trim();
  const zrokEmail = EMAIL_PATTERN.test(zrokInput);
  const zrokToken = Boolean(zrokInput) && !zrokEmail;
  const normalizedFixedName = fixedNameValue.trim().toLowerCase();
  const fixedNameValid = normalizedFixedName.length >= 3
    && normalizedFixedName.length <= 63
    && FIXED_NAME_PATTERN.test(normalizedFixedName);
  const zrokDownloading = zrok?.state === "downloading";
  const zrokTransitioning = ["checking", "downloading", "starting", "connecting"].includes(zrok?.state || "");
  const zrokDownloadPercent = Math.max(0, Math.min(100, Math.round(zrok?.downloadPercent || 0)));
  const zrokActionLabel = !zrok
    ? "检查中…"
    : !zrok.installed
      ? "下载程序"
      : zrokEmail
        ? "注册公网"
        : status?.primary?.provider === "frp" ? "保存绑定" : "启动公网";
  const zrokActionDisabled = busy
    || !zrok
    || zrokTransitioning
    || (zrok.installed && zrokToken && (!settings.enabled || !fixedNameValid))
    || (zrok.installed && !zrokInput && (!settings.enabled || !zrok.bound || !zrok.fixedName || zrok.running));
  const runApply = (operation: () => Promise<void>) => run(async () => {
    try { await operation(); }
    catch (error) {
      onNotice(error instanceof Error ? error.message : String(error));
    }
  });
  const saveZrokSetupValue = async (value: string) => {
    const previous = latestSettings.current;
    if (previous.zrokSetupValue === value) return;
    const next = { ...previous, zrokSetupValue: value };
    onChange(next);
    try {
      await onSave(next);
    } catch (error) {
      onChange(previous);
      throw error;
    }
  };
  const toggleWebService = () => runApply(() => applySettings({ ...settings, enabled: !settings.enabled }));
  const preparePrimaryAction = async (provider: PrimaryProvider) => {
    if (provider !== "zrok" || !zrok) return;
    const setupChanged = zrokInput !== settings.zrokSetupValue.trim() || fixedNameDirtyRef.current;
    if (!setupChanged) return;
    if (zrokInput) await saveZrokSetupValue(zrokInput);
    if (!settings.enabled) return;
    if (zrokEmail) throw new Error("当前输入是注册邮箱，请先点击右侧「注册公网」，收到 token 后再测试主入口。");
    if (!zrok.installed) throw new Error("请先下载 zrok 程序。");
    if (!zrokToken) throw new Error("请输入 zrok token。");
    if (!fixedNameValid) throw new Error("固定公网名称需为 3–63 位小写字母、数字或连字符，且首尾必须是字母或数字。");
    const result = await command<RemoteAccessStatus>("startZrok", {
      token: zrokInput,
      fixedName: normalizedFixedName,
    });
    if (!mounted.current) return;
    setStatus(result);
    if (result.status !== "ok") throw new Error(result.message || "zrok 配置保存失败");
    fixedNameDirtyRef.current = false;
    setFixedNameValue(result.zrok?.fixedName || normalizedFixedName);
  };
  const toggleNgrok = () => runApply(() => {
    const next = { ...latestSettings.current, ngrokEnabled: !latestSettings.current.ngrokEnabled };
    return applySettings(next, next.ngrokEnabled ? "ngrok 第二线路已启用。" : "ngrok 第二线路已停用，Token 已保留。");
  });
  const renderZrokBadge = () => {
    switch (zrok?.state) {
      case "ready":
        return desktopReady
          ? <span className="zrok-status-badge connected"><span className="zrok-status-dot" />可访问</span>
          : <span className="zrok-status-badge connecting" title="公网已连通，等待 Codex 桌面连接后才能读取会话。"><span className="zrok-status-dot" />等待 Codex 就绪</span>;
      case "checking":
        return <span className="zrok-status-badge connecting"><span className="zrok-status-dot" />正在检查</span>;
      case "downloading":
        return <span className="zrok-status-badge connecting"><span className="zrok-status-dot" />正在下载</span>;
      case "starting":
        return <span className="zrok-status-badge connecting"><span className="zrok-status-dot" />正在启动</span>;
      case "connecting":
        return <span className="zrok-status-badge connecting"><span className="zrok-status-dot" />正在验证</span>;
      case "retry_wait":
        return <span className="zrok-status-badge connecting"><span className="zrok-status-dot" />等待重试</span>;
      case "error":
        return <span className="zrok-status-badge error" title={zrok.error || "连接异常"}><span className="zrok-status-dot" />异常</span>;
      case "unbound":
        return <span className="zrok-status-badge"><span className="zrok-status-dot" />未绑定</span>;
      case "name_required":
        return <span className="zrok-status-badge"><span className="zrok-status-dot" />待设置名称</span>;
      case "missing":
        return <span className="zrok-status-badge"><span className="zrok-status-dot" />未安装</span>;
      default:
        return <span className="zrok-status-badge"><span className="zrok-status-dot" />已停止</span>;
    }
  };
  const handleZrokAction = () => runApply(async () => {
    if (!zrok) return;
    if (zrok.installed && !zrokEmail) {
      const latest = await command<RemoteAccessStatus>("status");
      if (!mounted.current) return;
      setStatus(latest);
      if (latest.status !== "ok") throw new Error(latest.message || "无法确认桌面连接，请稍后重试。");
      if (!latest.enabled || !latest.running) throw new Error("请先启用上方「远程会话网页」。");
    }
    if (zrokInput) await saveZrokSetupValue(zrokInput);
    if (!zrok.installed) {
      const result = await command<RemoteAccessStatus>("downloadZrok");
      if (!mounted.current) return;
      setStatus(result);
      if (result.status !== "ok") throw new Error(result.message || "zrok 下载任务启动失败");
      onSuccess("已开始下载 zrok 程序。");
      return;
    }
    if (zrokEmail) {
      const result = await command<{ status: string; message?: string }>("registerZrok", { email: zrokInput });
      if (result.status !== "ok") throw new Error(result.message || "zrok 注册失败");
      onSuccess(result.message || "注册邮件已发送，请检查邮箱。");
      return;
    }
    const fixedName = zrokToken ? normalizedFixedName : zrok.fixedName;
    const result = await command<RemoteAccessStatus>("startZrok", {
      token: zrokToken ? zrokInput : "",
      fixedName,
    });
    if (!mounted.current) return;
    setStatus(result);
    if (result.status !== "ok") throw new Error(result.message || "zrok 公网启动失败");
    fixedNameDirtyRef.current = false;
    setFixedNameValue(result.zrok?.fixedName || fixedName);
    onSuccess(result.primary?.provider === "frp" ? "zrok 绑定已保存，可验证并应用为主链路。"
      : result.zrok?.state === "ready" ? "zrok 固定公网已启动。" : "zrok 固定公网正在启动并验证访问。");
  });
  const testPublicUrl = () => runApply(async () => {
    if (!publicUrl) return;
    const result = await command<ZrokAccessTest>("testZrok");
    if (!mounted.current) return;
    await refresh();
    if (!mounted.current) return;
    if (result.status !== "ok") throw new Error(result.message || "公网访问验证失败，请检查网络连接后重试。");
    onSuccess(result.message || "公网访问验证通过。");
  });
  const saveNgrokToken = () => runApply(async () => {
    const nextToken = ngrokToken.trim();
    const result = await command<{ status: string; message?: string }>("saveNgrok", { token: nextToken });
    if (!mounted.current) return;
    if (result.status !== "ok") throw new Error(result.message || "ngrok 验证失败");
    ++ngrokTokenRevision.current;
    loadedNgrokToken.current = nextToken;
    ngrokTokenDirtyRef.current = false;
    setNgrokTokenConfigured(true);
    setNgrokToken(nextToken);
    setShowNgrokToken(false);
    await refresh();
    if (mounted.current) onSuccess(result.message || "ngrok Token 已保存并通过测试。");
  });
  const confirmKeyChange = () => credentials?.accessKeyConfigured === false || window.confirm("更换访问密钥会使所有已授权设备失效，需要重新输入新密钥。继续？");
  const applySettings = async (next: RemoteAccessSettings, successMessage?: string) => {
    const error = remoteAccessValidation(next);
    if (error) throw new Error(error);
    onChange(next);
    try {
      await onSave(next);
    } catch (error) {
      onChange({ ...latestSettings.current, enabled: settings.enabled, ngrokEnabled: settings.ngrokEnabled });
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
    const zrokNotice = result.zrok?.state === "ready"
      ? "zrok 固定公网可访问。"
      : result.zrok?.bound && result.zrok?.fixedName
        ? "zrok 固定公网正在启动并验证访问。"
        : "";
    const keyConfigured = result.accessKeyConfigured ?? credentials?.accessKeyConfigured ?? false;
    const authorizationNotice = keyConfigured
      ? ""
      : "访问密钥尚未设置，网页只显示登录页，无法进入会话。";
    onSuccess(successMessage || (result.running
      ? `设置已保存，远程网页服务已启动。${zrokNotice}${authorizationNotice}`
      : "设置已保存，远程访问已关闭。"));
  };
  const save = () => runApply(() => applySettings(settings));

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
            <div><Label htmlFor="remote-bind">监听 IP 地址</Label><Input id="remote-bind" value={settings.bindHost} onChange={event => patch({ bindHost: event.target.value.trim() })} /><small>仅支持本机回环地址；公网访问由所选主链路转发。</small></div>
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
                    if (settings.deviceSessionHours > 0) patch({ deviceSessionHours: 0 });
                    else patch({ deviceSessionHours: 168 });
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
                <Input id="remote-expiry" disabled value="未开启有效期限制（长期有效）" />
              )}
              <small>{settings.deviceSessionHours > 0 ? "对新登录设备生效，到期需重新输入密钥。" : "默认关闭，已授权设备长期有效。"}</small>
            </div>
          </div>
        </CardContent>
        <CardHeader className="panel-head border-t border-border pt-6 pb-[10px]">
          <div className="panel-head-copy">
            <div className="remote-public-title-row">
              <CardTitle>公网入口</CardTitle>
              <PrimaryRouteTabs value={editingPrimary} onChange={provider => { initializedPrimary.current = true; setEditingPrimary(provider); }} disabled={busy || Boolean(status?.primary?.switching)} />
            </div>
            <CardDescription>
              {editingPrimary === "zrok"
                ? <>免费，但是稳定性欠佳，建议开启 <strong>ngrok 第二线路</strong></>
                : <>自行搭建或者购买，速度快稳定性好，<strong>frp程序会被误杀需要自行加白</strong></>}
            </CardDescription>
          </div>
          <div className="panel-head-actions flex items-center gap-3">
            {status?.primary?.provider === "frp"
              ? <span className={`zrok-status-badge ${status.primary.frp.state === "ready" ? "connected" : status.primary.frp.state === "error" ? "error" : "connecting"}`}><span className="zrok-status-dot" />当前主链路：FRP · {status.primary.frp.state === "ready" ? "可访问" : status.primary.frp.state === "downloading" ? "正在下载" : status.primary.frp.state === "starting" ? "正在启动" : status.primary.frp.state === "error" ? "异常" : "已停止"}</span>
              : <><span className="text-xs text-muted-foreground">当前主链路：zrok</span>{renderZrokBadge()}</>}
          </div>
        </CardHeader>
        <CardContent>
          <PrimaryRouteControls status={status} token={credentials?.frpToken} busy={busy} enabled={settings.enabled}
            editing={editingPrimary} target={`${settings.bindHost}:${settings.port}`} command={command} run={run} refresh={refresh} prepare={preparePrimaryAction}>
          <div className="remote-access-fields remote-access-fields-flush">
            <div className="remote-access-wide">
              <div className="remote-access-field-heading">
                <div className="flex shrink-0 items-center gap-2">
                  <Label htmlFor="remote-zrok-account">zrok token 或注册邮箱</Label>
                  <a
                    className="inline-flex items-center gap-1 text-xs text-primary underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    href="https://api-v2.zrok.io/"
                    target="_blank"
                    rel="noopener noreferrer"
                    onClick={event => {
                      if (!onOpenExternalUrl) return;
                      event.preventDefault();
                      void onOpenExternalUrl(event.currentTarget.href);
                    }}
                  >
                    <ExternalLink className="h-3 w-3" aria-hidden="true" />
                    访问官网
                  </a>
                </div>
                {zrokDownloading ? (
                  <div className="zrok-download-progress">
                    <span>{zrok?.downloadTotalBytes ? `下载程序 ${zrokDownloadPercent}%` : "正在下载程序"}</span>
                    <div
                      className="zrok-download-track"
                      role="progressbar"
                      aria-label="下载 zrok 程序"
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={zrok?.downloadTotalBytes ? zrokDownloadPercent : undefined}
                    >
                      <div
                        className={`zrok-download-fill ${zrok?.downloadTotalBytes ? "" : "indeterminate"}`}
                        style={zrok?.downloadTotalBytes ? { width: `${zrokDownloadPercent}%` } : undefined}
                      />
                    </div>
                  </div>
                ) : null}
              </div>
              <div className={`remote-access-input-row zrok-setup-row ${zrokToken ? "with-fixed-name" : ""}`}>
                <Input
                  id="remote-zrok-account"
                  spellCheck={false}
                  autoComplete="off"
                  disabled={busy || zrokTransitioning}
                  placeholder={zrok?.bound ? "已绑定；也可输入邮箱重新注册" : "输入 zrok token 或邮箱"}
                  value={zrokValue}
                  onChange={event => setZrokValue(event.target.value)}
                />
                {zrokToken ? (
                  <Input
                    id="remote-zrok-fixed-name"
                    spellCheck={false}
                    autoComplete="off"
                    disabled={busy || zrokTransitioning}
                    maxLength={63}
                    aria-label="固定公网名称"
                    placeholder="固定公网名称"
                    value={fixedNameValue}
                    onChange={event => {
                      fixedNameDirtyRef.current = true;
                      setFixedNameValue(event.target.value.toLowerCase());
                    }}
                  />
                ) : null}
                <Button
                  variant="secondary"
                  className="h-9"
                  disabled={zrokActionDisabled}
                  aria-describedby="remote-desktop-guidance"
                  onClick={() => void handleZrokAction()}
                >
                  {busy ? "处理中…" : zrokActionLabel}
                </Button>
              </div>
              <p id="remote-desktop-guidance" className="remote-access-hint" role="status" aria-live="polite">{desktopGuidance}</p>
              {zrokToken && fixedNameValue && !fixedNameValid ? <small className="remote-access-inline-error">固定公网名称需为 3–63 位小写字母、数字或连字符，且首尾必须是字母或数字。</small> : null}
              {!zrokToken && !normalizedFixedName ? <small>输入邮箱时发送注册邮件；输入 token 时同时填写固定公网名称。点击右侧按钮后，当前邮箱或 token 会保存到本机 CodexElves 设置并在下次打开时回显。</small> : null}
            </div>
            <div className="remote-access-wide">
              <Label htmlFor="remote-zrok-url">固定公网地址</Label>
              <div className="remote-access-input-row remote-access-public-row">
                <div className="remote-access-key-input remote-access-public-input">
                  <Input id="remote-zrok-url" readOnly placeholder={zrokTransitioning ? "正在准备 zrok 固定公网地址…" : "绑定并启动后显示固定公网地址"} value={publicUrl} />
                  <div className="remote-access-key-actions">
                    <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={!publicUrl} aria-label="复制固定公网地址" onClick={() => void copy(publicUrl)}><Copy className="h-4 w-4" /></Button>
                  </div>
                </div>
                <Button variant="secondary" size="sm" className="h-9" disabled={busy || !settings.enabled || !publicUrl || !zrok?.running} onClick={() => void testPublicUrl()}>{busy ? "处理中…" : "测试访问"}</Button>
              </div>
              <small>关闭并重新启动 CodexElves 后复用同一地址；首次访问若出现 zrok 提示页，请点击「Visit Share」。</small>
            </div>
          </div>
          {!status?.primary && ["connecting", "retry_wait", "error"].includes(zrok?.state || "") && zrok?.error ? <p className="remote-access-error" role="alert">{zrok.error}</p> : null}
          {zrok?.state === "retry_wait" && zrok.retryAtMs ? <p className="remote-access-hint">将在 {new Date(zrok.retryAtMs).toLocaleTimeString()} 自动重试。</p> : null}
          </PrimaryRouteControls>
          {editingPrimary === "zrok" ? <div className="remote-access-fields">
            <div className="remote-access-wide">
              <div className="remote-access-expiry-heading">
                <span className="text-sm font-semibold">ngrok 第二线路</span>
                <button
                  type="button"
                  role="switch"
                  aria-label="启用 ngrok 第二线路"
                  aria-checked={settings.ngrokEnabled}
                  disabled={busy}
                  className={`context-enabled-switch checkpoint-enabled-switch ${settings.ngrokEnabled ? "active" : ""}`}
                  onClick={() => void toggleNgrok()}
                >
                  <span className="context-switch-track" aria-hidden="true"><span className="context-switch-thumb" /></span>
                  <span>{settings.ngrokEnabled ? "已启用" : "已停用"}</span>
                </button>
              </div>
              {settings.ngrokEnabled ? <>
              <Label className="sr-only" htmlFor="remote-ngrok-token">ngrok Token</Label>
              <div className="remote-access-input-row remote-access-key-row">
                <SecretInput
                  id="remote-ngrok-token"
                  label="ngrok Token"
                  disabled={busy}
                  value={ngrokToken}
                  visible={showNgrokToken}
                  minLength={20}
                  maxLength={512}
                  placeholder={ngrokTokenConfigured
                    ? loadedNgrokToken.current ? "输入新 Token 替换" : "已保存 Token 无法读取，请重新输入并保存"
                    : "输入 ngrok Token"}
                  onChange={value => {
                    ngrokTokenDirtyRef.current = true;
                    setNgrokToken(value);
                  }}
                  onToggleVisibility={() => setShowNgrokToken(value => !value)}
                  onCopy={() => void copy(ngrokToken)}
                />
                <Button variant="secondary" size="sm" className="h-9"
                  disabled={busy || !settings.enabled || !settings.ngrokEnabled || !ngrokToken.trim()}
                  onClick={() => void saveNgrokToken()}>{busy ? "处理中…" : "保存并测试"}</Button>
              </div>
              <small>{!settings.enabled ? "远程会话网页未启用，第二线路将在网页服务启动后连接。"
                  : status?.ngrok?.state === "ready" ? "第二通道已连接，安卓端将自动发现并同时使用。"
                : status?.ngrok?.state === "starting" ? "第二通道正在启动…"
                  : status?.ngrok?.error || "请保存并测试 Token，第二线路启用后安卓端会自动发现，无需填写第二个域名。"}</small>
              </> : null}
            </div>
          </div> : null}
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
          {loginLocked ? <p className="remote-access-error" role="alert">访问密钥连续输错超过3次，新设备登录已锁定。请点击「生成」，再点击右侧「保存」；只有不同的新密钥保存成功后才会解锁。已登录设备当前不受影响。</p> : null}
          <div className="remote-access-fields remote-access-fields-top">
            <div className="remote-access-wide"><Label htmlFor="remote-access-key">设置访问密钥</Label><div className="remote-access-input-row remote-access-key-row">
              <SecretInput
                id="remote-access-key"
                label="访问密钥"
                disabled={busy}
                visible={showKey}
                minLength={8}
                maxLength={64}
                placeholder={credentials?.accessKeyConfigured
                  ? credentials.key ? "输入新密钥替换" : "旧密钥未保存明文，请重新输入并保存"
                  : "8–64 个非空格 ASCII 字符，建议随机生成"}
                value={keyValue}
                onChange={value => {
                  keyDirtyRef.current = true;
                  setKeyDirty(true);
                  setKeyValue(value);
                }}
                onToggleVisibility={() => setShowKey(value => !value)}
                onCopy={() => void copy(keyValue)}
              />
              <Button variant="secondary" disabled={busy} onClick={() => void run(async () => {
              const result = await command<{ status: string; key?: string; message?: string }>("generateKey");
              if (result.status !== "ok" || !result.key) throw new Error(result.message || "密钥生成失败");
              keyDirtyRef.current = true;
              setKeyDirty(true);
              setKeyValue(result.key); setShowKey(false);
              setMessage(loginLocked
                ? "随机密钥已生成，尚未保存。请点击右侧「保存」；只有不同的新密钥保存成功后才会解除登录锁定，并使原有设备重新登录。"
                : "随机密钥已生成，尚未保存。点击右侧「保存」后生效，当前设备授权不受影响。");
            })}>生成</Button><Button disabled={busy || !keyDirty || !keyValue.trim()} onClick={() => void runApply(async () => {
              const nextKey = keyValue.trim();
              if (!/^[\x21-\x7e]{8,64}$/.test(nextKey)) throw new Error("密钥需为 8–64 个非空格 ASCII 字符。");
              if (!confirmKeyChange()) return;
              const result = await command<RemoteAccessCredentials>("setKey", { key: nextKey });
              if (result.status !== "ok") throw new Error(result.message || "访问密钥保存失败");
              ++loginLockRevision.current;
              const savedKey = result.key || nextKey;
              loadedKeyValue.current = savedKey;
              keyDirtyRef.current = false;
              setKeyDirty(false);
              setKeyValue(savedKey);
              setShowKey(false);
              setCredentials({ ...result, key: savedKey });
              if (typeof result.loginLocked === "boolean") setLoginLocked(result.loginLocked);
              onSuccess(result.loginLocked === true
                ? "访问密钥已保存，登录仍处于锁定状态。请生成不同的新密钥并保存。"
                : "访问密钥已保存并生效；更换密钥后，原有设备需使用新密钥重新登录。");
            })}>保存</Button></div><small>支持 8–64 个非空格 ASCII 字符；点击「保存」后以明文保存在本机，输入框默认使用圆点遮挡，可点击小眼睛查看。</small></div>
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

function SecretInput({
  id, label, value, visible, disabled, placeholder, minLength, maxLength,
  onChange, onToggleVisibility, onCopy,
}: {
  id: string;
  label: string;
  value: string;
  visible: boolean;
  disabled: boolean;
  placeholder: string;
  minLength: number;
  maxLength: number;
  onChange: (value: string) => void;
  onToggleVisibility: () => void;
  onCopy: () => void;
}) {
  return (
    <div className="remote-access-key-input">
      <Input
        id={id}
        type={visible ? "text" : "password"}
        autoComplete="off"
        spellCheck={false}
        disabled={disabled}
        value={value}
        placeholder={placeholder}
        minLength={minLength}
        maxLength={maxLength}
        onChange={event => onChange(event.target.value)}
      />
      <div className="remote-access-key-actions">
        <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={disabled || !value}
          aria-label={`${visible ? "隐藏" : "显示"}${label}`} aria-pressed={visible}
          onClick={onToggleVisibility}>
          {visible ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
        </Button>
        <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={disabled || !value}
          aria-label={`复制${label}`} onClick={onCopy}>
          <Copy className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
