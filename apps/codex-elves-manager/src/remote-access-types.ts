export type RemoteAccessSettings = {
  enabled: boolean;
  bindHost: string;
  port: number;
  publicUrl: string;
  cloudflareTunnelEnabled: boolean;
  cloudflaredPath: string;
  tlsCertificatePath: string;
  tlsPrivateKeyPath: string;
  deviceSessionHours: number;
  readOnly: boolean;
  allowApprovals: boolean;
};

export const defaultRemoteAccessSettings: RemoteAccessSettings = {
  enabled: false,
  bindHost: "127.0.0.1",
  port: 45222,
  publicUrl: "",
  cloudflareTunnelEnabled: false,
  cloudflaredPath: "cloudflared",
  tlsCertificatePath: "",
  tlsPrivateKeyPath: "",
  deviceSessionHours: 168,
  readOnly: false,
  allowApprovals: true,
};

export type RemoteAccessDevice = {
  id: string;
  name: string;
  createdAtMs: number;
  expiresAtMs: number;
  lastSeenAtMs: number;
};

export type RemoteAccessStatus = {
  status: string;
  message?: string;
  enabled: boolean;
  running: boolean;
  nativeConnected: boolean;
  error?: string | null;
  addresses: string[];
  devices: RemoteAccessDevice[];
  settings?: RemoteAccessSettings;
  accessKeyConfigured?: boolean;
  tunnel?: {
    enabled: boolean;
    running: boolean;
    connected: boolean;
    state: "stopped" | "downloading" | "connecting" | "connected" | "error";
    publicUrl: string;
    processId?: number | null;
    error?: string | null;
  };
};

export type RemoteAccessCredentials = {
  status: string;
  message?: string;
  accessKeyConfigured: boolean;
  tunnelTokenConfigured: boolean;
};

export type RemoteAccessProgram = {
  status: string;
  installed: boolean;
  path?: string;
  version?: string;
  message?: string;
};

export function remoteAccessValidation(settings: RemoteAccessSettings): string | null {
  if (!Number.isInteger(settings.port) || settings.port < 1 || settings.port > 65535) return "端口必须在 1–65535 之间。";
  if (!Number.isInteger(settings.deviceSessionHours) || settings.deviceSessionHours < 1 || settings.deviceSessionHours > 720) return "设备授权有效期必须在 1–720 小时之间。";
  const ipv4 = settings.bindHost.split(".");
  let loopback = false;
  if (ipv4.length === 4 && ipv4.every(part => /^(0|[1-9]\d{0,2})$/.test(part) && Number(part) <= 255)) {
    loopback = ipv4[0] === "127";
  } else if (/^[\da-f:.]+$/i.test(settings.bindHost) && settings.bindHost.includes(":")) {
    try {
      const url = new URL(`http://[${settings.bindHost}]/`);
      loopback = url.hostname === "[::1]";
    } catch { return "请填写有效的监听 IP 地址。"; }
  } else return "请填写有效的监听 IP 地址。";
  if (!!settings.tlsCertificatePath !== !!settings.tlsPrivateKeyPath) return "HTTPS 证书和私钥必须同时设置。";
  if (settings.enabled && !loopback && !settings.tlsCertificatePath) return "非本机监听必须配置 HTTPS 证书与私钥。";
  if (settings.publicUrl) {
    try {
      const url = new URL(settings.publicUrl);
      if (url.protocol !== "https:" || url.username || url.password || url.pathname !== "/" || url.search || url.hash) return "外部访问地址必须是无路径、凭据或查询参数的 HTTPS 地址。";
    } catch { return "外部访问地址无效。"; }
  }
  if (settings.cloudflareTunnelEnabled) {
    if (!loopback || settings.tlsCertificatePath) return "Cloudflare 隧道必须转发到本机 HTTP 监听地址，不需配置本机 HTTPS 证书。";
    if (!settings.publicUrl) return "Cloudflare 隧道需要填写已绑定的固定 HTTPS 域名。";
    const hostname = new URL(settings.publicUrl).hostname;
    if (!hostname.includes(".") || hostname.startsWith("[") || /^\d+\.\d+\.\d+\.\d+$/.test(hostname) || hostname.endsWith(".trycloudflare.com")) return "Cloudflare 隧道需要使用已绑定的域名。";
    if (settings.cloudflaredPath.includes("\0")) return "cloudflared 程序路径不能包含空字符。";
  }
  return null;
}
