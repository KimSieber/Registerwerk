#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
vorschau_vergleich.py - Vorschauformate nebeneinander beurteilen (nur lesend)

Rendert die erste Seite ausgewaehlter PDFs in 1024 px (bezogen auf A4) und
legt sie als WebP Q50, WebP Q25, AVIF Q40 und AVIF Q25 ab. Dazu entsteht eine
Seite vergleich.html, die alle Varianten nebeneinander zeigt.

Voraussetzungen:  python3 -m pip install pypdfium2 pillow

Beispiele:
    # dieselben PDFs wie in beispiele_vorschau/ der Inventur (Nr. 1 bis 20)
    python3 vorschau_vergleich.py --out ~/inventur/vergleich \
        --csv ~/inventur/stichprobe.csv --anzahl 20

    # einzelne Dateien
    python3 vorschau_vergleich.py --out ~/inventur/vergleich datei1.pdf datei2.pdf
"""
import argparse, csv, html, io, os, sys

A4_BREITE_ZOLL = 8.27
VARIANTEN = [("webp50", "WebP Q50", "WEBP", dict(quality=50, method=4), "webp"),
             ("webp25", "WebP Q25", "WEBP", dict(quality=25, method=4), "webp"),
             ("avif40", "AVIF Q40", "AVIF", dict(quality=40), "avif"),
             ("avif25", "AVIF Q25", "AVIF", dict(quality=25), "avif")]


def ist_graubild(bild):
    from PIL import ImageChops
    k = bild.copy(); k.thumbnail((200, 200))
    r, g, b = k.split()
    return (max(ImageChops.difference(r, g).getextrema()) <= 12 and
            max(ImageChops.difference(g, b).getextrema()) <= 12)


def main():
    ap = argparse.ArgumentParser(description="Vorschauformate vergleichen (nur lesend).")
    ap.add_argument("pdfs", nargs="*", help="einzelne PDF-Dateien")
    ap.add_argument("--csv", help="stichprobe.csv der Inventur")
    ap.add_argument("--anzahl", type=int, default=20, help="aus der CSV die ersten N (Standard 20)")
    ap.add_argument("--breite", type=int, default=1024, help="Breite in px bezogen auf A4")
    ap.add_argument("--seiten", type=int, default=1, help="Seiten je PDF (Standard 1)")
    ap.add_argument("--out", required=True, help="Ausgabeordner (ausserhalb der Dropbox)")
    a = ap.parse_args()

    import pypdfium2 as pdfium
    from PIL import features
    if not features.check("avif"):
        sys.exit("Die installierte Pillow-Version kann kein AVIF. Bitte aktualisieren:\n"
                 "  python3 -m pip install --upgrade pillow")

    eintraege = [(os.path.splitext(os.path.basename(p))[0][:8] or "datei", p) for p in a.pdfs]
    if a.csv:
        with open(os.path.expanduser(a.csv), encoding="utf-8") as fh:
            for z in csv.DictReader(fh, delimiter=";"):
                if z["status"] == "ok" and len(eintraege) < a.anzahl + len(a.pdfs):
                    eintraege.append((f"{int(z['nr']):04d}", z["pfad"]))
    if not eintraege:
        sys.exit("Keine PDFs angegeben (Dateien nennen oder --csv verwenden).")

    out = os.path.realpath(os.path.expanduser(a.out))
    os.makedirs(out, exist_ok=True)
    dpi = a.breite / A4_BREITE_ZOLL
    zeilen, summen = [], {v[0]: 0 for v in VARIANTEN}
    n_seiten = 0

    for nr, (kennung, pfad) in enumerate(eintraege, 1):
        pfad = os.path.expanduser(pfad)
        try:
            st = os.stat(pfad)
            if st.st_size == 0:
                print(f"uebersprungen (nur online oder leer): {kennung}", file=sys.stderr)
                continue
            pdf = pdfium.PdfDocument(pfad)
        except Exception as ex:
            print(f"uebersprungen ({type(ex).__name__}): {kennung}", file=sys.stderr)
            continue
        for s in range(min(a.seiten, len(pdf))):
            bild = pdf[s].render(scale=dpi / 72).to_pil().convert("RGB")
            if ist_graubild(bild):
                bild = bild.convert("L")
            zellen = []
            for key, name, fmt, kw, ext in VARIANTEN:
                puffer = io.BytesIO(); bild.save(puffer, fmt, **kw)
                datei = f"{kennung}_s{s + 1}_{key}.{ext}"
                with open(os.path.join(out, datei), "wb") as fh:
                    fh.write(puffer.getvalue())
                summen[key] += puffer.tell()
                zellen.append(f'<td><div class="k">{name} · {puffer.tell() / 1024:.0f} KB</div>'
                              f'<img src="{html.escape(datei)}" loading="lazy"></td>')
            n_seiten += 1
            zeilen.append(f'<tr><th colspan="{len(VARIANTEN)}">Nr. {html.escape(kennung)}, Seite {s + 1}'
                          f' · {bild.width} × {bild.height} px</th></tr><tr>{"".join(zellen)}</tr>')
        pdf.close()
        print(f"  {nr}/{len(eintraege)}", file=sys.stderr)

    if not n_seiten:
        sys.exit("Keine Seite gerendert.")
    kopf = " · ".join(f"{name}: {summen[key] / n_seiten / 1024:.0f} KB je Seite"
                      for key, name, *_ in VARIANTEN)
    with open(os.path.join(out, "vergleich.html"), "w", encoding="utf-8") as fh:
        fh.write(f"""<!DOCTYPE html><html lang="de"><meta charset="utf-8">
<title>Registerwerk: Vorschauvergleich</title>
<style>body{{font:14px -apple-system,Segoe UI,sans-serif;margin:16px;background:#f3f3f3;color:#1e2b45}}
table{{border-collapse:collapse}}th{{text-align:left;padding:18px 6px 6px}}
td{{vertical-align:top;padding:6px}}img{{max-width:none;display:block;background:#fff;border:1px solid #ccc}}
.k{{font-weight:600;margin-bottom:4px}}</style>
<h1>Vorschauvergleich, {a.breite} px bezogen auf A4</h1><p>{n_seiten} Seiten. Mittelwerte: {kopf}</p>
<p>Die Bilder stehen in Originalgroesse. Zum Vergleichen waagerecht scrollen.</p>
<table>{"".join(zeilen)}</table></html>""")
    print(f"Fertig: {os.path.join(out, 'vergleich.html')}", file=sys.stderr)


if __name__ == "__main__":
    main()
