#!/usr/bin/env bash
# 打包 macOS 应用：target/release/MDEasy.app
# 用法：scripts/bundle-macos.sh [--install] [--no-default] [--open]
#   --install     复制到 /Applications/MDEasy.app（文件关联指向安装后的路径，推荐）
#   --no-default  不把 MDEasy 设为 md / json / yaml / toml 的默认打开程序
#   --open        完成后启动
set -euo pipefail

INSTALL=0 SET_DEFAULT=1 OPEN=0
for arg in "$@"; do
  case $arg in
    --install) INSTALL=1 ;;
    --no-default) SET_DEFAULT=0 ;;
    --open) OPEN=1 ;;
    *) echo "未知参数：$arg" >&2; exit 2 ;;
  esac
done

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

doc_type() { # 名称 UTI...
  local name=$1; shift
  printf '    <dict>\n      <key>CFBundleTypeName</key><string>%s</string>\n      <key>CFBundleTypeRole</key><string>Editor</string>\n      <key>LSHandlerRank</key><string>Default</string>\n      <key>LSItemContentTypes</key>\n      <array>' "$name"
  for uti in "$@"; do printf '<string>%s</string>' "$uti"; done
  printf '</array>\n    </dict>\n'
}

imported_type() { # UTI 描述 父类型 扩展名...
  local uti=$1 desc=$2 parent=$3; shift 3
  printf '    <dict>\n      <key>UTTypeIdentifier</key><string>%s</string>\n      <key>UTTypeDescription</key><string>%s</string>\n      <key>UTTypeConformsTo</key><array><string>%s</string></array>\n      <key>UTTypeTagSpecification</key>\n      <dict><key>public.filename-extension</key><array>' "$uti" "$desc" "$parent"
  for ext in "$@"; do printf '<string>%s</string>' "$ext"; done
  printf '</array></dict>\n    </dict>\n'
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
$(doc_type Markdown net.daringfireball.markdown)
$(doc_type JSON public.json com.microsoft.jsonc)
$(doc_type YAML public.yaml)
$(doc_type TOML public.toml)
  </array>
  <key>UTImportedTypeDeclarations</key>
  <array>
$(imported_type net.daringfireball.markdown Markdown public.plain-text md markdown mdown mkd)
$(imported_type com.microsoft.jsonc "JSON with Comments" public.json jsonc)
  </array>
</dict>
</plist>
EOF

# 本地临时签名，避免 Gatekeeper 直接拒绝运行（分发需换成开发者证书 + 公证）
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

echo "已生成 $APP ($(du -sh "$APP" | cut -f1))"

if (( INSTALL )); then
  rm -rf /Applications/MDEasy.app
  cp -R "$APP" /Applications/MDEasy.app
  APP=/Applications/MDEasy.app
  echo "已安装到 $APP"
fi

# 向 Launch Services 注册，并设为默认打开程序
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$LSREGISTER" -f "$APP"
# 安装后注销构建目录里的副本：两者 bundle id 相同，否则系统可能仍用旧副本打开文件
(( INSTALL )) && "$LSREGISTER" -u target/release/MDEasy.app
if (( SET_DEFAULT )); then
  swift scripts/set-default-app.swift "$APP" \
    net.daringfireball.markdown public.json com.microsoft.jsonc public.yaml public.toml
fi

(( OPEN )) && open "$APP"
exit 0
