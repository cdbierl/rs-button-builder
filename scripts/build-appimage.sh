#!/usr/bin/env bash
set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
package_dir="$project_dir/packaging/appimage"
appdir="$project_dir/target/appimage/AppDir"
dist_dir="${APPIMAGE_OUTPUT_DIR:-$project_dir/dist}"
appimagetool_version=1.9.1

package_name=$(sed -n 's/^name = "\(.*\)"/\1/p' "$project_dir/Cargo.toml" | head -n 1)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$project_dir/Cargo.toml" | head -n 1)
if [[ -z "$package_name" || -z "$version" ]]; then
    echo "Could not read the package name and version from Cargo.toml." >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64)
        appimage_arch=x86_64
        appimagetool_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
        ;;
    aarch64 | arm64)
        appimage_arch=aarch64
        appimagetool_sha256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
        ;;
    *)
        echo "Unsupported AppImage architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

appimagetool=${APPIMAGETOOL:-}
if [[ -z "$appimagetool" ]]; then
    appimagetool=$(command -v appimagetool || true)
fi
if [[ -z "$appimagetool" || ! -x "$appimagetool" ]]; then
    tool_dir=/tmp/rs-button-builder-appimage
    appimagetool="$tool_dir/appimagetool-$appimagetool_version-$appimage_arch.AppImage"
    download="$appimagetool.download"
    url=${APPIMAGETOOL_URL:-"https://github.com/AppImage/appimagetool/releases/download/$appimagetool_version/appimagetool-$appimage_arch.AppImage"}
    expected_sha256=${APPIMAGETOOL_SHA256:-$appimagetool_sha256}
    install -d -m 700 "$tool_dir"
    cat >&2 <<'EOF'
Using appimagetool from the temporary tool cache.

This remote executable should be checked for malware before use. For a
pre-verified tool, set APPIMAGETOOL to its local executable path instead.
EOF
    if [[ ! -x "$appimagetool" ]]; then
        if ! command -v curl >/dev/null; then
            echo "curl is required to download appimagetool." >&2
            exit 1
        fi
        echo "Downloading the official upstream build..."
        curl --fail --location --proto '=https' --tlsv1.2 \
            --output "$download" "$url"
        mv -- "$download" "$appimagetool"
        chmod 700 "$appimagetool"
    fi
    printf '%s  %s\n' "$expected_sha256" "$appimagetool" | sha256sum --check -
fi

echo "Building release binary..."
cargo build --manifest-path "$project_dir/Cargo.toml" --locked --release

echo "Staging AppDir..."
install -Dm755 "$project_dir/target/release/$package_name" \
    "$appdir/usr/bin/$package_name"
install -Dm755 "$package_dir/AppRun" "$appdir/AppRun"
install -Dm644 "$package_dir/rs-button-builder.desktop" \
    "$appdir/rs-button-builder.desktop"
install -Dm644 "$package_dir/rs-button-builder.desktop" \
    "$appdir/usr/share/applications/rs-button-builder.desktop"
install -Dm644 "$package_dir/rs-button-builder.svg" \
    "$appdir/rs-button-builder.svg"
install -Dm644 "$package_dir/rs-button-builder.svg" \
    "$appdir/usr/share/icons/hicolor/scalable/apps/rs-button-builder.svg"
install -Dm644 "$project_dir/licenses/Apache-2.0.txt" \
    "$appdir/usr/share/doc/$package_name/Material-Symbols-Apache-2.0.txt"

mkdir -p "$dist_dir"
output="$dist_dir/Button_Builder-${version}-${appimage_arch}.AppImage"

echo "Creating $(basename -- "$output")..."
ARCH="$appimage_arch" APPIMAGE_EXTRACT_AND_RUN=1 \
    "$appimagetool" "$appdir" "$output"
chmod +x "$output"

echo "Created $output"
