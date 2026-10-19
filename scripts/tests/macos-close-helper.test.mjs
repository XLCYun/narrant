import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { URL } from "node:url";

const helperPath = new URL("../update-smoke/macos-close-narrant-window.swift", import.meta.url);
const adapterPath = new URL("../update-smoke/native-update-platform-adapter.mjs", import.meta.url);

test("macOS helper 观察窗口或正常退出时只操作 exact PID 和 bundle，不依赖 Accessibility", async () => {
  const [helper, adapter] = await Promise.all([
    readFile(helperPath, "utf8"),
    readFile(adapterPath, "utf8"),
  ]);

  assert.match(helper, /^import AppKit$/m);
  assert.doesNotMatch(helper, /ApplicationServices|Accessibility|AXUIElement|AXIsProcessTrusted|kAX[A-Za-z]+/);
  assert.match(helper, /\(2\.\.\.3\)\.contains\(CommandLine\.arguments\.count\)/);
  assert.match(helper, /guard CommandLine\.arguments\[2\] == "ready"/);
  assert.match(helper, /let rawPID = Int32\(CommandLine\.arguments\[1\]\), rawPID > 0/);
  assert.match(helper, /NSRunningApplication\(processIdentifier: pid_t\(rawPID\)\)/);
  assert.match(helper, /application\.bundleIdentifier == expectedBundleIdentifier/);
  assert.match(helper, /guard application\.terminate\(\) else/);
  assert.doesNotMatch(helper, /bundleURL|executableURL|localizedName|path\(|title/i);

  assert.match(adapter, /command\("swiftc", \["-parse-as-library", "-framework", "AppKit", MAC_CLOSE_HELPER, "-o", output\]/);
  assert.doesNotMatch(adapter, /-framework", "ApplicationServices|--probe-trust/);
});
