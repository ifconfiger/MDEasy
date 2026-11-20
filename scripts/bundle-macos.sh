#!/usr/bin/env bash
# 打包 macOS 应用：target/release/MDEasy.app
# 用法：scripts/bundle-macos.sh [--open]
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

if [[ ! -f assets/icon/MDEasy.icns ]]; then
  cargo run -q -p icon-gen
  iconutil -c icns assets/icon/MDEasy.iconset -o assets/icon/MDEasy.icns
fi

cargo build --release -p mdeasy

APP=target/release/MDEasy.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/mdeasy "$APP/Contents/MacOS/MDEasy"
cp assets/icon/MDEasy.icns "$APP/Contents/Resources/MDEasy.icns"

doc_type() { # 名称 扩展名...
  local name=$1; shift
  printf '    <dict>\n      <key>CFBundleTypeName</key><string>%s</string>\n      <key>CFBundleTypeRole</key><string>Editor</string>\n      <key>LSHandlerRank</key><string>Alternate</string>\n      <key>CFBundleTypeExtensions</key>\n      <array>' "$name"
  for ext in "$@"; do printf '<string>%s</string>' "$ext"; done
  printf '</array>\n    </dict>\n'
}

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>MDEasy</string>
  <key>CFBundleDisplayName</key><string>MDEasy</string>
  <key>CFBundleIdentifier</key><string>com.ifconfiger.mdeasy</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>MDEasy</string>
  <key>CFBundleIconFile</key><string>MDEasy</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
$(doc_type Markdown md markdown mdown mkd)
$(doc_type JSON json jsonc)
$(doc_type YAML yaml yml)
$(doc_type TOML toml)
  </array>
</dict>
</plist>
EOF

# 本地临时签名，避免 Gatekeeper 直接拒绝运行（分发需换成开发者证书 + 公证）
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

echo "已生成 $APP ($(du -sh "$APP" | cut -f1))"
[[ "${1:-}" == "--open" ]] && open "$APP"
exit 0
