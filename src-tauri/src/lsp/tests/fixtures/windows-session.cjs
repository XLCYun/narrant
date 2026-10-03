// 由 Windows Rust 测试显式启动；真实 stdio 上只输出 JSON-RPC 帧。
const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const directory = process.env.NARRANT_LSP_WINDOWS_SESSION_DIR;
const scenario = process.env.NARRANT_LSP_WINDOWS_SESSION_SCENARIO;
if (!directory || !scenario) {
  throw new Error("Windows LSP fixture requires its test directory and scenario");
}

// 发布完成的文件后才让另一进程读到，避免 PID 或同步标记的半写入状态。
function publish(name, contents = "ready") {
  const temporary = path.join(directory, `${name}.${process.pid}.tmp`);
  fs.writeFileSync(temporary, contents);
  fs.renameSync(temporary, path.join(directory, name));
}

// 短命角色不发布 PID，也不保留历史 HANDLE；累计覆盖只能来自被测所有者。
if (process.argv[2] === "--short-lived") process.exit(0);

// 夹具自身也有最终退出期限，批次场景为有限的 256 次 Windows 冷启动留出余量。
const watchdog = setTimeout(
  () => process.exit(124),
  scenario === "batch-parent-lives" ? 120_000 : 30_000,
);

if (process.argv[2] === "--descendant") {
  publish("descendant-pid", String(process.pid));
  // 不读取继承的 stdin，也不写 stdout；保持管道打开直到会话所有者回收。
} else if (process.argv[2] === "--root-only") {
  // 独立根角色用于清理错误测试，没有后代来掩盖 direct Child 的等待责任。
  publish("root-pid", String(process.pid));
} else {
  publish("root-pid", String(process.pid));
  const descendant = spawn(process.execPath, [__filename, "--descendant"], {
    // EOF 场景需要让父输出关闭可见；其他场景明确保留后代继承的管道。
    stdio: scenario === "initialize-eof" ? "ignore" : "inherit",
    windowsHide: true,
    // Windows 非 detached child 会进入 libuv 自身的 KILL_ON_CLOSE Job，父退出会提前杀叶。
    // detached 不设置 BREAKAWAY；Rust 侧还会验证叶进程仍属于准确的外层 Narrant Job。
    detached: process.platform === "win32",
  });
  descendant.on("error", () => process.exit(125));

  // 确认宿主已保存准确 HANDLE 后才允许服务端响应、失败或主动退出。
  function waitForHandles() {
    return new Promise((resolve) => {
      const poll = setInterval(() => {
        if (fs.existsSync(path.join(directory, "handles-ready"))) {
          clearInterval(poll);
          resolve();
        }
      }, 5);
    });
  }

  // 长度按 UTF-8 字节计算，和生产传输使用相同的标准 Content-Length 帧。
  function respond(id, result) {
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", id, result }));
    process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
    process.stdout.write(body);
  }

  let initialized = false;
  let shutdown = false;
  let batches = 0;
  let totalChildren = 0;
  // 一个批次仅启动固定数量自有短命进程，等待每个实际进程结束才回报结果。
  async function runBatch(count, concurrency, detached = false) {
    if (![2, 4, 32].includes(count) || ![1, 4].includes(concurrency)) {
      throw new Error("unexpected bounded batch configuration");
    }
    let next = 0;
    let active = 0;
    let peakActive = 0;
    async function worker() {
      while (next < count) {
        next += 1;
        await new Promise((resolve, reject) => {
          const child = spawn(process.execPath, [__filename, "--short-lived"], {
            stdio: "ignore",
            windowsHide: true,
            // 32 个子进程的正式批次始终传 false；仅有限诊断比较 Windows detached。
            detached: detached && process.platform === "win32",
          });
          active += 1;
          peakActive = Math.max(peakActive, active);
          child.once("error", reject);
          child.once("close", (code) => {
            active -= 1;
            if (code === 0) resolve();
            else reject(new Error(`short child exited with ${code}`));
          });
        });
      }
    }
    await Promise.all(Array.from({ length: concurrency }, () => worker()));
    batches += 1;
    totalChildren += count;
    return { batch: batches, completed: count, total: totalChildren, peakActive };
  }

  // 服务端检查协议顺序，错误顺序不能误成为一个通过的进程清理测试。
  async function handle(message) {
    if (message.method === "initialize") {
      await waitForHandles();
      if (scenario === "initialize-eof") {
        process.exit(0);
      }
      if (scenario === "initialize-malformed") {
        process.stdout.write("Content-Length: 2\r\n\r\nx!");
        return;
      }
      respond(message.id, { capabilities: {} });
    } else if (message.method === "initialized") {
      initialized = true;
      publish("initialized");
    } else if (message.method === "example/echo") {
      if (!initialized) throw new Error("query before initialized");
      respond(message.id, { echo: message.params.echo });
    } else if (message.method === "example/pending") {
      if (!initialized) throw new Error("pending query before initialized");
      publish("request-pending");
      if (scenario === "pending-parent-exits") process.exit(0);
    } else if (message.method === "example/batch") {
      if (!initialized) throw new Error("batch before initialized");
      if (message.params.count !== 32) throw new Error("unexpected batch size");
      respond(message.id, await runBatch(32, message.params.concurrency));
    } else if (message.method === "example/diagnostic-batch") {
      if (!initialized) throw new Error("diagnostic batch before initialized");
      if (![2, 4].includes(message.params.count) || typeof message.params.detached !== "boolean") {
        throw new Error("unexpected diagnostic batch configuration");
      }
      respond(message.id, await runBatch(message.params.count, 1, message.params.detached));
    } else if (message.method === "shutdown") {
      if (!initialized) throw new Error("shutdown before initialized");
      shutdown = true;
      publish("shutdown-received");
      respond(message.id, null);
    } else if (message.method === "exit") {
      if (!shutdown) throw new Error("exit before shutdown");
      publish("exit-received");
      if (scenario === "normal-parent-exits") process.exit(0);
    } else {
      throw new Error(`unexpected method ${message.method}`);
    }
  }

  let buffered = Buffer.alloc(0);
  let handling = Promise.resolve();
  // 顺序消费任意分块的 stdin，避免假定一个 data 事件等于一帧。
  process.stdin.on("data", (chunk) => {
    buffered = Buffer.concat([buffered, chunk]);
    for (;;) {
      const end = buffered.indexOf("\r\n\r\n");
      if (end === -1) return;
      const header = buffered.subarray(0, end).toString("ascii");
      const match = /^Content-Length: (\d+)$/i.exec(header);
      if (!match) throw new Error("unexpected LSP header");
      const length = Number(match[1]);
      if (buffered.length < end + 4 + length) return;
      const message = JSON.parse(buffered.subarray(end + 4, end + 4 + length));
      buffered = buffered.subarray(end + 4 + length);
      handling = handling.then(() => handle(message)).catch(() => process.exit(126));
    }
  });
  // 关闭输入不会让未完成的场景自行成功退出；最终期限仍由 watchdog 负责。
  process.stdin.on("error", () => {});
}

// 显式保留计时器引用，后代和停止回应的根进程都由真实清理路径结束。
void watchdog;
