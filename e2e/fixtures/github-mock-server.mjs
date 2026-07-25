import { createServer } from "node:http";

const port = Number(process.env.NARRANT_GITHUB_MOCK_PORT ?? "18472");
const json = (response, body) => {
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
};

const server = createServer((request, response) => {
  const url = new URL(request.url, `http://${request.headers.host}`);
  if (url.pathname === "/health") return json(response, { ok: true });
  if (url.pathname === "/repos/octo-org/octo-repo/pulls/42") return json(response, {
    title: "真实 Tauri 审核", number: 42, user: { login: "octocat" }, state: "open",
    draft: false, merged_at: null, base: { ref: "main", sha: "base-sha" },
    head: { ref: "feature", sha: "head-sha" },
    body: "**安全描述** <script>bad()</script> ![tracker](https://tracker.invalid/pixel.png)",
    changed_files: 1, additions: 1, deletions: 1,
  });
  if (url.pathname === "/repos/octo-org/octo-repo/compare/base-sha...head-sha") return json(response, {
    merge_base_commit: { sha: "comparison-base-sha" },
  });
  if (url.pathname === "/repos/octo-org/octo-repo/pulls/42/files") return json(response, [{
    filename: "README.md", status: "modified", additions: 1, deletions: 1, changes: 2,
  }]);
  if (url.pathname === "/repos/octo-org/octo-repo/git/commits/comparison-base-sha") return json(response, {
    tree: { sha: "comparison-base-tree" },
  });
  if (url.pathname === "/repos/octo-org/octo-repo/git/commits/head-sha") return json(response, {
    tree: { sha: "head-tree" },
  });
  if (url.pathname === "/repos/octo-org/octo-repo/git/trees/comparison-base-tree") return json(response, {
    truncated: false, tree: [{ path: "README.md", mode: "100644", type: "blob", sha: "old-blob", size: 4 }],
  });
  if (url.pathname === "/repos/octo-org/octo-repo/git/trees/head-tree") return json(response, {
    truncated: false, tree: [{ path: "README.md", mode: "100644", type: "blob", sha: "new-blob", size: 4 }],
  });
  if (url.pathname === "/repos/octo-org/octo-repo/git/blobs/old-blob") return json(response, {
    content: "b2xkCg==", encoding: "base64", size: 4,
  });
  if (url.pathname === "/repos/octo-org/octo-repo/git/blobs/new-blob") return json(response, {
    content: "bmV3Cg==", encoding: "base64", size: 4,
  });
  response.writeHead(404);
  response.end();
});

server.listen(port, "127.0.0.1");
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => server.close(() => process.exit(0)));
