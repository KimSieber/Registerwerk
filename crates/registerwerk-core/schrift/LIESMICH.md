# Schrift für die Textebene

`glyphless.ttf` ist die Schrift ohne sichtbare Zeichen aus Tesseract
(`tessdata/pdf.ttf`). Tesseract legt sie in die PDFs, die es selbst erzeugt.
Registerwerk bettet sie für die unsichtbare Textebene in Scans ein
(`src/textebene.rs`).

Herkunft: https://github.com/tesseract-ocr/tesseract, Lizenz Apache 2.0.
