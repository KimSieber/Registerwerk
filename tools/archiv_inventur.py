#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
archiv_inventur.py - Inventur und Stichprobe fuer das Dokumentenarchiv

Das Skript arbeitet NUR LESEND. Es aendert, verschiebt und loescht nichts
im Archiv. Ergebnisse landen ausschliesslich im Ausgabeordner (--out).

Stufe 1 - Inventur (immer):
    Laeuft durch die angegebenen Verzeichnisse und liest nur Metadaten
    (Name, Groesse, Datum). Dabei wird NICHTS aus der Cloud geladen.
    Ergebnis: Anzahl/GB je Dateityp und Hauptordner, lokal vs. "nur online",
    Groessenverteilung.

Stufe 2 - PDF-Stichprobe (abschaltbar mit --nur-inventur):
    Oeffnet eine Zufallsauswahl von PDFs, die LOKAL vorhanden sind
    ("nur online"-Dateien werden uebersprungen, damit nichts geladen wird).
    Misst: Seitenzahl, Textebene, Bildtyp (1-Bit/Grau/Farbe), Bytes je Seite,
    PDF-Erstellungsdatum und die Groesse von Vorschauen in mehreren
    Aufloesungen und Formaten.

Voraussetzungen:
    python3 -m pip install pypdfium2 pillow

Beispiele:
    # erst nur die Inventur (schnell, liest keine Dateiinhalte)
    python3 archiv_inventur.py --nur-inventur --out ~/inventur \
        /pfad/zum/verzeichnis1 /pfad/zum/verzeichnis2

    # danach mit Stichprobe
    python3 archiv_inventur.py --out ~/inventur --stichprobe 500 \
        /pfad/zum/verzeichnis1 /pfad/zum/verzeichnis2
"""

import argparse
import csv
import io
import os
import random
import re
import statistics
import sys
import time
from collections import Counter, defaultdict
from datetime import datetime

SF_DATALESS = 0x40000000          # macOS: Datei ist nur ein Cloud-Platzhalter
A4_BREITE_ZOLL = 8.27             # Vorschau-"Breite" bezieht sich auf A4
STANDARD_AUSSCHLUSS_NAMEN = {".dropbox.cache", ".dropbox", ".DS_Store", ".git",
                             "node_modules", ".Trash", "Icon\r"}
GROESSENKLASSEN = [
    (50 * 1024, "bis 50 KB"),
    (200 * 1024, "50-200 KB"),
    (500 * 1024, "200-500 KB"),
    (1024 * 1024, "500 KB-1 MB"),
    (5 * 1024 * 1024, "1-5 MB"),
    (20 * 1024 * 1024, "5-20 MB"),
    (100 * 1024 * 1024, "20-100 MB"),
    (float("inf"), "ueber 100 MB"),
]


# ---------------------------------------------------------------- Hilfen

def gb(n):
    return n / 1024 ** 3


def kb(n):
    return n / 1024


def ist_nur_online(st):
    """True, wenn die Datei nur als Cloud-Platzhalter vorliegt."""
    if getattr(st, "st_flags", 0) & SF_DATALESS:
        return True
    # Dropbox-Platzhalter melden je nach Version 0 Byte oder die echte
    # Groesse, belegen aber keinen Plattenplatz. Eine wirklich leere Datei
    # ist davon nicht zu unterscheiden und wird hier mitgezaehlt.
    return getattr(st, "st_blocks", 1) == 0 or st.st_size == 0


def groessenklasse(n):
    for grenze, name in GROESSENKLASSEN:
        if n <= grenze:
            return name
    return GROESSENKLASSEN[-1][1]


def endung(name):
    ext = os.path.splitext(name)[1].lower()
    return ext if ext else "(ohne Endung)"


def perzentil(werte, p):
    if not werte:
        return 0
    s = sorted(werte)
    i = min(len(s) - 1, max(0, int(round(p / 100 * (len(s) - 1)))))
    return s[i]


# ---------------------------------------------------------------- Stufe 1

def inventur(wurzeln, ausschluss_pfade):
    """Liefert Liste (pfad, groesse, mtime, nur_online, wurzel, hauptordner)."""
    dateien = []
    fehler = []
    ausschluss = [os.path.realpath(os.path.expanduser(p)) for p in ausschluss_pfade]
    letzter_stand = time.time()

    for wurzel in wurzeln:
        wurzel_real = os.path.realpath(os.path.expanduser(wurzel))
        if not os.path.isdir(wurzel_real):
            fehler.append((wurzel, "Verzeichnis nicht gefunden"))
            continue
        stapel = [wurzel_real]
        while stapel:
            ordner = stapel.pop()
            try:
                with os.scandir(ordner) as it:
                    for e in it:
                        if e.name in STANDARD_AUSSCHLUSS_NAMEN:
                            continue
                        pfad = e.path
                        try:
                            if e.is_symlink():
                                continue
                            if e.is_dir(follow_symlinks=False):
                                if any(pfad == a or pfad.startswith(a + os.sep)
                                       for a in ausschluss):
                                    continue
                                stapel.append(pfad)
                                continue
                            if not e.is_file(follow_symlinks=False):
                                continue
                            st = e.stat(follow_symlinks=False)
                        except OSError as ex:
                            fehler.append((pfad, str(ex)))
                            continue
                        rel = os.path.relpath(pfad, wurzel_real)
                        teile = rel.split(os.sep)
                        haupt = teile[0] if len(teile) > 1 else "(direkt)"
                        dateien.append((pfad, st.st_size, st.st_mtime,
                                        ist_nur_online(st), wurzel_real, haupt))
                        if time.time() - letzter_stand > 5:
                            print(f"  ... {len(dateien):,} Dateien gezaehlt",
                                  file=sys.stderr)
                            letzter_stand = time.time()
            except OSError as ex:
                fehler.append((ordner, str(ex)))
    return dateien, fehler


# ---------------------------------------------------------------- Stufe 2

def otsu_schwelle(bild_l):
    """Schwellwert fuer die 1-Bit-Wandlung (Verfahren nach Otsu)."""
    h = bild_l.histogram()
    gesamt = sum(h)
    summe = sum(i * h[i] for i in range(256))
    sum_b = w_b = 0
    max_var, schwelle = 0.0, 128
    for i in range(256):
        w_b += h[i]
        if w_b == 0:
            continue
        w_f = gesamt - w_b
        if w_f == 0:
            break
        sum_b += i * h[i]
        m_b = sum_b / w_b
        m_f = (summe - sum_b) / w_f
        var = w_b * w_f * (m_b - m_f) ** 2
        if var > max_var:
            max_var, schwelle = var, i
    return schwelle


def ist_graubild(bild_rgb):
    from PIL import ImageChops
    klein = bild_rgb.copy()
    klein.thumbnail((200, 200))
    r, g, b = klein.split()
    return (max(ImageChops.difference(r, g).getextrema()) <= 12 and
            max(ImageChops.difference(g, b).getextrema()) <= 12)


def kodiert(bild, fmt, **kw):
    puffer = io.BytesIO()
    bild.save(puffer, fmt, **kw)
    return puffer.getvalue()


def wortqualitaet(text):
    """Grobes Mass fuer die OCR-Guete: Anteil 'wortartiger' Tokens."""
    tokens = re.findall(r"\S+", text)
    if not tokens:
        return None
    gut = sum(1 for t in tokens
              if re.fullmatch(r"[A-Za-zÄÖÜäöüß]{2,}[.,;:!?]?", t)
              or re.fullmatch(r"[\d.,:/%€+-]+", t))
    return gut / len(tokens)


def pdf_untersuchen(pfad, groesse, breiten, max_seiten, avif, beispiel_ordner,
                    text_ordner, laufnr):
    import pypdfium2 as pdfium
    from PIL import Image

    erg = {"pfad": pfad, "bytes": groesse, "status": "ok"}
    try:
        pdf = pdfium.PdfDocument(pfad)
    except Exception as ex:                       # z. B. Passwortschutz
        erg["status"] = f"nicht lesbar: {type(ex).__name__}"
        return erg, []

    seitenwerte = []
    try:
        n = len(pdf)
        erg["seiten"] = n
        try:
            meta = pdf.get_metadata_dict()
        except Exception:
            meta = {}
        erg["erstellt"] = meta.get("CreationDate", "")
        erg["producer"] = (meta.get("Producer") or meta.get("Creator") or "")[:60]

        mit_text = ohne_text = scan_seiten = 0
        zeichen = 0
        bildtypen = Counter()
        filter_ = Counter()
        dpi_werte = []
        texte = []

        for i in range(n):
            seite = pdf[i]
            # --- Textebene
            try:
                tp = seite.get_textpage()
                text = tp.get_text_range()
                tp.close()
            except Exception:
                text = ""
            z = len(text.strip())
            zeichen += z
            if z >= 20:
                mit_text += 1
            else:
                ohne_text += 1
            if len(texte) < 3:
                texte.append(text)

            # --- groesstes eingebettetes Bild der Seite
            groesstes = None
            try:
                for obj in seite.get_objects(
                        filter=[pdfium.raw.FPDF_PAGEOBJ_IMAGE], max_depth=3):
                    m = obj.get_metadata()
                    px = m.width * m.height
                    if groesstes is None or px > groesstes[0]:
                        try:
                            f = obj.get_filters()
                        except Exception:
                            f = []
                        groesstes = (px, m.bits_per_pixel, m.horizontal_dpi, f)
            except Exception:
                pass
            if groesstes and groesstes[0] >= 300_000:
                scan_seiten += 1
                bpp = groesstes[1]
                typ = ("1-Bit" if bpp == 1 else "Grau 8-Bit" if bpp == 8
                       else "Farbe" if bpp >= 24 else f"{bpp}-Bit")
                bildtypen[typ] += 1
                filter_[groesstes[3][-1] if groesstes[3] else "(keiner)"] += 1
                if groesstes[2]:
                    dpi_werte.append(groesstes[2])

            # --- Vorschauen (nur die ersten max_seiten Seiten)
            if i < max_seiten:
                werte = {"seite": i + 1}
                grau = None
                for b in breiten:
                    dpi = b / A4_BREITE_ZOLL
                    bild = seite.render(scale=dpi / 72).to_pil().convert("RGB")
                    if grau is None:
                        grau = ist_graubild(bild)
                    if grau:
                        bild = bild.convert("L")
                    w50 = kodiert(bild, "WEBP", quality=50, method=4)
                    werte[f"webp50_{b}"] = len(w50)
                    werte[f"webp25_{b}"] = len(kodiert(bild, "WEBP", quality=25,
                                                       method=4))
                    if avif:
                        werte[f"avif25_{b}"] = len(kodiert(bild, "AVIF",
                                                           quality=25))
                    if beispiel_ordner and i == 0:
                        with open(os.path.join(
                                beispiel_ordner,
                                f"{laufnr:04d}_{b}px.webp"), "wb") as fh:
                            fh.write(w50)
                # 1-Bit-Variante in voller Lesequalitaet (200 dpi)
                voll = seite.render(scale=200 / 72).to_pil().convert("L")
                sw = voll.point(lambda v, s=otsu_schwelle(voll):
                                255 if v > s else 0).convert("1")
                werte["sw1bit_200dpi"] = len(kodiert(sw, "TIFF",
                                                     compression="group4"))
                werte["grau"] = grau
                seitenwerte.append(werte)
            seite.close()

        erg.update({
            "seiten_mit_text": mit_text, "seiten_ohne_text": ohne_text,
            "scan_seiten": scan_seiten, "zeichen": zeichen,
            "bildtypen": dict(bildtypen), "filter": dict(filter_),
            "dpi": round(statistics.median(dpi_werte)) if dpi_werte else "",
            "wortqualitaet": wortqualitaet(" ".join(texte)),
        })
        if text_ordner and any(t.strip() for t in texte):
            with open(os.path.join(text_ordner, f"{laufnr:04d}.txt"), "w",
                      encoding="utf-8") as fh:
                fh.write(f"# {pfad}\n\n" + "\n\n--- Seite ---\n\n".join(texte))
    except Exception as ex:
        erg["status"] = f"Fehler: {type(ex).__name__}: {ex}"
    finally:
        try:
            pdf.close()
        except Exception:
            pass
    return erg, seitenwerte


# ---------------------------------------------------------------- Bericht

def tabelle(kopf, zeilen):
    out = ["| " + " | ".join(kopf) + " |",
           "|" + "|".join("---" for _ in kopf) + "|"]
    out += ["| " + " | ".join(str(z) for z in zeile) + " |" for zeile in zeilen]
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser(
        description="Inventur und PDF-Stichprobe fuer das Dokumentenarchiv "
                    "(nur lesend).")
    ap.add_argument("verzeichnisse", nargs="+", help="zu pruefende Verzeichnisse")
    ap.add_argument("--out", required=True,
                    help="Ausgabeordner (ausserhalb der Dropbox!)")
    ap.add_argument("--exclude", action="append", default=[],
                    help="auszuschliessendes Verzeichnis (mehrfach moeglich)")
    ap.add_argument("--nur-inventur", action="store_true",
                    help="keine Dateiinhalte lesen, nur zaehlen")
    ap.add_argument("--stichprobe", type=int, default=500,
                    help="Anzahl zufaelliger PDFs (Standard 500)")
    ap.add_argument("--max-seiten", type=int, default=10,
                    help="je PDF hoechstens so viele Seiten rendern (Standard 10)")
    ap.add_argument("--breiten", default="640,1024,1600",
                    help="Vorschau-Breiten in px, bezogen auf A4")
    ap.add_argument("--beispiele", type=int, default=20,
                    help="von so vielen PDFs Vorschau und Text ablegen")
    ap.add_argument("--seed", type=int, default=2026,
                    help="Startwert der Zufallsauswahl (wiederholbar)")
    args = ap.parse_args()

    out = os.path.realpath(os.path.expanduser(args.out))
    for w in args.verzeichnisse:
        wr = os.path.realpath(os.path.expanduser(w))
        if out == wr or out.startswith(wr + os.sep):
            sys.exit("Abbruch: Der Ausgabeordner liegt in einem der geprueften "
                     "Verzeichnisse. Bitte einen Ordner ausserhalb waehlen.")
    if "dropbox" in out.lower():
        print("Hinweis: Der Ausgabeordner scheint in der Dropbox zu liegen. "
              "Die Beispiele enthalten Dokumentinhalte.", file=sys.stderr)
    os.makedirs(out, exist_ok=True)

    breiten = [int(b) for b in args.breiten.split(",")]
    start = time.time()

    # ---------------- Stufe 1
    print("Stufe 1: Inventur (liest nur Metadaten) ...", file=sys.stderr)
    dateien, fehler = inventur(args.verzeichnisse, args.exclude)
    if not dateien:
        sys.exit("Keine Dateien gefunden.")

    n_ges = len(dateien)
    b_ges = sum(d[1] for d in dateien)
    n_online = sum(1 for d in dateien if d[3])
    b_online = sum(d[1] for d in dateien if d[3])

    je_typ = defaultdict(lambda: [0, 0, 0])        # Anzahl, Bytes, online
    je_ordner = defaultdict(lambda: [0, 0, 0])
    je_klasse = defaultdict(lambda: [0, 0])
    je_jahr = defaultdict(lambda: [0, 0])
    for pfad, groesse, mtime, online, wurzel, haupt in dateien:
        t = je_typ[endung(pfad)]
        t[0] += 1; t[1] += groesse; t[2] += online
        o = je_ordner[os.path.basename(wurzel) + "/" + haupt]
        o[0] += 1; o[1] += groesse; o[2] += online
        k = je_klasse[groessenklasse(groesse)]
        k[0] += 1; k[1] += groesse
        j = je_jahr[datetime.fromtimestamp(mtime).year]
        j[0] += 1; j[1] += groesse

    pdfs = [d for d in dateien if endung(d[0]) == ".pdf"]
    pdf_bytes = sum(d[1] for d in pdfs)

    b = []
    b.append("# Inventur Dokumentenarchiv\n")
    b.append(f"Erstellt: {datetime.now():%Y-%m-%d %H:%M}  ")
    b.append(f"Geprueft: {len(args.verzeichnisse)} Verzeichnis(se)\n")
    for w_ in args.verzeichnisse:
        wr_ = os.path.realpath(os.path.expanduser(w_))
        b.append(f"- `{wr_}`: "
                 f"{sum(1 for d in dateien if d[4] == wr_):,} Dateien")
    b.append("")
    b.append("## Ueberblick\n")
    b.append(tabelle(["Kennzahl", "Wert"], [
        ["Dateien gesamt", f"{n_ges:,}"],
        ["Volumen gesamt", f"{gb(b_ges):.1f} GB"],
        ["davon lokal vorhanden", f"{n_ges - n_online:,} Dateien / "
                                  f"{gb(b_ges - b_online):.1f} GB"],
        ["davon nur online oder leer (Groesse nicht lesbar)",
         f"{n_online:,} Dateien"],
        ["PDF-Dateien", f"{len(pdfs):,} / {gb(pdf_bytes):.1f} GB"],
        ["davon PDFs nur online oder leer",
         f"{sum(1 for d in pdfs if d[3]):,}"],
        ["nicht lesbare Eintraege", f"{len(fehler):,}"],
    ]))

    b.append("\n## Dateitypen (Top 30 nach Volumen)\n")
    zeilen = sorted(je_typ.items(), key=lambda x: -x[1][1])[:30]
    b.append(tabelle(["Typ", "Anzahl", "GB", "Anteil Volumen", "nur online"], [
        [typ, f"{v[0]:,}", f"{gb(v[1]):.2f}", f"{v[1] / b_ges:.1%}", f"{v[2]:,}"]
        for typ, v in zeilen]))

    b.append("\n## Groessenverteilung aller Dateien\n")
    b.append(tabelle(["Klasse", "Anzahl", "Anteil Anzahl", "GB", "Anteil Volumen"], [
        [name, f"{je_klasse[name][0]:,}", f"{je_klasse[name][0] / n_ges:.1%}",
         f"{gb(je_klasse[name][1]):.2f}", f"{je_klasse[name][1] / b_ges:.1%}"]
        for _, name in GROESSENKLASSEN]))

    groessen = sorted((d[1] for d in dateien), reverse=True)
    b.append("\n## Wie stark bestimmen grosse Dateien das Volumen?\n")
    zeilen = []
    for anteil in (0.01, 0.05, 0.10, 0.25, 0.50):
        k = max(1, int(n_ges * anteil))
        zeilen.append([f"groesste {anteil:.0%} der Dateien",
                       f"{gb(sum(groessen[:k])):.1f} GB",
                       f"{sum(groessen[:k]) / b_ges:.1%}"])
    b.append(tabelle(["Auswahl", "Volumen", "Anteil am Gesamtvolumen"], zeilen))

    b.append("\n## Hauptordner (Top 40 nach Volumen)\n")
    zeilen = sorted(je_ordner.items(), key=lambda x: -x[1][1])[:40]
    b.append(tabelle(["Ordner", "Anzahl", "GB", "nur online"], [
        [name, f"{v[0]:,}", f"{gb(v[1]):.2f}", f"{v[2]:,}"] for name, v in zeilen]))

    b.append("\n## Dateien nach Jahr (Aenderungsdatum)\n")
    b.append(tabelle(["Jahr", "Anzahl", "GB"], [
        [j, f"{v[0]:,}", f"{gb(v[1]):.2f}"] for j, v in sorted(je_jahr.items())]))

    with open(os.path.join(out, "dateitypen.csv"), "w", newline="",
              encoding="utf-8") as fh:
        w = csv.writer(fh, delimiter=";")
        w.writerow(["typ", "anzahl", "bytes", "nur_online"])
        for typ, v in sorted(je_typ.items(), key=lambda x: -x[1][1]):
            w.writerow([typ, v[0], v[1], v[2]])
    if fehler:
        with open(os.path.join(out, "fehler.csv"), "w", newline="",
                  encoding="utf-8") as fh:
            w = csv.writer(fh, delimiter=";")
            w.writerow(["pfad", "meldung"])
            w.writerows(fehler)

    # ---------------- Stufe 2
    if not args.nur_inventur:
        try:
            import pypdfium2  # noqa: F401
            from PIL import features
        except ImportError:
            sys.exit("Fuer die Stichprobe fehlen Bibliotheken:\n"
                     "  python3 -m pip install pypdfium2 pillow\n"
                     "oder mit --nur-inventur starten.")
        avif = bool(features.check("avif"))
        lokale = [d for d in pdfs if not d[3] and d[1] > 0]
        rnd = random.Random(args.seed)
        auswahl = rnd.sample(lokale, min(args.stichprobe, len(lokale)))
        print(f"Stufe 2: Stichprobe mit {len(auswahl)} von {len(lokale):,} "
              f"lokalen PDFs ...", file=sys.stderr)

        bsp = os.path.join(out, "beispiele_vorschau")
        txt = os.path.join(out, "beispiele_text")
        os.makedirs(bsp, exist_ok=True)
        os.makedirs(txt, exist_ok=True)

        ergebnisse, alle_seiten = [], []
        for nr, d in enumerate(auswahl, 1):
            mit_bsp = nr <= args.beispiele
            erg, seiten = pdf_untersuchen(
                d[0], d[1], breiten, args.max_seiten, avif,
                bsp if mit_bsp else None, txt if mit_bsp else None, nr)
            erg["nr"] = nr
            ergebnisse.append(erg)
            alle_seiten += seiten
            if nr % 25 == 0:
                print(f"  ... {nr}/{len(auswahl)}", file=sys.stderr)

        ok = [e for e in ergebnisse if e["status"] == "ok" and e.get("seiten")]
        with open(os.path.join(out, "stichprobe.csv"), "w", newline="",
                  encoding="utf-8") as fh:
            w = csv.writer(fh, delimiter=";")
            w.writerow(["nr", "pfad", "status", "bytes", "seiten",
                        "seiten_mit_text", "seiten_ohne_text", "scan_seiten",
                        "zeichen", "bildtypen", "filter", "dpi", "erstellt",
                        "producer", "wortqualitaet"])
            for e in ergebnisse:
                w.writerow([e.get(k, "") for k in (
                    "nr", "pfad", "status", "bytes", "seiten", "seiten_mit_text",
                    "seiten_ohne_text", "scan_seiten", "zeichen", "bildtypen",
                    "filter", "dpi", "erstellt", "producer", "wortqualitaet")])

        b.append("\n# PDF-Stichprobe\n")
        if not ok:
            b.append("Keine auswertbaren PDFs in der Stichprobe.")
        else:
            seiten_ges = sum(e["seiten"] for e in ok)
            bytes_ges = sum(e["bytes"] for e in ok)
            seiten_je_pdf = seiten_ges / len(ok)
            mit_t = sum(e["seiten_mit_text"] for e in ok)
            ohne_t = sum(e["seiten_ohne_text"] for e in ok)
            scan = sum(e["scan_seiten"] for e in ok)
            pdf_ganz_ohne = sum(1 for e in ok if e["seiten_mit_text"] == 0)
            hochr_seiten = len(pdfs) * seiten_je_pdf

            b.append("Die Stichprobe stammt nur aus lokal vorhandenen PDFs. "
                     "Die Hochrechnung gilt fuer alle PDFs der geprueften "
                     "Verzeichnisse und ist entsprechend unsicher.\n")
            b.append(tabelle(["Kennzahl", "Wert"], [
                ["ausgewertete PDFs", f"{len(ok)} (nicht lesbar: "
                                      f"{len(ergebnisse) - len(ok)})"],
                ["Seiten je PDF", f"Mittel {seiten_je_pdf:.1f} / Median "
                 f"{statistics.median(e['seiten'] for e in ok):.0f} / Maximum "
                 f"{max(e['seiten'] for e in ok)}"],
                ["Original-KB je Seite", f"Mittel {kb(bytes_ges / seiten_ges):.0f} "
                 f"/ Median {kb(statistics.median(e['bytes'] / e['seiten'] for e in ok)):.0f}"],
                ["Seiten mit Textebene", f"{mit_t:,} ({mit_t / seiten_ges:.1%})"],
                ["Seiten ohne Textebene", f"{ohne_t:,} ({ohne_t / seiten_ges:.1%})"],
                ["PDFs ganz ohne Text", f"{pdf_ganz_ohne} "
                                        f"({pdf_ganz_ohne / len(ok):.1%})"],
                ["Scan-Seiten (Vollbild)", f"{scan:,} ({scan / seiten_ges:.1%})"],
                ["PDFs mit Erstellungsdatum",
                 f"{sum(1 for e in ok if e.get('erstellt')) / len(ok):.1%}"],
                ["hochgerechnete Seitenzahl", f"{hochr_seiten:,.0f}"],
            ]))

            bild = Counter()
            filt = Counter()
            prod = Counter()
            for e in ok:
                bild.update(e["bildtypen"])
                filt.update(e["filter"])
                prod[e.get("producer") or "(leer)"] += 1
            if bild:
                n_b = sum(bild.values())
                b.append("\n## Bildtyp der Scan-Seiten\n")
                b.append(tabelle(["Typ", "Seiten", "Anteil"], [
                    [t, f"{c:,}", f"{c / n_b:.1%}"] for t, c in bild.most_common()]))
                b.append("\n## Kompression der Scan-Bilder\n")
                b.append(tabelle(["Filter", "Seiten", "Anteil"], [
                    [t, f"{c:,}", f"{c / n_b:.1%}"] for t, c in filt.most_common()]))
            b.append("\n## Erzeugende Programme (Top 10)\n")
            b.append(tabelle(["Programm", "PDFs"], [
                [p, c] for p, c in prod.most_common(10)]))

            wq = [e["wortqualitaet"] for e in ok if e.get("wortqualitaet") is not None]
            if wq:
                b.append("\n## Guete der vorhandenen OCR-Texte (grobes Mass)\n")
                b.append("Anteil wortartiger Tokens je PDF. Werte unter etwa "
                         "60 % deuten auf schwache OCR hin.\n")
                b.append(tabelle(["Kennzahl", "Wert"], [
                    ["Median", f"{statistics.median(wq):.0%}"],
                    ["schwaechstes Zehntel", f"{perzentil(wq, 10):.0%}"],
                    ["PDFs unter 60 %", f"{sum(1 for x in wq if x < 0.6)} "
                                        f"({sum(1 for x in wq if x < 0.6) / len(wq):.1%})"],
                ]))

            if alle_seiten:
                b.append("\n## Vorschaugroessen je Seite und Hochrechnung\n")
                b.append(f"Gemessen an {len(alle_seiten):,} Seiten. Die Breite "
                         "bezieht sich auf A4; schmalere Seiten (Kassenbelege) "
                         "werden im selben Massstab gerendert.\n")
                varianten = []
                for br in breiten:
                    varianten.append((f"webp50_{br}", f"WebP Q50, {br} px"))
                    varianten.append((f"webp25_{br}", f"WebP Q25, {br} px"))
                    if avif:
                        varianten.append((f"avif25_{br}", f"AVIF Q25, {br} px"))
                varianten.append(("sw1bit_200dpi", "1-Bit, 200 dpi (volle Lesegroesse)"))
                zeilen = []
                for schluessel, name in varianten:
                    w_ = [s[schluessel] for s in alle_seiten if schluessel in s]
                    if not w_:
                        continue
                    mittel = statistics.mean(w_)
                    zeilen.append([name, f"{kb(mittel):.0f}",
                                   f"{kb(statistics.median(w_)):.0f}",
                                   f"{kb(perzentil(w_, 90)):.0f}",
                                   f"{gb(mittel * hochr_seiten):.1f}"])
                b.append(tabelle(["Variante", "KB Mittel", "KB Median",
                                  "KB 90. Perzentil", "GB hochgerechnet"], zeilen))
                grau_anteil = sum(1 for s in alle_seiten if s.get("grau")) / len(alle_seiten)
                b.append(f"\nAnteil Seiten ohne Farbe: {grau_anteil:.1%}")
                if not avif:
                    b.append("\nAVIF wurde nicht gemessen (die installierte "
                             "Pillow-Version unterstuetzt es nicht).")

            b.append("\n## Zum Ansehen\n")
            b.append(f"- `beispiele_vorschau/`: erste Seite von {args.beispiele} "
                     "PDFs in allen Breiten (WebP Q50), zur Beurteilung der "
                     "Lesbarkeit")
            b.append(f"- `beispiele_text/`: eingebetteter Text derselben PDFs "
                     "zum Gegenlesen")
            b.append("- `stichprobe.csv`: Einzelwerte je PDF")

    b.append(f"\n---\nLaufzeit: {time.time() - start:.0f} s")
    bericht = os.path.join(out, "bericht.md")
    with open(bericht, "w", encoding="utf-8") as fh:
        fh.write("\n".join(b) + "\n")
    print(f"Fertig. Bericht: {bericht}", file=sys.stderr)


if __name__ == "__main__":
    main()
