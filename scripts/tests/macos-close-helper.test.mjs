import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { URL } from "node:url";

const helperPath = new URL("../macos-close-narrant-window.swift", import.meta.url);
const adapterPath = new URL("../native-update-platform-adapter.mjs", import.meta.url);

test("macOS normal-exit helper 仅用 AppKit 对 exact PID 和 bundle 发出 terminate 请求", async () => {
  const [helper, adapter] = await Promise.all([
    readFile(helperPath, "utf8"),
    readFile(adapterPath, "utf8"),
  ]);

  assert.match(helper, /^import AppKit$/m);
  assert.doesNotMatch(helper, /ApplicationServices|Accessibility|AXUIElement|AXIsProcessTrusted|kAX[A-Za-z]+/);
  assert.match(helper, /CommandLine\.arguments\.count == 2/);
  assert.match(helper, /let rawPID = Int32\(CommandLine\.arguments\[1\]\), rawPID > 0/);
  assert.match(helper, /NSRunningApplication\(processIdentifier: pid_t\(rawPID\)\)/);
  assert.match(helper, /application\.bundleIdentifier == expectedBundleIdentifier/);
  assert.match(helper, /guard application\.terminate\(\) else/);
  assert.doesNotMatch(helper, /bundleURL|executableURL|localizedName|path\(|title/i);
  assert.equal((helper.match(/fail\("[^"]+"\)/g) ?? []).length, 3);

  assert.match(adapter, /command\("swiftc", \["-parse-as-library", "-framework", "AppKit", MAC_CLOSE_HELPER, "-o", output\]/);
  assert.doesNotMatch(adapter, /-framework", "ApplicationServices|--probe-trust/);
});
