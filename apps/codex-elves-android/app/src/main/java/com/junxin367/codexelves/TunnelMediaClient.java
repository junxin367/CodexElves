package com.junxin367.codexelves;

import java.io.ByteArrayInputStream;
import java.io.FilterInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InterruptedIOException;
import java.net.HttpURLConnection;
import java.net.URI;
import java.net.URISyntaxException;
import java.net.URL;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.function.BooleanSupplier;

/** Adds the ngrok access header to media elements without buffering their bodies. */
final class TunnelMediaClient {
    interface ConnectionFactory {
        HttpURLConnection open(URL url) throws IOException;
    }

    interface Completion {
        void finished(int status, long bytes, IOException failure);
    }

    static final class Response {
        final int status;
        final String reason;
        final String mime;
        final String charset;
        final Map<String, String> headers;
        final InputStream body;

        Response(HttpURLConnection connection, int status, InputStream body) throws IOException {
            this.status = status;
            String message = connection.getResponseMessage();
            this.reason = message != null && message.matches("[\\x20-\\x7e]+") ? message : "Response";
            String contentType = connection.getContentType();
            String[] parts = (contentType == null ? "application/octet-stream" : contentType).split(";");
            this.mime = parts[0].trim();
            String encoding = null;
            for (int index = 1; index < parts.length; index++) {
                String part = parts[index].trim();
                if (part.toLowerCase(Locale.ROOT).startsWith("charset=")) {
                    encoding = part.substring(8).trim().replace("\"", "");
                }
            }
            this.charset = encoding;
            this.headers = new LinkedHashMap<>();
            for (Map.Entry<String, List<String>> entry : connection.getHeaderFields().entrySet()) {
                String name = entry.getKey();
                if (name != null && entry.getValue() != null && !entry.getValue().isEmpty()
                    && !name.equalsIgnoreCase("Set-Cookie") && !name.equalsIgnoreCase("Connection")
                    && !name.equalsIgnoreCase("Transfer-Encoding")) {
                    this.headers.put(name, String.join(", ", entry.getValue()));
                }
            }
            this.body = body;
        }
    }

    private final ConnectionFactory factory;
    private final Set<HttpURLConnection> active = new HashSet<>();

    TunnelMediaClient() {
        this(url -> (HttpURLConnection) url.openConnection());
    }

    TunnelMediaClient(ConnectionFactory factory) {
        this.factory = factory;
    }

    static boolean handles(String rawUrl, String method) {
        if (!"GET".equals(method) && !"HEAD".equals(method)) return false;
        try {
            URI uri = new URI(rawUrl);
            return "https".equalsIgnoreCase(uri.getScheme()) && uri.getRawUserInfo() == null
                && uri.getRawFragment() == null && (uri.getPort() == -1 || uri.getPort() == 443)
                && uri.getHost() != null && uri.getHost().toLowerCase(Locale.ROOT).matches(
                    "[a-z0-9-]+\\.(ngrok-free\\.app|ngrok-free\\.dev|ngrok\\.app|ngrok\\.dev|ngrok\\.io)")
                && uri.getRawPath() != null && uri.getRawPath().matches(
                    "/api/(images/[A-Za-z0-9_-]{20,128}(/thumbnail)?|videos/[a-f0-9]{64}(/poster)?)");
        } catch (URISyntaxException | IllegalArgumentException invalid) {
            return false;
        }
    }

    Response open(
        String rawUrl,
        String method,
        Map<String, String> requestHeaders,
        BooleanSupplier stillCurrent,
        Completion completion
    ) throws IOException {
        if (!handles(rawUrl, method)) throw new IOException("媒体请求地址无效");
        checkCurrent(stillCurrent);
        HttpURLConnection connection = factory.open(new URL(rawUrl));
        synchronized (active) {
            checkCurrent(stillCurrent);
            active.add(connection);
        }
        int status = 0;
        try {
            connection.setConnectTimeout(15_000);
            connection.setReadTimeout(30_000);
            connection.setInstanceFollowRedirects(false);
            connection.setUseCaches(false);
            connection.setRequestMethod(method);
            for (Map.Entry<String, String> entry : requestHeaders.entrySet()) {
                String name = entry.getKey().toLowerCase(Locale.ROOT);
                boolean forward = name.equals("accept") || name.equals("accept-language")
                    || name.equals("user-agent") || name.equals("origin") || name.equals("cookie")
                    || name.equals("authorization") || name.equals("range") || name.equals("if-range");
                if (forward && entry.getValue() != null) {
                    connection.setRequestProperty(entry.getKey(), entry.getValue());
                }
            }
            // WebResourceResponse cannot represent 304; omit conditional cache validators.
            // Identity encoding keeps the forwarded length/range consistent with streamed bytes.
            connection.setRequestProperty("Accept-Encoding", "identity");
            connection.setRequestProperty("ngrok-skip-browser-warning", "1");
            status = connection.getResponseCode();
            checkCurrent(stillCurrent);
            if (status < 100 || status >= 300 && status < 400 || status > 599) {
                throw new IOException("媒体响应状态不受支持");
            }
            InputStream input = "HEAD".equals(method) || status == 204
                ? new ByteArrayInputStream(new byte[0])
                : status >= 400 ? connection.getErrorStream() : connection.getInputStream();
            if (input == null) input = new ByteArrayInputStream(new byte[0]);
            return new Response(connection, status,
                new StreamingBody(input, connection, stillCurrent, completion, status));
        } catch (IOException | RuntimeException failure) {
            release(connection);
            notifyCompletion(completion, status, 0,
                failure instanceof IOException ? (IOException) failure : new IOException("媒体请求失败"));
            throw failure;
        }
    }

    void cancelAll() {
        List<HttpURLConnection> connections;
        synchronized (active) {
            connections = new ArrayList<>(active);
            active.clear();
        }
        for (HttpURLConnection connection : connections) connection.disconnect();
    }

    private void release(HttpURLConnection connection) {
        synchronized (active) {
            active.remove(connection);
        }
        connection.disconnect();
    }

    private static void checkCurrent(BooleanSupplier stillCurrent) throws InterruptedIOException {
        if (Thread.currentThread().isInterrupted() || !stillCurrent.getAsBoolean()) {
            throw new InterruptedIOException("媒体请求已取消");
        }
    }

    private static void notifyCompletion(Completion completion, int status, long bytes, IOException failure) {
        try {
            completion.finished(status, bytes, failure);
        } catch (RuntimeException ignored) {
            // Diagnostics cannot change the outcome of a media request.
        }
    }

    private final class StreamingBody extends FilterInputStream {
        private final HttpURLConnection connection;
        private final BooleanSupplier stillCurrent;
        private final Completion completion;
        private final int status;
        private long bytes;
        private boolean finished;

        StreamingBody(InputStream input, HttpURLConnection connection, BooleanSupplier stillCurrent,
            Completion completion, int status) {
            super(input);
            this.connection = connection;
            this.stillCurrent = stillCurrent;
            this.completion = completion;
            this.status = status;
        }

        @Override
        public int read() throws IOException {
            byte[] one = new byte[1];
            return read(one, 0, 1) < 0 ? -1 : one[0] & 0xff;
        }

        @Override
        public int read(byte[] buffer, int offset, int length) throws IOException {
            if (finished) return -1;
            try {
                checkCurrent(stillCurrent);
                int count = in.read(buffer, offset, length);
                if (count < 0) finish(null);
                else bytes += count;
                return count;
            } catch (IOException failure) {
                finish(failure);
                throw failure;
            }
        }

        @Override
        public long skip(long count) throws IOException {
            if (finished) return 0;
            try {
                checkCurrent(stillCurrent);
                long skipped = in.skip(count);
                bytes += skipped;
                return skipped;
            } catch (IOException failure) {
                finish(failure);
                throw failure;
            }
        }

        @Override
        public void close() throws IOException {
            finish(null);
        }

        private void finish(IOException failure) {
            if (finished) return;
            finished = true;
            try {
                in.close();
            } catch (IOException closingFailure) {
                if (failure == null) failure = closingFailure;
            } finally {
                release(connection);
                notifyCompletion(completion, status, bytes, failure);
            }
        }
    }
}
