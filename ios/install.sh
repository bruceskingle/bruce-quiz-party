#!/bin/sh
# Builds the iOS app and installs it on the connected iPhone, then starts it: the one command for the iOS app. The
# phone must be paired with this Mac, by cable or Wi-Fi, with Developer Mode on. Signing is set in Signing.xcconfig,
# see Signing.xcconfig.example; with no phone connected it stops before building.
#
#   ios/install.sh            the first iPhone connected
#   ios/install.sh <name>     the iPhone of that name, as `xcrun devicectl list devices` shows it

set -eu
cd "$(dirname "$0")"

if [ ! -f Signing.xcconfig ]; then
    echo "First copy Signing.xcconfig.example to Signing.xcconfig and set your Apple team in it." >&2
    exit 1
fi

devices=$(mktemp)
trap 'rm -f "$devices"' EXIT
xcrun devicectl list devices --quiet --json-output "$devices" >/dev/null
device=$(python3 - "$devices" "${1:-}" <<'PYTHON'
import json, sys
wanted = sys.argv[2]
for device in json.load(open(sys.argv[1]))["result"]["devices"]:
    properties = device.get("deviceProperties", {})
    hardware = device.get("hardwareProperties", {})
    connection = device.get("connectionProperties", {})
    if hardware.get("platform") != "iOS" or connection.get("pairingState") != "paired":
        continue
    if connection.get("tunnelState") == "unavailable":
        continue
    if wanted and properties.get("name") != wanted:
        continue
    print(hardware["udid"])
    break
PYTHON
)
if [ -z "$device" ]; then
    echo "No iPhone is connected${1:+ named $1}: connect it by cable, unlock it, and try again." >&2
    exit 1
fi

xcodebuild -project QuizParty.xcodeproj -scheme QuizParty -configuration Release \
    -destination "id=$device" -derivedDataPath build/DerivedData -allowProvisioningUpdates -quiet build
xcrun devicectl device install app --device "$device" build/DerivedData/Build/Products/Release-iphoneos/QuizParty.app
# Starting it fails while the phone is locked, which is no reason to fail the install.
if ! xcrun devicectl device process launch --device "$device" org.skingle.bruce-quiz-party >/dev/null 2>&1; then
    echo "Installed. Unlock the phone and open Quiz Party."
fi
