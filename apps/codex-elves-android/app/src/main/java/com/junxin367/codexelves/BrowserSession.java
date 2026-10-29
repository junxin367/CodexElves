package com.junxin367.codexelves;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.content.Context;
import android.content.MutableContextWrapper;
import android.graphics.Bitmap;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.webkit.CookieManager;
import android.webkit.ConsoleMessage;
import android.webkit.HttpAuthHandler;
import android.webkit.JavascriptInterface;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.SslErrorHandler;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebStorage;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.webkit.ValueCallback;
import android.view.View;
import android.view.ViewGroup;
import androidx.webkit.JavaScriptReplyProxy;
import androidx.webkit.ProxyConfig;
import androidx.webkit.ProxyController;
import androidx.webkit.ScriptHandler;
import androidx.webkit.WebMessageCompat;
import androidx.webkit.UserAgentMetadata;
import androidx.webkit.WebStorageCompat;
import androidx.webkit.WebSettingsCompat;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;
import org.json.JSONException;
import org.json.JSONObject;
import org.json.JSONArray;
import org.json.JSONTokener;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

/** Main-thread-owned browser state; never retains a detached Activity. */
final class BrowserSession {
    static final class DiagnosticEvent {
        final long id;
        final long timeMs;
        final String phase;
        final String method;
        final String url;
        String status;
        String detail;
        String headers;
        String requestBody = "";
        String responseBody = "";
        String responseMime = "";
        String requestId = "";
        long generation;
        boolean generationPending;
        boolean nativeObserved;
        boolean mainFrame;
        String resourceType;
        long durationMs;
        long sizeBytes;
        int statusCode;
        boolean response;
        boolean failure;

        DiagnosticEvent(long id, long timeMs, String phase, String method, String url,
            String status, String detail, String headers, boolean mainFrame,
            String resourceType, long durationMs, long sizeBytes, int statusCode,
            boolean response, boolean failure) {
            this.id = id;
            this.timeMs = timeMs;
            this.phase = phase;
            this.method = method;
            this.url = url;
            this.status = status;
            this.detail = detail;
            this.headers = headers;
            this.mainFrame = mainFrame;
            this.resourceType = resourceType;
            this.durationMs = durationMs;
            this.sizeBytes = sizeBytes;
            this.statusCode = statusCode;
            this.response = response;
            this.failure = failure;
        }

        DiagnosticEvent copy() {
            DiagnosticEvent copy = new DiagnosticEvent(id, timeMs, phase, method, url, status, detail, headers,
                mainFrame, resourceType, durationMs, sizeBytes, statusCode, response, failure);
            copy.requestBody = requestBody;
            copy.responseBody = responseBody;
            copy.responseMime = responseMime;
            copy.requestId = requestId;
            copy.generation = generation;
            return copy;
        }
    }

    interface Listener {
        void changed();
        void invalidated(String message);
        void requestAuthentication(String host, HttpAuthHandler handler);
        void chooseFiles(ValueCallback<Uri[]> callback, WebChromeClient.FileChooserParams parameters);
    }
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    private static final int DIAGNOSTIC_EVENT_LIMIT = 300;
    // Desktop mode lays the page out at this width instead of the page's device-width viewport.
    private static final int DESKTOP_VIEWPORT_WIDTH = 1280;
    private static final String DESKTOP_VIEWPORT_SCRIPT = "(function(){if(window.top!==window)return;"
        + "var content='width=" + DESKTOP_VIEWPORT_WIDTH + "';"
        + "function apply(){var metas=document.querySelectorAll('meta[name=viewport]');"
        + "if(metas.length===0){if(!document.head)return;var meta=document.createElement('meta');"
        + "meta.name='viewport';document.head.appendChild(meta);metas=[meta];}"
        + "for(var i=0;i<metas.length;i++){if(metas[i].getAttribute('content')!==content)"
        + "metas[i].setAttribute('content',content);}}"
        + "apply();new MutationObserver(apply).observe(document,{childList:true,subtree:true,"
        + "attributes:true,attributeFilter:['name','content']});})();";
    private static final String CREDENTIAL_BRIDGE_NAME = "__codexElvesCredentialRecorder";
    private static final String NETWORK_BRIDGE_NAME = "__codexElvesNetworkRecorder";
    private static final String CREDENTIAL_COLLECTOR_SCRIPT = """
(function(){
  if (window.__codexElvesCredentialCollectorInstalled
      || window.__codexElvesCredentialCollectorWaiting) return;
  function status(phase, reason) {
    try {
      var recorder = window.__codexElvesCredentialRecorder;
      if (!recorder || typeof recorder.postMessage !== 'function') return;
      recorder.postMessage(JSON.stringify({ kind: 'status', origin: location.origin,
        generation: Number(window.__codexElvesCredentialPageGeneration) || 0,
        phase: phase, reason: reason }));
    } catch (error) {}
  }
  function install() {
    window.__codexElvesCredentialCollectorWaiting = false;
    if (window.__codexElvesCredentialCollectorInstalled) return;
    try {
      if (!document.documentElement) { status('failure', 'dom-not-ready'); return; }
      function meta(input) {
        return [input.name, input.id, input.autocomplete, input.placeholder,
          input.getAttribute('aria-label')].join(' ').toLowerCase();
      }
      function value(input, limit) {
        return String(input && input.value || '').slice(0, limit);
      }
      function textInputs(scope) {
        return Array.from(scope.querySelectorAll('input,select')).filter(function(input) {
          return !input.disabled && input.getClientRects().length > 0
            && !['password', 'hidden', 'checkbox', 'radio', 'submit', 'button'].includes(input.type);
        });
      }
      function subaccountField(input) {
        return /sub.?account|sacct|employee|staff|member|tenant|organiz|organis|workspace|team|company|domain|子账号|成员|租户|组织|团队|企业|空间/.test(meta(input));
      }
      function usernameField(input) {
        return /user.?name|email|e-mail|login|account|手机号|邮箱|账号|用户名/.test(meta(input));
      }
      var savedRecords = [], chooserHost = null, chooserList = null, chooserInput = null;
      function fillField(input, next) {
        if (!input) return;
        var prototype = input.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
        var descriptor = Object.getOwnPropertyDescriptor(prototype, 'value');
        if (descriptor && descriptor.set) descriptor.set.call(input, next); else input.value = next;
        input.dispatchEvent(new Event('input', { bubbles: true }));
        input.dispatchEvent(new Event('change', { bubbles: true }));
      }
      function loginContext(input) {
        var scope = input && input.closest ? input.closest('form') || document : document;
        var passwords = Array.from(scope.querySelectorAll('input[type=password]')).filter(function(field) {
          return !field.disabled && field.getClientRects().length > 0;
        });
        if (passwords.length !== 1) return null;
        var inputs = textInputs(scope);
        var subaccount = inputs.find(subaccountField);
        var username = inputs.find(function(field) { return field.autocomplete === 'username'; })
          || inputs.find(function(field) { return !subaccountField(field) && usernameField(field); })
          || inputs.find(function(field) { return !subaccountField(field); });
        if (!username && !subaccount) return null;
        return { password: passwords[0], username: username, subaccount: subaccount };
      }
      function canChooseCredentials(input) {
        if (!input || input.tagName !== 'INPUT' || input.disabled) return false;
        return input.type === 'password' || input.autocomplete === 'username'
          || usernameField(input) || subaccountField(input);
      }
      function hideCredentialChooser() {
        if (chooserHost) chooserHost.remove();
        chooserHost = null; chooserList = null; chooserInput = null;
      }
      function ensureCredentialChooser() {
        if (chooserHost) return;
        chooserHost = document.createElement('div');
        chooserHost.setAttribute('aria-label', '选择已保存的账号');
        chooserHost.style.cssText = 'all:initial;position:fixed;z-index:2147483647;display:block;';
        var shadow = chooserHost.attachShadow({ mode: 'open' });
        var style = document.createElement('style');
        style.textContent = '.panel{font:14px/1.4 -apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;' +
          'color:#202124;background:#fff;border:1px solid #dadce0;border-radius:8px;' +
          'box-shadow:0 4px 18px rgba(0,0,0,.2);overflow:auto;max-height:inherit;}' +
          'button{display:block;width:100%;box-sizing:border-box;padding:10px 12px;border:0;' +
          'border-bottom:1px solid #e8eaed;background:transparent;color:inherit;font:inherit;' +
          'text-align:left;cursor:pointer;}button:last-child{border-bottom:0}' +
          'button:hover,button:focus{background:#f1f3f4;outline:none}.primary,.secondary{' +
          'display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}' +
          '.primary{font-weight:500}.secondary{font-size:12px;color:#5f6368;margin-top:2px}' +
          '@media(prefers-color-scheme:dark){.panel{color:#e8eaed;background:#2d2e30;' +
          'border-color:#5f6368}button{border-color:#5f6368}button:hover,button:focus{' +
          'background:#3c4043}.secondary{color:#bdc1c6}}';
        chooserList = document.createElement('div');
        chooserList.className = 'panel'; chooserList.setAttribute('role', 'listbox');
        shadow.appendChild(style); shadow.appendChild(chooserList);
      }
      function matchingRecords(context, input) {
        var currentUsername = value(context.username, 512).trim();
        var currentSubaccount = value(context.subaccount, 512).trim();
        return savedRecords.filter(function(record) {
          if (!record || record.username && !context.username
              || record.subaccount && !context.subaccount) return false;
          if (input === context.subaccount) {
            return !currentUsername || currentUsername === record.username;
          }
          if (currentSubaccount && currentSubaccount !== record.subaccount) return false;
          return input !== context.password || !currentUsername || currentUsername === record.username;
        });
      }
      function positionCredentialChooser() {
        if (!chooserHost || !chooserInput || !chooserInput.isConnected) return;
        var rect = chooserInput.getBoundingClientRect(), viewportWidth = window.innerWidth;
        var viewportHeight = window.innerHeight, below = viewportHeight - rect.bottom - 8;
        var above = rect.top - 8, width = Math.min(Math.max(220, rect.width), viewportWidth - 8);
        chooserHost.style.width = width + 'px';
        chooserHost.style.left = Math.max(4, Math.min(rect.left, viewportWidth - width - 4)) + 'px';
        chooserHost.style.maxHeight = Math.max(80, Math.min(264, Math.max(above, below))) + 'px';
        var height = chooserHost.offsetHeight, top = rect.bottom + 4;
        if (height > below && above > below) top = Math.max(4, rect.top - height - 4);
        chooserHost.style.top = Math.max(4, Math.min(top, viewportHeight - height - 4)) + 'px';
      }
      function chooseCredential(context, record) {
        fillField(context.username, String(record.username || ''));
        fillField(context.subaccount, String(record.subaccount || ''));
        fillField(context.password, String(record.password || ''));
        try { context.password.focus({ preventScroll: true }); } catch (error) { context.password.focus(); }
        hideCredentialChooser();
      }
      function showCredentialChooser(input) {
        var context = loginContext(input);
        if (!context || !savedRecords.length) { hideCredentialChooser(); return; }
        var records = matchingRecords(context, input);
        if (!records.length) { hideCredentialChooser(); return; }
        hideCredentialChooser(); chooserInput = input; ensureCredentialChooser();
        records.forEach(function(record) {
          var button = document.createElement('button');
          button.type = 'button'; button.setAttribute('role', 'option');
          var primary = document.createElement('span');
          primary.className = 'primary';
          primary.textContent = String(record.username || record.subaccount || '');
          button.appendChild(primary);
          if (record.username && record.subaccount) {
            var secondary = document.createElement('span');
            secondary.className = 'secondary';
            secondary.textContent = '子账号：' + String(record.subaccount);
            button.appendChild(secondary);
          }
          button.addEventListener('mousedown', function(event) { event.preventDefault(); });
          button.addEventListener('click', function() { chooseCredential(context, record); });
          chooserList.appendChild(button);
        });
        document.documentElement.appendChild(chooserHost); positionCredentialChooser();
      }
      window.__codexElvesSetCredentialRecords = function(records) {
        savedRecords = Array.isArray(records) ? records.filter(function(record) {
          return record && typeof record === 'object' && record.password;
        }) : [];
        hideCredentialChooser();
      };
      document.addEventListener('focusin', function(event) {
        if (canChooseCredentials(event.target)) showCredentialChooser(event.target);
      }, true);
      document.addEventListener('click', function(event) {
        if (canChooseCredentials(event.target)) showCredentialChooser(event.target);
        else if (!chooserHost || !event.composedPath().includes(chooserHost)) hideCredentialChooser();
      }, true);
      document.addEventListener('scroll', hideCredentialChooser, true);
      window.addEventListener('resize', hideCredentialChooser);
      function capture() {
        if (!/^https?:$/.test(location.protocol)) return;
        var recorder = window.__codexElvesCredentialRecorder;
        if (!recorder || typeof recorder.postMessage !== 'function') return;
        status('capture', 'attempt');
        var passwords = Array.from(document.querySelectorAll('input[type=password]')).filter(function(input) {
          return !input.disabled && input.value && input.getClientRects().length > 0;
        });
        if (!passwords.length) { status('failure', 'missing-password'); return; }
        var password = passwords[0], scope = password.closest('form') || document;
        var scopePasswords = Array.from(scope.querySelectorAll('input[type=password]')).filter(function(input) {
          return !input.disabled && input.value && input.getClientRects().length > 0;
        });
        if (scopePasswords.length !== 1) { status('failure', 'ambiguous-password'); return; }
        password = scopePasswords[0];
        var inputs = textInputs(scope), username = '', subaccount = '';
        var namedUser = inputs.find(function(input) {
          return input.autocomplete === 'username' && value(input, 512).trim();
        });
        if (namedUser) username = value(namedUser, 512).trim();
        for (var index = 0; index < inputs.length; index++) {
          var input = inputs[index], current = value(input, 512).trim();
          if (!current) continue;
          if (!subaccount && subaccountField(input)) subaccount = current;
          else if (!username && usernameField(input)) username = current;
        }
        if (!username) {
          var fallback = inputs.find(function(input) {
            return !subaccountField(input) && value(input, 512).trim();
          });
          if (fallback) username = value(fallback, 512).trim();
        }
        if (!username && !subaccount) { status('failure', 'missing-fields'); return; }
        recorder.postMessage(JSON.stringify({ kind: 'record', origin: location.origin,
          generation: Number(window.__codexElvesCredentialPageGeneration) || 0,
          username: username, password: value(password, 4096), subaccount: subaccount }));
      }
      document.addEventListener('keydown', function(event) {
        if (event.key === 'Escape') hideCredentialChooser();
      }, true);
      document.addEventListener('submit', capture, true);
      document.addEventListener('click', function(event) {
        var target = event.target && event.target.closest
          ? event.target.closest('button,input,a,[role=button],[onclick]') : null;
        if (!target) return;
        var type = String(target.getAttribute('type') || '').toLowerCase();
        var label = [target.value, target.innerText, target.getAttribute('aria-label')].join(' ');
        if (type === 'submit' || target.tagName === 'BUTTON' && !type
            || /登\\s*录|登\\s*入|sign.?in|log.?in|submit|提交|继续|next/i.test(label)) capture();
      }, true);
      document.addEventListener('keydown', function(event) {
        if (event.key === 'Enter' && event.target && event.target.tagName === 'INPUT') capture();
      }, true);
      window.__codexElvesCredentialCollectorInstalled = true;
      status('install', 'ready');
    } catch (error) { status('failure', 'install'); }
  }
  if (document.documentElement) install();
  else {
    window.__codexElvesCredentialCollectorWaiting = true;
    document.addEventListener('DOMContentLoaded', install, { once: true });
    status('install', 'waiting-dom');
  }
})();
""";
    private static final String NETWORK_COLLECTOR_SCRIPT = """
(function(){
  if (window.__codexElvesNetworkCollectorInstalled) return;
  window.__codexElvesNetworkCollectorInstalled = true;
  var REQUEST_LIMIT = 32768, RESPONSE_LIMIT = 65536, nextRequestId = 0;
  function newRequestId() {
    return Date.now().toString(36) + '-' + (++nextRequestId) + '-' + Math.random().toString(36).slice(2, 10);
  }
  function currentGeneration() { return Number(window.__codexElvesNetworkPageGeneration) || 0; }
  function sensitiveKey(key) {
    return /pass(?:word|code)?|pwd|token|secret|authorization|cookie|credential|api[_-]?key|account|username|cacct|sacct/i.test(String(key || ''));
  }
  function redactJson(value) {
    if (Array.isArray(value)) return value.map(redactJson);
    if (value && typeof value === 'object') {
      var result = {};
      Object.keys(value).forEach(function(key) {
        result[key] = sensitiveKey(key) ? '<redacted>' : redactJson(value[key]);
      });
      return result;
    }
    return value;
  }
  function redactText(value) {
    var trimmed = value.trim();
    if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
      try { return JSON.stringify(redactJson(JSON.parse(value))); } catch (error) {}
    }
    return value.replace(/((?:^|[?&;\\s])[^?=&;\\s]*(?:pass(?:word|code)?|pwd|token|secret|authorization|cookie|credential|api[_-]?key|account|username|cacct|sacct)[^=&;\\s]*=)[^&;\\s]*/gi, '$1<redacted>');
  }
  function limited(value, limit) {
    value = redactText(String(value == null ? '' : value));
    return value.length > limit ? value.slice(0, limit) + '\\n…[内容已截断]' : value;
  }
  function body(value, limit) {
    if (value == null) return '';
    if (typeof value === 'string') return limited(value, limit);
    if (typeof URLSearchParams !== 'undefined' && value instanceof URLSearchParams)
      return limited(value.toString(), limit);
    if (typeof FormData !== 'undefined' && value instanceof FormData) {
      var entries = [];
      value.forEach(function(item, key) {
        entries.push({ name: key, value: sensitiveKey(key) ? '<redacted>'
          : typeof item === 'string' ? item : '[File ' + String(item && item.name || 'unnamed') + ']' });
      });
      return limited(JSON.stringify(entries), limit);
    }
    if (typeof Blob !== 'undefined' && value instanceof Blob)
      return '[Blob ' + value.type + ' ' + value.size + ' bytes]';
    if (typeof ArrayBuffer !== 'undefined' && value instanceof ArrayBuffer)
      return '[ArrayBuffer ' + value.byteLength + ' bytes]';
    if (typeof ArrayBuffer !== 'undefined' && ArrayBuffer.isView && ArrayBuffer.isView(value))
      return '[' + value.constructor.name + ' ' + value.byteLength + ' bytes]';
    if (typeof ReadableStream !== 'undefined' && value instanceof ReadableStream) return '[ReadableStream]';
    try { return limited(JSON.stringify(value), limit); }
    catch (error) { return limited(String(value), limit); }
  }
  function textual(mime) {
    return !mime || /^text[/]/i.test(mime) || /json|xml|javascript|ecmascript|x-www-form-urlencoded/i.test(mime);
  }
  function binary(mime, size) {
    return '[非文本响应' + (mime ? ' ' + mime : '') + (Number(size) > 0 ? ' ' + Number(size) + ' bytes' : '') + ']';
  }
  function report(kind, info, requestBody, responseBody, status, mime, failure, detail) {
    try {
      var recorder = window.__codexElvesNetworkRecorder;
      if (!recorder || typeof recorder.record !== 'function') return;
      var url = new URL(info.target, location.href).href;
      if (!/^https?:$/.test(new URL(url).protocol)) return;
      var payload = { kind: kind, requestId: info.id, resourceType: info.resourceType,
        origin: location.origin, generation: currentGeneration(), method: info.method, url: url,
        status: Number(status) || 0, mime: String(mime || '') };
      if (requestBody !== undefined) payload.requestBody = requestBody;
      if (responseBody !== undefined) payload.responseBody = responseBody;
      if (failure === true) payload.failure = true;
      if (detail) payload.detail = String(detail).slice(0, 128);
      recorder.record(JSON.stringify(payload));
    } catch (error) {}
  }
  if (typeof window.fetch === 'function') {
    var originalFetch = window.fetch;
    window.fetch = function(input, init) {
      var request = typeof Request !== 'undefined' && input instanceof Request ? input : null;
      var info = { id: newRequestId(), resourceType: 'fetch',
        method: String(init && init.method || request && request.method || 'GET').toUpperCase(),
        target: String(request ? request.url : input) };
      var args = arguments;
      report('start', info);
      if (init && init.body != null) report('requestBody', info, body(init.body, REQUEST_LIMIT));
      else if (request && !/^(?:GET|HEAD)$/.test(info.method)) {
        request.clone().text().then(function(text) {
          report('requestBody', info, limited(text, REQUEST_LIMIT));
        }).catch(function() {});
      }
      return originalFetch.apply(this, args).then(function(response) {
        var mime = response.headers.get('content-type') || '';
        if (textual(mime)) response.clone().text().then(function(text) {
          report('response', info, undefined, limited(text, RESPONSE_LIMIT), response.status, mime, false);
        }).catch(function() {});
        else report('response', info, undefined, binary(mime, response.headers.get('content-length')),
          response.status, mime, false);
        return response;
      }, function(error) {
        report('response', info, undefined, undefined, 0, '', true, error && error.name || 'FetchError');
        throw error;
      });
    };
  }
  if (typeof XMLHttpRequest !== 'undefined') {
    var open = XMLHttpRequest.prototype.open, send = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function(method, target) {
      this.__codexElvesNetwork = { id: newRequestId(), resourceType: 'xhr',
        method: String(method || 'GET').toUpperCase(), target: String(target) };
      return open.apply(this, arguments);
    };
    XMLHttpRequest.prototype.send = function(value) {
      var info = this.__codexElvesNetwork;
      if (info) {
        report('start', info);
        report('requestBody', info, body(value, REQUEST_LIMIT));
        this.addEventListener('loadend', function() {
          var response = '', type = this.responseType;
          if (type === 'json') {
            try { response = JSON.stringify(this.response); } catch (error) {}
          } else if (type === '' || type === 'text') response = this.responseText;
          else if (type === 'document' && this.response && typeof XMLSerializer !== 'undefined')
            response = new XMLSerializer().serializeToString(this.response);
          else if (type === 'blob' && this.response) response = '[Blob ' + this.response.type + ' ' + this.response.size + ' bytes]';
          else if (type === 'arraybuffer' && this.response) response = '[ArrayBuffer ' + this.response.byteLength + ' bytes]';
          else if (this.response) response = '[' + type + ' response]';
          report('response', info, undefined, limited(response, RESPONSE_LIMIT),
            this.status, this.getResponseHeader('content-type') || '', this.status === 0, this.status === 0 ? 'NetworkError' : '');
        });
      }
      return send.apply(this, arguments);
    };
  }
})();
""";
    // One retained view; detach replaces its MutableContextWrapper base with the application.
    @SuppressLint("StaticFieldLeak")
    private static BrowserSession current;
    private static boolean directoryConfigured, clearing;
    private static Runnable afterClear;
    private static long openGeneration;
    final WebView view;
    final BrowserAuthority.Snapshot identity;
    final BrowserPreferences preferences;
    private final IBinder authority;
    private final MutableContextWrapper context;
    private final AutoCloseable observer;
    private final BrowserProxy proxy;
    private final BrowserCredentialStore credentialStore;
    private boolean credentialRecordingEnabled, credentialMessageSupported;
    private String credentialError = "";
    private String credentialPageOrigin = "", credentialPreviousPageOrigin = "";
    private long credentialPageGeneration, credentialPreviousPageGeneration;
    private Listener listener;
    private boolean closed, attached, routeReady;
    private long proxyGeneration;
    private long pageGeneration;
    private String networkPageOrigin = "";
    private boolean networkDocumentStartAvailable, networkFallbackReloaded;
    private String pendingNavigationUrl = "";
    private String localFileThreadId = "";
    private String localFileOrigin = "";
    private boolean mainFrameStarted;
    private boolean mainFrameFinished;
    private String mobileUserAgent = "";
    private String androidUserAgent = "";
    private String desktopUserAgent = "";
    private String iphoneUserAgent = "";
    private String userAgentMode;
    private String pageTitle = "";
    private boolean desktopRootScrollLocked;
    private ScriptHandler desktopViewportScript;
    private ScriptHandler networkCollectorScript;
    private ScriptHandler credentialCollectorScript;
    private final ArrayDeque<DiagnosticEvent> diagnosticEvents = new ArrayDeque<>();
    private final Map<String, DiagnosticEvent> latestNetworkEvents = new HashMap<>();
    private final Map<String, DiagnosticEvent> networkEventsByRequestId = new HashMap<>();
    private final Map<String, ArrayDeque<DiagnosticEvent>> pendingNetworkEvents = new HashMap<>();
    private final Set<String> performanceResourceKeys = new HashSet<>();
    private long nextDiagnosticId;
    private final Runnable desktopRootScrollPolicyRefresh = this::updateDesktopRootScrollPolicy;
    private final Runnable diagnosticRefresh = () -> {
        if (!closed && listener != null) listener.changed();
    };
    boolean home = true;
    String url = "", error = "";
    int progress = 100;

    private static String errorEventSuffix(Throwable error) {
        if (error == null) return "";
        String value = error.getClass().getSimpleName().toLowerCase(Locale.ROOT)
            .replaceAll("[^a-z0-9]+", "-");
        return value.replaceAll("^-+|-+$", "");
    }

    private static void initializationFailure(
        IBinder authority, String stage, Throwable error, JSONObject fields
    ) {
        BrowserAuthority.recordDiagnostic(authority, "native.browser.init", fields);
        BrowserAuthority.recordDiagnostic(authority, "native.browser.init." + stage, fields);
        String suffix = errorEventSuffix(error);
        if (!suffix.isEmpty()) {
            BrowserAuthority.recordDiagnostic(authority,
                "native.browser.init." + stage + "." + suffix, fields);
        }
    }

    private static void initializationFailure(IBinder authority, String stage, JSONObject fields) {
        initializationFailure(authority, stage, null, fields);
    }

    static void prepare(IBinder authority) {
        if (Build.VERSION.SDK_INT < 28) throw new IllegalStateException("浏览器需要 Android 9 或更新版本");
        if (!directoryConfigured) {
            WebView.setDataDirectorySuffix("codex-elves-browser");
            directoryConfigured = true;
        }
        boolean proxyOverride = WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE);
        boolean reverseBypass = WebViewFeature.isFeatureSupported(
            WebViewFeature.PROXY_OVERRIDE_REVERSE_BYPASS);
        boolean deleteBrowsingData = WebViewFeature.isFeatureSupported(
            WebViewFeature.DELETE_BROWSING_DATA);
        int featureMask = (proxyOverride ? 1 : 0) | (reverseBypass ? 2 : 0)
            | (deleteBrowsingData ? 4 : 0);
        BrowserAuthority.recordDiagnostic(authority, "native.browser.init",
            DiagnosticsPolicy.fields("phase", "request", "routeName", "prepare",
                "errorCode", featureMask, "reason", "request"));
        // The browser now sends every WebView request through the computer.
        // Reverse bypass is no longer required for routing, but it remains in
        // the diagnostic mask so older clients can still report provider capabilities.
        if (!proxyOverride) {
            initializationFailure(authority, "prepare",
                DiagnosticsPolicy.fields("phase", "failure", "routeName", "prepare",
                    "errorCode", featureMask, "reason", "unsupported"));
            throw new IllegalStateException("请更新 Android System WebView 后使用浏览器");
        }
    }

    static void acquire(Activity activity, IBinder authority,
        Consumer<BrowserSession> ready, Consumer<String> failure) {
        long generation = ++openGeneration;
        String stage = "prepare";
        try {
            prepare(authority);
            stage = "authority";
            BrowserAuthority.Snapshot identity = BrowserAuthority.read(authority);
            if (current != null && !current.closed && current.identity.owner().equals(identity.owner())) {
                current.context.setBaseContext(activity);
                ready.accept(current);
                return;
            }
            if (current != null) {
                current.destroy();
                current = null;
            }
            Runnable create = () -> {
                if (generation != openGeneration || activity.isFinishing() || activity.isDestroyed()) return;
                String createStage = "authority";
                AtomicReference<String> sessionStage = new AtomicReference<>("session");
                try {
                    BrowserAuthority.Snapshot fresh = BrowserAuthority.read(authority);
                    if (!identity.owner().equals(fresh.owner())) throw new IllegalStateException("浏览器授权已失效");
                    createStage = "session";
                    current = new BrowserSession(activity, authority, fresh, sessionStage::set);
                    createStage = "ready";
                    activity.getSharedPreferences("codex_elves_browser_identity", Context.MODE_PRIVATE)
                        .edit().putString("owner", fresh.owner()).apply();
                    ready.accept(current);
                } catch (Exception unavailable) {
                    String failedStage = "session".equals(createStage) ? sessionStage.get() : createStage;
                    initializationFailure(authority, failedStage, unavailable,
                        DiagnosticsPolicy.fields("phase", "failure", "routeName", failedStage,
                            "errorType", DiagnosticsPolicy.errorType(unavailable), "reason", "failed"));
                    failure.accept("无法打开浏览器，请返回会话后重试");
                }
            };
            String previousOwner = activity.getSharedPreferences("codex_elves_browser_identity", Context.MODE_PRIVATE)
                .getString("owner", "");
            if (clearing || !identity.owner().equals(previousOwner)) {
                stage = "profile";
                afterClear = create;
                clearProfile(authority);
            } else create.run();
        } catch (Exception unavailable) {
            initializationFailure(authority, stage, unavailable,
                DiagnosticsPolicy.fields("phase", "failure", "routeName", stage,
                    "errorType", DiagnosticsPolicy.errorType(unavailable), "reason", "failed"));
            failure.accept(unavailable.getMessage() == null ? "浏览器当前不可用" : unavailable.getMessage());
        }
    }

    @SuppressLint("RequiresFeature") // DELETE_BROWSING_DATA is checked before the optional call.
    private static void clearProfile(IBinder authority) {
        if (clearing) return;
        clearing = true;
        Runnable complete = () -> {
            clearing = false;
            Runnable next = afterClear; afterClear = null;
            // Let the WebView provider finish its cleanup callback before creating
            // a WebView in the same process.
            if (next != null) MAIN.post(next);
        };
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.DELETE_BROWSING_DATA)) {
            BrowserAuthority.recordDiagnostic(authority, "native.browser.init",
                DiagnosticsPolicy.fields("phase", "success", "routeName", "profile",
                    "reason", "unsupported"));
            complete.run();
            return;
        }
        try {
            WebStorageCompat.deleteBrowsingData(WebStorage.getInstance(), () -> {
                BrowserAuthority.recordDiagnostic(authority, "native.browser.init",
                    DiagnosticsPolicy.fields("phase", "success", "routeName", "profile",
                        "reason", "success"));
                complete.run();
            });
        } catch (RuntimeException failure) {
            initializationFailure(authority, "profile",
                DiagnosticsPolicy.fields("phase", "failure", "routeName", "profile",
                    "errorType", DiagnosticsPolicy.errorType(failure), "reason", "failed"));
            // Profile cleanup is best-effort. It must never prevent the browser from opening.
            complete.run();
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    private BrowserSession(Activity activity, IBinder authority, BrowserAuthority.Snapshot identity,
        Consumer<String> initializationStage) throws Exception {
        this.authority = authority; this.identity = identity;
        initializationStage.accept("preferences");
        context = new MutableContextWrapper(activity);
        preferences = new BrowserPreferences(activity);
        initializationStage.accept("proxy");
        proxy = new BrowserProxy(authority, () -> {
            try {
                BrowserAuthority.Snapshot fresh = BrowserAuthority.read(authority);
                if (!identity.owner().equals(fresh.owner())) return null;
                return new BrowserProxy.Credentials(fresh.origin(), fresh.cookie());
            } catch (Exception expired) { MAIN.post(() -> invalidate("设备授权已失效，请返回会话重新连接")); return null; }
        }, () -> MAIN.post(() -> invalidate("设备授权已失效，请返回会话重新连接")));
        proxy.setActive(false);
        initializationStage.accept("webview");
        view = new WebView(context);
        view.setOnScrollChangeListener((webView, scrollX, scrollY, oldScrollX, oldScrollY) -> {
            if (desktopRootScrollLocked && scrollY != 0) {
                webView.scrollTo(scrollX, 0);
            }
        });
        view.setSaveEnabled(false);
        credentialStore = new BrowserCredentialStore(context.getApplicationContext());
        credentialRecordingEnabled = credentialStore.isRecordingEnabled(identity.owner());
        networkDocumentStartAvailable = WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT);
        credentialMessageSupported = WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER);
        if (credentialMessageSupported) {
            try { registerCredentialBridge(); }
            catch (RuntimeException unavailable) {
                credentialMessageSupported = false;
                diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                    "reason", "bridge-unsupported"));
            }
        }
        view.addJavascriptInterface(new NetworkRecorderBridge(), NETWORK_BRIDGE_NAME);
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setSupportZoom(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        settings.setUseWideViewPort(true);
        settings.setLoadWithOverviewMode(true);
        settings.setSupportMultipleWindows(false);
        settings.setMediaPlaybackRequiresUserGesture(true);
        try {
            mobileUserAgent = settings.getUserAgentString();
            androidUserAgent = androidUserAgent(mobileUserAgent);
            desktopUserAgent = desktopUserAgent(mobileUserAgent);
            iphoneUserAgent = iphoneUserAgent();
            userAgentMode = preferences.loadUserAgentMode(identity.origin());
            applyDisplayMode(settings);
        } catch (RuntimeException unavailable) {
            // UA customization is optional; keep the WebView default so startup can continue.
            mobileUserAgent = "";
            androidUserAgent = "";
            desktopUserAgent = "";
            iphoneUserAgent = "";
            userAgentMode = BrowserPreferences.UA_ANDROID;
            diagnostic("native.browser.ua", DiagnosticsPolicy.fields(
                "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                "reason", "unsupported"));
        }
        try {
            CookieManager.getInstance().setAcceptCookie(true);
            CookieManager.getInstance().setAcceptThirdPartyCookies(view, false);
            installNetworkCollector(view);
            installCredentialCollector(view);
        } catch (RuntimeException unavailable) {
            // Cookie policy must not prevent the browser surface from opening.
            diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                "reason", "unsupported"));
        }
        view.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView webView, int value) {
                progress = value;
                changed();
                if (value >= 100 && BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    MAIN.postDelayed(desktopRootScrollPolicyRefresh, 250L);
                }
            }
            @Override public void onReceivedTitle(WebView webView, String title) {
                if (!home && title != null && !title.trim().isEmpty()) {
                    pageTitle = title.trim();
                    if (mainFrameFinished && allowed(Uri.parse(url))) {
                        preferences.recordHistory(identity.origin(), url, pageTitle, System.currentTimeMillis());
                    }
                }
                changed();
            }
            @Override public boolean onConsoleMessage(ConsoleMessage message) {
                recordDiagnosticEvent("console", "CONSOLE", message == null ? "" : message.message(),
                    message == null ? "" : message.sourceId() + ":" + message.lineNumber(),
                    "", false, "");
                return true;
            }
            @Override public boolean onShowFileChooser(WebView webView, ValueCallback<Uri[]> callback,
                FileChooserParams parameters) {
                if (!attached || closed || listener == null) { callback.onReceiveValue(null); return true; }
                listener.chooseFiles(callback, parameters);
                return true;
            }
        });
        view.setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView webView, WebResourceRequest request) {
                Uri target = request.getUrl();
                if (!allowed(target)) {
                    if (request.isForMainFrame()) { error = "仅支持 HTTP 和 HTTPS 网页"; changed(); }
                    return true;
                }
                if (request.isForMainFrame()) networkPageOrigin = webOrigin(target.toString());
                return false;
            }
            @Override public WebResourceResponse shouldInterceptRequest(WebView webView, WebResourceRequest request) {
                // No page can impersonate the dedicated proxy's authentication host.
                Uri resource = request.getUrl();
                boolean network = "http".equalsIgnoreCase(resource.getScheme()) || "https".equalsIgnoreCase(resource.getScheme());
                if (network) {
                    recordNetworkRequest(request.getMethod(), resource.toString(),
                        request.isForMainFrame(), request.isRedirect() ? "redirect" : "",
                        sanitizedHeaders(request.getRequestHeaders()));
                }
                if (network && !allowed(resource)) return new WebResourceResponse("text/plain", "UTF-8",
                    403, "Forbidden", java.util.Map.of(),
                    new java.io.ByteArrayInputStream(new byte[0]));
                return null;
            }
            @Override public void onPageStarted(WebView webView, String address, Bitmap favicon) {
                if (home) {
                    webView.setVisibility(View.INVISIBLE);
                    return;
                }
                if (!mainFrameStarted && !sameAddress(address, pendingNavigationUrl)) return;
                mainFrameStarted = true;
                mainFrameFinished = false;
                pendingNavigationUrl = address;
                ++pageGeneration;
                credentialPreviousPageOrigin = credentialPageOrigin;
                credentialPreviousPageGeneration = credentialPageGeneration;
                credentialPageOrigin = webOrigin(address);
                credentialPageGeneration = pageGeneration;
                webView.evaluateJavascript(
                    "window.__codexElvesCredentialPageGeneration=" + pageGeneration + ";", null);
                networkPageOrigin = webOrigin(address);
                for (DiagnosticEvent event : networkEventsByRequestId.values()) {
                    if (event.generationPending && networkPageOrigin.equals(webOrigin(event.url))) {
                        event.generation = pageGeneration;
                        event.generationPending = false;
                    }
                }
                webView.evaluateJavascript(
                    "window.__codexElvesNetworkPageGeneration=" + pageGeneration + ";", null);
                installNetworkCollector(webView);
                recordNetworkRequest("GET", address, true, "", "");
                recordDiagnosticEvent("page-start", "GET", address, "", "", true, "");
                diagnostic("native.browser.page", DiagnosticsPolicy.urlFields(address,
                    "phase", "start", "generation", pageGeneration));
                webView.setVisibility(View.VISIBLE);
                url = address; error = ""; progress = 0; pageTitle = ""; changed();
            }
            @Override public void onPageFinished(WebView webView, String address) {
                if (home || !mainFrameStarted) return;
                mainFrameStarted = true;
                pendingNavigationUrl = address;
                if (BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    webView.scrollTo(0, webView.getScrollY());
                    MAIN.postDelayed(desktopRootScrollPolicyRefresh, 120L);
                }
                webView.setVisibility(View.VISIBLE);
                completeNetworkRequest("GET", address, 200, "200", "document", 0L, true);
                collectPerformanceSnapshot(webView);
                installCredentialCollector(webView);
                installNetworkCollector(webView);
                MAIN.postDelayed(() -> {
                    if (!closed && mainFrameFinished) {
                        collectPerformanceSnapshot(view);
                        finalizeNetworkEvents();
                    }
                }, 350L);
                recordDiagnosticEvent("page-finish", "GET", address, "", "page-loaded", true, "");
                diagnostic("native.browser.page", DiagnosticsPolicy.urlFields(address,
                    "phase", "complete", "generation", pageGeneration));
                url = address;
                if (!home && allowed(Uri.parse(address))) {
                    String title = pageTitle.isEmpty() ? webView.getTitle() : pageTitle;
                    preferences.recordHistory(identity.origin(), address, title, System.currentTimeMillis());
                }
                mainFrameFinished = true;
                changed();
                if (!networkDocumentStartAvailable && !networkFallbackReloaded) {
                    networkFallbackReloaded = true;
                    reload();
                }
            }
            @Override public void onScaleChanged(WebView webView, float oldScale, float newScale) {
                if (BrowserPreferences.UA_DESKTOP.equals(userAgentMode)) {
                    // The page height can cross the viewport boundary while the user
                    // pinches. Recompute the short-page policy instead of retaining
                    // the state from the initial page scale.
                    MAIN.removeCallbacks(desktopRootScrollPolicyRefresh);
                    MAIN.post(desktopRootScrollPolicyRefresh);
                }
            }
            @Override public void onReceivedError(WebView webView, WebResourceRequest request, WebResourceError failure) {
                if (home || !allowed(request.getUrl())) return;
                String target = request.getUrl().toString();
                boolean mainFrame = request.isForMainFrame();
                if (mainFrame && !mainFrameStarted) return;
                int code = failure.getErrorCode();
                String detail = failure.getDescription() == null
                    ? "load-failed" : failure.getDescription().toString();
                if (!mainFrame && ignorableResourceFailure(request.getUrl(), detail)) {
                    finishNetworkRequest(request.getMethod(), target, 0, "resource-cancelled",
                        detail, false, inferResourceType(request.getUrl(), false), false);
                    return;
                }
                finishNetworkRequest(request.getMethod(), target, 0, "失败", detail,
                    mainFrame, inferResourceType(request.getUrl(), mainFrame), true);
                recordDiagnosticEvent("failure", request.getMethod(), target, "", detail,
                    mainFrame, "");
                if (mainFrame) {
                    webView.setVisibility(View.VISIBLE);
                    diagnostic("native.browser.error", DiagnosticsPolicy.urlFields(
                        target, "phase", "failure",
                        "errorCode", code, "errorType",
                        code == WebViewClient.ERROR_TIMEOUT ? "timeout" : "network",
                        "reason", code == WebViewClient.ERROR_TIMEOUT ? "timeout" : "load-failed",
                        "generation", pageGeneration));
                    error = "网页加载失败，请检查地址或网络后重试"; changed();
                }
            }
            @Override public void onReceivedHttpError(WebView webView, WebResourceRequest request, WebResourceResponse response) {
                if (home || !allowed(request.getUrl())) return;
                String target = request.getUrl().toString();
                boolean mainFrame = request.isForMainFrame();
                if (mainFrame && !mainFrameStarted) return;
                int statusCode = response.getStatusCode();
                finishNetworkRequest(request.getMethod(), target, statusCode,
                    Integer.toString(statusCode), response.getReasonPhrase(), mainFrame,
                    inferResourceType(request.getUrl(), mainFrame), statusCode >= 400);
                recordDiagnosticEvent("http-error", request.getMethod(), target,
                    Integer.toString(statusCode), response.getReasonPhrase(), mainFrame,
                    sanitizedHeaders(response.getResponseHeaders()));
                if (mainFrame) {
                    diagnostic("native.browser.http", DiagnosticsPolicy.urlFields(
                        target, "phase", "response",
                        "status", statusCode,
                        "mime", DiagnosticsPolicy.mime(response.getMimeType()),
                        "reason", "http-error", "generation", pageGeneration));
                    if (statusCode >= 500) {
                        error = "网页请求失败（" + statusCode + "），请稍后重试"; changed();
                    }
                }
            }
            @Override public void onReceivedSslError(WebView webView, SslErrorHandler handler,
                android.net.http.SslError failure) {
                handler.cancel();
                boolean mainFrame = mainFrameStarted;
                finishNetworkRequest("GET", failure.getUrl(), 0, "SSL",
                    "SSL " + failure.getPrimaryError(), mainFrame,
                    inferResourceType(Uri.parse(failure.getUrl()), mainFrame), true);
                recordDiagnosticEvent("ssl-error", "GET", failure.getUrl(), "",
                    "SSL " + failure.getPrimaryError(), mainFrame, "");
                diagnostic("native.browser.ssl", DiagnosticsPolicy.urlFields(
                    failure.getUrl(), "phase", "failure", "errorType", "ssl",
                    "errorCode", failure.getPrimaryError(), "reason", "ssl-failed",
                    "generation", pageGeneration));
                // Only onReceivedError's isForMainFrame may replace the page with an error.
            }
            @Override public void onReceivedHttpAuthRequest(WebView webView, HttpAuthHandler handler, String host, String realm) {
                if (attached && !closed && proxy.matchesChallenge(host, realm))
                    handler.proceed(BrowserProxy.USER, proxy.password());
                else if (attached && listener != null && !BrowserRoutePolicy.reservedProxyHost(host))
                    listener.requestAuthentication(host, handler);
                else handler.cancel();
            }
            @Override public boolean onRenderProcessGone(WebView webView, RenderProcessGoneDetail detail) {
                recordDiagnosticEvent("failure", "GET", url, "",
                    detail.didCrash() ? "renderer-crashed" : "renderer-killed", true, "");
                diagnostic("native.browser.renderer", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", "renderer",
                    "reason", detail.didCrash() ? "renderer-crashed" : "renderer-killed"));
                invalidate("浏览器页面已退出，请返回会话后重新打开");
                return true;
            }
        });
        initializationStage.accept("authority");
        AutoCloseable registration;
        try { registration = BrowserAuthority.observe(authority, () -> MAIN.post(() -> invalidate("浏览器授权已失效"))); }
        catch (Exception unavailable) { proxy.close(); view.destroy(); throw unavailable; }
        observer = registration;
    }

    private static boolean allowed(Uri target) {
        if (!"http".equalsIgnoreCase(target.getScheme()) && !"https".equalsIgnoreCase(target.getScheme())) return false;
        try { return !BrowserRoutePolicy.reservedProxyHost(target.getHost()); }
        catch (IllegalArgumentException invalid) { return false; }
    }

    private static boolean sameAddress(String first, String second) {
        if (first == null || second == null || first.isEmpty() || second.isEmpty()) return false;
        if (first.equals(second)) return true;
        Uri left = Uri.parse(first).normalizeScheme();
        Uri right = Uri.parse(second).normalizeScheme();
        String leftPath = left.getPath() == null || left.getPath().isEmpty() ? "/" : left.getPath();
        String rightPath = right.getPath() == null || right.getPath().isEmpty() ? "/" : right.getPath();
        return left.getScheme() != null && left.getScheme().equals(right.getScheme())
            && left.getHost() != null && left.getHost().equalsIgnoreCase(right.getHost())
            && left.getPort() == right.getPort()
            && leftPath.equals(rightPath)
            && java.util.Objects.equals(left.getQuery(), right.getQuery())
            && java.util.Objects.equals(left.getFragment(), right.getFragment());
    }

    private static boolean sameDocument(String loaded, String target) {
        return loaded != null && !loaded.isEmpty()
            && sameAddress(withoutFragment(loaded), withoutFragment(target));
    }

    private static String withoutFragment(String address) {
        int fragment = address.indexOf('#');
        return fragment < 0 ? address : address.substring(0, fragment);
    }

    void attach(Activity activity, Listener listener, Runnable ready) {
        if (closed) { listener.invalidated("浏览器已关闭"); return; }
        this.listener = listener;
        context.setBaseContext(activity);
        syncIdentityCookie(() -> applyRules(() -> {
                if (closed || this.listener != listener || activity.isFinishing()) return;
                attached = true;
                proxy.setActive(!home);
                if (!home) { view.onResume(); view.resumeTimers(); }
                ready.run(); changed();
            }, () -> {
                if (!closed && this.listener == listener) {
                    listener.invalidated("网络设置应用失败，请返回会话后重试");
                }
            }), () -> {
                if (!closed && this.listener == listener) {
                    listener.invalidated("设备会话 Cookie 写入失败，请返回会话后重试");
                }
            });
    }

    void detach(Listener owner) {
        if (listener != owner && context.getBaseContext() != owner) return;
        listener = null; attached = false; ++proxyGeneration;
        flushCookies();
        view.onPause(); view.pauseTimers();
        proxy.setActive(false);
        if (view.getParent() instanceof ViewGroup parent) parent.removeView(view);
        context.setBaseContext(context.getApplicationContext());
    }

    @SuppressLint("RequiresFeature") // prepare() checks proxy support before constructing a session.
    private void applyRules(Runnable complete, Runnable failure) {
        long generation = ++proxyGeneration;
        routeReady = false;
        proxy.setActive(false);
        ProxyConfig.Builder configuration = new ProxyConfig.Builder()
            .addProxyRule("http://" + proxy.host() + ":" + proxy.port())
            .removeImplicitRules();
        diagnostic("native.browser.route", DiagnosticsPolicy.fields(
            "phase", "request", "routeName", "proxy-all",
            "host", proxy.host(), "count", 0, "port", proxy.port()));
        try {
            ProxyController.getInstance().setProxyOverride(configuration.build(), MAIN::post, () -> {
                if (!closed && generation == proxyGeneration) {
                    routeReady = true;
                    proxy.setActive(attached && !home);
                    diagnostic("native.browser.route", DiagnosticsPolicy.fields(
                        "phase", "success", "routeName", "proxy-all",
                        "host", proxy.host(), "count", 0, "port", proxy.port()));
                    complete.run();
                }
            });
        } catch (RuntimeException exception) {
            diagnostic("native.browser.route", DiagnosticsPolicy.fields(
                "phase", "failure", "routeName", "proxy-all",
                "errorType", DiagnosticsPolicy.errorType(exception), "reason", "failed"));
            if (!closed && generation == proxyGeneration && failure != null) {
                failure.run();
            }
        }
    }

    private void syncIdentityCookie(Runnable complete, Runnable failure) {
        CookieManager cookies = CookieManager.getInstance();
        try {
            cookies.setAcceptCookie(true);
            cookies.setAcceptThirdPartyCookies(view, false);
            String cookie = identity.cookie()
                + "; Path=/; HttpOnly; SameSite=Strict"
                + (identity.origin().startsWith("https://") ? "; Secure" : "");
            cookies.setCookie(identity.origin(), cookie, accepted -> {
                if (closed) return;
                if (!Boolean.TRUE.equals(accepted)) {
                    diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                        "phase", "failure", "reason", "write-rejected"));
                    failure.run();
                    return;
                }
                cookies.flush();
                if (!containsCookie(cookies.getCookie(identity.origin()), identity.cookie())) {
                    diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                        "phase", "failure", "reason", "readback-missing"));
                    failure.run();
                    return;
                }
                diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                    "phase", "success", "reason", "synced"));
                complete.run();
            });
        } catch (RuntimeException unavailable) {
            diagnostic("native.browser.cookies", DiagnosticsPolicy.fields(
                "phase", "failure", "reason", "unsupported",
                "errorType", DiagnosticsPolicy.errorType(unavailable)));
            failure.run();
        }
    }

    private static boolean containsCookie(String header, String expected) {
        if (header == null || expected == null || expected.isEmpty()) return false;
        for (String value : header.split(";")) {
            if (expected.equals(value.trim())) return true;
        }
        return false;
    }

    long pageGeneration() { return pageGeneration; }
    String diagnosticPageUrl() { return redactDiagnosticUrl(url); }
    List<BrowserPreferences.HistoryEntry> history() {
        return preferences.loadHistory(identity.origin());
    }
    List<BrowserPreferences.HistoryEntry> recentHistory(int limit) {
        return preferences.loadHistory(identity.origin(), limit);
    }
    List<BrowserPreferences.BookmarkEntry> bookmarks() {
        return preferences.loadBookmarks(identity.origin());
    }
    List<DiagnosticEvent> diagnosticEvents() {
        synchronized (diagnosticEvents) {
            List<DiagnosticEvent> snapshot = new ArrayList<>();
            for (DiagnosticEvent event : diagnosticEvents) snapshot.add(event.copy());
            return snapshot;
        }
    }
    void clearDiagnosticEvents() {
        MAIN.removeCallbacks(diagnosticRefresh);
        synchronized (diagnosticEvents) {
            diagnosticEvents.clear();
            latestNetworkEvents.clear();
            networkEventsByRequestId.clear();
            pendingNetworkEvents.clear();
            performanceResourceKeys.clear();
        }
        changed();
    }
    boolean bookmarked() {
        return !home && preferences.isBookmarked(identity.origin(), url);
    }
    String userAgentMode() { return userAgentMode; }
    boolean credentialRecordingEnabled() { return credentialRecordingEnabled; }
    void setCredentialRecordingEnabled(boolean enabled) {
        if (closed) return;
        credentialStore.setRecordingEnabled(identity.owner(), enabled);
        credentialRecordingEnabled = enabled;
        credentialError = "";
        if (enabled) {
            installCredentialCollector(view);
        } else view.evaluateJavascript(
            "window.__codexElvesSetCredentialRecords&&window.__codexElvesSetCredentialRecords([]);", null);
        changed();
    }
    List<BrowserCredentialStore.Entry> credentials() {
        return credentialStore.list(identity.owner());
    }
    String credentialError() { return credentialError; }
    void deleteCredential(String id) {
        credentialStore.delete(identity.owner(), id); pushCredentialRecords(view); changed();
    }
    void clearCredentials() {
        credentialStore.clear(identity.owner()); pushCredentialRecords(view); changed();
    }
    void recordHttpCredentials(String username, String password) {
        if (!credentialRecordingEnabled) return;
        String origin = webOrigin(view.getUrl());
        try {
            if (origin.isEmpty()) {
                credentialFailure("页面地址已变化，请返回登录页后重试", "origin-mismatch", null); return;
            }
            if (!credentialStore.record(identity.owner(), origin, username, password, "")) {
                credentialFailure("未识别到账号或密码，请使用账号密码登录后重试", "missing-fields", null); return;
            }
            credentialError = "";
            diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
                "phase", "success", "reason", "http-auth", "generation", credentialPageGeneration));
            changed();
        } catch (RuntimeException failure) {
            credentialFailure("账号密码保存失败，请检查系统密钥状态后重试", "failed", failure);
        }
    }
    void toggleBookmark() {
        if (closed || home || !allowed(Uri.parse(url))) return;
        preferences.toggleBookmark(identity.origin(), url, view.getTitle(), System.currentTimeMillis());
        changed();
    }
    void removeBookmark(String target) {
        if (closed || target == null || target.isEmpty()) return;
        preferences.removeBookmark(identity.origin(), target);
        changed();
    }
    void clearHistory() {
        preferences.clearHistory(identity.origin());
        changed();
    }
    void removeHistory(String target) {
        if (closed || target == null || target.isEmpty()) return;
        preferences.removeHistory(identity.origin(), target);
        changed();
    }
    void clearBookmarks() {
        preferences.clearBookmarks(identity.origin());
        changed();
    }
    void saveUserAgentMode(String mode, Runnable complete) {
        if (!BrowserPreferences.UA_ANDROID.equals(mode)
            && !BrowserPreferences.UA_DESKTOP.equals(mode)
            && !BrowserPreferences.UA_IPHONE.equals(mode)) {
            throw new IllegalArgumentException("网站显示方式无效");
        }
        if (closed || userAgentMode.equals(mode)) {
            if (complete != null) complete.run();
            return;
        }
        userAgentMode = mode;
        preferences.saveUserAgentMode(identity.origin(), mode);
        applyDisplayMode(view.getSettings());
        if (!home && attached) {
            reload();
        }
        changed();
        if (complete != null) complete.run();
    }
    boolean canDeliverFiles(long generation) {
        return !closed && attached && !home && generation == pageGeneration;
    }
    void navigate(String input) {
        if (closed || !attached) return;
        if (!routeReady) throw new IllegalArgumentException("网络通道正在更新，请稍后再试");
        String target;
        if (BrowserRoutePolicy.isWorkspaceViewUrl(input)) {
            localFileThreadId = BrowserRoutePolicy.workspaceViewThread(input);
            localFileOrigin = identity.origin();
            target = input.trim();
        } else if (BrowserRoutePolicy.isLocalFileInput(input)) {
            if (localFileThreadId.isEmpty() || localFileOrigin.isEmpty())
                throw new IllegalArgumentException("本地文件请从远程会话的文件链接打开");
            target = BrowserRoutePolicy.workspaceViewUrl(localFileOrigin, localFileThreadId, input);
        } else {
            target = BrowserRoutePolicy.navigationUrl(input);
        }
        diagnostic("native.browser.navigation", DiagnosticsPolicy.urlFields(target,
            "phase", "request", "routeName", "computer"));
        if (home) view.clearHistory();
        boolean sameDocumentLoad = sameDocument(view.getUrl(), target);
        view.stopLoading();
        if (sameDocumentLoad) {
            // Fragment and same-URL loads fire no page callbacks, so the loaded document must stay visible.
            view.setVisibility(View.VISIBLE);
            mainFrameStarted = true;
            mainFrameFinished = true;
            progress = 100;
        } else {
            view.setVisibility(View.INVISIBLE);
            mainFrameStarted = false;
            mainFrameFinished = false;
            pageTitle = "";
            progress = 0;
        }
        pendingNavigationUrl = target;
        home = false; error = ""; url = target; proxy.setActive(true); ++pageGeneration;
        networkPageOrigin = webOrigin(target);
        view.evaluateJavascript(
            "window.__codexElvesNetworkPageGeneration=" + pageGeneration + ";", null);
        view.onResume(); view.resumeTimers(); view.loadUrl(target); changed();
    }
    void showHome() {
        home = true;
        error = "";
        url = "";
        pendingNavigationUrl = "";
        mainFrameStarted = false;
        mainFrameFinished = false;
        pageTitle = "";
        progress = 100;
        view.stopLoading();
        view.clearHistory();
        view.setVisibility(View.INVISIBLE);
        proxy.setActive(false);
        view.onPause();
        view.pauseTimers();
        changed();
    }
    boolean back() {
        if (!home && view.canGoBack()) {
            error = "";
            android.webkit.WebBackForwardList history = view.copyBackForwardList();
            int previousIndex = history.getCurrentIndex() - 1;
            pendingNavigationUrl = previousIndex >= 0
                ? history.getItemAtIndex(previousIndex).getUrl() : "";
            view.goBack();
            return true;
        }
        if (!home) { showHome(); return true; }
        return false;
    }
    void reload() {
        if (!closed && attached && !home) {
            error = "";
            String currentUrl = view.getUrl();
            pendingNavigationUrl = currentUrl == null || currentUrl.isEmpty() ? url : currentUrl;
            mainFrameStarted = false;
            mainFrameFinished = false;
            pageTitle = "";
            progress = 0;
            view.setVisibility(View.INVISIBLE);
            view.reload();
            changed();
        }
    }

    private void recordDiagnosticEvent(String phase, String method, String url, String status,
        String detail, boolean mainFrame, String headers) {
        String safeUrl = redactDiagnosticUrl(url);
        String safeDetail = detail == null ? "" : detail.length() > 1024 ? detail.substring(0, 1024) : detail;
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
                phase == null ? "" : phase, method == null ? "" : method,
                safeUrl, status == null ? "" : status, safeDetail,
                headers == null ? "" : headers, mainFrame, "", 0L, 0L,
                parseStatusCode(status), phase != null && "http-error".equals(phase),
                phase != null && ("failure".equals(phase) || "ssl-error".equals(phase)));
            diagnosticEvents.addLast(event);
            trimDiagnosticEventsLocked();
        }
        MAIN.removeCallbacks(diagnosticRefresh);
        MAIN.postDelayed(diagnosticRefresh, 120L);
    }

    private DiagnosticEvent createNetworkEventLocked(String method, String safeUrl, boolean mainFrame,
        String resourceType, String requestId, long generation) {
        DiagnosticEvent event = new DiagnosticEvent(++nextDiagnosticId, System.currentTimeMillis(),
            "request", method == null ? "GET" : method, safeUrl, "", "", "",
            mainFrame, resourceType == null ? "" : resourceType, 0L, 0L, 0, false, false);
        event.requestId = requestId == null ? "" : requestId;
        event.generation = generation;
        diagnosticEvents.addLast(event);
        String key = networkKey(method, safeUrl);
        latestNetworkEvents.put(key, event);
        pendingNetworkEvents.computeIfAbsent(key, ignored -> new ArrayDeque<>()).addLast(event);
        if (!event.requestId.isEmpty()) networkEventsByRequestId.put(event.requestId, event);
        return event;
    }

    private DiagnosticEvent pendingNativeStartLocked(String key) {
        ArrayDeque<DiagnosticEvent> queue = pendingNetworkEvents.get(key);
        if (queue == null) return null;
        for (DiagnosticEvent event : queue) if (!event.nativeObserved) return event;
        return null;
    }

    private DiagnosticEvent pendingScriptEventLocked(String key) {
        ArrayDeque<DiagnosticEvent> queue = pendingNetworkEvents.get(key);
        if (queue == null) return null;
        for (DiagnosticEvent event : queue) if (event.requestId.isEmpty() && !event.response) return event;
        return null;
    }

    private DiagnosticEvent pendingNetworkEventLocked(String key) {
        ArrayDeque<DiagnosticEvent> queue = pendingNetworkEvents.get(key);
        return queue == null ? null : queue.peekFirst();
    }

    private void removePendingNetworkEventLocked(DiagnosticEvent event) {
        if (event == null) return;
        String key = networkKey(event.method, event.url);
        ArrayDeque<DiagnosticEvent> queue = pendingNetworkEvents.get(key);
        if (queue == null) return;
        queue.remove(event);
        if (queue.isEmpty()) pendingNetworkEvents.remove(key);
    }

    private static boolean scriptResourceType(String value) {
        return "xhr".equals(value) || "fetch".equals(value) || "xmlhttprequest".equals(value);
    }

    private static String mergeResourceType(String current, String next) {
        if (scriptResourceType(current)) return current;
        return next == null || next.isEmpty() ? current : next;
    }

    private void recordNetworkRequest(String method, String url, boolean mainFrame,
        String detail, String headers) {
        String safeUrl = redactDiagnosticUrl(url);
        String key = networkKey(method, safeUrl);
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = pendingNativeStartLocked(key);
            if (event == null) event = createNetworkEventLocked(method, safeUrl, mainFrame,
                inferResourceType(Uri.parse(url), mainFrame), "", 0L);
            event.nativeObserved = true;
            if (detail != null && !detail.isEmpty()) event.detail = detail;
            if (headers != null && !headers.isEmpty()) event.headers = headers;
            event.mainFrame = event.mainFrame || mainFrame;
            event.resourceType = mergeResourceType(event.resourceType, inferResourceType(Uri.parse(url), mainFrame));
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void finishNetworkRequest(String method, String url, int statusCode, String status,
        String detail, boolean mainFrame, String resourceType, boolean failure) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            String key = networkKey(method, safeUrl);
            DiagnosticEvent event = pendingNetworkEventLocked(key);
            if (event == null) event = latestNetworkEvents.get(key);
            if (event == null) event = createNetworkEventLocked(method, safeUrl, mainFrame, resourceType, "", 0L);
            event.status = status == null ? "" : status;
            event.detail = detail == null ? "" : detail;
            event.mainFrame = mainFrame;
            event.nativeObserved = true;
            event.resourceType = mergeResourceType(event.resourceType, resourceType);
            event.statusCode = statusCode;
            event.response = !failure && (statusCode > 0 || "200".equals(status) || "完成".equals(status));
            event.failure = failure;
            event.durationMs = Math.max(0L, System.currentTimeMillis() - event.timeMs);
            removePendingNetworkEventLocked(event);
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void completeNetworkRequest(String method, String url, int statusCode, String status,
        String resourceType, long sizeBytes, boolean response) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            String key = networkKey(method, safeUrl);
            DiagnosticEvent event = pendingNetworkEventLocked(key);
            if (event == null) event = latestNetworkEvents.get(key);
            if (event == null) event = createNetworkEventLocked(method, safeUrl, true,
                resourceType == null ? "document" : resourceType, "", 0L);
            event.status = status == null ? "" : status;
            event.statusCode = statusCode;
            event.resourceType = mergeResourceType(event.resourceType, resourceType);
            event.sizeBytes = Math.max(0L, sizeBytes);
            event.response = response;
            event.durationMs = Math.max(event.durationMs, System.currentTimeMillis() - event.timeMs);
            removePendingNetworkEventLocked(event);
            trimDiagnosticEventsLocked();
        }
        scheduleDiagnosticRefresh();
    }

    private void collectPerformanceSnapshot(WebView webView) {
        if (Build.VERSION.SDK_INT < 19 || webView == null) return;
        String script = "(function(){try{var n=performance.getEntriesByType('navigation')[0];"
            + "var r=performance.getEntriesByType('resource').map(function(e){return{"
            + "name:e.name,initiatorType:e.initiatorType,duration:e.duration,"
            + "transferSize:e.transferSize||0,encodedBodySize:e.encodedBodySize||0,"
            + "startTime:e.startTime};});return JSON.stringify({navigation:n?{name:location.href,"
            + "duration:n.duration,transferSize:n.transferSize||0}:null,resources:r});}"
            + "catch(e){return JSON.stringify({resources:[]});}})()";
        webView.evaluateJavascript(script, this::applyPerformanceSnapshot);
    }

    private void applyPerformanceSnapshot(String encoded) {
        if (encoded == null || encoded.isEmpty()) return;
        try {
            Object value = new JSONTokener(encoded).nextValue();
            if (!(value instanceof String)) return;
            JSONObject root = new JSONObject((String) value);
            JSONObject navigation = root.optJSONObject("navigation");
            if (navigation != null) {
                updatePerformanceEvent(navigation.optString("name", url), "document",
                    navigation.optDouble("duration", 0d),
                    navigation.optLong("transferSize", 0L));
            }
            JSONArray resources = root.optJSONArray("resources");
            if (resources == null) return;
            for (int index = 0; index < resources.length(); index++) {
                JSONObject resource = resources.optJSONObject(index);
                if (resource == null) continue;
                String target = resource.optString("name", "");
                if (target.isEmpty()) continue;
                String key = redactDiagnosticUrl(target) + "|" + resource.optDouble("startTime", 0d);
                synchronized (diagnosticEvents) {
                    if (!performanceResourceKeys.add(key)) continue;
                }
                updatePerformanceEvent(target, resource.optString("initiatorType", ""),
                    resource.optDouble("duration", 0d),
                    Math.max(resource.optLong("transferSize", 0L),
                        resource.optLong("encodedBodySize", 0L)));
            }
        } catch (JSONException ignored) {
            // Performance entries are an optional enhancement and must not affect loading.
        } catch (RuntimeException ignored) {
            // Performance entries are an optional enhancement and must not affect loading.
        }
    }

    private void updatePerformanceEvent(String url, String resourceType, double duration,
        long sizeBytes) {
        String safeUrl = redactDiagnosticUrl(url);
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = latestNetworkEvents.get(networkKey("GET", safeUrl));
            if (event == null) {
                for (DiagnosticEvent candidate : diagnosticEvents) {
                    if ("request".equals(candidate.phase) && safeUrl.equals(candidate.url)) {
                        event = candidate;
                    }
                }
            }
            if (event == null) return;
            event.resourceType = mergeResourceType(event.resourceType,
                normalizeResourceType(resourceType, event.resourceType));
            event.durationMs = Math.max(event.durationMs, Math.round(Math.max(0d, duration)));
            event.sizeBytes = Math.max(event.sizeBytes, Math.max(0L, sizeBytes));
            if (!event.failure && event.statusCode == 0) {
                event.statusCode = 200;
                event.status = "200";
            }
            event.response = true;
            removePendingNetworkEventLocked(event);
        }
        scheduleDiagnosticRefresh();
    }

    private void finalizeNetworkEvents() {
        long now = System.currentTimeMillis();
        synchronized (diagnosticEvents) {
            for (DiagnosticEvent event : diagnosticEvents) {
                if (!"request".equals(event.phase) || event.failure || event.response
                    || "resource-cancelled".equals(event.status) || !event.requestId.isEmpty()
                    || scriptResourceType(event.resourceType)) continue;
                event.statusCode = 200;
                event.status = "200";
                event.response = true;
                event.durationMs = Math.max(1L, now - event.timeMs);
                removePendingNetworkEventLocked(event);
            }
        }
        scheduleDiagnosticRefresh();
    }

    private static String networkKey(String method, String url) {
        String normalized = url == null ? "" : url;
        int fragment = normalized.indexOf('#');
        if (fragment >= 0) normalized = normalized.substring(0, fragment);
        return (method == null ? "GET" : method.toUpperCase(Locale.ROOT)) + "|" + normalized;
    }

    private static int parseStatusCode(String status) {
        if (status == null || status.isEmpty()) return 0;
        try { return Integer.parseInt(status); }
        catch (NumberFormatException ignored) { return 0; }
    }

    private void trimDiagnosticEventsLocked() {
        while (diagnosticEvents.size() > DIAGNOSTIC_EVENT_LIMIT) {
            DiagnosticEvent removed = diagnosticEvents.removeFirst();
            if (removed != null) {
                latestNetworkEvents.values().removeIf(value -> value == removed);
                if (!removed.requestId.isEmpty()) networkEventsByRequestId.remove(removed.requestId, removed);
                removePendingNetworkEventLocked(removed);
            }
        }
    }

    private void scheduleDiagnosticRefresh() {
        MAIN.removeCallbacks(diagnosticRefresh);
        MAIN.postDelayed(diagnosticRefresh, 120L);
    }

    private static String normalizeResourceType(String value, String fallback) {
        if (value == null || value.isEmpty()) return fallback == null ? "other" : fallback;
        if ("script".equalsIgnoreCase(value)) return "script";
        if ("css".equalsIgnoreCase(value) || "stylesheet".equalsIgnoreCase(value)) return "stylesheets";
        if ("img".equalsIgnoreCase(value) || "image".equalsIgnoreCase(value)) return "image";
        if ("font".equalsIgnoreCase(value)) return "font";
        if ("xmlhttprequest".equalsIgnoreCase(value) || "fetch".equalsIgnoreCase(value)) return "xhr";
        if ("document".equalsIgnoreCase(value) || "navigation".equalsIgnoreCase(value)) return "document";
        return value;
    }

    private static String inferResourceType(Uri target, boolean mainFrame) {
        if (mainFrame) return "document";
        if (target == null) return "other";
        String path = target.getPath() == null ? "" : target.getPath().toLowerCase(Locale.ROOT);
        if (path.endsWith(".js") || path.endsWith(".mjs")) return "script";
        if (path.endsWith(".css")) return "stylesheets";
        if (path.matches(".*\\.(png|jpe?g|gif|webp|svg|ico|avif)$")) return "image";
        if (path.matches(".*\\.(mp4|webm|mov|m3u8|mpd|mp3|wav|ogg|m4a|aac|flac)$")) return "media";
        if (path.matches(".*\\.(woff2?|ttf|otf|eot)$")) return "font";
        if (path.contains("/api/") || path.contains("/graphql") || path.contains("/ajax/")
            || "POST".equalsIgnoreCase(target.getQueryParameter("method"))) return "xhr";
        return "other";
    }

    private static String sanitizedHeaders(java.util.Map<String, String> values) {
        if (values == null || values.isEmpty()) return "";
        StringBuilder result = new StringBuilder();
        for (java.util.Map.Entry<String, String> entry : values.entrySet()) {
            String name = entry.getKey() == null ? "" : entry.getKey().trim();
            if (isSensitiveHeader(name)) continue;
            String value = entry.getValue() == null ? "" : entry.getValue().trim();
            if (value.length() > 512) value = value.substring(0, 512);
            if (result.length() > 0) result.append('\n');
            result.append(name).append(": ").append(value);
            if (result.length() > 4096) break;
        }
        return result.toString();
    }
    private static boolean isSensitiveHeader(String name) {
        String normalized = name == null ? "" : name.trim().toLowerCase(java.util.Locale.ROOT);
        return normalized.isEmpty() || "cookie".equals(normalized)
            || "authorization".equals(normalized) || "proxy-authorization".equals(normalized)
            || "set-cookie".equals(normalized) || "x-api-key".equals(normalized)
            || "x-auth-token".equals(normalized) || "x-access-token".equals(normalized)
            || "x-csrf-token".equals(normalized) || normalized.contains("password")
            || normalized.contains("secret");
    }
    private static String redactDiagnosticUrl(String value) {
        if (value == null || value.isEmpty()) return "";
        String result = value;
        String[] names = {
            "access_token", "accessToken", "api_key", "apiKey", "auth", "authorization",
            "code", "key", "password", "secret", "session", "token"
        };
        for (String name : names) {
            result = result.replaceAll(
                "(?i)([?#&]" + java.util.regex.Pattern.quote(name) + "=)[^&#]*",
                "$1<redacted>");
        }
        return result.length() > 4096 ? result.substring(0, 4096) : result;
    }
    private static boolean ignorableResourceFailure(Uri target, String detail) {
        String path = target == null ? "" : target.getPath();
        String message = detail == null ? "" : detail.toLowerCase(Locale.ROOT);
        return path != null && path.toLowerCase(Locale.ROOT).endsWith("/favicon.ico")
            && (message.contains("err_socket_not_connected")
                || message.contains("err_aborted")
                || message.contains("err_connection_reset")
                || message.contains("err_failed"));
    }
    private void applyDisplayMode(WebSettings settings) {
        boolean desktop = BrowserPreferences.UA_DESKTOP.equals(userAgentMode);
        desktopRootScrollLocked = false;
        String userAgent = BrowserPreferences.UA_DESKTOP.equals(userAgentMode)
            ? desktopUserAgent
            : BrowserPreferences.UA_IPHONE.equals(userAgentMode)
            ? iphoneUserAgent : androidUserAgent;
        settings.setUserAgentString(userAgent);
        settings.setUseWideViewPort(true);
        settings.setLoadWithOverviewMode(true);
        settings.setLayoutAlgorithm(desktop
            ? WebSettings.LayoutAlgorithm.NORMAL
            : WebSettings.LayoutAlgorithm.TEXT_AUTOSIZING);
        // Desktop UA must retain pinch zoom as well. The UA changes layout and
        // server-side content selection; it must not remove the browser's zoom
        // gesture or prevent users from fitting more of a page on screen.
        settings.setSupportZoom(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        view.setHorizontalScrollBarEnabled(false);
        view.setOverScrollMode(View.OVER_SCROLL_NEVER);
        view.setNestedScrollingEnabled(false);
        applyUserAgentMetadata(settings);
        applyDesktopViewport(desktop);
        diagnostic("native.browser.display", DiagnosticsPolicy.fields(
            "phase", "success", "reason", "success"));
    }
    private void applyDesktopViewport(boolean desktop) {
        if (!desktop) {
            if (desktopViewportScript != null) desktopViewportScript.remove();
            desktopViewportScript = null;
            return;
        }
        if (desktopViewportScript == null
            && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
            // The override is optional; a rejected script must not stop the browser from opening.
            try {
                desktopViewportScript = WebViewCompat.addDocumentStartJavaScript(
                    view, DESKTOP_VIEWPORT_SCRIPT, Collections.singleton("*"));
            } catch (RuntimeException unavailable) {
                diagnostic("native.browser.display", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                    "reason", "unsupported"));
            }
        }
    }
    private void updateDesktopRootScrollPolicy() {
        if (closed || home || !BrowserPreferences.UA_DESKTOP.equals(userAgentMode)
            || view.getHeight() <= 0) {
            desktopRootScrollLocked = false;
            return;
        }
        int contentHeight = Math.round(view.getContentHeight() * view.getScale());
        desktopRootScrollLocked = contentHeight > 0 && contentHeight <= view.getHeight() + 8;
        if (desktopRootScrollLocked) view.scrollTo(view.getScrollX(), 0);
        String overflowY = desktopRootScrollLocked ? "hidden" : "";
        String overflowX = "auto";
        view.evaluateJavascript(
            "(function(){var d=document.documentElement,b=document.body,oy='" + overflowY
                + "',ox='" + overflowX + "';"
                + "if(d){d.style.overflowY=oy;d.style.overflowX=ox;}"
                + "if(b){b.style.overflowY=oy;b.style.overflowX=ox;}})()",
            null);
    }
    private void applyUserAgentMetadata(WebSettings settings) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.USER_AGENT_METADATA)) return;
        boolean desktop = BrowserPreferences.UA_DESKTOP.equals(userAgentMode);
        boolean iphone = BrowserPreferences.UA_IPHONE.equals(userAgentMode);
        String chrome = chromeVersion(mobileUserAgent);
        UserAgentMetadata.Builder metadata = new UserAgentMetadata.Builder()
            .setMobile(!desktop)
            .setFullVersion(chrome)
            .setPlatform(desktop ? "Windows" : iphone ? "iOS" : "Android")
            .setPlatformVersion(desktop ? "10.0.0"
                : iphone ? "17.0.0" : matchGroup(mobileUserAgent, "Android\\s+([^;)]*)", "13"))
            .setArchitecture(desktop ? "x86" : "arm")
            .setBitness(desktop ? 64 : UserAgentMetadata.BITNESS_DEFAULT)
            .setWow64(false);
        if (WebViewFeature.isFeatureSupported(WebViewFeature.USER_AGENT_METADATA_FORM_FACTORS)) {
            metadata.setFormFactors(List.of(
                desktop ? UserAgentMetadata.FORM_FACTOR_DESKTOP : UserAgentMetadata.FORM_FACTOR_MOBILE));
        }
        WebSettingsCompat.setUserAgentMetadata(settings, metadata.build());
    }
    private static String androidUserAgent(String source) {
        String chrome = chromeVersion(source);
        String android = matchGroup(source, "Android\\s+([^;)]*)", "13");
        return "Mozilla/5.0 (Linux; Android " + android + "; K) "
            + "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/" + chrome
            + " Mobile Safari/537.36";
    }
    private static String desktopUserAgent(String source) {
        String chrome = chromeVersion(source);
        return "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
            + "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/" + chrome
            + " Safari/537.36";
    }
    private static String iphoneUserAgent() {
        return "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) "
            + "AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 "
            + "Mobile/15E148 Safari/604.1";
    }
    private static String chromeVersion(String source) {
        return matchGroup(source, "Chrome/([0-9.]+)", "120.0.0.0");
    }
    private static String matchGroup(String source, String expression, String fallback) {
        if (source == null) return fallback;
        Matcher matcher = Pattern.compile(expression).matcher(source);
        return matcher.find() ? matcher.group(1) : fallback;
    }
    private void installCredentialCollector(WebView webView) {
        if (!credentialRecordingEnabled || webView == null) return;
        if (!credentialMessageSupported) {
            credentialFailure("当前 WebView 不支持安全凭据记录，请更新 Android System WebView", "bridge-unsupported", null);
            return;
        }
        if (credentialCollectorScript == null
            && WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
            try {
                credentialCollectorScript = WebViewCompat.addDocumentStartJavaScript(
                    webView, CREDENTIAL_COLLECTOR_SCRIPT, Collections.singleton("*"));
            } catch (RuntimeException unavailable) {
                diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                    "reason", "unsupported"));
            }
        }
        webView.evaluateJavascript(CREDENTIAL_COLLECTOR_SCRIPT, null);
        pushCredentialRecords(webView);
    }
    private void pushCredentialRecords(WebView webView) {
        if (!credentialRecordingEnabled || webView == null) return;
        String origin = webOrigin(webView.getUrl());
        if (origin.isEmpty()) return;
        try {
            JSONArray records = new JSONArray();
            for (BrowserCredentialStore.Entry entry : credentialStore.forOrigin(identity.owner(), origin)) {
                JSONObject record = new JSONObject();
                record.put("username", entry.username);
                record.put("password", entry.password);
                record.put("subaccount", entry.subaccount);
                records.put(record);
            }
            webView.evaluateJavascript("window.__codexElvesSetCredentialRecords&&"
                + "window.__codexElvesSetCredentialRecords(" + records + ");", null);
        } catch (JSONException | RuntimeException ignored) {
            // Unavailable records never block navigation or the loaded page.
        }
    }
    private void installNetworkCollector(WebView webView) {
        if (webView == null) return;
        if (networkCollectorScript == null && networkDocumentStartAvailable) {
            try {
                networkCollectorScript = WebViewCompat.addDocumentStartJavaScript(
                    webView, NETWORK_COLLECTOR_SCRIPT, Collections.singleton("*"));
            } catch (RuntimeException unavailable) {
                networkDocumentStartAvailable = false;
                diagnostic("native.browser.network", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", DiagnosticsPolicy.errorType(unavailable),
                    "reason", "unsupported"));
            }
        }
        webView.evaluateJavascript(NETWORK_COLLECTOR_SCRIPT, null);
    }
    private static String webOrigin(String value) {
        try {
            Uri target = Uri.parse(value == null ? "" : value);
            String scheme = target.getScheme(), host = target.getHost();
            if (!("http".equalsIgnoreCase(scheme) || "https".equalsIgnoreCase(scheme))
                || host == null || host.isEmpty()) return "";
            int port = target.getPort();
            return scheme.toLowerCase(Locale.ROOT) + "://" + host.toLowerCase(Locale.ROOT)
                + (port > 0 ? ":" + port : "");
        } catch (RuntimeException invalid) { return ""; }
    }
    private boolean currentNetworkMessage(String origin, long generation) {
        return !origin.isEmpty() && origin.equals(networkPageOrigin)
            && (generation == pageGeneration || generation == 0L);
    }
    private void startNetworkFromScript(String requestId, String method, String target, String resourceType,
        boolean generationPending) {
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = networkEventsByRequestId.get(requestId);
            if (event != null && event.generation != pageGeneration && !event.generationPending) {
                diagnostic("native.browser.network", DiagnosticsPolicy.fields(
                    "phase", "failure", "reason", "stale-callback"));
                return;
            }
            String key = networkKey(method, target);
            if (event == null) event = pendingScriptEventLocked(key);
            if (event == null) event = createNetworkEventLocked(method, target, false, resourceType,
                requestId, pageGeneration);
            else if (event.requestId.isEmpty()) {
                event.requestId = requestId;
                event.generation = pageGeneration;
                networkEventsByRequestId.put(requestId, event);
            }
            event.resourceType = resourceType;
            event.generationPending = generationPending;
            latestNetworkEvents.put(key, event);
            trimDiagnosticEventsLocked();
        }
    }
    private void updateNetworkFromScript(String kind, JSONObject value) {
        synchronized (diagnosticEvents) {
            DiagnosticEvent event = networkEventsByRequestId.get(value.optString("requestId", ""));
            if (event == null || event.generation != pageGeneration && !event.generationPending) {
                diagnostic("native.browser.network", DiagnosticsPolicy.fields(
                    "phase", "failure", "reason", "stale-callback"));
                return;
            }
            event.resourceType = mergeResourceType(event.resourceType, value.optString("resourceType", "xhr"));
            if (value.has("requestBody")) event.requestBody = value.optString("requestBody", "");
            if (value.has("responseBody")) event.responseBody = value.optString("responseBody", "");
            String mime = value.optString("mime", "");
            if (!mime.isEmpty()) event.responseMime = mime;
            if ("response".equals(kind)) {
                int statusCode = value.optInt("status", 0);
                boolean failure = value.optBoolean("failure", false);
                event.statusCode = statusCode;
                event.status = statusCode > 0 ? Integer.toString(statusCode) : failure ? "失败" : event.status;
                event.detail = value.optString("detail", event.detail);
                event.response = !failure;
                event.failure = failure;
                event.durationMs = Math.max(event.durationMs, System.currentTimeMillis() - event.timeMs);
                removePendingNetworkEventLocked(event);
            }
            trimDiagnosticEventsLocked();
        }
        if (value.has("requestBody")) diagnostic("native.browser.network", DiagnosticsPolicy.fields(
            "phase", "body", "reason", "request"));
        if (value.has("responseBody")) diagnostic("native.browser.network", DiagnosticsPolicy.fields(
            "phase", "body", "reason", "response"));
    }
    private final class NetworkRecorderBridge {
        @JavascriptInterface public void record(String payload) {
            if (payload == null || payload.length() > 220_000) return;
            MAIN.post(() -> {
                if (closed || !attached) return;
                try {
                    JSONObject value = new JSONObject(payload);
                    String kind = value.optString("kind", "");
                    String requestId = value.optString("requestId", "");
                    if (!requestId.matches("[A-Za-z0-9-]{1,120}")) return;
                    String origin = webOrigin(value.optString("origin", ""));
                    long generation = value.optLong("generation", -1L);
                    if (!currentNetworkMessage(origin, generation)) {
                        diagnostic("native.browser.network", DiagnosticsPolicy.fields(
                            "phase", "failure", "reason", "stale-callback"));
                        return;
                    }
                    String method = value.optString("method", "GET").toUpperCase(Locale.ROOT);
                    String target = redactDiagnosticUrl(value.optString("url", ""));
                    if (target.isEmpty() || !allowed(Uri.parse(target))) return;
                    String resourceType = value.optString("resourceType", "xhr");
                    if ("start".equals(kind)) startNetworkFromScript(requestId, method, target, resourceType,
                        generation == 0L);
                    else if ("requestBody".equals(kind) || "response".equals(kind)) {
                        updateNetworkFromScript(kind, value);
                    } else return;
                    scheduleDiagnosticRefresh();
                } catch (JSONException | RuntimeException ignored) {
                    diagnostic("native.browser.network", DiagnosticsPolicy.fields(
                        "phase", "failure", "errorType", DiagnosticsPolicy.errorType(ignored), "reason", "failed"));
                }
            });
        }
    }
    @SuppressLint("RequiresFeature")
    private void registerCredentialBridge() {
        WebViewCompat.addWebMessageListener(view, CREDENTIAL_BRIDGE_NAME, Collections.singleton("*"),
            (WebView sourceView, WebMessageCompat message, Uri sourceOrigin, boolean isMainFrame,
                JavaScriptReplyProxy replyProxy) -> {
                if (sourceView != view || !isMainFrame) return;
                String payload = message.getData();
                if (payload == null || payload.length() > 16_384) {
                    MAIN.post(() -> credentialFailure("凭据消息无效，请刷新网页后重试", "invalid-payload", null));
                    return;
                }
                String origin = webOrigin(sourceOrigin == null ? "" : sourceOrigin.toString());
                MAIN.post(() -> handleCredentialMessage(payload, origin));
            });
    }
    @SuppressLint("RequiresFeature")
    private void unregisterCredentialBridge() {
        WebViewCompat.removeWebMessageListener(view, CREDENTIAL_BRIDGE_NAME);
    }
    private boolean credentialPageMatches(String origin, long generation) {
        if (origin.isEmpty() || generation <= 0L) return false;
        return generation == credentialPageGeneration && origin.equals(credentialPageOrigin)
            || generation == credentialPreviousPageGeneration && origin.equals(credentialPreviousPageOrigin);
    }
    private void handleCredentialStatus(JSONObject value) {
        String phase = value.optString("phase", ""), reason = value.optString("reason", "");
        diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
            "phase", phase, "reason", reason, "generation", value.optLong("generation", 0L)));
        if ("install".equals(phase) && "ready".equals(reason)) {
            credentialError = ""; changed(); return;
        }
        if (!"failure".equals(phase)) return;
        credentialError = switch (reason) {
            case "missing-password", "missing-fields" -> "未识别到账号或密码，请使用账号密码登录后重试";
            case "ambiguous-password" -> "检测到多个密码输入框，请在登录页只保留一个密码框后重试";
            default -> "账号密码采集器初始化失败，请刷新网页后重试";
        };
        changed();
    }
    private void handleCredentialMessage(String payload, String sourceOrigin) {
        if (closed || !attached || !credentialRecordingEnabled) return;
        try {
            JSONObject value = new JSONObject(payload);
            String origin = webOrigin(value.optString("origin", ""));
            if (sourceOrigin.isEmpty() || !sourceOrigin.equals(origin)) {
                credentialFailure("页面地址已变化，请返回登录页后重试", "origin-mismatch", null); return;
            }
            if ("status".equals(value.optString("kind", "record"))) {
                handleCredentialStatus(value); return;
            }
            if (!credentialPageMatches(origin, value.optLong("generation", -1L))) {
                credentialFailure("页面地址已变化，请返回登录页后重试", "generation-mismatch", null); return;
            }
            String username = value.optString("username", ""), password = value.optString("password", "");
            String subaccount = value.optString("subaccount", "");
            if (password.isEmpty() || username.trim().isEmpty() && subaccount.trim().isEmpty()) {
                credentialFailure("未识别到账号或密码，请使用账号密码登录后重试", "missing-fields", null); return;
            }
            boolean recorded = credentialStore.record(identity.owner(), origin, username, password, subaccount);
            if (!recorded) {
                credentialFailure("未识别到账号或密码，请使用账号密码登录后重试", "missing-fields", null); return;
            }
            credentialError = "";
            diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
                "phase", "success", "reason", "success", "generation", value.optLong("generation", 0L)));
            pushCredentialRecords(view);
            changed();
        } catch (JSONException | RuntimeException failure) {
            credentialFailure("账号密码保存失败，请检查系统密钥状态后重试", "failed", failure);
        }
    }
    private void credentialFailure(String message, String reason, Throwable error) {
        credentialError = message;
        if (error == null) diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
            "phase", "failure", "reason", reason));
        else diagnostic("native.browser.credentials", DiagnosticsPolicy.fields(
            "phase", "failure", "errorType", DiagnosticsPolicy.errorType(error), "reason", reason));
        changed();
    }
    private void changed() { if (!closed && listener != null) listener.changed(); }
    private void diagnostic(String name, JSONObject fields) {
        BrowserAuthority.recordDiagnostic(authority, name, fields);
    }

    private void invalidate(String message) {
        if (closed) return;
        Listener previous = listener;
        destroy();
        if (current == this) {
            current = null; ++openGeneration; afterClear = null;
        }
        if (previous != null) previous.invalidated(message);
    }

    private void destroy() {
        if (closed) return;
        closed = true; attached = false; listener = null; ++proxyGeneration;
        flushCookies();
        MAIN.removeCallbacks(diagnosticRefresh);
        proxy.close();
        try { observer.close(); } catch (Exception ignored) { }
        if (view.getParent() instanceof ViewGroup parent) parent.removeView(view);
        view.stopLoading(); view.setWebChromeClient(null); view.setWebViewClient(null);
        if (credentialMessageSupported) unregisterCredentialBridge();
        view.removeJavascriptInterface(NETWORK_BRIDGE_NAME); view.destroy();
        context.setBaseContext(context.getApplicationContext());
    }

    private static void flushCookies() {
        try {
            CookieManager.getInstance().flush();
        } catch (RuntimeException ignored) {
            // Cookie persistence is best effort during Activity/process teardown.
        }
    }
}
