#!/usr/bin/env bash
set -euo pipefail

# PhotonCAM macOS Packaging Script
# Supports:
#   ./scripts/build_dmg.sh universal   (Builds Universal 2 binary: Apple Silicon + Intel) [DEFAULT]
#   ./scripts/build_dmg.sh arm64       (Apple Silicon only)
#   ./scripts/build_dmg.sh intel       (Intel x86_64 only)
#   ./scripts/build_dmg.sh all         (Builds all 3 macOS variants)

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)
DEFAULT_SIGNING_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | grep "Apple Development" | head -n 1 | cut -d '"' -f 2 || true)
SIGNING_IDENTITY="${SIGNING_IDENTITY:-${DEFAULT_SIGNING_IDENTITY:-}}"
DIST_DIR="$PROJECT_DIR/dist"
mkdir -p "$DIST_DIR"

MODE="${1:-universal}"
export PATH="$HOME/.cargo/bin:$PATH"

package_target() {
    local target_name="$1"    # "arm64", "intel-x86_64", or "universal"
    local bin_source="$2"
    local vol_name="PhotonCAM Beta ${VERSION}"
    local app_dir="$DIST_DIR/PhotonCAM_Beta_${target_name}.app"
    local dmg_root="$DIST_DIR/dmg_${target_name}_root"
    local dmg_path="$DIST_DIR/PhotonCAM-Beta-${VERSION}-${target_name}.dmg"
    local zip_path="$DIST_DIR/PhotonCAM-Beta-${VERSION}-macos-${target_name}.zip"

    echo "==> Constructing ${target_name} .app bundle..."
    rm -rf "$app_dir" "$dmg_root"
    mkdir -p "$app_dir/Contents/MacOS"
    mkdir -p "$app_dir/Contents/Resources"

    cp "$bin_source" "$app_dir/Contents/MacOS/PhotonCAM"
    chmod +x "$app_dir/Contents/MacOS/PhotonCAM"
    cp "$PROJECT_DIR/Info.plist" "$app_dir/Contents/Info.plist"
    cp "$PROJECT_DIR/assets/photoncam.icns" "$app_dir/Contents/Resources/photoncam.icns"

    echo "==> Code signing ${target_name} bundle..."
    if [ -n "$SIGNING_IDENTITY" ]; then
        codesign --force --deep --sign "$SIGNING_IDENTITY" "$app_dir"
        codesign --verify --deep --strict "$app_dir"
    else
        echo "    (No Apple Development signing identity found; signing ad-hoc)"
        codesign --force --deep --sign - "$app_dir"
    fi

    echo "==> Packaging DMG: $dmg_path..."
    mkdir -p "$dmg_root"
    cp -R "$app_dir" "$dmg_root/PhotonCAM Beta.app"
    ln -s /Applications "$dmg_root/Applications"
    rm -f "$dmg_path"
    hdiutil create -volname "$vol_name" -srcfolder "$dmg_root" -ov -format UDZO "$dmg_path"

    echo "==> Packaging ZIP: $zip_path..."
    rm -f "$zip_path"
    (cd "$DIST_DIR" && zip -r "$zip_path" "PhotonCAM_Beta_${target_name}.app")

    if [[ "$target_name" == "universal" || "$target_name" == "arm64" ]]; then
        echo "==> Installing latest build into /Applications/PhotonCAM Beta.app..."
        rm -rf "/Applications/PhotonCAM Beta.app"
        cp -R "$app_dir" "/Applications/PhotonCAM Beta.app"
    fi

    rm -rf "$dmg_root" "$app_dir"
    echo "==> Done: $dmg_path"
}

build_arm64() {
    echo "==> Building arm64 release binary..."
    cargo build --release --bin photoncam --target aarch64-apple-darwin --features gui
}

build_intel() {
    echo "==> Building x86_64 (Intel) release binary..."
    cargo build --release --bin photoncam --target x86_64-apple-darwin --features gui
}

case "$MODE" in
    arm64)
        build_arm64
        package_target "arm64" "$PROJECT_DIR/target/aarch64-apple-darwin/release/photoncam"
        ;;
    intel)
        build_intel
        package_target "intel-x86_64" "$PROJECT_DIR/target/x86_64-apple-darwin/release/photoncam"
        ;;
    universal)
        build_arm64
        build_intel
        mkdir -p "$DIST_DIR"
        UNIV_BIN="$DIST_DIR/photoncam_universal_tmp"
        echo "==> Creating Universal 2 binary with lipo..."
        lipo -create -output "$UNIV_BIN" \
            "$PROJECT_DIR/target/aarch64-apple-darwin/release/photoncam" \
            "$PROJECT_DIR/target/x86_64-apple-darwin/release/photoncam"
        package_target "universal" "$UNIV_BIN"
        rm -f "$UNIV_BIN"
        ;;
    all)
        build_arm64
        build_intel
        package_target "arm64" "$PROJECT_DIR/target/aarch64-apple-darwin/release/photoncam"
        package_target "intel-x86_64" "$PROJECT_DIR/target/x86_64-apple-darwin/release/photoncam"
        UNIV_BIN="$DIST_DIR/photoncam_universal_tmp"
        echo "==> Creating Universal 2 binary with lipo..."
        lipo -create -output "$UNIV_BIN" \
            "$PROJECT_DIR/target/aarch64-apple-darwin/release/photoncam" \
            "$PROJECT_DIR/target/x86_64-apple-darwin/release/photoncam"
        package_target "universal" "$UNIV_BIN"
        rm -f "$UNIV_BIN"
        ;;
    *)
        echo "Usage: $0 [universal|arm64|intel|all]"
        exit 1
        ;;
esac

echo "=========================================="
echo " macOS Packaging Complete!"
echo "=========================================="
