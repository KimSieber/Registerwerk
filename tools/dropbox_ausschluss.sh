#!/bin/bash
# dropbox_ausschluss.sh - nimmt Build-Ordner von der Dropbox-Synchronisation aus
#
# Das Repo liegt im Cloud-Ordner. Die Build-Ordner sind gross, aendern sich
# staendig und lassen sich jederzeit neu erzeugen. Dieses Skript markiert sie
# so, dass Dropbox sie uebergeht. Die Ordner bleiben im Projekt liegen.
#
# Seit v0.1.5 auch bibliotheken/ (PDFium, Quelltext und Sprachdaten von
# Tesseract): Was das Programm als native Bibliothek laedt, soll nicht ueber
# die Dropbox von einem anderen Geraet kommen. Auf jedem Geraet laden
# tools/pdfium_holen.sh und tools/tesseract_holen.sh den Ordner selbst.
#
# Achtung bei einem Ordner, den die Dropbox schon abgleicht: Die klassische
# Dropbox entfernt ihn dann aus der Cloud und von den anderen Geraeten. Auf
# diesem Geraet bleibt er liegen.
#
# Aufruf im Terminal auf dem Mac, einmal je Geraet:
#     bash tools/dropbox_ausschluss.sh
#
# Die Markierung haengt am Ordner. Wird ein Ordner geloescht und neu angelegt
# (zum Beispiel durch "cargo clean"), das Skript bitte erneut ausfuehren.
# Es loescht nichts und kann beliebig oft laufen.

set -u
cd "$(dirname "$0")/.." || exit 1

ORDNER=("target" "node_modules" "dist" "src-tauri/gen" "bibliotheken")

if [ "$(uname)" != "Darwin" ]; then
    echo "Dieses Skript ist fuer macOS. Unter Windows:"
    echo "  Set-Content -Path <Ordner> -Stream com.dropbox.ignored -Value 1"
    exit 1
fi

for o in "${ORDNER[@]}"; do
    mkdir -p "$o"
    # Zwei Markierungen, je nach Bauart des Dropbox-Clients:
    # com.dropbox.ignored fuer den klassischen Client,
    # com.apple.fileprovider.ignore#P fuer Dropbox ueber den macOS File Provider.
    ok=0
    xattr -w com.dropbox.ignored 1 "$o" 2>/dev/null && ok=1
    xattr -w 'com.apple.fileprovider.ignore#P' 1 "$o" 2>/dev/null && ok=1
    if [ "$ok" = 1 ]; then
        echo "ausgeschlossen: $o"
    else
        echo "FEHLER: $o konnte nicht markiert werden"
    fi
done

echo
echo "Pruefung: Die Ordner sollten im Finder ein graues Minus-Symbol tragen."
