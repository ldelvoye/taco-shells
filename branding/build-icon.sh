#!/usr/bin/env bash
# Renders the icon master into the two macOS .icns files the build needs: the
# stable icon, and a hue-rotated one so the dev app is not mistaken for it in the
# Dock. Needs rsvg-convert (brew install librsvg); iconutil ships with macOS.
set -eu

ROOT="$( cd "$( dirname "${BASH_SOURCE[0]:-$0}" )/.." && pwd )"
SVG="${ROOT}/branding/icons/taco-shells.svg"
BUILD="${ROOT}/build"

command -v rsvg-convert > /dev/null || { echo "no rsvg-convert: brew install librsvg" >&2; exit 1; }

tmp="$( mktemp -d )"
trap 'rm -rf "${tmp}"' EXIT
mkdir -p "${BUILD}"

render_icns() {
  local svg="$1"
  local out="$2"
  local iconset="${tmp}/$( basename "${out}" .icns ).iconset"
  mkdir -p "${iconset}"

  render() {
    rsvg-convert -w "$1" -h "$1" "${svg}" -o "${iconset}/$2.png"
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
  iconutil -c icns "${iconset}" -o "${out}"
  echo "wrote ${out}"
}

render_icns "${SVG}" "${BUILD}/icon.icns"

# The dev icon is the same artwork with its hues turned, generated here rather
# than committed so the two can never drift apart. The filter goes on the inner
# artwork group, not the clipped one, to keep it clear of the squircle mask.
dev_svg="${tmp}/taco-shells-dev.svg"
sed \
  -e 's|</defs>|  <filter id="dev"><feColorMatrix type="hueRotate" values="150"/></filter>\n  </defs>|' \
  -e 's|<g transform="translate(100 100) scale(0.8047)">|<g transform="translate(100 100) scale(0.8047)" filter="url(#dev)">|' \
  "${SVG}" > "${dev_svg}"

# Both substitutions are checked. Verifying only the reference would let a failed
# definition pass, and the dev icon would render identical to the stable one.
grep -q '<filter id="dev">' "${dev_svg}" || { echo "dev icon: <defs> moved, so the filter was never defined" >&2; exit 1; }
grep -q 'filter="url(#dev)"' "${dev_svg}" || { echo "dev icon: the artwork group moved, so the filter is not referenced" >&2; exit 1; }

render_icns "${dev_svg}" "${BUILD}/icon-dev.icns"
