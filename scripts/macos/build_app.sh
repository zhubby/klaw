#!/bin/bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: build_app.sh --target <target> --version <version> --output-dir <dir>
EOF
}

target=""
version=""
output_dir=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target)
            target="$2"
            shift 2
            ;;
        --version)
            version="$2"
            shift 2
            ;;
        --output-dir)
            output_dir="$2"
            shift 2
            ;;
        *)
            usage >&2
            exit 1
            ;;
    esac
done

if [[ -z "$target" || -z "$version" || -z "$output_dir" ]]; then
    usage >&2
    exit 1
fi

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
binary_path="$repo_root/target/$target/release/klaw"
icon_path="$repo_root/klaw-gui/assets/icons/logo.icns"
template_path="$script_dir/Info.plist.template"
app_name="Klaw"
bundle_identifier="io.klaw.app"
app_dir="$repo_root/$output_dir/$app_name.app"
contents_dir="$app_dir/Contents"
macos_dir="$contents_dir/MacOS"
resources_dir="$contents_dir/Resources"
plist_path="$contents_dir/Info.plist"

if [[ ! -f "$binary_path" ]]; then
    echo "expected compiled binary at $binary_path" >&2
    exit 1
fi

if [[ ! -f "$icon_path" ]]; then
    echo "expected icon at $icon_path" >&2
    exit 1
fi

if [[ ! -f "$template_path" ]]; then
    echo "expected Info.plist template at $template_path" >&2
    exit 1
fi

rm -rf "$app_dir"
mkdir -p "$macos_dir" "$resources_dir"

cp "$binary_path" "$macos_dir/$app_name"
chmod 755 "$macos_dir/$app_name"
cp "$icon_path" "$resources_dir/logo.icns"

sed \
    -e "s|__APP_NAME__|$app_name|g" \
    -e "s|__EXECUTABLE_NAME__|$app_name|g" \
    -e "s|__BUNDLE_IDENTIFIER__|$bundle_identifier|g" \
    -e "s|__VERSION__|$version|g" \
    "$template_path" > "$plist_path"

plutil -lint "$plist_path" >/dev/null

[[ -x "$macos_dir/$app_name" ]]
[[ -f "$resources_dir/logo.icns" ]]
[[ -f "$plist_path" ]]

echo "built app bundle at $app_dir"
