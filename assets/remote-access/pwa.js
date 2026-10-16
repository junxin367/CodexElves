import { isAndroidContainer } from "./platform.js";

const shellVersion = 1;
const zrokShareHostnamePattern = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.shares\.zrok\.io$/i;
let registrationPromise = null;

export function prepareZrokInterstitialCookie() {
  if (isAndroidContainer() || globalThis.location?.protocol !== "https:"
      || !zrokShareHostnamePattern.test(globalThis.location.hostname)
      || !globalThis.document)
    return false;
  try {
    globalThis.document.cookie =
      "zrok_interstitial=1; Path=/; Secure; SameSite=Strict; Max-Age=604800";
    return true;
  } catch {
    return false;
  }
}

async function registerServiceWorker() {
  if (isAndroidContainer() || !globalThis.isSecureContext || !("serviceWorker" in navigator))
    return null;
  if (registrationPromise) return registrationPromise;
  registrationPromise = (async () => {
    const response = await fetch("/app-resources.json", {
      cache: "no-store",
      credentials: "same-origin",
      redirect: "error"
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const manifest = await response.json();
    if (manifest?.schemaVersion !== 1 || !Number.isInteger(manifest.minShellVersion)
        || manifest.minShellVersion > shellVersion
        || !/^[a-f0-9]{64}$/.test(manifest.version)
        || !Array.isArray(manifest.files))
      throw new Error("网页资源清单不受支持");
    return navigator.serviceWorker.register(
      `/sw.js?resources=${encodeURIComponent(manifest.version)}`,
      { scope: "/", updateViaCache: "none" }
    );
  })().finally(() => { registrationPromise = null; });
  return registrationPromise;
}

prepareZrokInterstitialCookie();
void registerServiceWorker().catch(() => {});
window.addEventListener("pageshow", prepareZrokInterstitialCookie);
window.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") prepareZrokInterstitialCookie();
});
window.addEventListener("online", () => {
  prepareZrokInterstitialCookie();
  void registerServiceWorker().catch(() => {});
});
