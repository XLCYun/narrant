import { createServer } from "node:http";

const port = Number(process.env.NARRANT_GITHUB_MOCK_PORT ?? "18472");
const baseSha = process.env.NARRANT_E2E_BASE_SHA;
const headSha = process.env.NARRANT_E2E_HEAD_SHA;
let activeModelRequests = 0;
const json = (response, body) => {
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
};

const server = createServer((request, response) => {
  const url = new URL(request.url, `http://${request.headers.host}`);
  if (url.pathname === "/health") return json(response, { ok: true });
  if (url.pathname === "/model/status") return json(response, { active: activeModelRequests });
  // 保持模型响应未完成，避免首帧断言依赖外部服务速度；测试结束时统一关闭连接。
  if (url.pathname === "/model/chat/completions" && request.method === "POST") {
    activeModelRequests++;
    request.resume();
    response.once("close", () => { activeModelRequests--; });
    return;
  }
  const pullRequest = {
    title: "真实 Tauri 审核", number: 42, user: { login: "octocat" }, state: "open",
    draft: false, merged_at: null, base: { ref: "main", sha: baseSha },
    head: { ref: "feature", sha: headSha },
    updated_at: "2026-08-19T00:00:00Z",
    html_url: "https://github.com/octo-org/octo-repo/pull/42",
    body: "**安全描述** <script>bad()</script> ![tracker](https://tracker.invalid/pixel.png)",
  };
  if (url.pathname === "/repos/octo-org/octo-repo/pulls/42") return json(response, pullRequest);
  if (url.pathname === "/repos/octo-org/octo-repo/pulls" && url.searchParams.get("page") !== "1") {
    return json(response, []);
  }
  if (url.pathname === "/repos/octo-org/octo-repo/pulls") return json(response, [pullRequest]);
  response.writeHead(404);
  response.end();
});

server.listen(port, "127.0.0.1");
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => {
  server.closeAllConnections();
  server.close(() => process.exit(0));
});
