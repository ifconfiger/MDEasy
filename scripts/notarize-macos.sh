#!/usr/bin/env bash
# 提交到 Apple 公证服务，等待结果并把公证票据钉到目标上（stapler）。
# 用法：scripts/notarize-macos.sh <要提交的文件> <要 staple 的路径>
#   .app 需要先压成 zip 提交：scripts/notarize-macos.sh notarize.zip target/release/MDEasy.app
# 凭证（App Store Connect API Key）：APPLE_NOTARY_KEY（.p8 内容或其 base64）、
# APPLE_NOTARY_KEY_ID、APPLE_NOTARY_ISSUER。
set -euo pipefail

SUBMIT="$1"
TARGET="$2"

KEY_DIR="$(mktemp -d)"
KEY="$KEY_DIR/AuthKey.p8"
trap 'rm -rf "$KEY_DIR"' EXIT
if [[ "$APPLE_NOTARY_KEY" == *"BEGIN PRIVATE KEY"* ]]; then
  printf '%s\n' "$APPLE_NOTARY_KEY" > "$KEY"
else
  echo "$APPLE_NOTARY_KEY" | base64 --decode > "$KEY"
fi
AUTH=(--key "$KEY" --key-id "$APPLE_NOTARY_KEY_ID" --issuer "$APPLE_NOTARY_ISSUER")

RESULT="$(xcrun notarytool submit "$SUBMIT" "${AUTH[@]}" --wait --timeout 30m --output-format json)"
echo "$RESULT"
ID="$(echo "$RESULT" | jq -r .id)"
STATUS="$(echo "$RESULT" | jq -r .status)"
# 无论成败都打印日志，便于排查（通过的提交也可能带警告）
xcrun notarytool log "$ID" "${AUTH[@]}" || true
if [[ "$STATUS" != "Accepted" ]]; then
  echo "公证失败：$STATUS" >&2
  exit 1
fi

xcrun stapler staple "$TARGET"
xcrun stapler validate "$TARGET"
