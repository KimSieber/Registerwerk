//! Probe der Textebene (v0.0.5): N PDFs mit Scan-Seiten ohne Text,
//! gleichmäßig verteilt. Die Seiten werden erkannt, die Textebene
//! eingebettet und die neue Fassung wie in der Anwendung geprüft.
//!
//! Die neuen Fassungen landen in target/textebene_muster/. Die Originale
//! bleiben unberührt. Die Muster enthalten Dokumentinhalte und gehören nicht
//! ins Repo.

use std::path::{Path, PathBuf};
use std::time::Instant;

use registerwerk_core::ocr::{self, Graubild};
use registerwerk_core::textebene::{self, SeitenEbene};
use registerwerk_core::{guete, pdf, woerter};

const MUSTER_ORDNER: &str = "target/textebene_muster";

pub fn lauf(pdfium: &pdf::Pdfium, dateien: &[PathBuf], n: usize) {
    let Some(basis) = crate::ocrtest::tessdata_basis() else {
        println!("Sprachdaten nicht gefunden. Zuerst: bash tools/tesseract_holen.sh");
        return;
    };
    let mut erkenner = match registerwerk_ocr::Erkenner::neu(&basis.join(ocr::MODELLE), ocr::SPRACHEN) {
        Ok(e) => e,
        Err(f) => {
            println!("{f}");
            return;
        }
    };
    // PDFs mit Scan-Seiten ohne Text
    let mut kandidaten: Vec<(PathBuf, Vec<u32>)> = Vec::new();
    for pfad in dateien {
        if std::fs::metadata(pfad).map(|m| m.len()).unwrap_or(0) == 0 {
            continue;
        }
        let Ok(inhalt) = pdf::lesen(pdfium, pfad) else { continue };
        let seiten: Vec<u32> =
            inhalt.seiten.iter().filter(|s| s.ist_scan() && guete::zaehlen(&s.text).ohne_text()).map(|s| s.nr).collect();
        if !seiten.is_empty() {
            kandidaten.push((pfad.clone(), seiten));
        }
    }
    let schritt = (kandidaten.len() as f64 / n.max(1) as f64).max(1.0);
    let auswahl: Vec<_> = (0..n.min(kandidaten.len())).map(|i| &kandidaten[(i as f64 * schritt) as usize]).collect();
    println!("{} PDFs mit Scan-Seiten ohne Text, Probe mit {}", kandidaten.len(), auswahl.len());
    let _ = std::fs::create_dir_all(MUSTER_ORDNER);
    let (mut ok, mut fehler, mut ohne, mut vorher, mut nachher, mut seiten_n) = (0, 0, 0, 0u64, 0u64, 0);
    let start = Instant::now();
    for (i, (pfad, nummern)) in auswahl.iter().enumerate() {
        let mut ebenen = Vec::new();
        let r = ocr::pdf_rendern(pdfium, pfad, nummern, &mut |nr, bild: Graubild, dpi| {
            if let Ok(e) = erkenner.erkennen(&bild.grau, bild.breite, bild.hoehe, dpi) {
                if !guete::zaehlen(e.text.trim()).ohne_text() {
                    ebenen.push(SeitenEbene { nr, woerter: woerter::aus_tesseract(&e.woerter, bild.breite, bild.hoehe) });
                }
            }
            true
        });
        let name = format!("{:03}_{}", i + 1, pfad.file_name().unwrap_or_default().to_string_lossy());
        if let Err(f) = r {
            println!("FEHLER  {name}: Rendern {f}");
            fehler += 1;
            continue;
        }
        if ebenen.is_empty() {
            ohne += 1;
            continue;
        }
        let original = std::fs::read(pfad).unwrap_or_default();
        let t = Instant::now();
        let neu = match textebene::einbetten(&original, &ebenen) {
            Ok(n) => n,
            Err(f) => {
                println!("FEHLER  {name}: {f}");
                fehler += 1;
                continue;
            }
        };
        let ziel = Path::new(MUSTER_ORDNER).join(&name);
        if std::fs::write(&ziel, &neu).is_err() {
            println!("FEHLER  {name}: Schreiben");
            fehler += 1;
            continue;
        }
        let nr: Vec<u32> = ebenen.iter().map(|e| e.nr).collect();
        match pdf::textebene_pruefen(pdfium, pfad, &ziel, &nr) {
            Ok(_) => {
                ok += 1;
                seiten_n += nr.len();
                vorher += original.len() as u64;
                nachher += neu.len() as u64;
                let w: usize = ebenen.iter().map(|e| e.woerter.len()).sum();
                println!(
                    "ok      {name}: Seiten {nr:?}, {w} Wörter, +{} Byte, {} ms",
                    neu.len() - original.len(),
                    t.elapsed().as_millis()
                );
            }
            Err(f) => {
                println!("FEHLER  {name}: Prüfung {f}");
                fehler += 1;
            }
        }
    }
    println!(
        "\n{ok} geschrieben ({seiten_n} Seiten), {fehler} Fehler, {ohne} ohne brauchbaren Text, {:.1} s",
        start.elapsed().as_secs_f64()
    );
    if ok > 0 {
        println!(
            "Zuwachs {} KB ({:.2} %), {:.1} KB je Seite. Muster in {MUSTER_ORDNER}/",
            (nachher - vorher) / 1024,
            100.0 * (nachher - vorher) as f64 / vorher as f64,
            (nachher - vorher) as f64 / 1024.0 / seiten_n as f64
        );
    }
}
