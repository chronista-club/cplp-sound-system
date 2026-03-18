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

REPO_ROOT="$(cd ../.. && pwd)"

# 1. Rust staticlib ビルド
echo "==> Building cplp-ffi ($MODE)..."
(cd "$REPO_ROOT" && cargo build -p cplp-ffi $CARGO_PROFILE)

# 2. cbindgen でヘッダー自動生成
HEADER_PATH="CplpBridge/cplp_ffi.h"
if command -v cbindgen &>/dev/null; then
    echo "==> Generating $HEADER_PATH via cbindgen..."
    cbindgen \
        --config "$REPO_ROOT/crates/cplp-ffi/cbindgen.toml" \
        --crate cplp-ffi \
        --output "$HEADER_PATH" \
        "$REPO_ROOT"

    # ヘッダーの差分を検出（開発者への通知）
    if git -C "$REPO_ROOT" diff --quiet -- "apple/CplpSoundSystem/$HEADER_PATH" 2>/dev/null; then
        echo "    (no changes)"
    else
        echo "    *** cplp_ffi.h updated — commit the new header ***"
    fi
elif [ "${CBINDGEN_REQUIRED:-0}" = "1" ]; then
    echo "ERROR: cbindgen is required but not found. Install with: cargo install cbindgen"
    exit 1
else
    echo "==> cbindgen not found, using existing $HEADER_PATH"
    echo "    Install with: cargo install cbindgen"
fi

# 3. xcodegen でプロジェクト生成
echo "==> Generating Xcode project..."
xcodegen generate --quiet

# 4. xcodebuild
echo "==> Building CplpSoundSystem ($MODE)..."
xcodebuild -project CplpSoundSystem.xcodeproj \
    -scheme CplpSoundSystem \
    -configuration "$XCODE_CONFIG" \
    build \
    -quiet

# 5. ビルド成果物をコピー
DERIVED_DATA="$HOME/Library/Developer/Xcode/DerivedData"
APP_SRC=$(find "$DERIVED_DATA" -path "*/CplpSoundSystem-*/Build/Products/$XCODE_CONFIG/CPLP Sound System.app" -maxdepth 8 2>/dev/null | grep -v Index.noindex | head -1)

if [ -z "$APP_SRC" ]; then
    echo "ERROR: CPLP Sound System.app not found in DerivedData"
    exit 1
fi

APP_DIR=".build/CplpSoundSystem.app"
rm -rf "$APP_DIR"
mkdir -p .build
cp -R "$APP_SRC" "$APP_DIR"

echo "==> CplpSoundSystem.app built: $APP_DIR"
echo ""
echo "起動: open \"$APP_DIR\""
echo "インストール: cp -R \"$APP_DIR\" ~/Applications/"
