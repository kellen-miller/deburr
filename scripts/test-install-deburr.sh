#!/usr/bin/env bash
# Exercise scripts/install-deburr.sh against archives built from this checkout.
#
# Builds deburr for the host target with a test version, packages it the way
# the release workflow does, then checks that the install script accepts a
# good checksum and rejects a corrupted checksum, a wrong version, and a
# non-HTTPS source. Run from the repository root.
set -euo pipefail

version=0.0.0-install-test
host=$(rustc -vV | sed -n 's/^host: //p')

case "$host" in
  *-pc-windows-msvc)
    binary=deburr.exe
    archive="deburr-$host.zip"
    ;;
  *)
    binary=deburr
    archive="deburr-$host.tar.gz"
    ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir "$work/dist" "$work/pack"
DEBURR_VERSION=$version cargo build --release --locked --target-dir "$work/target"
cp "$work/target/release/$binary" "$work/pack/$binary"

case "$archive" in
  *.zip)
    (cd "$work/pack" && powershell -NoProfile -Command "Compress-Archive -Path $binary -DestinationPath ../dist/$archive")
    ;;
  *)
    tar -czf "$work/dist/$archive" -C "$work/pack" "$binary"
    ;;
esac

(cd "$work/dist" && sha256sum "$archive" > SHA256SUMS 2>/dev/null || shasum -a 256 "$archive" > SHA256SUMS)

printf '== good checksum installs\n'
bash scripts/install-deburr.sh "$version" "$host" "$work/dist" "$work/good"
test "$("$work/good/$binary" --version)" = "deburr $version"

printf '== wrong version is refused\n'
if bash scripts/install-deburr.sh 9.9.9 "$host" "$work/dist" "$work/wrong-version"; then
  printf 'wrong version was accepted\n' >&2
  exit 1
fi
test ! -e "$work/wrong-version/$binary"

printf '== corrupted checksum is refused\n'
sed 's/^./0/' "$work/dist/SHA256SUMS" > "$work/dist/SHA256SUMS.corrupt"
mv "$work/dist/SHA256SUMS.corrupt" "$work/dist/SHA256SUMS"
if bash scripts/install-deburr.sh "$version" "$host" "$work/dist" "$work/corrupt"; then
  printf 'corrupted checksum was accepted\n' >&2
  exit 1
fi
test ! -e "$work/corrupt/$binary"

printf '== non-HTTPS source is refused\n'
if bash scripts/install-deburr.sh "$version" "$host" http://example.invalid/releases "$work/http"; then
  printf 'plain HTTP source was accepted\n' >&2
  exit 1
fi

printf 'install script checks passed\n'
