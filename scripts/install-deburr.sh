#!/usr/bin/env bash
# Install a released deburr binary after verifying its checksum.
#
# Usage: install-deburr.sh VERSION TARGET SOURCE DEST
#
#   VERSION  release version without the leading v, for example 1.0.0
#   TARGET   Rust target triple of the archive to install
#   SOURCE   https:// URL prefix of the release assets, or a local directory
#            that holds deburr-TARGET.(tar.gz|zip) and SHA256SUMS
#   DEST     directory that receives the deburr executable
#
# The archive and SHA256SUMS are fetched over HTTPS only. A checksum
# mismatch, a missing checksum entry, or a version mismatch is an error and
# leaves DEST without a new binary.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  printf 'usage: %s VERSION TARGET SOURCE DEST\n' "${0##*/}" >&2
  exit 2
fi

version=$1
target=$2
source_location=$3
dest=$4

if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$ ]]; then
  printf 'invalid version: %s\n' "$version" >&2
  exit 2
fi

case "$target" in
  x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | x86_64-apple-darwin | aarch64-apple-darwin)
    archive="deburr-$target.tar.gz"
    binary=deburr
    ;;
  x86_64-pc-windows-msvc)
    archive="deburr-$target.zip"
    binary=deburr.exe
    ;;
  *)
    printf 'unsupported target: %s\n' "$target" >&2
    exit 2
    ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

case "$source_location" in
  https://*)
    for name in "$archive" SHA256SUMS; do
      curl --proto '=https' --proto-redir '=https' --tlsv1.2 \
        --fail --silent --show-error --location \
        --output "$work/$name" "${source_location%/}/$name"
    done
    ;;
  *://*)
    printf 'refusing non-HTTPS source: %s\n' "$source_location" >&2
    exit 2
    ;;
  *)
    if [[ ! -d $source_location ]]; then
      printf 'source is neither an https:// URL nor a directory: %s\n' "$source_location" >&2
      exit 2
    fi
    cp "$source_location/$archive" "$source_location/SHA256SUMS" "$work/"
    ;;
esac

expected=$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1 }' "$work/SHA256SUMS")
if [[ -z $expected || $expected == *$'\n'* ]]; then
  printf 'SHA256SUMS must list %s exactly once\n' "$archive" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$work/$archive" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$work/$archive" | awk '{ print $1 }')
fi

if [[ $(tr '[:upper:]' '[:lower:]' <<<"$actual") != "$(tr '[:upper:]' '[:lower:]' <<<"$expected")" ]]; then
  printf 'checksum mismatch for %s\n  expected %s\n  actual   %s\n' "$archive" "$expected" "$actual" >&2
  exit 1
fi

mkdir "$work/unpacked"
case "$archive" in
  *.tar.gz) tar -xzf "$work/$archive" -C "$work/unpacked" ;;
  *.zip) unzip -q "$work/$archive" -d "$work/unpacked" ;;
esac

if [[ ! -f $work/unpacked/$binary ]]; then
  printf '%s does not contain %s\n' "$archive" "$binary" >&2
  exit 1
fi

chmod +x "$work/unpacked/$binary"
reported=$("$work/unpacked/$binary" --version)
if [[ $reported != "deburr $version" ]]; then
  printf 'version mismatch: expected "deburr %s", binary reports "%s"\n' "$version" "$reported" >&2
  exit 1
fi

mkdir -p "$dest"
mv -f "$work/unpacked/$binary" "$dest/$binary"
printf 'installed deburr %s (%s) to %s\n' "$version" "$target" "$dest/$binary"
