#!/bin/bash
# tesseract_holen.sh - laedt Quelltext und Sprachdaten von Tesseract fuer Registerwerk
#
# Tesseract wird statisch ins Programm gebaut (crates/registerwerk-ocr). Der
# Quelltext von Tesseract und Leptonica liegt nicht im Repo. Dieses Skript holt
# ihn von GitHub und legt ihn nach bibliotheken/tesseract/ (per .gitignore
# ausgeschlossen):
#
#     bibliotheken/tesseract/quelle/tesseract/   Quelltext Tesseract (Apache-2.0)
#     bibliotheken/tesseract/quelle/leptonica/   Quelltext Leptonica (BSD-2-Clause)
#     bibliotheken/tesseract/tessdata/standard/  deu, eng aus tessdata (im Einsatz)
#     bibliotheken/tesseract/tessdata/best/      deu, eng aus tessdata_best, nur mit --best
#     bibliotheken/tesseract/LICENSE-*           Lizenztexte
#     bibliotheken/tesseract/VERSION             Versionsangaben
#
# Die Versionen sind fest eingetragen. Nach einer Aenderung baut cargo die
# Bibliothek beim naechsten Lauf neu.
#
# Aufruf im Projektordner, einmal je Geraet:
#     bash tools/tesseract_holen.sh           Quelltext und Sprachdaten standard
#     bash tools/tesseract_holen.sh --best    zusaetzlich tessdata_best, nur fuer
#                                             Vergleiche mit pdf_pruefen --ocr
#
# Das Skript ueberschreibt einen vorhandenen Stand in bibliotheken/tesseract/
# und loescht sonst nichts.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

TESSERACT_VERSION="5.5.2"
LEPTONICA_VERSION="1.87.0"
SPRACHEN="deu eng"
ZIEL="bibliotheken/tesseract"
SAETZE="standard"
if [ "${1:-}" = "--best" ]; then SAETZE="standard best"; fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

lade_quelle() {
    local name="$1" url="$2"
    echo "Lade $name ..."
    curl -fsSL -o "$TMP/$name.tgz" "$url"
    mkdir -p "$TMP/$name"
    tar xzf "$TMP/$name.tgz" -C "$TMP/$name" --strip-components=1
    mkdir -p "$ZIEL/quelle/$name"
    cp -R "$TMP/$name/." "$ZIEL/quelle/$name/"
}

lade_quelle tesseract "https://github.com/tesseract-ocr/tesseract/archive/refs/tags/${TESSERACT_VERSION}.tar.gz"
lade_quelle leptonica "https://github.com/DanBloomberg/leptonica/archive/refs/tags/${LEPTONICA_VERSION}.tar.gz"

for SATZ in $SAETZE; do
    case "$SATZ" in
        standard) REPO="tessdata" ;;
        best)     REPO="tessdata_best" ;;
    esac
    mkdir -p "$ZIEL/tessdata/$SATZ"
    for SPRACHE in $SPRACHEN; do
        echo "Lade $SPRACHE.traineddata ($SATZ) ..."
        curl -fsSL -o "$ZIEL/tessdata/$SATZ/$SPRACHE.traineddata" \
            "https://github.com/tesseract-ocr/$REPO/raw/main/$SPRACHE.traineddata"
    done
done

cp "$ZIEL/quelle/tesseract/LICENSE" "$ZIEL/LICENSE-tesseract"
cp "$ZIEL/quelle/leptonica/leptonica-license.txt" "$ZIEL/LICENSE-leptonica"
curl -fsSL -o "$ZIEL/LICENSE-tessdata" "https://github.com/tesseract-ocr/tessdata/raw/main/LICENSE"

cat > "$ZIEL/VERSION" <<EOF
TESSERACT=$TESSERACT_VERSION
LEPTONICA=$LEPTONICA_VERSION
SPRACHEN=$SPRACHEN
GELADEN=$(date +%Y-%m-%d)
EOF

echo
echo "Abgelegt in $ZIEL:"
cat "$ZIEL/VERSION"
ls -l "$ZIEL"/tessdata/*
