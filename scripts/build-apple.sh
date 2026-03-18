#!/usr/bin/env bash
set -euo pipefail

# build-apple.sh — macOS arm64 向け cplp-ffi staticlib ビルド
#
# Usage:
#   ./scripts/build-apple.sh           # release ビルド
#   ./scripts/build-apple.sh debug     # debug ビルド

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PROFILE="${1:-release}"
PROFILE_FLAG=""
if [ "$PROFILE" = "release" ]; then
    PROFILE_FLAG="--release"
fi

MACOS_TARGET="aarch64-apple-darwin"

# TODO: visionOS ターゲット (aarch64-apple-visionos / aarch64-apple-visionos-sim) は
#       2026-03 時点で rustup target list に存在しない。
#       Tier 3 として nightly で実験的サポートがあるが、stable では利用不可。
#       visionOS 対応時は Xcode の xcrun + カスタム target spec JSON を検討する。
# VISIONOS_TARGET="aarch64-apple-visionos"

echo "==> macOS arm64 ($MACOS_TARGET) $PROFILE ビルド開始"

# ターゲットがインストールされていなければ追加
if ! rustup target list --installed | grep -q "$MACOS_TARGET"; then
    echo "==> ターゲット $MACOS_TARGET を追加中..."
    rustup target add "$MACOS_TARGET"
fi

cd "$PROJECT_ROOT"
cargo build --target "$MACOS_TARGET" -p cplp-ffi $PROFILE_FLAG

OUTPUT_DIR="$PROJECT_ROOT/target/$MACOS_TARGET/$PROFILE"
STATIC_LIB="$OUTPUT_DIR/libcplp_ffi.a"

if [ -f "$STATIC_LIB" ]; then
    SIZE=$(du -h "$STATIC_LIB" | cut -f1)
    echo "==> 成功: $STATIC_LIB ($SIZE)"
else
    echo "==> エラー: $STATIC_LIB が見つかりません" >&2
    exit 1
fi

# cbindgen で生成されたヘッダーもコピー
HEADER="$OUTPUT_DIR/cplp_ffi.h"
if [ -f "$HEADER" ]; then
    echo "==> ヘッダー: $HEADER"
fi

echo "==> 完了"
