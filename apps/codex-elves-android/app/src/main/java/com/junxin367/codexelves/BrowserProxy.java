package com.junxin367.codexelves;

import android.os.IBinder;
import org.json.JSONObject;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.Response;
import okhttp3.WebSocket;
import okhttp3.WebSocketListener;
import okio.ByteString;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.Proxy;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;
import java.util.function.Supplier;

/** Authenticated local forward proxy. Every browser request reaches the computer through it. */
final class BrowserProxy implements AutoCloseable {
    record Credentials(String origin, String cookie) { }
    interface Listener { void unauthorized(); }
    static final String USER = "browser";
    private static final int MAX_HEADER = 32768;
    private static final int MAX_QUEUE = 256 * 1024;
    // Keep the active client count aligned with the manager browser-tunnel limit.
    private static final int MAX_CONNECTIONS = 32;
    private static final int ACCEPT_BACKLOG = 64;
    private final Supplier<Credentials> credentials;
    private final Listener listener;
    private final IBinder authority;
    private final ServerSocket server;
    private final String host;
    private final ExecutorService workers = Executors.newFixedThreadPool(MAX_CONNECTIONS);
    private final Semaphore slots = new Semaphore(MAX_CONNECTIONS);
    private final Set<Socket> clients = ConcurrentHashMap.newKeySet();
    private final Set<Tunnel> tunnels = ConcurrentHashMap.newKeySet();
    private final OkHttpClient http = new OkHttpClient.Builder()
        .proxy(Proxy.NO_PROXY).followRedirects(false).followSslRedirects(false)
        .connectTimeout(15, TimeUnit.SECONDS).readTimeout(0, TimeUnit.SECONDS)
        .pingInterval(25, TimeUnit.SECONDS).build();
    private final String password = randomToken();
    private final String realm = "codex-browser-" + randomToken();
    private final byte[] expectedAuthorization = ("Basic " + Base64.getEncoder().encodeToString(
        (USER + ":" + password).getBytes(StandardCharsets.US_ASCII))).getBytes(StandardCharsets.US_ASCII);
    private volatile boolean closed;
    private volatile boolean active = true;

    BrowserProxy(IBinder authority, Supplier<Credentials> credentials,
        Listener listener)
        throws IOException {
        this.authority = authority;
        this.credentials = credentials;
        this.listener = listener;
        server = bindServer();
        host = server.getInetAddress().getHostAddress();
        Thread acceptor = new Thread(this::accept, "codex-browser-proxy");
        acceptor.setDaemon(true);
        acceptor.start();
    }

    private static ServerSocket bindServer() throws IOException {
        IOException failure = null;
        for (String candidate : List.of(
            BrowserRoutePolicy.PROXY_HOST, BrowserRoutePolicy.PROXY_FALLBACK_HOST)) {
            ServerSocket socket = new ServerSocket();
            try {
                socket.bind(new InetSocketAddress(InetAddress.getByName(candidate), 0), ACCEPT_BACKLOG);
                return socket;
            } catch (IOException unavailable) {
                failure = unavailable;
                try { socket.close(); } catch (IOException ignored) { }
            }
        }
        throw failure == null ? new IOException("Unable to bind browser proxy") : failure;
    }

    private static String randomToken() {
        byte[] bytes = new byte[24];
        new SecureRandom().nextBytes(bytes);
        return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes);
    }

    int port() { return server.getLocalPort(); }
    String host() { return host; }
    String password() { return password; }
    boolean matchesChallenge(String host, String challengeRealm) {
        try { return !closed && realm.equals(challengeRealm) && BrowserRoutePolicy.reservedProxyHost(host); }
        catch (IllegalArgumentException invalid) { return false; }
    }

    void closeConnections() {
        for (Socket socket : clients) closeSocket(socket);
        for (Tunnel tunnel : tunnels) tunnel.cancel();
    }

    void setActive(boolean value) {
        active = value;
        if (!value) closeConnections();
    }

    private void accept() {
        while (!closed) {
            try {
                Socket socket = server.accept();
                if (closed || !active) {
                    closeSocket(socket);
                    continue;
                }
                boolean acquired = false;
                while (!closed && active && !acquired) {
                    try { acquired = slots.tryAcquire(500, TimeUnit.MILLISECONDS); }
                    catch (InterruptedException interrupted) {
                        Thread.currentThread().interrupt();
                        break;
                    }
                }
                if (!acquired) {
                    if (!closed) {
                        diagnostic("native.browser.proxy", DiagnosticsPolicy.fields(
                            "phase", "failure", "routeName", "capacity", "reason", "failed"));
                    }
                    closeSocket(socket);
                    continue;
                }
                clients.add(socket);
                try { workers.execute(() -> handle(socket)); }
                catch (RuntimeException stopped) {
                    clients.remove(socket); slots.release(); closeSocket(socket);
                }
            } catch (IOException stopped) {
                if (!closed) close();
            }
        }
    }

    private void handle(Socket socket) {
        Tunnel tunnel = null;
        boolean responseStarted = false;
        try {
            socket.setSoTimeout(15000);
            socket.setTcpNoDelay(true);
            InputStream input = socket.getInputStream();
            OutputStream output = socket.getOutputStream();
            String header = readHeader(input);
            String[] lines = header.split("\r\n");
            String authorization = null;
            for (int index = 1; index < lines.length; index++) {
                int colon = lines[index].indexOf(':');
                if (colon > 0 && "proxy-authorization".equalsIgnoreCase(lines[index].substring(0, colon))) {
                    if (authorization != null) throw new IOException("Duplicate proxy credentials");
                    authorization = lines[index].substring(colon + 1).trim();
                }
            }
            if (authorization == null || !MessageDigest.isEqual(expectedAuthorization,
                authorization.getBytes(StandardCharsets.US_ASCII))) {
                output.write(("HTTP/1.1 407 Proxy Authentication Required\r\n"
                    + "Proxy-Authenticate: Basic realm=\"" + realm + "\"\r\n"
                    + "Content-Length: 0\r\nConnection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
                return;
            }
            ParsedRequest request = parseRequest(lines);
            diagnostic("native.browser.proxy", DiagnosticsPolicy.fields(
                "phase", "request", "host", request.host, "port", request.port));
            if (!active) throw new IOException("Browser is inactive");
            Credentials identity = credentials.get();
            if (identity == null) throw new IOException("Authorization expired");
            tunnel = new Tunnel(socket, output, request, identity);
            tunnels.add(tunnel);
            if (closed || !active) tunnel.cancel();
            if (!tunnel.ready.await(20, TimeUnit.SECONDS) || !tunnel.connected)
                throw new IOException("Computer connection failed");
            socket.setSoTimeout(0);
            if (request.connect) {
                output.write("HTTP/1.1 200 Connection Established\r\n\r\n".getBytes(StandardCharsets.US_ASCII));
                output.flush();
                responseStarted = true;
            } else {
                tunnel.send(request.header);
                responseStarted = true;
            }
            byte[] buffer = new byte[16384];
            int count;
            while (!closed && !tunnel.finished && (count = input.read(buffer)) != -1)
                tunnel.send(ByteString.of(buffer, 0, count));
            if (!closed && !tunnel.finished) {
                tunnel.socket.send("{\"eof\":true}");
                tunnel.done.await(120, TimeUnit.SECONDS);
            }
        } catch (Exception failure) {
            if (tunnel == null || !tunnel.cleanClose) {
                diagnostic("native.browser.proxy", DiagnosticsPolicy.fields(
                    "phase", "failure", "errorType", DiagnosticsPolicy.errorType(failure),
                    "reason", "failed"));
            }
            if (!responseStarted && !socket.isClosed()) {
                try {
                    socket.getOutputStream().write(("HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n"
                        + "Connection: close\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
                } catch (IOException ignored) { /* The browser may already have cancelled. */ }
            }
            if (failure instanceof InterruptedException) Thread.currentThread().interrupt();
        } finally {
            if (tunnel != null) { tunnels.remove(tunnel); tunnel.cancel(); }
            clients.remove(socket); slots.release(); closeSocket(socket);
        }
    }

    private record ParsedRequest(String host, int port, boolean connect, ByteString header) { }

    private ParsedRequest parseRequest(String[] lines) throws Exception {
        String[] start = lines[0].split(" ", -1);
        if (start.length != 3 || !start[0].matches("[A-Z]{1,24}") || !"HTTP/1.1".equals(start[2]))
            throw new IOException("Invalid proxy request");
        boolean connect = "CONNECT".equals(start[0]);
        URI target = new URI(connect ? "https://" + start[1] : start[1]);
        if (target.getRawUserInfo() != null || target.getHost() == null
            || target.getFragment() != null
            || !connect && !"http".equalsIgnoreCase(target.getScheme()) && !"ws".equalsIgnoreCase(target.getScheme()))
            throw new IOException("Invalid target");
        String host = BrowserRoutePolicy.normalizeHost(target.getHost());
        int port = target.getPort() == -1 ? connect ? 443 : 80 : target.getPort();
        if (port < 1 || port > 65535 || BrowserRoutePolicy.reservedProxyHost(host)) {
            diagnostic("native.browser.proxy", DiagnosticsPolicy.fields(
                "phase", "failure", "host", host, "port", Math.max(1, Math.min(65535, port)),
                "routeName", "invalid-target", "reason", "failed"));
            throw new IOException("Invalid proxy target");
        }
        boolean upgrade = false;
        String hostHeader = null;
        List<String> headers = new ArrayList<>();
        for (int index = 1; index < lines.length; index++) {
            String line = lines[index];
            int colon = line.indexOf(':');
            if (colon < 1 || !line.substring(0, colon).matches("[!#$%&'*+.^_`|~0-9A-Za-z-]+")
                || line.matches("(?s).*[\\x00-\\x08\\x0b\\x0c\\x0e-\\x1f\\x7f].*"))
                throw new IOException("Invalid HTTP header");
            String name = line.substring(0, colon), value = line.substring(colon + 1).trim();
            if ("Host".equalsIgnoreCase(name)) {
                if (hostHeader != null) throw new IOException("Duplicate Host");
                hostHeader = value;
                continue;
            }
            if ("Upgrade".equalsIgnoreCase(name)) {
                if (!"websocket".equalsIgnoreCase(value)) throw new IOException("Unsupported upgrade");
                upgrade = true;
            }
            if (!name.equalsIgnoreCase("Proxy-Authorization") && !name.equalsIgnoreCase("Proxy-Connection")
                && !name.equalsIgnoreCase("Connection")) headers.add(line);
        }
        String authority = (host.contains(":") ? "[" + host + "]" : host) + ":" + port;
        if (hostHeader != null) {
            URI supplied = new URI((connect ? "https://" : "http://") + hostHeader);
            int suppliedPort = supplied.getPort() == -1 ? connect ? 443 : 80 : supplied.getPort();
            if (!host.equals(BrowserRoutePolicy.normalizeHost(supplied.getHost())) || port != suppliedPort)
                throw new IOException("Host does not match target");
        }
        String path = target.getRawPath();
        if (path == null || path.isEmpty()) path = "/";
        if (target.getRawQuery() != null) path += "?" + target.getRawQuery();
        String rewritten = start[0] + " " + path + " HTTP/1.1\r\nHost: " + authority
            + "\r\n" + (headers.isEmpty() ? "" : String.join("\r\n", headers) + "\r\n")
            + "Connection: " + (upgrade ? "Upgrade" : "close") + "\r\n\r\n";
        return new ParsedRequest(host, port, connect, ByteString.of(rewritten.getBytes(StandardCharsets.ISO_8859_1)));
    }

    private void diagnostic(String name, JSONObject fields) {
        BrowserAuthority.recordDiagnostic(authority, name, fields);
    }

    private static String readHeader(InputStream input) throws IOException {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        int matched = 0;
        byte[] delimiter = {'\r', '\n', '\r', '\n'};
        while (bytes.size() < MAX_HEADER) {
            int value = input.read();
            if (value < 0) throw new IOException("Incomplete proxy request");
            bytes.write(value);
            matched = value == delimiter[matched] ? matched + 1 : value == '\r' ? 1 : 0;
            if (matched == 4)
                return bytes.toString(StandardCharsets.ISO_8859_1.name()).substring(0, bytes.size() - 4);
        }
        throw new IOException("Proxy header too large");
    }

    private final class Tunnel extends WebSocketListener {
        private final Socket client;
        private final OutputStream output;
        private final ParsedRequest target;
        private final CountDownLatch ready = new CountDownLatch(1), done = new CountDownLatch(1);
        private final WebSocket socket;
        private volatile boolean connected, finished, cleanClose;

        Tunnel(Socket client, OutputStream output, ParsedRequest target, Credentials identity) {
            this.client = client; this.output = output; this.target = target;
            String url = identity.origin.replaceFirst("^http", "ws") + "/api/browser/tunnel";
            Request request = new Request.Builder().url(url).header("Cookie", identity.cookie)
                .header("Origin", identity.origin).header("skip_zrok_interstitial", "true").build();
            socket = http.newWebSocket(request, this);
        }

        @Override public void onOpen(WebSocket webSocket, Response response) {
            if (closed || !active || client.isClosed()) { webSocket.cancel(); finish(); return; }
            diagnostic("native.browser.tunnel", DiagnosticsPolicy.fields(
                "phase", "start", "host", target.host, "port", target.port));
            try {
                webSocket.send(new JSONObject().put("host", target.host).put("port", target.port).toString());
            } catch (Exception invalid) { finish(); }
        }

        @Override public void onMessage(WebSocket webSocket, String text) {
            try {
                JSONObject reply = new JSONObject(text);
                if (connected || !reply.optBoolean("ok")) {
                    if (reply.optInt("code") == 401) listener.unauthorized();
                    finish();
                    return;
                }
                connected = true; ready.countDown();
                diagnostic("native.browser.tunnel", DiagnosticsPolicy.fields(
                    "phase", "success", "host", target.host, "port", target.port));
            } catch (Exception invalid) { finish(); }
        }

        @Override public void onMessage(WebSocket webSocket, ByteString bytes) {
            if (!connected || finished || bytes.size() > 65536) { finish(); return; }
            try { output.write(bytes.toByteArray()); output.flush(); }
            catch (IOException cancelled) { finish(); }
        }

        void send(ByteString bytes) throws IOException, InterruptedException {
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(30);
            while (!closed && !finished && socket.queueSize() + bytes.size() > MAX_QUEUE) {
                if (System.nanoTime() >= deadline) throw new IOException("Tunnel upload timed out");
                synchronized (this) { wait(20); }
            }
            if (closed || finished || !socket.send(bytes)) throw new IOException("Tunnel closed");
        }

        private void finish() {
            finished = true; ready.countDown(); done.countDown(); closeSocket(client);
            synchronized (this) { notifyAll(); }
        }
        private void finishClean() {
            finished = true; ready.countDown(); done.countDown();
            try { client.shutdownInput(); } catch (IOException ignored) { }
            synchronized (this) { notifyAll(); }
        }
        void cancel() { finish(); socket.cancel(); }
        @Override public void onClosing(WebSocket webSocket, int code, String reason) {
            boolean valid = validCloseCode(code);
            cleanClose = valid;
            diagnostic("native.browser.tunnel", DiagnosticsPolicy.fields(
                "phase", valid ? "complete" : "failure", "host", target.host, "port", target.port,
                "closeCode", Math.max(0, Math.min(65535, code)),
                "reason", valid ? "success" : "failed"));
            try { webSocket.close(valid ? code : 1000, null); }
            catch (IllegalArgumentException invalid) { webSocket.cancel(); }
            if (valid) finishClean(); else finish();
        }
        @Override public void onClosed(WebSocket webSocket, int code, String reason) {
            if (cleanClose) finishClean(); else finish();
        }
        @Override public void onFailure(WebSocket webSocket, Throwable failure, Response response) {
            if (!cleanClose) {
                diagnostic("native.browser.tunnel", DiagnosticsPolicy.fields(
                    "phase", "failure", "host", target.host, "port", target.port,
                    "errorType", DiagnosticsPolicy.errorType(failure), "reason", "failed"));
            }
            if (response != null && response.code() == 401) listener.unauthorized();
            finish();
        }
    }

    private static boolean validCloseCode(int code) {
        return code == 1000 || code == 1001 || code == 1002 || code == 1003
            || code >= 1007 && code <= 1014 || code >= 3000 && code <= 4999;
    }

    private static void closeSocket(Socket socket) {
        try { socket.close(); } catch (IOException ignored) { }
    }

    @Override public void close() {
        if (closed) return;
        closed = true;
        try { server.close(); } catch (IOException ignored) { }
        closeConnections();
        workers.shutdownNow();
        http.dispatcher().cancelAll();
        http.connectionPool().evictAll();
        http.dispatcher().executorService().shutdown();
    }
}
