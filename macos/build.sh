#!/usr/bin/env bash
# Builds TimeAtlas.app for macOS, and optionally signs, notarizes and
# packages it.
#
#   macos/build.sh                      Unsigned app for this Mac (quick test)
#   macos/build.sh --developer-id       Signed with your Developer ID, notarized,
#                                       stapled, and wrapped in a .dmg
#   macos/build.sh --app-store          Signed for the Mac App Store and packaged
#                                       as a .pkg ready for Transporter
#
# Options and environment variables:
#   --universal            Build for both Apple Silicon and Intel (default for
#                          --app-store; otherwise only this Mac's chip)
#   BUNDLE_ID              Bundle identifier (default io.github.sicambium.timeatlas)
#   SIGN_IDENTITY          Codesigning identity. Defaults to the first
#                          "Developer ID Application" (or "Apple Distribution"
#                          for --app-store) certificate in your keychain.
#   INSTALLER_IDENTITY     For --app-store: "3rd Party Mac Developer Installer"
#                          or "Mac Installer Distribution" identity.
#   PROVISIONING_PROFILE   For --app-store: path to the Mac App Store
#                          provisioning profile for BUNDLE_ID.
#   NOTARY_PROFILE         For --developer-id: a notarytool keychain profile
#                          (create once with `xcrun notarytool store-credentials`).
#                          Default "TimeAtlas". Set NOTARIZE=0 to skip.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
MODE="local"
UNIVERSAL=0
for arg in "$@"; do
  case "$arg" in
    --developer-id) MODE="developer-id" ;;
    --app-store) MODE="app-store"; UNIVERSAL=1 ;;
    --universal) UNIVERSAL=1 ;;
    -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

BUNDLE_ID="${BUNDLE_ID:-io.github.sicambium.timeatlas}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
BUILD="${BUILD_NUMBER:-$(date +%Y%m%d%H%M)}"
DIST="$ROOT/dist"
APP="$DIST/TimeAtlas.app"

step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

# --- Compile --------------------------------------------------------------
export MACOSX_DEPLOYMENT_TARGET=12.0
if [[ $UNIVERSAL == 1 ]]; then
  step "Building universal binary (Apple Silicon + Intel)"
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  BIN="$DIST/timeatlas-universal"
  mkdir -p "$DIST"
  lipo -create -output "$BIN" \
    target/aarch64-apple-darwin/release/timeatlas \
    target/x86_64-apple-darwin/release/timeatlas
else
  step "Building for this Mac ($(uname -m))"
  cargo build --release
  BIN="target/release/timeatlas"
fi

# --- Assemble the bundle --------------------------------------------------
step "Assembling TimeAtlas.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/TimeAtlas"
sed -e "s/__BUNDLE_ID__/$BUNDLE_ID/" -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" \
  macos/Info.plist > "$APP/Contents/Info.plist"
cp macos/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp macos/PrivacyInfo.xcprivacy "$APP/Contents/Resources/PrivacyInfo.xcprivacy"
printf 'APPL????' > "$APP/Contents/PkgInfo"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

find_identity() {
  security find-identity -v -p codesigning | sed -n "s/.*\"\($1[^\"]*\)\".*/\1/p" | head -1
}

case "$MODE" in
  local)
    step "Ad-hoc signing for local use"
    codesign --force --deep --sign - "$APP"
    echo "Built $APP"
    echo "Open it with: open \"$APP\""
    ;;

  developer-id)
    IDENTITY="${SIGN_IDENTITY:-$(find_identity 'Developer ID Application')}"
    [[ -n "$IDENTITY" ]] || { echo "No 'Developer ID Application' certificate found in your keychain." >&2; exit 1; }
    step "Signing with: $IDENTITY"
    codesign --force --timestamp --options runtime \
      --entitlements macos/TimeAtlas.entitlements \
      --sign "$IDENTITY" "$APP"
    codesign --verify --strict --verbose=2 "$APP"

    DMG="$DIST/TimeAtlas-$VERSION.dmg"
    step "Creating $DMG"
    rm -f "$DMG"
    STAGE="$(mktemp -d)"
    cp -R "$APP" "$STAGE/"
    ln -s /Applications "$STAGE/Applications"
    hdiutil create -volname "TimeAtlas" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
    rm -rf "$STAGE"
    codesign --force --timestamp --sign "$IDENTITY" "$DMG"

    if [[ "${NOTARIZE:-1}" == 1 ]]; then
      step "Notarizing (this can take a few minutes)"
      xcrun notarytool submit "$DMG" --keychain-profile "${NOTARY_PROFILE:-TimeAtlas}" --wait
      xcrun stapler staple "$DMG"
      xcrun stapler staple "$APP"
      spctl --assess --type execute --verbose "$APP"
    fi
    echo "Done: $APP and $DMG"
    ;;

  app-store)
    IDENTITY="${SIGN_IDENTITY:-$(find_identity 'Apple Distribution')}"
    [[ -n "$IDENTITY" ]] || IDENTITY="$(find_identity '3rd Party Mac Developer Application')"
    [[ -n "$IDENTITY" ]] || { echo "No 'Apple Distribution' certificate found in your keychain." >&2; exit 1; }
    INSTALLER="${INSTALLER_IDENTITY:-$(security find-identity -v | sed -n 's/.*"\(3rd Party Mac Developer Installer[^"]*\)".*/\1/p;s/.*"\(Mac Installer Distribution[^"]*\)".*/\1/p' | head -1)}"
    [[ -n "$INSTALLER" ]] || { echo "No Mac installer certificate found in your keychain." >&2; exit 1; }
    [[ -n "${PROVISIONING_PROFILE:-}" && -f "$PROVISIONING_PROFILE" ]] || {
      echo "Set PROVISIONING_PROFILE to your Mac App Store provisioning profile." >&2; exit 1; }
    TEAM_ID="$(sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p' <<<"$IDENTITY")"
    [[ -n "$TEAM_ID" ]] || { echo "Could not read the team ID from '$IDENTITY'." >&2; exit 1; }

    step "Embedding provisioning profile"
    cp "$PROVISIONING_PROFILE" "$APP/Contents/embedded.provisionprofile"

    ENT="$(mktemp -t timeatlas-ent).plist"
    cp macos/TimeAtlas.entitlements "$ENT"
    /usr/libexec/PlistBuddy -c "Add :com.apple.application-identifier string $TEAM_ID.$BUNDLE_ID" "$ENT"
    /usr/libexec/PlistBuddy -c "Add :com.apple.developer.team-identifier string $TEAM_ID" "$ENT"

    step "Signing with: $IDENTITY"
    codesign --force --timestamp --options runtime --entitlements "$ENT" --sign "$IDENTITY" "$APP"
    codesign --verify --strict --verbose=2 "$APP"
    rm -f "$ENT"

    PKG="$DIST/TimeAtlas-$VERSION-AppStore.pkg"
    step "Building installer package with: $INSTALLER"
    productbuild --component "$APP" /Applications --sign "$INSTALLER" "$PKG"
    echo "Done: $PKG"
    echo "Upload it with Apple's Transporter app, or:"
    echo "  xcrun altool --upload-package \"$PKG\" --type macos --apple-id <id> --bundle-id $BUNDLE_ID ..."
    ;;
esac
