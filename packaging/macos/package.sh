#!/usr/bin/env bash
# Build, sign and, when it can, notarize BIMCraft for macOS:
#
#   $DIST/bimcraft-<version>-macos-<arch>.dmg      BIMCraft.app, to drag to Applications
#   $DIST/bimcraft-cli-<version>-macos-<arch>.zip  bimcraft-cli, the readme and the licences
#
# Usage: packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--skip-build]
#
# Signing (env):
#   MACOS_SIGN_IDENTITY  the codesign identity (a name or a SHA-1). Default "-", ad hoc: it runs on
#                        the Mac that built it, and Gatekeeper turns it down elsewhere. In CI,
#                        import-cert.sh sets it.
#   MACOS_KEYCHAIN       the keychain holding it (optional)
# Notarization (env; all three, and a real identity): APPLE_ID, APPLE_PASSWORD (an app-specific
# password), APPLE_TEAM_ID.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/macos"

ARCH=universal
SKIP_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) ARCH="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
case "$ARCH" in
  universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  aarch64) TARGETS=(aarch64-apple-darwin) ;;
  x86_64) TARGETS=(x86_64-apple-darwin) ;;
  *) echo "unknown --arch $ARCH" >&2; exit 2 ;;
esac

# As LSMinimumSystemVersion in Info.plist.in.
export MACOSX_DEPLOYMENT_TARGET=11.0
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/BIMCraft.app"
DMG="$DIST/bimcraft-$VERSION-macos-$ARCH.dmg"
CLI_ZIP="$DIST/bimcraft-cli-$VERSION-macos-$ARCH.zip"

NOTARIZE=0
if [ "$IDENTITY" = "-" ]; then
  warn "macOS: signed ad hoc (no MACOS_SIGN_IDENTITY), so not notarized"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID aren't all set, so signed but not notarized"
fi

echo "==> BIMCraft $VERSION for macOS ($ARCH), identity: $IDENTITY, notarize: $NOTARIZE"

# ---- build -------------------------------------------------------------------------------------
if [ "$SKIP_BUILD" = 0 ]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p bimcraft -p bimcraft-cli "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin"
for bin in bimcraft bimcraft-cli; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$bin"); done
  lipo -create -output "$WORK/bin/$bin" "${inputs[@]}"
  lipo -info "$WORK/bin/$bin"
done

# ---- signing -----------------------------------------------------------------------------------
sign() {
  # A real identity gets a secure timestamp; an ad hoc signature can't have one.
  local ts=(--timestamp)
  [ "$IDENTITY" = "-" ] && ts=(--timestamp=none)
  local kc=()
  [ -n "${MACOS_KEYCHAIN:-}" ] && kc=(--keychain "$MACOS_KEYCHAIN")
  codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"
}

notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file") (this can take a few minutes)"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    fi
    echo "error: notarizing $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
}

# ---- BIMCraft.app ------------------------------------------------------------------------------
echo "==> assembling $APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
# The executable and the icon carry the name Finder shows (CFBundleExecutable, CFBundleIconFile).
cp "$WORK/bin/bimcraft" "$APP/Contents/MacOS/BIMCraft"
cp "$ROOT/assets/app-icon/bimcraft.icns" "$APP/Contents/Resources/BIMCraft.icns"
# The licences, the notice and the fonts' OFL, sealed with the bundle's signature (it is signed below).
copy_docs "$APP/Contents/Resources"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" -e "s/@BUILD_SHA@/$BUILD_SHA/g" \
  "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"

# Inside out: the executable, then the bundle (no --deep on the bundle's own signature).
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/BIMCraft"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

if [ "$NOTARIZE" = 1 ]; then
  ditto -c -k --keepParent "$APP" "$WORK/BIMCraft-notarize.zip"
  notarize "$WORK/BIMCraft-notarize.zip"
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
  spctl --assess --type execute -vvv "$APP"
fi

# ---- the DMG -----------------------------------------------------------------------------------
echo "==> building $DMG"
STAGE="$WORK/dmg"
MNT="$WORK/mnt"
# hdiutil fails on CI runners now and then ("Resource busy"): try again.
retry() { local n; for n in 1 2 3 4 5; do "$@" && return 0; sleep $((n * 3)); done; return 1; }
mount_dmg() { mkdir -p "$MNT"; retry hdiutil attach "$@" -nobrowse -noautoopen -mountpoint "$MNT"; }
unmount_dmg() { retry hdiutil detach "$MNT" || hdiutil detach -force "$MNT"; }

mkdir -p "$STAGE"
ditto "$APP" "$STAGE/BIMCraft.app"
xattr -cr "$STAGE" # before the link: xattr follows it into /Applications
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
# Not `makehybrid -hfs`: it writes Finder info into the image's catalog, which turns into a
# com.apple.FinderInfo attribute on a file once the app is copied out, and `codesign --strict`
# refuses the app for it (issue 15). A plain copy into a volume carries only what the staging tree has.
# The image doesn't open its window when mounted: `bless --openfolder` is refused on Apple Silicon.
retry hdiutil create -srcfolder "$STAGE" -volname BIMCraft -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG"
# Only a real identity signs the image. An ad hoc signature on it makes Gatekeeper refuse to mount a
# downloaded copy ("Move to Trash"); unsigned, it mounts, and the app inside is still checked below.
if [ "$IDENTITY" != "-" ]; then
  sign "$DMG"
  codesign --verify --strict --verbose=2 "$DMG"
fi

# What a user gets is the app copied out of the finished image, as Finder and the updater
# (crates/update/src/macos.rs) copy it: that copy has to verify, not only the staging tree.
rm -rf "$WORK/from-dmg.app"
mount_dmg "$DMG" -readonly -noverify
ditto "$MNT/BIMCraft.app" "$WORK/from-dmg.app" || true
unmount_dmg
codesign --verify --strict --deep --verbose=2 "$WORK/from-dmg.app"
if [ "$NOTARIZE" = 1 ]; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
  spctl --assess --type open --context context:primary-signature -vvv "$DMG"
fi

# ---- the CLI -----------------------------------------------------------------------------------
echo "==> building $CLI_ZIP"
CLI_DIR="$WORK/bimcraft-cli-$VERSION-macos-$ARCH"
mkdir -p "$CLI_DIR"
cp "$WORK/bin/bimcraft-cli" "$CLI_DIR/"
copy_docs "$CLI_DIR"
sign --options runtime "$CLI_DIR/bimcraft-cli"
codesign --verify --strict --verbose=2 "$CLI_DIR/bimcraft-cli"
rm -f "$CLI_ZIP"
ditto -c -k --keepParent "$CLI_DIR" "$CLI_ZIP"
# A bare executable can't hold a stapled ticket: Gatekeeper looks its notarization up online.
if [ "$NOTARIZE" = 1 ]; then notarize "$CLI_ZIP"; fi

"$WORK/bin/bimcraft-cli" --version
"$APP/Contents/MacOS/BIMCraft" --version
echo "==> done"
ls -lh "$DMG" "$CLI_ZIP"
