#!/bin/bash
set -euo pipefail

# 两条发布流水线共用 Apple Silicon 版本；只安装到本次任务的临时目录。
# 官方摘要：https://www.alibabacloud.com/help/en/oss/developer-reference/install-ossutil
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]]
: "${RUNNER_TEMP:?需要 CI 临时目录}"
: "${GITHUB_PATH:?需要 Actions PATH 文件}"
install_root="$(mktemp -d "$RUNNER_TEMP/narrant-ossutil.XXXXXX")"
archive="$install_root/ossutil.zip"
curl --fail --location --silent --show-error --retry 2 --max-time 120 \
  --output "$archive" \
  https://gosspublic.alicdn.com/ossutil/1.7.19/ossutil-v1.7.19-mac-arm64.zip
printf '%s  %s\n' \
  10ece4d328c5d2440833adc5f4167168e9b2a4c5d364f673b0c45bcc4fd02ec5 \
  "$archive" | shasum -a 256 --check
unzip -q "$archive" -d "$install_root"
mkdir -p "$install_root/bin"
install -m 0755 "$install_root/ossutil-v1.7.19-mac-arm64/ossutil" "$install_root/bin/ossutil"
"$install_root/bin/ossutil" --version
printf '%s\n' "$install_root/bin" >> "$GITHUB_PATH"
