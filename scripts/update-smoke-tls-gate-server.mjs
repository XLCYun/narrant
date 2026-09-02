import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:https";
import process from "node:process";

const CONTROL_BOUND = 256 * 1024;

/** 解析 helper 的固定成对参数。 */
function parseArguments(arguments_) {
  const values = {};
  for (let index = 0; index < arguments_.length; index += 2) {
    const name = arguments_[index];
    const value = arguments_[index + 1];
    if (!name?.startsWith("--") || value === undefined) throw new Error(`无效 TLS helper 参数：${name ?? ""}`);
    values[name.slice(2)] = value;
  }
  return values;
}

/** 输出一行有界 JSON 事件。 */
function emit(event) {
  const line = `${JSON.stringify(event)}\n`;
  if (Buffer.byteLength(line) > CONTROL_BOUND) throw new Error("TLS helper 事件过大");
  process.stdout.write(line);
}

/** root helper 只服务 route config 中两个 exact GET。 */
async function main() {
  const options = parseArguments(process.argv.slice(2));
  for (const name of ["hostname", "certificate", "key", "routes"]) if (!options[name]) throw new Error(`缺少 --${name}`);
  if (options.hostname !== "github.com") throw new Error("TLS helper hostname 必须是 github.com");
  const [certificate, key, routeBytes] = await Promise.all([
    readFile(options.certificate), readFile(options.key), readFile(options.routes),
  ]);
  if (routeBytes.length <= 0 || routeBytes.length > CONTROL_BOUND) throw new Error("TLS helper route config 大小无效");
  const definitions = JSON.parse(routeBytes.toString("utf8"));
  if (!Array.isArray(definitions) || definitions.length !== 2) throw new Error("TLS helper route config 必须精确两项");
  const routes = new Map();
  for (const definition of definitions) {
    if (routes.has(definition.pathname) || !definition.pathname?.startsWith("/cyunlab/narrant/releases/")) {
      throw new Error("TLS helper route 重复或越界");
    }
    const information = await stat(definition.file);
    if (!information.isFile() || information.size <= 0) throw new Error("TLS helper route 文件无效");
    routes.set(definition.pathname, { ...definition, size: information.size });
  }
  const observed = new Set();
  const server = createServer({ cert: certificate, key }, (request, response) => {
    const route = routes.get(request.url ?? "");
    if (request.method !== "GET" || ![options.hostname, `${options.hostname}:443`].includes(request.headers.host ?? "") || !route) {
      request.resume();
      response.writeHead(404, { connection: "close", "content-length": "0" });
      response.end();
      return;
    }
    if (observed.has(request.url)) {
      request.resume();
      response.writeHead(409, { connection: "close", "content-length": "0" });
      response.end();
      return;
    }
    observed.add(request.url);
    request.resume();
    response.writeHead(200, {
      connection: "close",
      "content-type": route.contentType,
      "content-length": String(route.size),
      "cache-control": "no-store",
    });
    const input = createReadStream(route.file);
    input.once("error", () => response.destroy());
    input.pipe(response);
    emit({ event: "request", pathname: request.url });
    if (observed.size === routes.size) emit({ event: "all-requested" });
  });
  server.on("clientError", (_error, socket) => socket.destroy());
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen({ host: "127.0.0.1", port: 443, exclusive: true }, resolve);
  });
  emit({ event: "ready" });
  let closing = false;
  const close = (status) => {
    if (closing) return;
    closing = true;
    server.close(() => process.exit(status));
  };
  let control = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    control += chunk;
    if (Buffer.byteLength(control) > CONTROL_BOUND) throw new Error("TLS helper control 过大");
    for (;;) {
      const newline = control.indexOf("\n");
      if (newline < 0) break;
      const command = JSON.parse(control.slice(0, newline));
      control = control.slice(newline + 1);
      if (command.command !== "close") throw new Error("TLS helper control command 无效");
      close(0);
    }
  });
  process.stdin.once("end", () => close(0));
  process.once("SIGTERM", () => close(0));
  process.once("SIGINT", () => close(0));
}

main().catch((error) => {
  process.stderr.write(`${error instanceof Error ? error.message : "TLS helper 失败"}\n`);
  process.exitCode = 1;
});
import { Buffer } from "node:buffer";
