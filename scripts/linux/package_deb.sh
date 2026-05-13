#!/bin/bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: package_deb.sh --binary <path> --version <version> --output-dir <dir> [options]

Options:
  --arch <deb-arch>        Debian architecture. Defaults to dpkg or host arch.
  --package <name>         Debian package name. Defaults to klaw.
  --maintainer <value>     Maintainer field. Defaults to Klaw Maintainers <maintainers@klaw.local>.
  --depends <value>        Depends field override.
EOF
}

binary_path=""
version=""
output_dir=""
deb_arch=""
package_name="klaw"
maintainer="Klaw Maintainers <maintainers@klaw.local>"
depends="libc6 (>= 2.31), libgcc-s1, libgtk-3-0, libgdk-pixbuf-2.0-0, libatk1.0-0, libpango-1.0-0, libcairo2"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary)
            binary_path="$2"
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
        --arch)
            deb_arch="$2"
            shift 2
            ;;
        --package)
            package_name="$2"
            shift 2
            ;;
        --maintainer)
            maintainer="$2"
            shift 2
            ;;
        --depends)
            depends="$2"
            shift 2
            ;;
        *)
            usage >&2
            exit 1
            ;;
    esac
done

if [[ -z "$binary_path" || -z "$version" || -z "$output_dir" ]]; then
    usage >&2
    exit 1
fi

if ! command -v dpkg-deb >/dev/null 2>&1; then
    echo "error: dpkg-deb not found; install dpkg-dev or dpkg" >&2
    exit 1
fi

if [[ ! -f "$binary_path" ]]; then
    echo "expected compiled binary at $binary_path" >&2
    exit 1
fi

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
icon_path="$repo_root/klaw-gui/assets/icons/Icon-macOS-Default-1024x1024@1x.png"

if [[ ! -f "$icon_path" ]]; then
    echo "expected icon at $icon_path" >&2
    exit 1
fi

if [[ -z "$deb_arch" ]]; then
    if command -v dpkg >/dev/null 2>&1; then
        deb_arch="$(dpkg --print-architecture)"
    else
        case "$(uname -m)" in
            x86_64) deb_arch="amd64" ;;
            aarch64 | arm64) deb_arch="arm64" ;;
            armv7l | armv7) deb_arch="armhf" ;;
            *)
                echo "could not infer Debian architecture; pass --arch" >&2
                exit 1
                ;;
        esac
    fi
fi

case "$deb_arch" in
    amd64 | arm64 | armhf) ;;
    *)
        echo "warning: packaging with unverified Debian architecture '$deb_arch'" >&2
        ;;
esac

if [[ "$output_dir" = /* ]]; then
    output_path="$output_dir"
else
    output_path="$repo_root/$output_dir"
fi

mkdir -p "$output_path"

staging_dir="$(mktemp -d)"
trap 'rm -rf "$staging_dir"' EXIT

package_root="$staging_dir/${package_name}_${version}_${deb_arch}"
debian_dir="$package_root/DEBIAN"
bin_dir="$package_root/usr/bin"
desktop_dir="$package_root/usr/share/applications"
icon_dir="$package_root/usr/share/icons/hicolor/1024x1024/apps"
doc_dir="$package_root/usr/share/doc/$package_name"

mkdir -p "$debian_dir" "$bin_dir" "$desktop_dir" "$icon_dir" "$doc_dir"

install -m 755 "$binary_path" "$bin_dir/klaw"
install -m 644 "$icon_path" "$icon_dir/klaw.png"

cat > "$desktop_dir/io.klaw.Klaw.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Klaw
Comment=Klaw desktop workbench
Exec=klaw gui
Icon=klaw
Terminal=false
Categories=Development;Utility;
StartupWMClass=Klaw
EOF

cat > "$doc_dir/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: klaw
License: MIT
EOF

installed_size="$(du -sk "$package_root/usr" | awk '{print $1}')"

cat > "$debian_dir/control" <<EOF
Package: $package_name
Version: $version
Section: utils
Priority: optional
Architecture: $deb_arch
Maintainer: $maintainer
Depends: $depends
Installed-Size: $installed_size
Homepage: https://github.com/zhubby/klaw
Description: Klaw local agent workbench
 Klaw provides a desktop GUI and CLI for local agent workflows, runtime
 integrations, tools, sessions, scheduling, and observability.
EOF

(
    cd "$package_root"
    find . -type d -exec chmod 755 {} +
    find usr -type f -exec chmod 644 {} +
    chmod 755 usr/bin/klaw
    find usr -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums
)

deb_name="${package_name}_${version}_${deb_arch}.deb"
deb_path="$output_path/$deb_name"
rm -f "$deb_path"

if dpkg-deb --help 2>/dev/null | grep -q -- '--root-owner-group'; then
    dpkg-deb --build --root-owner-group "$package_root" "$deb_path" >/dev/null
else
    dpkg-deb --build "$package_root" "$deb_path" >/dev/null
fi

echo "built deb at $deb_path"
