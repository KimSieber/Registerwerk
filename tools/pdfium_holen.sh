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
# Die Version ist fest eingetragen, ebenso die SHA-256-Pruefsumme jedes
# Archivs (v0.1.4). Ein Archiv mit anderer Pruefsumme wird nicht entpackt.
# Pruefsummen aus der Release-Seite (Feld "digest" der GitHub-API).
#
# pdfium-render wird mit dem Feature "pdfium_NNNN" in
# crates/registerwerk-core/Cargo.toml gebaut. Es nennt die Schnittstelle, die
# pdfium-render anspricht, und darf aelter sein als der Build hier, aber nie
# neuer. Stand v0.1.4: Build 8179, Feature pdfium_8125 (neuestes, das
# pdfium-render kennt). Bei jedem Wechsel beides pruefen.
#
# Aufruf im Projektordner, einmal je Geraet:
#     bash tools/pdfium_holen.sh              Plattform dieses Geraets
#     bash tools/pdfium_holen.sh mac-univ     bestimmte Plattform
#
# Plattformen: mac-arm64, mac-x64, mac-univ, linux-x64, linux-arm64, win-x64
# Das Skript ueberschreibt einen vorhandenen Stand und loescht sonst nichts.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

VERSION="8179"
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

# SHA-256 der Archive von chromium/8179 (veroeffentlicht 23.11.2026)
case "$PLATTFORM" in
    mac-arm64)   SUMME="44b7c6f55dd33454d0909728bd0adf6c86a8e141b36f07a6b6f50e228e1dfb46" ;;
    mac-x64)     SUMME="b29bbfe37efcad5b443fa3556c0485ec96fc1992d5b19d84df49d48fbed1d0d2" ;;
    mac-univ)    SUMME="7512d05e5efdb977f00148b9325ebd788ec936a84751cb945c7135c6a2525383" ;;
    linux-x64)   SUMME="b4766a6bf2856efbcc6c3cc7d6ae9022bdc6bb36ddc1acdd92d1f1873f773f4b" ;;
    linux-arm64) SUMME="18bb6d568257b28d14f6c3b8bfb48caf73d4615b319ca4d43b424c2a03431d9f" ;;
    win-x64)     SUMME="836a77c9053f624839c66010cddea7b675628a36b8831d2330178bb169d241b6" ;;
    *) echo "Keine Pruefsumme fuer Plattform ${PLATTFORM} hinterlegt."; exit 1 ;;
esac

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${VERSION}/pdfium-${PLATTFORM}.tgz"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Lade PDFium ${VERSION} fuer ${PLATTFORM} ..."
curl -fsSL -o "$TMP/pdfium.tgz" "$URL"

if command -v shasum >/dev/null 2>&1; then
    IST="$(shasum -a 256 "$TMP/pdfium.tgz" | cut -d' ' -f1)"
else
    IST="$(sha256sum "$TMP/pdfium.tgz" | cut -d' ' -f1)"
fi
if [ "$IST" != "$SUMME" ]; then
    echo "Pruefsumme stimmt nicht. Erwartet ${SUMME}, erhalten ${IST}. Nichts geaendert."
    exit 1
fi
echo "Pruefsumme stimmt."
tar xzf "$TMP/pdfium.tgz" -C "$TMP"

mkdir -p "$ZIEL/lib" "$ZIEL/licenses"
# Bibliothek nicht ueberschreiben, sondern ersetzen (v0.1.4): erst unter einem
# Hilfsnamen ablegen, dann umbenennen. macOS beendet sonst jeden Prozess, der
# eine an Ort und Stelle ueberschriebene signierte Bibliothek laedt (SIGKILL).
ersetzen() {
    local ziel="$ZIEL/lib/$(basename "$1")"
    cp "$1" "$ziel.neu"
    mv -f "$ziel.neu" "$ziel"
}
for f in "$TMP"/lib/*; do [ -f "$f" ] && ersetzen "$f"; done
# Unter Windows liegt die DLL in bin/
if [ -d "$TMP/bin" ]; then for f in "$TMP"/bin/*.dll; do [ -f "$f" ] && ersetzen "$f"; done; fi
cp "$TMP/LICENSE" "$ZIEL/LICENSE"
cp "$TMP"/licenses/* "$ZIEL/licenses/"
cp "$TMP/VERSION" "$ZIEL/VERSION"

echo
echo "Abgelegt in $ZIEL:"
ls -l "$ZIEL/lib"
grep BUILD "$ZIEL/VERSION"
