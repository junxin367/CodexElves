import { useEffect, useRef, useState } from "react";
import { Copy, ExternalLink, Eye, EyeOff, Smartphone, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { remoteAccessValidation, type RemoteAccessCredentials, type RemoteAccessSettings, type RemoteAccessStatus, type ZrokAccessTest } from "@/remote-access-types";

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const FIXED_NAME_PATTERN = /^[a-z0-9](?:[a-z0-9-]{1,61}[a-z0-9])?$/;

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
  const [credentials, setCredentials] = useState<RemoteAccessCredentials | null>(null);
  const [zrokValue, setZrokValue] = useState(settings.zrokSetupValue);
  const [fixedNameValue, setFixedNameValue] = useState("");
  const [keyValue, setKeyValue] = useState("");
  const [keyDirty, setKeyDirty] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const mounted = useRef(true);
  const refreshing = useRef(false);
  const loadedZrokSetupValue = useRef(settings.zrokSetupValue);
  const loadedKeyValue = useRef<string | null>(null);
  const keyDirtyRef = useRef(false);
  const latestSettings = useRef(settings);
  latestSettings.current = settings;
  const patch = (value: Partial<RemoteAccessSettings>) => onChange({ ...latestSettings.current, ...value });
  const refresh = async () => {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      const saved = await command<RemoteAccessCredentials>("credentials");
      if (!mounted.current) return;
      if (saved.status === "ok") {
        setCredentials(saved);
        const savedKey = saved.key || "";
        if (!keyDirtyRef.current && loadedKeyValue.current !== savedKey) {
          loadedKeyValue.current = savedKey;
          setKeyValue(savedKey);
          setShowKey(false);
        }
      }
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
    const active = ["checking", "downloading", "starting", "connecting"].includes(status?.zrok?.state || "");
    const timer = window.setInterval(() => void refresh(), active ? 500 : 5000);
    return () => clearInterval(timer);
  }, [status?.zrok?.state]);
  useEffect(() => {
    if (settings.zrokSetupValue === loadedZrokSetupValue.current) return;
    loadedZrokSetupValue.current = settings.zrokSetupValue;
    setZrokValue(settings.zrokSetupValue);
  }, [settings.zrokSetupValue]);
  useEffect(() => {
    if (!fixedNameValue && status?.zrok?.fixedName) setFixedNameValue(status.zrok.fixedName);
  }, [fixedNameValue, status?.zrok?.fixedName]);
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
        : "启动公网";
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
  const renderZrokBadge = () => {
    switch (zrok?.state) {
      case "ready":
        return <span className="zrok-status-badge connected"><span className="zrok-status-dot" />可访问</span>;
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
    setFixedNameValue(result.zrok?.fixedName || fixedName);
    onSuccess(result.zrok?.state === "ready" ? "zrok 固定公网已启动。" : "zrok 固定公网正在启动并验证访问。");
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
  const confirmKeyChange = () => credentials?.accessKeyConfigured === false || window.confirm("更换访问密钥会使所有已授权设备失效，需要重新输入新密钥。继续？");
  const applySettings = async (next: RemoteAccessSettings) => {
    const error = remoteAccessValidation(next);
    if (error) throw new Error(error);
    onChange(next);
    try {
      await onSave(next);
    } catch (error) {
      onChange({ ...latestSettings.current, enabled: settings.enabled });
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
    onSuccess(result.running
      ? `设置已保存，远程网页服务已启动。${zrokNotice}${authorizationNotice}`
      : "设置已保存，远程访问已关闭。");
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
            <div><Label htmlFor="remote-bind">监听 IP 地址</Label><Input id="remote-bind" value={settings.bindHost} onChange={event => patch({ bindHost: event.target.value.trim() })} /><small>仅支持本机回环地址；公网访问由 zrok 转发。</small></div>
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
            <CardTitle>公网入口 · zrok</CardTitle>
            <CardDescription>绑定一次后复用固定公网地址；程序退出时远程网页与 zrok 同步停止。</CardDescription>
          </div>
          <div className="panel-head-actions flex items-center gap-3">
            {renderZrokBadge()}
          </div>
        </CardHeader>
        <CardContent>
          <div className="remote-access-fields">
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
                    onChange={event => setFixedNameValue(event.target.value.toLowerCase())}
                  />
                ) : null}
                <Button
                  variant="secondary"
                  className="h-9"
                  disabled={zrokActionDisabled}
                  onClick={() => void handleZrokAction()}
                >
                  {busy ? "处理中…" : zrokActionLabel}
                </Button>
              </div>
              {zrokToken && fixedNameValue && !fixedNameValid ? <small className="remote-access-inline-error">固定公网名称需为 3–63 位小写字母、数字或连字符，且首尾必须是字母或数字。</small> : null}
              <small>输入邮箱时发送注册邮件；输入 token 时同时填写固定公网名称。点击右侧按钮后，当前邮箱或 token 会保存到本机 CodexElves 设置并在下次打开时回显。</small>
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
          {["connecting", "retry_wait", "error"].includes(zrok?.state || "") && zrok?.error ? <p className="remote-access-error" role="alert">{zrok.error}</p> : null}
          {zrok?.state === "retry_wait" && zrok.retryAtMs ? <p className="remote-access-hint">将在 {new Date(zrok.retryAtMs).toLocaleTimeString()} 自动重试。</p> : null}
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
              <Input
                id="remote-access-key"
                type={showKey ? "text" : "password"}
                spellCheck={false}
                disabled={busy}
                autoComplete="off"
                minLength={8}
                maxLength={64}
                placeholder={credentials?.accessKeyConfigured
                  ? credentials.key ? "输入新密钥替换" : "旧密钥未保存明文，请重新输入并保存"
                  : "8–64 个非空格 ASCII 字符，建议随机生成"}
                value={keyValue}
                onChange={event => {
                  keyDirtyRef.current = true;
                  setKeyDirty(true);
                  setKeyValue(event.target.value);
                }}
              />
              <div className="remote-access-key-actions">
                <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={busy || !keyValue} aria-label={showKey ? "隐藏访问密钥" : "显示访问密钥"} onClick={() => setShowKey(value => !value)}>{showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}</Button>
                <Button variant="ghost" size="sm" className="h-7 w-7 p-0" disabled={busy || !keyValue} aria-label="复制访问密钥" onClick={() => void copy(keyValue)}><Copy className="h-4 w-4" /></Button>
              </div>
            </div><Button variant="secondary" disabled={busy} onClick={() => void run(async () => {
              const result = await command<{ status: string; key?: string; message?: string }>("generateKey");
              if (result.status !== "ok" || !result.key) throw new Error(result.message || "密钥生成失败");
              keyDirtyRef.current = true;
              setKeyDirty(true);
              setKeyValue(result.key); setShowKey(false);
              setMessage("随机密钥已生成，尚未保存。点击右侧「保存」后生效，当前设备授权不受影响。");
            })}>生成</Button><Button disabled={busy || !keyDirty || !keyValue.trim()} onClick={() => void runApply(async () => {
              const nextKey = keyValue.trim();
              if (!/^[\x21-\x7e]{8,64}$/.test(nextKey)) throw new Error("密钥需为 8–64 个非空格 ASCII 字符。");
              if (!confirmKeyChange()) return;
              const result = await command<RemoteAccessCredentials>("setKey", { key: nextKey });
              if (result.status !== "ok") throw new Error(result.message || "访问密钥保存失败");
              const savedKey = result.key || nextKey;
              loadedKeyValue.current = savedKey;
              keyDirtyRef.current = false;
              setKeyDirty(false);
              setKeyValue(savedKey);
              setShowKey(false);
              setCredentials({ ...result, key: savedKey });
              onSuccess("访问密钥已明文保存到本机并生效，默认以圆点遮挡，可点击小眼睛查看。原有设备需重新输入新密钥连接。");
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
