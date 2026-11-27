#!/usr/bin/env bash
set -euo pipefail
umask 022

if [[ $# -ne 4 && ! ( $# -eq 5 && $5 == --mcp-beta ) ]]; then
    echo "Usage: $0 VERSION DATE TARGET_DIR OUTPUT.dmg [--mcp-beta]" >&2
    exit 2
fi

version=$1
date=$2
target_dir=$3
output=$4
mode=${5:-}

if [[ ! $version =~ ^[0-9][A-Za-z0-9.+~-]*$ ]]; then
    echo "Invalid macOS package version: $version" >&2
    exit 2
fi
if [[ ! $date =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    echo "Invalid release date: $date" >&2
    exit 2
fi

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

name=CreaBIM
identifier=io.github.chopinregis.creabim
executable=CreaBIM
app_binary=$executable
if [[ $mode == --mcp-beta ]]; then
    name='CreaBIM MCP Beta'
    identifier=io.github.chopinregis.creabim.beta.mcp
    executable=CreaBIM-Beta
    app_binary=creabim-app
fi

app="$stage/$name.app"
contents="$app/Contents"
install -d "$contents/MacOS" "$contents/Resources"
install -m 0755 "$target_dir/creabim-app" "$contents/MacOS/$app_binary"
install -m 0755 "$target_dir/creabim" "$contents/MacOS/creabim-cli"
install -m 0644 crates/creabim-app/assets/logo/creabim.icns \
    "$contents/Resources/creabim.icns"
install -m 0644 LICENSE "$stage/LICENSE"
if [[ $mode == --mcp-beta ]]; then
    mcp="$contents/Resources/mcp"
    install -d "$mcp"
    install -m 0644 tools/mcp/{server,project,live}.py "$mcp/"
    install -m 0644 packaging/macos/mcp-beta-env.sh "$contents/Resources/"
    install -m 0755 packaging/macos/creabim-mcp "$contents/MacOS/"
    install -m 0644 packaging/macos/MCP_BETA_README.txt "$stage/READ ME FIRST.txt"

    # The bundled wheels must match the host architecture used to build the app.
    source packaging/macos/mcp-beta-env.sh
    python=$(creabim_mcp_python)
    arch=$("$python" -I -S -c 'import platform; print(platform.machine())')
    for binary in "$contents/MacOS/$app_binary" "$contents/MacOS/creabim-cli"; do
        if [[ $(lipo -archs "$binary") != "$arch" ]]; then
            echo "The Beta needs a $arch build of both CreaBIM binaries." >&2
            exit 1
        fi
    done
    "$python" -m pip install --disable-pip-version-check --only-binary=:all: \
        --no-compile --target "$mcp/python" -r tools/mcp/requirements.txt

    cat > "$contents/MacOS/$executable" <<'SH'
#!/bin/sh
set -eu
bin=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
. "$bin/../Resources/mcp-beta-env.sh"
creabim_mcp_environment
exec "$bin/creabim-app" "$@"
SH
    chmod 0755 "$contents/MacOS/$executable"
else
    install -m 0644 docs/RELEASE_README.txt "$stage/READ ME FIRST.txt"
fi
ln -s /Applications "$stage/Applications"

bundle_version=${version%%[-+~]*}
cat > "$contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>$executable</string>
    <key>CFBundleIdentifier</key>
    <string>$identifier</string>
    <key>CFBundleName</key>
    <string>$name</string>
    <key>CFBundleDisplayName</key>
    <string>$name</string>
    <key>CFBundleIconFile</key>
    <string>creabim</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$bundle_version</string>
    <key>CFBundleVersion</key>
    <string>$bundle_version</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

if [[ $mode == --mcp-beta ]]; then
    plutil -lint "$contents/Info.plist"
    codesign --force --sign - "$contents/MacOS/$app_binary" \
        "$contents/MacOS/creabim-cli" "$contents/MacOS/creabim-mcp"
    codesign --force --sign - "$app"
    codesign --verify --deep --strict "$app"
fi

mkdir -p "$(dirname "$output")"
# hdiutil's own guess at the image size, from -srcfolder alone, came out too
# small on GitHub's macos-26 runner ("create failed - No space left on
# device", 1.2.0-rc.1 test run). Give it the folder's size plus a quarter
# and 64 MB for the file system.
stage_mb=$(du -sm "$stage" | cut -f1)
size_mb=$((stage_mb + stage_mb / 4 + 64))
echo "Disk image: ${stage_mb} MB of files, image size ${size_mb} MB"
hdiutil create -volname "$name $version" -srcfolder "$stage" \
    -size "${size_mb}m" -ov -format UDZO "$output"
echo "Created $output for $name $version (release date $date)"
