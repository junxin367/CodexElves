(() => {
  const helperBase = window.__CODEX_SESSION_DELETE_HELPER__ || "http://127.0.0.1:45221";
  const build = window.__CODEX_ELVES_BUILD__ || "unknown";
  const version = window.__CODEX_ELVES_VERSION__ || "unknown";
  const runtimeKey = "__codexElvesBootstrapRuntime";
  const runtime = window[runtimeKey] || {};
  window[runtimeKey] = runtime;

  if (
    runtime.build === build &&
    runtime.helperBase === helperBase &&
    runtime.status === "ready" &&
    typeof window.__codexElvesRefreshRuntime === "function"
  ) {
    window.__codexElvesRefreshRuntime();
    return;
  }

  runtime.build = build;
  runtime.helperBase = helperBase;
  runtime.version = version;
  if (runtime.loading) return;
  runtime.loading = true;

  function reportFailure(message) {
    runtime.status = "failed";
    runtime.error = message;
    reportDiagnostic("renderer_features_install_failed", { message });
  }

  function reportDiagnostic(event, detail) {
    try {
      fetch(`${helperBase}/diagnostics/log`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          event,
          build,
          version,
          ...(detail || {}),
        }),
      });
    } catch (_) {}
  }

  async function installViaBridge() {
    if (typeof window.__codexSessionDeleteBridge !== "function") {
      throw new Error("bridge is not ready");
    }
    const result = await window.__codexSessionDeleteBridge("/runtime/install-renderer-features", {
      build,
      version,
    });
    if (!result || result.status !== "ok") {
      throw new Error(result?.message || "feature install failed");
    }
    runtime.status = "ready";
  }

  async function installViaFetchedScript() {
    const cacheBuster = encodeURIComponent(build);
    const response = await fetch(`${helperBase}/inject/renderer-features.js?build=${cacheBuster}`, {
      cache: "no-store",
    });
    if (!response.ok) {
      throw new Error(`feature script fetch failed: ${response.status}`);
    }
    const source = await response.text();
    (0, eval)(`${source}\n//# sourceURL=codex-elves-renderer-features.js`);
    try {
      const userScripts = await fetch(`${helperBase}/inject/user-scripts.js?build=${cacheBuster}`, {
        cache: "no-store",
      });
      if (!userScripts.ok) {
        throw new Error(`user script fetch failed: ${userScripts.status}`);
      }
      const userSource = await userScripts.text();
      if (userSource.trim()) {
        (0, eval)(`${userSource}\n//# sourceURL=codex-elves-user-scripts.js`);
      }
      runtime.status = "ready_fallback";
      runtime.degraded = true;
      reportDiagnostic("renderer_features_install_fallback", { userScripts: "loaded" });
    } catch (userScriptError) {
      runtime.status = "ready_fallback_degraded";
      runtime.degraded = true;
      runtime.error = userScriptError?.message || String(userScriptError);
      reportDiagnostic("renderer_features_install_fallback_degraded", {
        message: runtime.error,
      });
    }
  }

  (async () => {
    try {
      await installViaBridge();
    } catch (bridgeError) {
      try {
        await installViaFetchedScript();
      } catch (fetchError) {
        reportFailure(`${bridgeError?.message || bridgeError}; ${fetchError?.message || fetchError}`);
      }
    } finally {
      runtime.loading = false;
    }
  })();
})();
