#!/usr/bin/env bash
# Renders the icon master into a macOS .icns. Pass an output path, or take the
# default. Needs rsvg-convert (brew install librsvg); iconutil ships with macOS.
set -eu

ROOT="$( cd "$( dirname "${BASH_SOURCE[0]:-$0}" )/.." && pwd )"
SVG="${ROOT}/branding/icons/taqueria.svg"
OUT="${1:-${ROOT}/build/icon.icns}"

command -v rsvg-convert > /dev/null || { echo "no rsvg-convert: brew install librsvg" >&2; exit 1; }

tmp="$( mktemp -d )"
trap 'rm -rf "${tmp}"' EXIT
iconset="${tmp}/taqueria.iconset"
mkdir -p "${iconset}" "$( dirname "${OUT}" )"

render() {
  rsvg-convert -w "$1" -h "$1" "${SVG}" -o "${iconset}/$2.png"
}

render   16 icon_16x16
render   32 icon_16x16@2x
render   32 icon_32x32
render   64 icon_32x32@2x
render  128 icon_128x128
render  256 icon_128x128@2x
render  256 icon_256x256
render  512 icon_256x256@2x
render  512 icon_512x512
render 1024 icon_512x512@2x

# Not png2icns: it writes the ic08/ic09/ic10 slots as raw ARGB where macOS
# expects PNG, and the Dock renders the result blank.
iconutil -c icns "${iconset}" -o "${OUT}"

echo "wrote ${OUT}"
