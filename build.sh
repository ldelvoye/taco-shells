#!/usr/bin/env bash
# Builds Taqueria from the VSCodium release pinned in upstream.json.
#
# -e only: the VSCodium scripts sourced below are written for `set -ex` and read
# unset variables by design, so -u aborts them before they do any work.
set -e

# BASH_SOURCE is unset under zsh, where this resolves to $PWD instead.
ROOT="$( cd "$( dirname "${BASH_SOURCE[0]:-$0}" )" && pwd )"
BUILD="${ROOT}/.build"
VSCODIUM="${BUILD}/vscodium"
LOCK="${BUILD}/build.lock"
OVERRIDES="${ROOT}/branding/product.overrides.json"
PYTHON311="${HOME}/.local/bin/python3.11"

# -e because jq -r prints the string "null" for a missing key and exits 0.
VSCODIUM_TAG="$( jq -er '.vscodium.tag' "${ROOT}/upstream.json" )"
NODE_VERSION="$( jq -er '.toolchain.node' "${ROOT}/upstream.json" )"

[ -x "${PYTHON311}" ] || { echo "no python3.11 at ${PYTHON311}" >&2; exit 1; }

# apply_patch substitutes these into every !!APP_NAME!! placeholder in the patch
# set. They do not reach product.json, which is branded separately below.
export APP_NAME="Taqueria"
export BINARY_NAME="taco"
export ORG_NAME="ldelvoye"
export GH_REPO_PATH="ldelvoye/taqueria"
export ASSETS_REPOSITORY="ldelvoye/taqueria"

export OS_NAME="osx"
export VSCODE_ARCH="arm64"
export VSCODE_QUALITY="stable"
export SHOULD_BUILD="yes"
export SHOULD_BUILD_REH="no"
export SHOULD_BUILD_REH_WEB="no"
export CI_BUILD="no"
export VSCODE_SKIP_NODE_VERSION_CHECK="yes"
export MAX_OLD_SPACE_SIZE="8192"

# node-gyp would otherwise find Python 3.14, which has no distutils.
export npm_config_python="${PYTHON311}"

echo "==> Taqueria from VSCodium ${VSCODIUM_TAG} on node ${NODE_VERSION}"

# Concurrent runs share .build and interleave silently. macOS has no flock.
mkdir -p "${BUILD}"
mkdir "${LOCK}" 2> /dev/null || {
  echo "another build holds ${LOCK}; remove it if no build is running" >&2
  exit 1
}
trap 'rmdir "${LOCK}" 2> /dev/null || true' EXIT

# Staged then moved, so the directory existing means the clone finished: git
# creates it up front, and an interrupted clone would be skipped forever.
if [ ! -d "${VSCODIUM}" ]; then
  staging="$( mktemp -d "${BUILD}/vscodium.XXXXXX" )"
  git clone --branch "${VSCODIUM_TAG}" --depth 1 \
    https://github.com/VSCodium/vscodium.git "${staging}"
  mv "${staging}" "${VSCODIUM}"
fi

# git apply refuses an already-patched tree.
rm -rf "${VSCODIUM}/vscode" "${VSCODIUM}/VSCode-darwin-${VSCODE_ARCH}"

build_icon() {
  local out="${VSCODIUM}/src/stable/resources/darwin"
  local svg="${ROOT}/branding/icons/taqueria.svg"
  local tmp iconset size
  tmp="$( mktemp -d )"
  iconset="${tmp}/taqueria.iconset"
  mkdir -p "${iconset}" "${out}"

  rsvg-convert -w   16 -h   16 "${svg}" -o "${iconset}/icon_16x16.png"
  rsvg-convert -w   32 -h   32 "${svg}" -o "${iconset}/icon_16x16@2x.png"
  rsvg-convert -w   32 -h   32 "${svg}" -o "${iconset}/icon_32x32.png"
  rsvg-convert -w   64 -h   64 "${svg}" -o "${iconset}/icon_32x32@2x.png"
  rsvg-convert -w  128 -h  128 "${svg}" -o "${iconset}/icon_128x128.png"
  rsvg-convert -w  256 -h  256 "${svg}" -o "${iconset}/icon_128x128@2x.png"
  rsvg-convert -w  256 -h  256 "${svg}" -o "${iconset}/icon_256x256.png"
  rsvg-convert -w  512 -h  512 "${svg}" -o "${iconset}/icon_256x256@2x.png"
  rsvg-convert -w  512 -h  512 "${svg}" -o "${iconset}/icon_512x512.png"
  rsvg-convert -w 1024 -h 1024 "${svg}" -o "${iconset}/icon_512x512@2x.png"

  # Not png2icns: it writes the ic08/ic09/ic10 slots as raw ARGB where macOS
  # expects PNG, and the Dock renders the result blank.
  iconutil -c icns "${iconset}" -o "${out}/code.icns"

  size="$( stat -f %z "${out}/code.icns" )"
  rm -rf "${tmp}"
  echo "==> icon written (${size} bytes)"
}

# Before prepare_vscode.sh, which copies src/stable over the checkout at its top.
build_icon

cd "${VSCODIUM}"
# shellcheck disable=SC1091
. ./get_repo.sh
# shellcheck disable=SC1091
. ./prepare_vscode.sh

# prepare_vscode.sh ends with `cd ..`.
cd vscode

# The repo root has a different product.json holding extension allow-lists.
# Merging into that one fails silently: jq reads our overrides back out and the
# identity line below still prints correctly. Only the checkout's has nameShort.
jq -e 'has("nameShort")' product.json > /dev/null \
  || { echo "wrong product.json: expected the vscode checkout's" >&2; exit 1; }

doc_count="$( jq -s 'length' "${OVERRIDES}" )"
[ "${doc_count}" = "1" ] \
  || { echo "expected one JSON object in ${OVERRIDES}, found ${doc_count}" >&2; exit 1; }

merged="$( jq -s '.[0] * .[1]' product.json "${OVERRIDES}" )"
printf '%s\n' "${merged}" > product.json

# Assigned first: a substitution inside an argument to echo does not trip set -e.
identity="$( jq -er '.nameLong + " / " + .applicationName + " / " + .dataFolderName' product.json )"
echo "==> identity: ${identity}"

# The extension cull goes here. Not earlier: VSCodium patches 22 of them.

export NODE_OPTIONS="--max-old-space-size=${MAX_OLD_SPACE_SIZE}"
export VSCODE_PUBLISH_COUNTER=1

npm run gulp vscode-min-prepack

# A win32 native module the extension ships regardless of target.
rm -f .build/extensions/ms-vscode.js-debug/src/win32-app-container-tokens.*.node

npm run copy-policy-dto --prefix build
node build/lib/policies/policyGenerator.ts build/lib/policies/policyData.jsonc darwin
npm run gulp "vscode-darwin-${VSCODE_ARCH}-min-packing"

# build_cli.sh deliberately not sourced: it builds the 21MB tunnel binary.

cd ..

APP="${VSCODIUM}/VSCode-darwin-${VSCODE_ARCH}/${APP_NAME}.app"
[ -d "${APP}" ] || { echo "no bundle at ${APP}" >&2; exit 1; }

# Around 300MB, and discarded by VSCodium's own release pipeline.
maps="$( find "${APP}" -name '*.map' -print -delete | wc -l | tr -d ' ' )"
echo "==> stripped ${maps} source maps"

# arm64 Gatekeeper refuses unsigned bundles.
codesign --force --deep --sign - "${APP}"
codesign --verify --verbose=2 "${APP}"

echo
echo "==> ${APP}"
du -sh "${APP}"
