# Schriften

## Textebene

`glyphless.ttf` ist die Schrift ohne sichtbare Zeichen aus Tesseract
(`tessdata/pdf.ttf`). Tesseract legt sie in die PDFs, die es selbst erzeugt.
Registerwerk bettet sie für die unsichtbare Textebene in Scans ein
(`src/textebene.rs`).

Herkunft: https://github.com/tesseract-ocr/tesseract, Lizenz Apache 2.0.

## Vorschau von Textdateien

`DejaVuSansMono.ttf` ist die Schrift, mit der Registerwerk die Vorschau von
Textdateien (txt, md) zeichnet (`src/textbild.rs`, v0.0.7). Sie ist im Programm
eingebettet, damit die Vorschau auf jedem Gerät gleich aussieht.

Herkunft: https://dejavu-fonts.github.io/, Lizenz in `LICENSE-DejaVu.txt`
(Bitstream Vera, Änderungen von DejaVu gemeinfrei). Weitergabe erlaubt, der
Lizenztext muss beiliegen.
