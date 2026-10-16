const messageTypes = new Set(["ready", "authenticated", "auth-required", "logged-out"]);

export function isAndroidContainer() {
  return typeof globalThis.CodexElvesAndroid?.postMessage === "function";
}

export function postPlatformMessage(type) {
  if (!messageTypes.has(type) || !isAndroidContainer()) return false;
  try {
    globalThis.CodexElvesAndroid.postMessage(JSON.stringify({ type }));
    return true;
  } catch {
    return false;
  }
}
