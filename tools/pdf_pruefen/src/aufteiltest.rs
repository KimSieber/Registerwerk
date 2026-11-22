//! Probe des Aufteilens zu großer Scheiben (v0.0.8): kopiert das Register
//! nach `target/aufteilen_probe/`, teilt dort die Scheiben und prüft, dass
//! danach jede Vorschau Byte für Byte dieselbe ist. Das Register der
//! Anwendung bleibt unberührt. Die Anwendung vorher schließen.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use registerwerk_core::Register;

const ZIEL: &str = "target/aufteilen_probe";

fn gehoert_dazu(name: &str) -> bool {
    name.starts_with("register.db") || (name.starts_with("vorschau-") && name.contains(".db"))
}

fn dateien(ordner: &Path) -> Vec<(String, u64)> {
    let mut liste: Vec<(String, u64)> = std::fs::read_dir(ordner)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            gehoert_dazu(&name).then(|| (name, e.metadata().map(|m| m.len()).unwrap_or(0)))
        })
        .collect();
    liste.sort();
    liste
}

fn kopieren(quelle: &Path, ziel: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(ziel)?;
    // Reste einer früheren Probe
    for (name, _) in dateien(ziel) {
        std::fs::remove_file(ziel.join(name))?;
    }
    let mut summe = 0;
    for (name, bytes) in dateien(quelle) {
        std::fs::copy(quelle.join(&name), ziel.join(&name))?;
        summe += bytes;
    }
    Ok(summe)
}

/// Prüfsumme jeder Vorschau, nach (Datei, Seite).
fn abdruck(r: &Register) -> HashMap<(i64, u32), (usize, u64)> {
    let mut alle = HashMap::new();
    for (datei, nr) in r.vorschau_verweise().expect("Verweise lesen") {
        if let Some(avif) = r.vorschau(datei, nr).expect("Vorschau lesen") {
            let mut h = DefaultHasher::new();
            avif.hash(&mut h);
            alle.insert((datei, nr), (avif.len(), h.finish()));
        }
    }
    alle
}

fn zeigen(ordner: &Path) {
    for (name, bytes) in dateien(ordner) {
        println!("  {name:<24} {:>9.1} MB", bytes as f64 / 1e6);
    }
}

pub fn lauf(quelle: Option<PathBuf>) {
    let Some(quelle) = quelle else {
        eprintln!("Aufruf: --aufteilen <Ordner mit register.db>");
        std::process::exit(2);
    };
    if !quelle.join("register.db").exists() {
        eprintln!("Kein Register in {}", quelle.display());
        std::process::exit(2);
    }
    let ziel = PathBuf::from(ZIEL);
    let start = Instant::now();
    match kopieren(&quelle, &ziel) {
        Ok(bytes) => println!("Register kopiert nach {} ({:.1} MB, {:.1} s)", ziel.display(), bytes as f64 / 1e6, start.elapsed().as_secs_f64()),
        Err(e) => {
            eprintln!("Kopieren: {e}");
            std::process::exit(1);
        }
    }
    println!("Vorher:");
    zeigen(&ziel);

    let ergebnis = {
        let mut r = Register::oeffnen(&ziel).expect("Register öffnen");
        let vorher = abdruck(&r);
        println!("{} Vorschauen, Aufteilen nötig: {}", vorher.len(), r.aufteilen_noetig().expect("Stand"));
        let mut zuletzt = Instant::now();
        let e = r
            .scheiben_aufteilen(
                |f| {
                    if zuletzt.elapsed().as_secs() >= 2 {
                        zuletzt = Instant::now();
                        println!("  {} von {} Dokumenten, {}", f.erledigt, f.gesamt, f.datei);
                    }
                },
                &AtomicBool::new(false),
            )
            .expect("Aufteilen");
        let nachher = abdruck(&r);
        let gleich = vorher == nachher;
        println!(
            "Aufgeteilt in {:.1} s: {} Vorschauen aus {} Dokumenten ({:.1} MB) in {} neue Scheiben, {} verkleinert",
            e.dauer_ms as f64 / 1000.0,
            e.bilder,
            e.dateien,
            e.bytes as f64 / 1e6,
            e.neue_scheiben,
            e.verdichtet
        );
        println!("Aufteilen noch nötig: {}", r.aufteilen_noetig().expect("Stand"));
        println!("Alle {} Vorschauen unverändert lesbar: {}", nachher.len(), if gleich { "ja" } else { "NEIN" });
        gleich
    };
    // Erst nach dem Schließen stehen die endgültigen Größen fest
    println!("Nachher:");
    zeigen(&ziel);
    if !ergebnis {
        std::process::exit(1);
    }
}
