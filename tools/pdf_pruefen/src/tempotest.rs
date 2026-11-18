//! Tempotest der Vorschauen (v0.0.3): Rendern, Farberkennung und AVIF-Kodierung
//! mit mehreren Geschwindigkeiten des Encoders messen.

use std::path::PathBuf;
use std::time::Instant;

use registerwerk_core::vorschau::{self, Bild};

const GESCHWINDIGKEITEN: [u8; 3] = [6, 8, 10];
const SEITEN_JE_PDF: usize = 4;
const MUSTER: usize = 12;
const MUSTER_ORDNER: &str = "target/vorschau_muster";

pub fn lauf(pdfium: &registerwerk_core::pdf::Pdfium, dateien: &[PathBuf], n: usize) {
    let lokal: Vec<&PathBuf> =
        dateien.iter().filter(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) > 0).collect();
    if lokal.is_empty() {
        println!("Keine lokalen PDFs gefunden.");
        return;
    }
    let schritt = (lokal.len() as f64 / n.max(1) as f64).max(1.0);
    let auswahl: Vec<&PathBuf> =
        (0..n).map(|i| (i as f64 * schritt) as usize).take_while(|&i| i < lokal.len()).map(|i| lokal[i]).collect();
    let kerne = std::thread::available_parallelism().map(|k| k.get()).unwrap_or(1);
    let _ = std::fs::create_dir_all(MUSTER_ORDNER);
    let mut liste = String::new();
    let stride = (auswahl.len() / MUSTER).max(1);

    let mut seiten = 0usize;
    let mut render_ms = 0f64;
    let mut farbig = 0usize;
    let mut farb_stufen = [0usize; 5]; // 0, <0,1 %, <1 %, <10 %, >=10 %
    let mut kodier_ms = [0f64; GESCHWINDIGKEITEN.len()];
    let mut groessen: [Vec<usize>; GESCHWINDIGKEITEN.len()] = Default::default();
    let mut pixel = 0u64;

    println!("PDFs in der Stichprobe: {} von {} lokalen, {} Kerne", auswahl.len(), lokal.len(), kerne);
    for (nr, pfad) in auswahl.iter().enumerate() {
        let t = Instant::now();
        let bilder: Vec<Bild> = match vorschau::pdf_rendern(pdfium, pfad, SEITEN_JE_PDF) {
            Ok(b) => b,
            Err(e) => {
                println!("FEHLER  {}: {e}", pfad.display());
                continue;
            }
        };
        render_ms += t.elapsed().as_secs_f64() * 1000.0;
        for (s, bild) in bilder.iter().enumerate() {
            seiten += 1;
            pixel += (bild.breite * bild.hoehe) as u64;
            let anteil = vorschau::farbanteil(bild);
            let ist_farbig = anteil >= vorschau::FARB_ANTEIL;
            farbig += ist_farbig as usize;
            farb_stufen[match anteil {
                0.0 => 0,
                a if a < 0.001 => 1,
                a if a < 0.01 => 2,
                a if a < 0.1 => 3,
                _ => 4,
            }] += 1;
            for (i, &g) in GESCHWINDIGKEITEN.iter().enumerate() {
                let t = Instant::now();
                match vorschau::kodieren(bild, ist_farbig, g, Some(1)) {
                    Ok(avif) => {
                        kodier_ms[i] += t.elapsed().as_secs_f64() * 1000.0;
                        groessen[i].push(avif.len());
                        if g == vorschau::GESCHWINDIGKEIT && liste.lines().count() < MUSTER && s == 0 && nr % stride == 0 {
                            let name = format!(
                                "{:03}_s{}_{}_{:.2}pct.avif",
                                nr,
                                s + 1,
                                if ist_farbig { "farbig" } else { "grau" },
                                anteil * 100.0
                            );
                            let _ = std::fs::write(format!("{MUSTER_ORDNER}/{name}"), &avif);
                            liste.push_str(&format!("{name}\t{}\n", pfad.display()));
                        }
                    }
                    Err(e) => println!("FEHLER Kodieren {}: {e}", pfad.display()),
                }
            }
        }
    }
    let _ = std::fs::write(format!("{MUSTER_ORDNER}/liste.txt"), &liste);
    if seiten == 0 {
        return;
    }
    let s = seiten as f64;
    println!();
    println!("Seiten:            {seiten}, im Mittel {:.0} x 1000 Pixel", pixel as f64 / s / 1000.0);
    println!("Rendern:           {:.1} ms je Seite (einschließlich Öffnen der PDF)", render_ms / s);
    println!("Farbig:            {farbig} ({:.0} %)", farbig as f64 * 100.0 / s);
    println!(
        "Farbanteil:        0: {} | <0,1 %: {} | <1 %: {} | <10 %: {} | ab 10 %: {}",
        farb_stufen[0], farb_stufen[1], farb_stufen[2], farb_stufen[3], farb_stufen[4]
    );
    println!();
    println!("Geschw.  ms/Seite  KB Mittel  KB Median  Seiten/s ({kerne} Kerne)  11.131 S.   1,5 Mio. S.");
    for (i, &g) in GESCHWINDIGKEITEN.iter().enumerate() {
        let mut gr = groessen[i].clone();
        gr.sort_unstable();
        let mittel = gr.iter().sum::<usize>() as f64 / gr.len().max(1) as f64 / 1024.0;
        let median = gr.get(gr.len() / 2).copied().unwrap_or(0) as f64 / 1024.0;
        let ms = kodier_ms[i] / s;
        // Kodieren parallel auf allen Kernen, Rendern nacheinander: der langsamere Teil bestimmt das Tempo
        let je_seite_ms = (ms / kerne as f64).max(render_ms / s);
        let pro_s = 1000.0 / je_seite_ms;
        println!(
            "{g:>7}  {ms:>8.0}  {mittel:>9.1}  {median:>9.1}  {pro_s:>20.1}  {:>8.1} min  {:>8.1} h",
            11131.0 / pro_s / 60.0,
            1_500_000.0 / pro_s / 3600.0
        );
    }
    println!();
    println!("Muster (Geschwindigkeit {}): {MUSTER_ORDNER}/, Zuordnung in liste.txt", vorschau::GESCHWINDIGKEIT);
}

/// Rendert die erste Seite von `n` gleichmäßig verteilten lokalen PDFs und legt
/// sie unverändert als PPM ab, als Vorlage für den Vergleich von Encodern.
pub fn rendern_ablegen(pdfium: &registerwerk_core::pdf::Pdfium, dateien: &[PathBuf], n: usize) {
    const ORDNER: &str = "target/render_muster";
    let lokal: Vec<&PathBuf> =
        dateien.iter().filter(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) > 0).collect();
    let schritt = (lokal.len() as f64 / n.max(1) as f64).max(1.0);
    let _ = std::fs::create_dir_all(ORDNER);
    let mut liste = String::new();
    for i in 0..n {
        let Some(pfad) = lokal.get((i as f64 * schritt) as usize) else { break };
        match vorschau::pdf_rendern(pdfium, pfad, 1) {
            Ok(bilder) => {
                let Some(bild) = bilder.first() else { continue };
                let farbig = vorschau::ist_farbig(bild);
                let name = format!("{i:03}_{}.ppm", if farbig { "farbig" } else { "grau" });
                let mut ppm = format!("P6\n{} {}\n255\n", bild.breite, bild.hoehe).into_bytes();
                ppm.extend_from_slice(&bild.rgb);
                let _ = std::fs::write(format!("{ORDNER}/{name}"), ppm);
                liste.push_str(&format!("{name}\t{}\n", pfad.display()));
            }
            Err(e) => println!("FEHLER  {}: {e}", pfad.display()),
        }
    }
    let _ = std::fs::write(format!("{ORDNER}/liste.txt"), &liste);
    println!("{} Seiten in {ORDNER}/", liste.lines().count());
}
