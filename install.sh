#!/bin/sh
set -eu

REPO_URL="https://github.com/ldelvoye/taco-shells"
APP_NAME="Taco Shells.app"

fail() {
  echo "taco-shells: $1" >&2
  exit 1
}

check_platform() {
  system=$(uname -s)
  if [ "$system" != "Darwin" ]; then
    fail "Taco Shells only runs on macOS."
  fi
  # uname -m reports x86_64 in a terminal running under Rosetta.
  arm64=$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)
  if [ "$arm64" != "1" ]; then
    fail "Taco Shells needs a Mac with Apple silicon."
  fi
}

choose_install_dir() {
  if [ -n "${TACO_SHELLS_INSTALL_DIR:-}" ]; then
    install_dir="$TACO_SHELLS_INSTALL_DIR"
  else
    install_dir="/Applications"
  fi
  if [ ! -d "$install_dir" ] || [ ! -w "$install_dir" ]; then
    fail "$install_dir is not a writable directory."
  fi
}

refuse_if_running() {
  if pgrep -f "$install_dir/$APP_NAME/Contents/MacOS/" >/dev/null 2>&1; then
    fail "Taco Shells is running. Quit it and run this again, or use its Update menu."
  fi
}

latest_zip_url() {
  latest_page="$REPO_URL/releases/latest"
  final_url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$latest_page") ||
    fail "could not look up the latest release."
  tag="${final_url##*/}"
  case "$tag" in
    v*) ;;
    *) fail "could not find a stable release to install." ;;
  esac
  version="${tag#v}"
  zip_url="$REPO_URL/releases/download/$tag/taco-shells-$version-arm64.zip"
}

choose_zip_url() {
  if [ -n "${TACO_SHELLS_ZIP_URL:-}" ]; then
    zip_url="$TACO_SHELLS_ZIP_URL"
  else
    latest_zip_url
  fi
}

download_and_unpack() {
  work_dir=$(mktemp -d)
  trap 'rm -rf "$work_dir"' EXIT
  zip_file="$work_dir/taco-shells.zip"
  unpacked="$work_dir/unpacked"
  echo "Downloading $zip_url"
  curl -fSL --progress-bar -o "$zip_file" "$zip_url" ||
    fail "the download failed."
  ditto -x -k "$zip_file" "$unpacked" ||
    fail "the download is not a valid zip."
  if [ ! -d "$unpacked/$APP_NAME" ]; then
    fail "the download does not contain $APP_NAME."
  fi
}

replace_install() {
  target="$install_dir/$APP_NAME"
  rm -rf "$target"
  mv "$unpacked/$APP_NAME" "$target"
  echo "Installed $APP_NAME to $install_dir."
  open "$target"
}

main() {
  check_platform
  choose_install_dir
  refuse_if_running
  choose_zip_url
  download_and_unpack
  replace_install
}

# Called on the last line so a download cut off halfway runs nothing.
main "$@"
