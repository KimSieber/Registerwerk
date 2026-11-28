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
# Die Versionen sind fest eingetragen, die Sprachdaten auf einen festen Commit
# von tessdata und tessdata_best. Jede geladene Datei wird gegen ihre SHA-256
# geprueft, bevor sie entpackt oder abgelegt wird. Stimmt eine Pruefsumme
# nicht, bricht das Skript ab und laesst den vorhandenen Stand unveraendert.
# Nach einer Aenderung baut cargo die Bibliothek beim naechsten Lauf neu.
#
# Neue Version eintragen: Version oder Commit aendern, die Datei einmal laden
# und ihre Pruefsumme mit `shasum -a 256 <Datei>` ermitteln (v0.1.5).
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

TESSERACT_VERSION="5.5.5"
TESSERACT_SHA256="b02cf6f37cfc95be5bf7e4b2d39e51997b1ac41f7beb4e2bd16777bb67561c79"
LEPTONICA_VERSION="1.87.0"
LEPTONICA_SHA256="fa2b40c5caea96d1bb93a97486262aed8731b69ce25a84a6bf5d25323e33f631"

# Fester Stand der Sprachdaten (Commits vom 09.03.2024, Stand 28.11.2026 aktuell)
TESSDATA_COMMIT="ced78752cc61322fb554c280d13360b35b8684e4"
TESSDATA_BEST_COMMIT="e12c65a915945e4c28e237a9b52bc4a8f39a0cec"
SPRACHEN="deu eng"
ZIEL="bibliotheken/tesseract"
SAETZE="standard"
if [ "${1:-}" = "--best" ]; then SAETZE="standard best"; fi

# Pruefsumme je Datei der Sprachdaten: Satz_Sprache und LICENSE
erwartet() {
    case "$1" in
        standard_deu) echo "896b3b4956503ab9daa10285db330881b2d74b70d889b79262cc534b9ec699a4" ;;
        standard_eng) echo "daa0c97d651c19fba3b25e81317cd697e9908c8208090c94c3905381c23fc047" ;;
        best_deu)     echo "8407331d6aa0229dc927685c01a7938fc5a641d1a9524f74838cdac599f0d06e" ;;
        best_eng)     echo "8280aed0782fe27257a68ea10fe7ef324ca0f8d85bd2fd145d1c2b560bcb66ba" ;;
        LICENSE)      echo "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30" ;;
        *)            echo "" ;;
    esac
}

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Prueft die SHA-256 einer Datei, bricht bei Abweichung ab
pruefen() {
    local datei="$1" soll="$2" ist
    if [ -z "$soll" ]; then
        echo "Keine Pruefsumme eingetragen fuer $datei" >&2
        exit 1
    fi
    if command -v shasum >/dev/null 2>&1; then
        ist="$(shasum -a 256 "$datei" | cut -d' ' -f1)"
    else
        ist="$(sha256sum "$datei" | cut -d' ' -f1)"
    fi
    if [ "$ist" != "$soll" ]; then
        echo "Pruefsumme stimmt nicht: $(basename "$datei")" >&2
        echo "  erwartet $soll" >&2
        echo "  geladen  $ist" >&2
        echo "Abbruch, vorhandener Stand bleibt unveraendert." >&2
        exit 1
    fi
}

lade_quelle() {
    local name="$1" url="$2" soll="$3"
    echo "Lade $name ..."
    curl -fsSL -o "$TMP/$name.tgz" "$url"
    pruefen "$TMP/$name.tgz" "$soll"
    mkdir -p "$TMP/$name"
    tar xzf "$TMP/$name.tgz" -C "$TMP/$name" --strip-components=1
}

# Erst alles laden und pruefen, dann ablegen
for SATZ in $SAETZE; do
    case "$SATZ" in
        standard) REPO="tessdata";      COMMIT="$TESSDATA_COMMIT" ;;
        best)     REPO="tessdata_best"; COMMIT="$TESSDATA_BEST_COMMIT" ;;
    esac
    mkdir -p "$TMP/tessdata/$SATZ"
    for SPRACHE in $SPRACHEN; do
        echo "Lade $SPRACHE.traineddata ($SATZ) ..."
        curl -fsSL -o "$TMP/tessdata/$SATZ/$SPRACHE.traineddata" \
            "https://github.com/tesseract-ocr/$REPO/raw/$COMMIT/$SPRACHE.traineddata"
        pruefen "$TMP/tessdata/$SATZ/$SPRACHE.traineddata" "$(erwartet "${SATZ}_${SPRACHE}")"
    done
done
curl -fsSL -o "$TMP/LICENSE-tessdata" "https://github.com/tesseract-ocr/tessdata/raw/$TESSDATA_COMMIT/LICENSE"
pruefen "$TMP/LICENSE-tessdata" "$(erwartet LICENSE)"

lade_quelle tesseract "https://github.com/tesseract-ocr/tesseract/archive/refs/tags/${TESSERACT_VERSION}.tar.gz" "$TESSERACT_SHA256"
lade_quelle leptonica "https://github.com/DanBloomberg/leptonica/archive/refs/tags/${LEPTONICA_VERSION}.tar.gz" "$LEPTONICA_SHA256"

# Alles geprueft, jetzt ablegen
for name in tesseract leptonica; do
    mkdir -p "$ZIEL/quelle/$name"
    cp -R "$TMP/$name/." "$ZIEL/quelle/$name/"
done

for SATZ in $SAETZE; do
    mkdir -p "$ZIEL/tessdata/$SATZ"
    cp "$TMP/tessdata/$SATZ/"*.traineddata "$ZIEL/tessdata/$SATZ/"
done

cp "$ZIEL/quelle/tesseract/LICENSE" "$ZIEL/LICENSE-tesseract"
cp "$ZIEL/quelle/leptonica/leptonica-license.txt" "$ZIEL/LICENSE-leptonica"
cp "$TMP/LICENSE-tessdata" "$ZIEL/LICENSE-tessdata"

cat > "$ZIEL/VERSION" <<EOF
TESSERACT=$TESSERACT_VERSION
LEPTONICA=$LEPTONICA_VERSION
SPRACHEN=$SPRACHEN
TESSDATA=$TESSDATA_COMMIT
GELADEN=$(date +%Y-%m-%d)
EOF

echo
echo "Abgelegt in $ZIEL:"
cat "$ZIEL/VERSION"
ls -l "$ZIEL"/tessdata/*
