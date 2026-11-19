//! Messung der OCR (v0.0.4): Seiten ohne oder mit schwachem Text mit 300 dpi
//! rendern und mit den vorhandenen Sätzen der Sprachdaten erkennen,
//! "standard" (tessdata) und, falls mit `tesseract_holen.sh --best` geladen,
//! "best" (tessdata_best). Misst Zeit, Güte und wie oft der erkannte Text
//! übernommen würde (Regel von Verfahren 1, auch für schwachen Text).
//!
//! Die erkannten Texte landen zum Vergleich in target/ocr_muster/. Sie
//! enthalten Dokumentinhalte und gehören nicht ins Repo.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use registerwerk_core::guete::{self, Zaehlung};
use registerwerk_core::ocr::{self, Fall, Graubild};
use registerwerk_core::pdf;

const SAETZE: [&str; 2] = ["standard", "best"];
const MUSTER_ORDNER: &str = "target/ocr_muster";

struct Kandidat {
    pfad: PathBuf,
    nr: u32,
    fall: Fall,
    alt: Zaehlung,
}

#[derive(Default, Clone)]
struct Satzergebnis {
    ms: f64,
    uebernommen: usize,
    wortartig: usize,
    tokens: usize,
    konfidenz: i64,
    ohne_text: usize,
}

struct Seitenergebnis {
    index: usize,
    je_satz: Vec<(String, i32, Duration)>,
}

/// Sucht den Ordner mit den Sätzen der Sprachdaten.
pub fn tessdata_basis() -> Option<PathBuf> {
    if let Some(o) = std::env::var_os("REGISTERWERK_TESSDATA") {
        return Some(PathBuf::from(o));
    }
    let start = std::env::current_dir().ok()?;
    start.ancestors().map(|o| o.join(ocr::ENTWICKLUNGS_ORDNER)).find(|o| o.is_dir())
}

pub fn lauf(pdfium: &pdf::Pdfium, dateien: &[PathBuf], n: usize) {
    let Some(basis) = tessdata_basis() else {
        println!("Sprachdaten nicht gefunden. Zuerst: bash tools/tesseract_holen.sh");
        return;
    };
    let saetze: Vec<&str> = SAETZE.into_iter().filter(|s| ocr::tessdata_vollstaendig(&basis.join(s))).collect();
    if saetze.is_empty() {
        println!("Keine Sprachdaten in {}. Zuerst: bash tools/tesseract_holen.sh", basis.display());
        return;
    }
    println!("Tesseract {}, Sprachdaten aus {}", registerwerk_ocr::version(), basis.display());

    // Kandidaten aus allen lokalen PDFs sammeln
    let t = Instant::now();
    let mut alle: Vec<Kandidat> = Vec::new();
    let mut gelesen = 0;
    for pfad in dateien {
        if std::fs::metadata(pfad).map(|m| m.len()).unwrap_or(0) == 0 {
            continue;
        }
        let Ok(inhalt) = pdf::lesen(pdfium, pfad) else { continue };
        gelesen += 1;
        for s in &inhalt.seiten {
            let alt = guete::zaehlen(&s.text);
            if let Some(fall) = Fall::bestimmen(&alt, s.bildanteil) {
                alle.push(Kandidat { pfad: pfad.clone(), nr: s.nr, fall, alt });
            }
        }
    }
    let zaehle = |f: Fall| alle.iter().filter(|k| k.fall == f).count();
    println!(
        "{gelesen} PDFs gelesen in {:.1} s. Seiten zur OCR: {} (ohne Text {}, Scan schwach {}, digital schwach {})",
        t.elapsed().as_secs_f64(),
        alle.len(),
        zaehle(Fall::OhneText),
        zaehle(Fall::ScanSchwach),
        zaehle(Fall::DigitalSchwach)
    );
    if alle.is_empty() {
        return;
    }
    let schritt = (alle.len() as f64 / n.max(1) as f64).max(1.0);
    let auswahl: Vec<&Kandidat> =
        (0..n).map(|i| (i as f64 * schritt) as usize).take_while(|&i| i < alle.len()).map(|i| &alle[i]).collect();
    let kerne = std::thread::available_parallelism().map(|k| k.get()).unwrap_or(1);
    println!("Stichprobe: {} Seiten, {} Fäden, Sätze: {}\n", auswahl.len(), kerne, saetze.join(", "));
    let _ = std::fs::create_dir_all(MUSTER_ORDNER);

    // Erkenner je Faden und Satz
    let mut erkenner_je_faden = Vec::new();
    for _ in 0..kerne {
        let mut je_satz = Vec::new();
        for satz in &saetze {
            match registerwerk_ocr::Erkenner::neu(&basis.join(satz), ocr::SPRACHEN) {
                Ok(e) => je_satz.push(e),
                Err(f) => {
                    println!("{f}");
                    return;
                }
            }
        }
        erkenner_je_faden.push(je_satz);
    }

    let start = Instant::now();
    let mut render_ms = 0f64;
    let mut ergebnisse: Vec<Option<Seitenergebnis>> = (0..auswahl.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let (tx, rx) = mpsc::sync_channel::<(usize, Graubild, u32)>(kerne);
        let rx = Arc::new(Mutex::new(rx));
        let (erg_tx, erg_rx) = mpsc::channel::<Seitenergebnis>();
        for mut je_satz in erkenner_je_faden {
            let rx = Arc::clone(&rx);
            let erg_tx = erg_tx.clone();
            s.spawn(move || loop {
                let Ok(Ok((index, bild, dpi))) = rx.lock().map(|r| r.recv()) else { break };
                let mut je = Vec::new();
                for e in je_satz.iter_mut() {
                    let t = Instant::now();
                    let r = e.erkennen(&bild.grau, bild.breite, bild.hoehe, dpi);
                    let d = t.elapsed();
                    match r {
                        Ok(r) => je.push((r.text.trim().to_string(), r.konfidenz, d)),
                        Err(f) => je.push((format!("FEHLER {f}"), 0, d)),
                    }
                }
                if erg_tx.send(Seitenergebnis { index, je_satz: je }).is_err() {
                    break;
                }
            });
        }
        drop(erg_tx);
        for (index, k) in auswahl.iter().enumerate() {
            let t = Instant::now();
            let r = ocr::pdf_rendern(pdfium, &k.pfad, &[k.nr], &mut |_, bild, dpi| tx.send((index, bild, dpi)).is_ok());
            render_ms += t.elapsed().as_secs_f64() * 1000.0;
            if let Err(f) = r {
                println!("FEHLER Rendern {} S. {}: {f}", k.pfad.display(), k.nr);
            }
        }
        drop(tx);
        for e in erg_rx {
            let i = e.index;
            ergebnisse[i] = Some(e);
        }
    });
    let dauer = start.elapsed();

    let mut summe = vec![Satzergebnis::default(); saetze.len()];
    let (mut alt_w, mut alt_t) = (0usize, 0usize);
    let kopf: String = saetze.iter().map(|s| format!("{s:>14}  ")).collect();
    println!("{:<15} {:>5} {:>5}  {kopf}Datei", "Fall", "Seite", "Güte");
    for (k, e) in auswahl.iter().zip(&ergebnisse) {
        let Some(e) = e else { continue };
        alt_w += k.alt.wortartig;
        alt_t += k.alt.tokens;
        let mut spalten = Vec::new();
        for (i, (text, konfidenz, d)) in e.je_satz.iter().enumerate() {
            let z = guete::zaehlen(text);
            let s = &mut summe[i];
            s.ms += d.as_secs_f64() * 1000.0;
            s.wortartig += z.wortartig;
            s.tokens += z.tokens;
            s.konfidenz += *konfidenz as i64;
            if ocr::uebernehmen(k.fall, &k.alt, &z) {
                s.uebernommen += 1;
            }
            if z.ohne_text() {
                s.ohne_text += 1;
            }
            spalten.push(format!("{:>3} {:>4}ms {:>3}", prozent(z.guete()), d.as_millis(), konfidenz));
            let name = format!(
                "{MUSTER_ORDNER}/{:03}_{}_S{}_{}.txt",
                e.index,
                k.pfad.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default(),
                k.nr,
                saetze[i]
            );
            let _ = std::fs::write(name, text);
        }
        let spalten: String = spalten.iter().map(|s| format!("{s}  ")).collect();
        println!("{:<15} {:>5} {:>5}  {spalten}{}", k.fall.als_text(), k.nr, prozent(k.alt.guete()), kurz(&k.pfad));
    }
    let n = auswahl.len().max(1) as f64;
    println!();
    println!("Spalten je Satz: Güte in %, Zeit je Seite, Konfidenz laut Tesseract");
    println!("Güte vorher (alle Seiten zusammen): {}", prozent((alt_t > 0).then(|| alt_w as f64 / alt_t as f64)));
    for (i, satz) in saetze.iter().enumerate() {
        let s = &summe[i];
        println!(
            "{satz:<9} {:>6.0} ms je Seite, Güte {}, Konfidenz {:.0}, übernommen {} von {}, ohne Text {}",
            s.ms / n,
            prozent((s.tokens > 0).then(|| s.wortartig as f64 / s.tokens as f64)),
            s.konfidenz as f64 / n,
            s.uebernommen,
            auswahl.len(),
            s.ohne_text
        );
    }
    println!("Rendern: {:.0} ms je Seite", render_ms / n);
    println!(
        "Gesamt: {:.1} s für {} Seiten mit allen Sätzen auf {kerne} Fäden",
        dauer.as_secs_f64(),
        auswahl.len()
    );
    for (i, satz) in saetze.iter().enumerate() {
        let je_seite = summe[i].ms / n / kerne as f64;
        println!(
            "Hochrechnung {satz}: {:.0} Seiten je Minute auf {kerne} Fäden, alle {} Seiten des Bestands in {:.0} min",
            60_000.0 / je_seite.max(0.001),
            alle.len(),
            alle.len() as f64 * je_seite / 60_000.0
        );
    }
    println!("Texte zum Vergleich: {MUSTER_ORDNER}/");
}

fn prozent(g: Option<f64>) -> String {
    g.map(|g| format!("{:.0}", g * 100.0)).unwrap_or_else(|| "–".into())
}

fn kurz(pfad: &Path) -> String {
    let teile: Vec<_> = pfad.iter().rev().take(2).collect();
    teile.iter().rev().map(|t| t.to_string_lossy()).collect::<Vec<_>>().join("/")
}
