#!/bin/zsh
# Build, sign, notarize and staple Perch for distribution outside the App
# Store, on the Mac that holds the Developer ID certificate.
#
# The order matters. A ticket is stapled to the app after Apple accepts it,
# and the disk image has to be built from the stapled app, or a Mac with no
# network at install time cannot check the app inside it. Tauri rebuilds and
# re-signs the app every time it bundles a disk image, so the image is made
# here with hdiutil once the app is final, then signed, notarized and stapled
# in its own right.
#
# Nothing secret is on the command line or in a file. The signing identity
# is picked by name from the login keychain and the notarization credential
# is the keychain profile made by `xcrun notarytool store-credentials`.
#
#   PERCH_SIGNING_IDENTITY   "Developer ID Application: Name (TEAMID)"
#   PERCH_NOTARY_PROFILE     the notarytool keychain profile, default "perch"
set -euo pipefail

identity=${PERCH_SIGNING_IDENTITY:?set PERCH_SIGNING_IDENTITY to the Developer ID Application identity}
profile=${PERCH_NOTARY_PROFILE:-perch}

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
app="$root/target/release/bundle/macos/Perch.app"
version=$(python3 -c "import json; print(json.load(open('$here/src-tauri/tauri.conf.json'))['version'])")
arch=$(uname -m)
out="$root/target/release/bundle/dmg"
dmg="$out/Perch_${version}_${arch}.dmg"

echo "== building and signing the app"
(cd "$here" && APPLE_SIGNING_IDENTITY="$identity" npx tauri build --bundles app)
codesign --verify --deep --strict "$app"

echo "== notarizing the app"
zip="$out/Perch.zip"
mkdir -p "$out"
ditto -c -k --keepParent "$app" "$zip"
xcrun notarytool submit "$zip" --keychain-profile "$profile" --wait
rm -f "$zip"
xcrun stapler staple "$app"
xcrun stapler validate "$app"

echo "== building the disk image from the stapled app"
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname Perch -srcfolder "$stage" -ov -format UDZO -quiet "$dmg"
rm -rf "$stage"
codesign --sign "$identity" --timestamp "$dmg"

echo "== notarizing the disk image"
xcrun notarytool submit "$dmg" --keychain-profile "$profile" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"

echo "== what Gatekeeper says"
spctl -a -vv -t open --context context:primary-signature "$dmg"
spctl -a -vv -t exec "$app"
shasum -a 256 "$dmg"
