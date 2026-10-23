import { createServer } from "node:http";
import { readFile } from "node:fs/promises";

const port = Number(process.env.PROTOTYPE_PORT || 1491);
const root = new URL("./", import.meta.url);
const files = new Map([
  ["/", ["index.html", "text/html; charset=utf-8"]],
  ["/index.html", ["index.html", "text/html; charset=utf-8"]],
  ["/style.css", ["style.css", "text/css; charset=utf-8"]],
  ["/app.js", ["app.js", "text/javascript; charset=utf-8"]],
  ["/logo.png", ["logo.png", "image/png"]],
]);
const server = createServer(async (request, response) => {
  const file = files.get(new URL(request.url, "http://127.0.0.1").pathname);
  if (!file || !["GET", "HEAD"].includes(request.method)) {
    response.writeHead(404).end(); return;
  }
  try {
    const bytes = await readFile(new URL(file[0], root));
    response.writeHead(200, {
      "Content-Type": file[1], "Cache-Control": "no-store",
      "Content-Security-Policy": "default-src 'self'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    });
    response.end(request.method === "HEAD" ? undefined : bytes);
  } catch { response.writeHead(500).end("原型文件读取失败"); }
});
server.listen(port, "127.0.0.1", () => console.log(`原型已启动：http://127.0.0.1:${port}`));
