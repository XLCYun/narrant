#!/usr/bin/env bash

set -euo pipefail

# 计算仓库与 Apple 私密文件目录，确保脚本可从任意工作目录运行。
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
APPLE_DIR="$REPO_DIR/.apple"
ENV_FILE="$APPLE_DIR/.env"

# 保存原钥匙串搜索列表，构建结束后恢复用户环境。
ORIGINAL_KEYCHAINS=()
TEMP_DIR=""
TEMP_KEYCHAIN=""

# 删除临时钥匙串并恢复构建前的钥匙串搜索列表。
cleanup() {
  local exit_code=$?

  if ((${#ORIGINAL_KEYCHAINS[@]} > 0)); then
    security list-keychains -d user -s "${ORIGINAL_KEYCHAINS[@]}" >/dev/null 2>&1 || true
  fi

  if [[ -n "$TEMP_KEYCHAIN" ]]; then
    security delete-keychain "$TEMP_KEYCHAIN" >/dev/null 2>&1 || true
  fi

  if [[ -n "$TEMP_DIR" && -d "$TEMP_DIR" ]]; then
    rm -rf -- "$TEMP_DIR"
  fi

  exit "$exit_code"
}

trap cleanup EXIT INT TERM

# 输出错误信息并终止构建。
fail() {
  echo "错误：$*" >&2
  exit 1
}

# 查找目录中唯一匹配的凭据文件，避免误用旧证书。
find_single_file() {
  local pattern=$1
  local label=$2
  local files=()

  while IFS= read -r -d '' file; do
    files+=("$file")
  done < <(find "$APPLE_DIR" -maxdepth 1 -type f -name "$pattern" -print0)

  ((${#files[@]} == 1)) || fail "$APPLE_DIR 中需要且只能有一个 $label 文件，当前找到 ${#files[@]} 个。"
  printf '%s' "${files[0]}"
}

[[ "$(uname -s)" == "Darwin" ]] || fail "签名后的 macOS 安装包必须在 macOS 上构建。"
command -v security >/dev/null || fail "找不到 macOS security 命令。"
command -v pnpm >/dev/null || fail "找不到 pnpm，请先安装项目要求的 pnpm。"
[[ -f "$ENV_FILE" ]] || fail "缺少 $ENV_FILE。"

# 加载本地 Apple 配置；该文件已由 .apple/ 规则整体忽略。
set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

: "${APPLE_API_ISSUER:?请在 .apple/.env 中设置 APPLE_API_ISSUER}"
: "${APPLE_API_KEY:?请在 .apple/.env 中设置 APPLE_API_KEY}"
: "${APPLE_SIGNING_IDENTITY:?请在 .apple/.env 中设置 APPLE_SIGNING_IDENTITY}"

P12_FILE="$(find_single_file '*.p12' '.p12')"
APPLE_API_KEY_PATH="$(find_single_file '*.p8' '.p8')"
export APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH APPLE_SIGNING_IDENTITY

# 未持久化密码时安全地从当前终端读取，不在命令输出中回显。
if [[ -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]]; then
  [[ -t 0 ]] || fail "非交互环境需要在 .apple/.env 中设置 APPLE_CERTIFICATE_PASSWORD。"
  read -r -s -p "请输入 developer-id-application.p12 的密码：" APPLE_CERTIFICATE_PASSWORD
  echo
fi

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/narrant-signing.XXXXXX")"
TEMP_KEYCHAIN="$TEMP_DIR/narrant-signing.keychain-db"
TEMP_KEYCHAIN_PASSWORD="$(openssl rand -hex 24)"

# 记录并扩展钥匙串搜索列表，让 codesign 能发现临时导入的签名身份。
while IFS= read -r keychain; do
  keychain="${keychain#\"}"
  keychain="${keychain%\"}"
  [[ -n "$keychain" ]] && ORIGINAL_KEYCHAINS+=("$keychain")
done < <(security list-keychains -d user | sed 's/^[[:space:]]*//')

security create-keychain -p "$TEMP_KEYCHAIN_PASSWORD" "$TEMP_KEYCHAIN"
security set-keychain-settings -lut 21600 "$TEMP_KEYCHAIN"
security unlock-keychain -p "$TEMP_KEYCHAIN_PASSWORD" "$TEMP_KEYCHAIN"

# 导入 Apple 中间证书等公开证书，确保新机器也能建立完整信任链。
while IFS= read -r -d '' certificate_file; do
  security import "$certificate_file" -k "$TEMP_KEYCHAIN" >/dev/null
done < <(find "$APPLE_DIR" -maxdepth 1 -type f -name '*.cer' -print0)

security import "$P12_FILE" \
  -k "$TEMP_KEYCHAIN" \
  -P "$APPLE_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign \
  -T /usr/bin/security >/dev/null
security set-key-partition-list \
  -S apple-tool:,apple:,codesign: \
  -s \
  -k "$TEMP_KEYCHAIN_PASSWORD" \
  "$TEMP_KEYCHAIN" >/dev/null
security list-keychains -d user -s "$TEMP_KEYCHAIN" "${ORIGINAL_KEYCHAINS[@]}"

# 在真正构建前确认指定身份可用，尽早暴露证书名或密码错误。
security find-identity -v -p codesigning "$TEMP_KEYCHAIN" | grep -Fq "$APPLE_SIGNING_IDENTITY" \
  || fail "临时钥匙串中未找到 APPLE_SIGNING_IDENTITY 指定的有效证书。"

echo "正在构建、签名并公证 macOS DMG……"
cd "$REPO_DIR"
BUILD_MARKER="$TEMP_DIR/build-started"
touch "$BUILD_MARKER"
pnpm tauri build --bundles dmg

# 定位本次生成的应用和 DMG，避免误检目录中的历史安装包。
PRODUCT_NAME="$(node -e "const config = require('./src-tauri/tauri.conf.json'); process.stdout.write(config.productName)")"
DMG_FILES=()
while IFS= read -r -d '' dmg_file; do
  DMG_FILES+=("$dmg_file")
done < <(find "$REPO_DIR/src-tauri/target/release/bundle/dmg" \
  -maxdepth 1 \
  -type f \
  -name '*.dmg' \
  -newer "$BUILD_MARKER" \
  -print0)

((${#DMG_FILES[@]} > 0)) || fail "构建完成，但未找到本次生成的 DMG。"

for dmg_file in "${DMG_FILES[@]}"; do
  # Tauri 创建 DMG 后会清理中间应用目录，因此挂载安装包校验其中的应用。
  mount_dir="$TEMP_DIR/dmg-mount"
  mkdir -p "$mount_dir"
  hdiutil attach -readonly -nobrowse -mountpoint "$mount_dir" "$dmg_file" >/dev/null

  set +e
  codesign --verify --deep --strict --verbose=2 "$mount_dir/$PRODUCT_NAME.app"
  app_codesign_status=$?
  xcrun stapler validate "$mount_dir/$PRODUCT_NAME.app"
  app_stapler_status=$?
  spctl --assess --type execute --verbose=4 "$mount_dir/$PRODUCT_NAME.app"
  app_gatekeeper_status=$?
  set -e

  hdiutil detach "$mount_dir" >/dev/null
  rmdir "$mount_dir"
  ((app_codesign_status == 0)) || fail "DMG 内的应用代码签名无效。"
  ((app_stapler_status == 0)) || fail "DMG 内的应用缺少有效公证票据。"
  ((app_gatekeeper_status == 0)) || fail "Gatekeeper 未接受 DMG 内的应用。"

  echo "正在公证最终安装包：$dmg_file"
  codesign --verify --strict --verbose=2 "$dmg_file"
  xcrun notarytool submit "$dmg_file" \
    --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" \
    --key "$APPLE_API_KEY_PATH" \
    --wait
  xcrun stapler staple "$dmg_file"
  xcrun stapler validate "$dmg_file"
  spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg_file"
done

echo "签名、公证及校验全部通过。"
printf '安装包：\n'
printf '  %s\n' "${DMG_FILES[@]}"
