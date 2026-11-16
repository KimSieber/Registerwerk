#!/bin/bash
# pdfium_holen.sh - laedt die PDFium-Bibliothek fuer Registerwerk
#
# PDFium ist eine C++-Bibliothek und liegt nicht im Repo. Dieses Skript holt
# einen fertigen Build von github.com/bblanchon/pdfium-binaries und legt ihn
# nach bibliotheken/pdfium/ (per .gitignore ausgeschlossen):
#
#     bibliotheken/pdfium/lib/       die Bibliothek (libpdfium.dylib, .so, pdfium.dll)
#     bibliotheken/pdfium/LICENSE    Lizenz von PDFium (BSD-3-Clause)
#     bibliotheken/pdfium/licenses/  Lizenzen der enthaltenen Fremdbibliotheken
#     bibliotheken/pdfium/VERSION    Versionsangabe des Builds
#
# Die Version ist fest eingetragen. Sie muss zur Funktionsversion passen, mit
# der pdfium-render in crates/registerwerk-core/Cargo.toml gebaut wird
# (Feature "pdfium_NNNN"). Beides immer gemeinsam aendern.
#
# Aufruf im Projektordner, einmal je Geraet:
#     bash tools/pdfium_holen.sh              Plattform dieses Geraets
#     bash tools/pdfium_holen.sh mac-univ     bestimmte Plattform
#
# Plattformen: mac-arm64, mac-x64, mac-univ, linux-x64, linux-arm64, win-x64
# Das Skript ueberschreibt einen vorhandenen Stand und loescht sonst nichts.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

VERSION="8125"
ZIEL="bibliotheken/pdfium"

if [ $# -ge 1 ]; then
    PLATTFORM="$1"
else
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)  PLATTFORM="mac-arm64" ;;
        Darwin-x86_64) PLATTFORM="mac-x64" ;;
        Linux-x86_64)  PLATTFORM="linux-x64" ;;
        Linux-aarch64) PLATTFORM="linux-arm64" ;;
        *) echo "Plattform nicht erkannt: $(uname -s)-$(uname -m). Bitte angeben, z.B. mac-arm64."; exit 1 ;;
    esac
fi

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${VERSION}/pdfium-${PLATTFORM}.tgz"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Lade PDFium ${VERSION} fuer ${PLATTFORM} ..."
curl -fsSL -o "$TMP/pdfium.tgz" "$URL"
tar xzf "$TMP/pdfium.tgz" -C "$TMP"

mkdir -p "$ZIEL/lib" "$ZIEL/licenses"
cp "$TMP"/lib/* "$ZIEL/lib/" 2>/dev/null || true
# Unter Windows liegt die DLL in bin/
if [ -d "$TMP/bin" ]; then cp "$TMP"/bin/*.dll "$ZIEL/lib/" 2>/dev/null || true; fi
cp "$TMP/LICENSE" "$ZIEL/LICENSE"
cp "$TMP"/licenses/* "$ZIEL/licenses/"
cp "$TMP/VERSION" "$ZIEL/VERSION"

echo
echo "Abgelegt in $ZIEL:"
ls -l "$ZIEL/lib"
grep BUILD "$ZIEL/VERSION"
