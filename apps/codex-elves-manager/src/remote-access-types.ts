export type RemoteAccessSettings = {
  enabled: boolean;
  bindHost: string;
  port: number;
  zrokSetupValue: string;
  deviceSessionHours: number;
  readOnly: boolean;
  allowApprovals: boolean;
};

export const defaultRemoteAccessSettings: RemoteAccessSettings = {
  enabled: false,
  bindHost: "127.0.0.1",
  port: 45222,
  zrokSetupValue: "",
  deviceSessionHours: 0,
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

export type ZrokState =
  | "missing"
  | "checking"
  | "downloading"
  | "unbound"
  | "name_required"
  | "stopped"
  | "starting"
  | "connecting"
  | "ready"
  | "retry_wait"
  | "error";

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
  loginLocked?: boolean;
  primary?: {
    provider: PrimaryProvider;
    publicUrl: string | null;
    switching: boolean;
    error?: string | null;
    frp: FrpStatus;
  };
  ngrok?: {
    configured: boolean;
    state: "stopped" | "starting" | "ready" | "error";
    error?: string | null;
  };
  zrok?: {
    state: ZrokState;
    running: boolean;
    connected: boolean;
    publicUrl: string;
    fixedName: string;
    error?: string | null;
    retryAtMs?: number | null;
    installed: boolean;
    bound: boolean;
    version?: string | null;
    downloadedBytes?: number;
    downloadTotalBytes?: number;
    downloadPercent?: number;
  };
};

export type RemoteAccessCredentials = {
  status: string;
  message?: string;
  accessKeyConfigured: boolean;
  loginLocked?: boolean;
  key?: string;
  ngrokTokenConfigured?: boolean;
  ngrokToken?: string;
  frpToken?: string;
};

export type PrimaryProvider = "zrok" | "frp";
export type FrpConfig = {
  serverAddr: string;
  serverPort: number;
  remotePort: number;
  publicOrigin: string;
  httpsMode: "provider" | "localCertificate";
};
export const defaultFrpConfig: FrpConfig = {
  serverAddr: "", serverPort: 7000, remotePort: 0, publicOrigin: "", httpsMode: "provider",
};
export type FrpStatus = {
  config: FrpConfig;
  state: "stopped" | "starting" | "ready" | "error" | "https_required";
  running: boolean;
  tokenConfigured: boolean;
  certificateName: string;
  privateKeyName: string;
  httpsConfigured: boolean;
  error?: string | null;
};

export type ZrokAccessTest = {
  status: string;
  publicUrl?: string;
  message?: string;
  httpStatus?: number;
  authRequired?: boolean;
};

export function remoteAccessValidation(settings: RemoteAccessSettings): string | null {
  if (!Number.isInteger(settings.port) || settings.port < 1 || settings.port > 65535) return "端口必须在 1–65535 之间。";
  if (settings.deviceSessionHours !== 0 && (!Number.isInteger(settings.deviceSessionHours) || settings.deviceSessionHours < 1 || settings.deviceSessionHours > 720)) return "设备授权有效期必须在 1–720 小时之间。";
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
  if (settings.enabled && !loopback) return "远程会话网页仅允许监听本机回环地址。";
  return null;
}
