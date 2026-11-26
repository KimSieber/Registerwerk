#!/usr/bin/env python3
"""Erzeugt die Test-PDF für die Rust-Tests (v0.1.2).

    python3 tools/testpdf_erzeugen.py crates/registerwerk-core/tests/daten/test.pdf

Zwei Seiten A4, ohne private Inhalte:

1. Seite mit eingebettetem Text (Helvetica) und einem roten Balken. Gilt als
   digital erzeugt und farbig.
2. Scan-Seite: ein Graustufenbild über die ganze Seite, ohne Text. Darauf
   steht gut lesbar Text, den Tesseract erkennen soll.

Braucht Pillow und die Schrift DejaVu Sans. Das Ergebnis liegt im Repo, das
Skript muss nur bei Änderungen neu laufen. Ohne Zufall und ohne Zeitstempel:
Gleiche Eingaben ergeben dieselbe Datei.
"""

import io
import sys
import zlib

from PIL import Image, ImageDraw, ImageFont

BREITE, HOEHE = 595, 842  # A4 in Punkt
DPI = 150

SEITE1 = [
    (72, 760, 20, "Registerwerk Testdokument"),
    (72, 720, 12, "Rechnung Nummer 4711 vom 15.03.2024"),
    (72, 700, 12, "Betrag 123,45 EUR, zahlbar bis 30.03.2024"),
    (72, 680, 12, "Diese Datei enthält keine privaten Inhalte."),
]
SEITE2 = ["Kontoauszug", "Testbank Stuttgart", "Guthaben 987,65"]

SCHRIFTEN = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/Library/Fonts/DejaVuSans.ttf",
]


def schrift(groesse):
    for s in SCHRIFTEN:
        try:
            return ImageFont.truetype(s, groesse)
        except OSError:
            pass
    sys.exit("Schrift DejaVu Sans nicht gefunden")


def text_pdf(s):
    """Text für einen PDF-String in WinAnsi (Latin-1 reicht hier)."""
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)").encode("cp1252")


def scanbild():
    b, h = BREITE * DPI // 72, HOEHE * DPI // 72
    bild = Image.new("L", (b, h), 255)
    zeichnen = ImageDraw.Draw(bild)
    f = schrift(72)
    y = 250
    for zeile in SEITE2:
        zeichnen.text((150, y), zeile, font=f, fill=0)
        y += 140
    puffer = io.BytesIO()
    bild.save(puffer, "JPEG", quality=60, optimize=True)
    return b, h, puffer.getvalue()


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    inhalt1 = b"1 0 0 RG 1 0 0 rg 72 600 200 20 re f\n0 g\n"
    for x, y, groesse, text in SEITE1:
        inhalt1 += b"BT /F1 %d Tf %d %d Td (" % (groesse, x, y) + text_pdf(text) + b") Tj ET\n"
    b, h, jpeg = scanbild()
    inhalt2 = b"q %d 0 0 %d 0 0 cm /Im1 Do Q\n" % (BREITE, HOEHE)

    def strom(daten, zusatz=b""):
        z = zlib.compress(daten, 9)
        return b"<< /Length %d /Filter /FlateDecode%s >>\nstream\n" % (len(z), zusatz) + z + b"\nendstream"

    objekte = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>"
        % (BREITE, HOEHE),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /XObject << /Im1 7 0 R >> >> /Contents 8 0 R >>"
        % (BREITE, HOEHE),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        strom(inhalt1),
        b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceGray /BitsPerComponent 8 "
        b"/Filter /DCTDecode /Length %d >>\nstream\n" % (b, h, len(jpeg)) + jpeg + b"\nendstream",
        strom(inhalt2),
        b"<< /Producer (Registerwerk tools/testpdf_erzeugen.py) /CreationDate (D:20240315120000+01'00') >>",
    ]
    pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"
    lagen = []
    for i, o in enumerate(objekte, 1):
        lagen.append(len(pdf))
        pdf += b"%d 0 obj\n" % i + o + b"\nendobj\n"
    xref = len(pdf)
    pdf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objekte) + 1)
    for l in lagen:
        pdf += b"%010d 00000 n \n" % l
    pdf += b"trailer\n<< /Size %d /Root 1 0 R /Info %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objekte) + 1, len(objekte), xref)
    with open(sys.argv[1], "wb") as f:
        f.write(pdf)
    print(f"{sys.argv[1]}: {len(pdf)} Byte")


if __name__ == "__main__":
    main()
