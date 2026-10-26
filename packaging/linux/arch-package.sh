#!/usr/bin/env bash
# Build an Arch Linux package from a BIMCraft Linux tarball, with makepkg:
#
#   $DIST/bimcraft-<version>-linux-<arch>.pkg.tar.zst     sudo pacman -U <file>
#
# Usage: packaging/linux/arch-package.sh [--arch x86_64|aarch64|riscv64] [TARBALL]
#
# TARBALL defaults to $DIST/bimcraft-<version>-linux-<arch>.tar.gz, from package.sh. The architecture
# defaults to the host's and may be another's: the package only holds the tarball's files, so it
# needs no cross build. Needs makepkg (on Arch, or in the archlinux:base-devel image, as CI does)
# and an ordinary user, which makepkg insists on.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

ARCH="$(uname -m)"
TARBALL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    -h | --help) sed -n '2,12p' "$0"; exit 0 ;;
    -*) echo "unknown argument: $1" >&2; exit 2 ;;
    *) TARBALL="$1"; shift ;;
  esac
done
case "$ARCH" in
  x86_64 | riscv64) ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="bimcraft-$VERSION-linux-$ARCH"
TARBALL="${TARBALL:-$DIST/$BASENAME.tar.gz}"
[ -f "$TARBALL" ] || { echo "error: $TARBALL not found (run packaging/linux/package.sh --formats tar first)" >&2; exit 1; }
for tool in makepkg bsdtar zstd; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found (this needs Arch's makepkg)" >&2; exit 1; }
done
[ "$(id -u)" != 0 ] || { echo "error: makepkg refuses to run as root" >&2; exit 1; }

echo "==> BIMCraft $VERSION Arch package for $ARCH from $(basename "$TARBALL")"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/bimcraft-arch.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/stage"
mkdir -p "$STAGE"
tar -xzf "$TARBALL" -C "$STAGE" --strip-components=1
[ -x "$STAGE/bin/bimcraft" ] || { echo "error: $TARBALL has no bin/bimcraft" >&2; exit 1; }

# pkgver allows no hyphen: 0.5.0-rc.1 becomes 0.5.0_rc.1.
sed -e "s|@STAGE@|$STAGE|g" -e "s|@PKGVER@|${VERSION//-/_}|g" -e "s|@ARCH@|$ARCH|g" >"$WORK/PKGBUILD" <<'PKGBUILD'
pkgname=bimcraft
pkgver=@PKGVER@
pkgrel=1
pkgdesc='Open-source building information modeling: plans, sections, 3D, schedules and IFC'
arch=('@ARCH@')
url='https://github.com/IPedrax/BIMCraft'
license=('Apache-2.0')
# The window and OpenGL are loaded while it runs. Without the X11 libraries a session that isn't Wayland
# gets no window at all, so they are dependencies (an optional dependency isn't installed with the package).
depends=('glibc' 'gcc-libs' 'libxkbcommon' 'libxkbcommon-x11' 'libx11' 'libxcursor' 'libxi' 'libxrandr' 'libglvnd'
         'hicolor-icon-theme')
optdepends=(
  'wayland: native Wayland windows'
  'mesa: OpenGL drivers'
  'xdg-desktop-portal: file dialogs'
  'dbus: file dialogs through the portal'
)
conflicts=('bimcraft-bin' 'bimcraft-git')
options=('!strip' '!debug')

package() {
  mkdir -p "$pkgdir/usr"
  cp -dr --no-preserve=ownership '@STAGE@/bin' '@STAGE@/share' "$pkgdir/usr/"
  find "$pkgdir/usr" -type d -exec chmod 0755 {} +
  find "$pkgdir/usr" -type f -perm /111 -exec chmod 0755 {} +
  find "$pkgdir/usr" -type f ! -perm /111 -exec chmod 0644 {} +
  install -Dm644 -t "$pkgdir/usr/share/licenses/$pkgname" "$pkgdir"/usr/share/doc/bimcraft/LICENSE-* "$pkgdir"/usr/share/doc/bimcraft/NOTICE
  for font in ibm-plex-sans ibm-plex-mono; do
    install -Dm644 "$pkgdir/usr/share/doc/bimcraft/fonts/$font/OFL.txt" "$pkgdir/usr/share/licenses/$pkgname/fonts/$font/OFL.txt"
  done
}
PKGBUILD

# CARCH and CHOST say what the package is for, so an x86_64 host can pack an aarch64 tarball.
{
  [ ! -f /etc/makepkg.conf ] || echo 'source /etc/makepkg.conf'
  printf '%s\n' "CARCH=\"$ARCH\"" "CHOST=\"$ARCH-unknown-linux-gnu\"" "PKGEXT='.pkg.tar.zst'" 'COMPRESSZST=(zstd -c -T0 --ultra -20 -)'
} >"$WORK/makepkg.conf"

SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}"
export SOURCE_DATE_EPOCH
(cd "$WORK" && makepkg --config "$WORK/makepkg.conf" --nodeps --clean --force)

shopt -s nullglob
built=("$WORK"/*.pkg.tar.zst)
[ "${#built[@]}" -eq 1 ] || { echo "error: expected one package, found ${#built[@]}" >&2; exit 1; }
cp -f "${built[0]}" "$DIST/$BASENAME.pkg.tar.zst"
chmod 0644 "$DIST/$BASENAME.pkg.tar.zst"
echo "wrote $DIST/$BASENAME.pkg.tar.zst"
bsdtar -xOf "$DIST/$BASENAME.pkg.tar.zst" .PKGINFO
echo "==> done"
