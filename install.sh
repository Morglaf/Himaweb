#!/bin/sh
# Installe le dernier binaire HimaWeb depuis GitHub Releases.
# Usage :
#   curl -sSL https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.sh | sudo sh
#   curl -sSL https://raw.githubusercontent.com/Morglaf/Himaweb/master/install.sh | PREFIX=~/.local sh

set -eu

die() {
  printf '%s\n' "$1" >&2
  exit "${2-1}"
}

DESTDIR="${DESTDIR:-}"
PREFIX="${PREFIX:-"$DESTDIR/usr/local"}"
RELEASES_URL="https://github.com/Morglaf/Himaweb/releases"

binary=himaweb
system=$(uname -s | tr '[:upper:]' '[:lower:]')
machine=$(uname -m | tr '[:upper:]' '[:lower:]')

case $system in
  msys*|mingw*|cygwin*|win*)
    target=x86_64-windows
    binary=himaweb.exe
    ;;
  linux|freebsd)
    case $machine in
      x86_64|amd64) target=x86_64-linux ;;
      arm64|aarch64) target=aarch64-linux ;;
      *) die "Architecture non supportée: $machine ($system)" ;;
    esac
    ;;
  darwin)
    case $machine in
      x86_64) target=x86_64-darwin ;;
      arm64|aarch64) target=aarch64-darwin ;;
      *) die "Architecture non supportée: $machine ($system)" ;;
    esac
    ;;
  *)
    die "Système non supporté: $system"
    ;;
esac

tmpdir=$(mktemp -d) || die "Impossible de créer un répertoire temporaire"
trap 'rm -rf "$tmpdir"' EXIT

echo "Téléchargement de la dernière release ($target)…"
curl -sLfLo "$tmpdir/himaweb.tgz" \
  "$RELEASES_URL/latest/download/himaweb.$target.tgz" \
  || die "Échec du téléchargement. Vérifiez qu’une release existe : $RELEASES_URL"

echo "Installation du binaire…"
tar -xzf "$tmpdir/himaweb.tgz" -C "$tmpdir"

mkdir -p "$PREFIX/bin"
cp -f -- "$tmpdir/$binary" "$PREFIX/bin/$binary"
chmod +x "$PREFIX/bin/$binary"

die "$("$PREFIX/bin/$binary" --version) installé dans $PREFIX/bin" 0
