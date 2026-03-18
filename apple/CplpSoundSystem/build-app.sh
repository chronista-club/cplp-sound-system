#!/bin/bash
# CplpSoundSystem.app ビルドスクリプト
#
# 使い方:
#   ./build-app.sh          # Debug ビルド
#   ./build-app.sh release  # Release ビルド
#
# 出力: .build/CplpSoundSystem.app

set -euo pipefail
cd "$(dirname "$0")"

MODE="${1:-debug}"
XCODE_CONFIG="Debug"
CARGO_PROFILE=""

if [ "$MODE" = "release" ]; then
    XCODE_CONFIG="Release"
    CARGO_PROFILE="--release"
fi

# 1. Rust staticlib ビルド
echo "🔨 Building cplp-ffi ($MODE)..."
(cd ../.. && cargo build -p cplp-ffi $CARGO_PROFILE)

# 2. cbindgen でヘッダー再生成（インストール済みの場合のみ）
if command -v cbindgen &>/dev/null; then
    echo "📝 Regenerating cplp_ffi.h..."
    (cd ../.. && cbindgen \
        --config crates/cplp-ffi/cbindgen.toml \
        --crate cplp-ffi \
        --output apple/CplpSoundSystem/CplpBridge/cplp_ffi.h)
else
    echo "⚠️  cbindgen not found — skipping header generation"
    echo "   Install: cargo install cbindgen"
fi

# 3. xcodegen でプロジェクト生成
echo "⚙️  Generating Xcode project..."
xcodegen generate --quiet

# 4. xcodebuild
echo "🔨 Building CplpSoundSystem ($MODE)..."
xcodebuild -project CplpSoundSystem.xcodeproj \
    -scheme CplpSoundSystem \
    -configuration "$XCODE_CONFIG" \
    build \
    -quiet

# 5. ビルド成果物をコピー
DERIVED_DATA="$HOME/Library/Developer/Xcode/DerivedData"
APP_SRC=$(find "$DERIVED_DATA" -path "*/CplpSoundSystem-*/Build/Products/$XCODE_CONFIG/CplpSoundSystem.app" -maxdepth 8 2>/dev/null | grep -v Index.noindex | head -1)

if [ -z "$APP_SRC" ]; then
    echo "❌ CplpSoundSystem.app not found in DerivedData"
    exit 1
fi

APP_DIR=".build/CplpSoundSystem.app"
rm -rf "$APP_DIR"
mkdir -p .build
cp -R "$APP_SRC" "$APP_DIR"

echo "✅ CplpSoundSystem.app built: $APP_DIR"
echo ""
echo "起動: open $APP_DIR"
echo "インストール: cp -R $APP_DIR ~/Applications/"
