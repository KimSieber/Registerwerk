//! pdf_pruefen: prüft PDFs mit dem Kern von Registerwerk, ohne das Register
//! anzufassen. Nützlich nach einem PDFium-Update, auf einem neuen Gerät oder
//! für einzelne Problemdateien. Öffnet nur lesend.
//!
//! Aufruf aus dem Projektordner:
//!
//!     cargo run --release -p pdf_pruefen -- [--still] [--ohne-text] [--text] [--max N] <PDF oder Ordner> ...
//!
//! --still      nur Fehler und Zusammenfassung
//! --ohne-text  nur PDFs mit Seiten ohne Text, mit Seitennummern (d = digital)
//! --text       zusätzlich die ersten 80 Zeichen der ersten Seite
//!
//! PDFium wird in `REGISTERWERK_PDFIUM` oder in `bibliotheken/pdfium/lib`
//! ab dem Arbeitsverzeichnis aufwärts gesucht. Dateien mit 0 Byte
//! ("nur online") werden übergangen, damit nichts aus der Cloud geladen wird.

use std::path::{Path, PathBuf};
use std::time::Instant;

use registerwerk_core::{guete, pdf};

#[derive(Default)]
struct Summe {
    gelesen: usize,
    online: usize,
    fehler: usize,
    seiten: usize,
    ohne_text: usize,
    ohne_text_scan: usize,
    scan: usize,
    schwach: usize,
    pdf_scan: usize,
    pdf_digital: usize,
    pdf_gemischt: usize,
    pdf_ohne_text: usize,
    pdf_schwach: usize,
    mit_datum: usize,
}

fn main() {
    let (mut still, mut nur_ohne_text, mut text_zeigen, mut max) = (false, false, false, usize::MAX);
    let mut ziele: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--still" => still = true,
            "--ohne-text" => nur_ohne_text = true,
            "--text" => text_zeigen = true,
            "--max" => max = args.next().and_then(|n| n.parse().ok()).unwrap_or(usize::MAX),
            _ => ziele.push(PathBuf::from(a)),
        }
    }
    if ziele.is_empty() {
        eprintln!("Aufruf: pdf_pruefen [--still] [--ohne-text] [--text] [--max N] <PDF oder Ordner> ...");
        std::process::exit(2);
    }

    let ordner = std::env::var_os("REGISTERWERK_PDFIUM")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok().and_then(|d| pdf::suchen_ab(&d)));
    let Some(ordner) = ordner else {
        eprintln!("PDFium nicht gefunden. Zuerst: bash tools/pdfium_holen.sh");
        std::process::exit(1);
    };
    let pdfium = match pdf::einbinden(&ordner) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("PDFium: {}", ordner.display());

    let mut dateien = Vec::new();
    for z in &ziele {
        sammeln(z, &mut dateien);
    }
    dateien.sort();

    let mut s = Summe::default();
    let start = Instant::now();
    for pfad in &dateien {
        if s.gelesen >= max {
            break;
        }
        if std::fs::metadata(pfad).map(|m| m.len()).unwrap_or(0) == 0 {
            s.online += 1;
            continue;
        }
        s.gelesen += 1;
        let t = Instant::now();
        let inhalt = match pdf::lesen(pdfium, pfad) {
            Ok(i) => i,
            Err(e) => {
                s.fehler += 1;
                println!("FEHLER  {}: {e}", pfad.display());
                continue;
            }
        };
        let n = inhalt.seiten.len();
        let (mut zeichen, mut tokens, mut wortartig, mut scans) = (0, 0, 0, 0);
        let mut leer: Vec<String> = Vec::new();
        for seite in &inhalt.seiten {
            let z = guete::zaehlen(&seite.text);
            zeichen += z.zeichen;
            tokens += z.tokens;
            wortartig += z.wortartig;
            if seite.ist_scan() {
                scans += 1;
            }
            if z.ohne_text() {
                s.ohne_text += 1;
                if seite.ist_scan() {
                    s.ohne_text_scan += 1;
                }
                leer.push(format!("{}{}", seite.nr, if seite.ist_scan() { "" } else { "d" }));
            } else if z.guete().is_some_and(|g| g < guete::SCHWELLE) {
                s.schwach += 1;
            }
        }
        s.seiten += n;
        s.scan += scans;
        let art = if scans == n { "Scan" } else if scans == 0 { "digital" } else { "gemischt" };
        match art {
            "Scan" => s.pdf_scan += 1,
            "digital" => s.pdf_digital += 1,
            _ => s.pdf_gemischt += 1,
        }
        if leer.len() == n {
            s.pdf_ohne_text += 1;
        } else if tokens > 0 && (wortartig as f64) < guete::SCHWELLE * tokens as f64 {
            s.pdf_schwach += 1;
        }
        let erstellt = inhalt.erstellt.as_deref().and_then(registerwerk_core::datum::aus_pdf_datum);
        if erstellt.is_some() {
            s.mit_datum += 1;
        }

        if nur_ohne_text {
            if !leer.is_empty() {
                println!("{n:>4} S. {:>4} o.T. {art:<8} Seiten {}  {}", leer.len(), leer.join(","), pfad.display());
            }
            continue;
        }
        if still {
            continue;
        }
        let guete_text = if tokens > 0 { format!("{:>3.0} %", wortartig as f64 * 100.0 / tokens as f64) } else { "  –  ".into() };
        println!(
            "{n:>4} S. {zeichen:>7} Z. {:>2} o.T. Güte {guete_text} {art:<8} {:<10} {:>5} ms  {}",
            leer.len(),
            erstellt.unwrap_or_else(|| "-".into()),
            t.elapsed().as_millis(),
            pfad.display()
        );
        if text_zeigen {
            let erste: String = inhalt
                .seiten
                .first()
                .map(|s| s.text.split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect();
            println!("         | {erste}");
        }
    }
    let dauer = start.elapsed();
    println!();
    println!("PDFs gelesen:          {}", s.gelesen);
    println!("nur online, übergangen: {}", s.online);
    println!("Fehler:                {}", s.fehler);
    println!("Seiten:                {}", s.seiten);
    println!("Seiten ohne Text:      {} (Scan {}, digital {})", s.ohne_text, s.ohne_text_scan, s.ohne_text - s.ohne_text_scan);
    println!("Seiten schwach:        {}", s.schwach);
    println!("Seiten als Scan:       {}", s.scan);
    println!("PDFs Scan / digital / gemischt: {} / {} / {}", s.pdf_scan, s.pdf_digital, s.pdf_gemischt);
    println!("PDFs ohne Text / schwach: {} / {}", s.pdf_ohne_text, s.pdf_schwach);
    println!("mit Erstellungsdatum:  {}", s.mit_datum);
    println!(
        "Dauer:                 {:.1} s ({:.1} ms je Seite)",
        dauer.as_secs_f64(),
        if s.seiten > 0 { dauer.as_secs_f64() * 1000.0 / s.seiten as f64 } else { 0.0 }
    );
}

fn sammeln(pfad: &Path, ziel: &mut Vec<PathBuf>) {
    if pfad.is_dir() {
        if let Ok(eintraege) = std::fs::read_dir(pfad) {
            for e in eintraege.flatten() {
                sammeln(&e.path(), ziel);
            }
        }
    } else if pfad.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        ziel.push(pfad.to_path_buf());
    }
}
