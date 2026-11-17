#!/bin/sh
# Makes the apps' icons from art/icon.png, a square picture whose corners may be rounded off in black, with ImageMagick
# (`brew install imagemagick`) and macOS's iconutil. Run it again after changing the picture.
#
# - iOS rounds the corners itself, so its icon is the picture full bleed, 1024 pixels, opaque, with the black corners
#   filled in with the colour beside them.
# - macOS does not, so its icon follows Apple's template: the picture in a rounded square of 824 pixels, in the middle
#   of a transparent 1024, made into the controller's icon.icns and the PNGs Dioxus asks for.
set -eu
cd "$(dirname "$0")/.."
source=art/icon.png
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Each black corner is flood-filled from its very corner with the colour a little way in.
size=$(magick identify -format %w "$source")
in=$((size * 9 / 100))
far=$((size - 1))
fill=""
for corner in "0,0 $in,$in" "$far,0 $((far - in)),$in" "0,$far $in,$((far - in))" "$far,$far $((far - in)),$((far - in))"; do
    set -- $corner
    colour=$(magick "$source" -format "%[pixel:p{$2}]" info:)
    fill="$fill -fill '$colour' -draw \"color $1 floodfill\""
done
eval magick "$source" -alpha off -fuzz 12% $fill -resize 1024x1024 "$work/full.png"

# iOS: one 1024 pixel icon, which Xcode scales for every place it is shown.
magick "$work/full.png" -alpha off ios/QuizParty/Assets.xcassets/AppIcon.appiconset/AppIcon.png

# macOS: the template's rounded square, 824 pixels with corners of 185, on a transparent 1024.
magick "$work/full.png" -resize 824x824 \
    \( -size 824x824 xc:black -fill white -draw "roundrectangle 0,0 823,823 185,185" \) \
    -alpha off -compose CopyOpacity -composite \
    -background none -compose Over -gravity center -extent 1024x1024 "PNG32:$work/mac.png"
iconset="$work/icon.iconset"
mkdir -p "$iconset" server/icons
for points in 16 32 128 256 512; do
    magick "$work/mac.png" -resize ${points}x${points} "$iconset/icon_${points}x${points}.png"
    magick "$work/mac.png" -resize $((points * 2))x$((points * 2)) "$iconset/icon_${points}x${points}@2x.png"
done
iconutil -c icns -o server/icons/icon.icns "$iconset"
cp "$iconset/icon_32x32.png" server/icons/32x32.png
cp "$iconset/icon_128x128.png" server/icons/128x128.png
cp "$iconset/icon_128x128@2x.png" server/icons/128x128@2x.png
echo "Made the iOS icon and server/icons."
