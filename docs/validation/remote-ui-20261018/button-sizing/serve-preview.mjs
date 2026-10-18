import { createServer } from "node:http";
import { readFileSync } from "node:fs";

// 仅为本地按钮效果复核提供静态页面，不连接会话服务或执行写操作。
const pagePath = new URL("./button-preview.html", import.meta.url);
createServer((request, response) => {
  if (request.headers.host !== "127.0.0.1:1470" || request.method !== "GET") {
    response.writeHead(403);
    response.end();
    return;
  }
  if (request.url !== "/" && request.url !== "/button-preview.html") {
    response.writeHead(404);
    response.end();
    return;
  }
  response.writeHead(200, {
    "Content-Type": "text/html; charset=utf-8",
    "Cache-Control": "no-store",
    "X-Content-Type-Options": "nosniff"
  });
  response.end(readFileSync(pagePath));
}).listen(1470, "127.0.0.1", () => {
  console.log("按钮效果对照页：http://127.0.0.1:1470");
});
