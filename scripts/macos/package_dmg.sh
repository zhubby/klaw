#!/bin/bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: package_dmg.sh --app-path <path> --output-path <path>
EOF
}

app_path=""
output_path=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --app-path)
            app_path="$2"
            shift 2
            ;;
        --output-path)
            output_path="$2"
            shift 2
            ;;
        *)
            usage >&2
            exit 1
            ;;
    esac
done

if [[ -z "$app_path" || -z "$output_path" ]]; then
    usage >&2
    exit 1
fi

if [[ ! -d "$app_path" ]]; then
    echo "expected app bundle at $app_path" >&2
    exit 1
fi

mkdir -p "$(dirname "$output_path")"
rm -f "$output_path"

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

cp -R "$app_path" "$temp_dir/"
ln -s /Applications "$temp_dir/Applications"

hdiutil create \
    -volname "Klaw" \
    -srcfolder "$temp_dir" \
    -ov \
    -format UDZO \
    "$output_path" >/dev/null

echo "built dmg at $output_path"
