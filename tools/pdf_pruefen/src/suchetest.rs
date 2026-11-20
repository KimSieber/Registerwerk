//! Messung der Suche (v0.0.6): baut den Suchindex in einer Kopie des
//! Registers auf und misst Größe, Dauer und Antwortzeiten. Das Register der
//! Anwendung bleibt unberührt.
//!
//! Die Suchanfragen werden aus der Wortliste gezogen: häufige, mittlere und
//! seltene Wörter, Wörter mit Tippfehler, auseinandergerissene Wörter und
//! zwei Wörter derselben Seite. Die Ausgabe enthält damit Wörter aus den
//! Dokumenten und gehört nach `target/`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use registerwerk_core::suche::Anfrage;
use registerwerk_core::Register;

/// Seiten des ganzen Archivs für die Hochrechnung (Konzept, Abschnitt 13).
const ARCHIV_SEITEN: f64 = 1_500_000.0;
const ZIEL: &str = "target/suche_messung";
const DATEIEN: [&str; 3] = ["register.db", "register.db-wal", "register.db-shm"];

/// Standardort des Registers unter macOS (Datenordner der Anwendung).
fn standard_quelle() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Application Support/de.sieber-bw.registerwerk"))
}

pub fn lauf(quelle: Option<PathBuf>, neu: bool, anfragen: &[String]) {
    let ziel = PathBuf::from(ZIEL);
    let vorhanden = ziel.join(DATEIEN[0]).exists();
    if neu || !vorhanden {
        let Some(quelle) = quelle.or_else(standard_quelle) else {
            eprintln!("Register-Ordner nicht gefunden. Aufruf: --suche <Ordner mit register.db>");
            std::process::exit(2);
        };
        if !quelle.join(DATEIEN[0]).exists() {
            eprintln!("Kein Register in {}", quelle.display());
            std::process::exit(2);
        }
        if let Err(e) = kopieren(&quelle, &ziel) {
            eprintln!("Kopieren: {e}");
            std::process::exit(1);
        }
        println!("Register kopiert: {} -> {}", quelle.display(), ziel.display());
        messen(&ziel);
    }
    if !anfragen.is_empty() {
        let r = oeffnen(&ziel);
        for a in anfragen {
            anfrage_zeigen(&r, a);
        }
    }
}

fn kopieren(quelle: &Path, ziel: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(ziel)?;
    for d in DATEIEN {
        let (von, nach) = (quelle.join(d), ziel.join(d));
        if von.exists() {
            std::fs::copy(&von, &nach)?;
        } else if nach.exists() {
            std::fs::remove_file(&nach)?;
        }
    }
    Ok(())
}

fn oeffnen(ordner: &Path) -> Register {
    match Register::oeffnen(ordner) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Register öffnen: {e}");
            std::process::exit(1);
        }
    }
}

fn mb(bytes: f64) -> String {
    format!("{:.1} MB", bytes / 1_000_000.0)
}

fn dateigroesse(r: &Register) -> f64 {
    r.sql_ausfuehren("PRAGMA wal_checkpoint(TRUNCATE)").ok();
    r.pfad().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len() as f64).unwrap_or(0.0)
}

fn messen(ordner: &Path) {
    let mut r = oeffnen(ordner);
    // Ausgangsgröße ohne Freiraum, ohne Suchindex
    r.sql_ausfuehren("INSERT INTO suche_datei (suche_datei) VALUES ('delete-all');
                      INSERT INTO suche_kompakt (suche_kompakt) VALUES ('delete-all');
                      INSERT INTO suche_wort (suche_wort) VALUES ('delete-all');
                      UPDATE datei SET suche_stand = NULL;
                      VACUUM;")
        .expect("Register leeren");
    let vorher = dateigroesse(&r);

    let start = Instant::now();
    let aufbau = r.suche_neu_aufbauen(|_| {}, &AtomicBool::new(false)).expect("Aufbau");
    let dauer_aufbau = start.elapsed().as_secs_f64();
    r.sql_ausfuehren("VACUUM").expect("VACUUM");
    let nachher = dateigroesse(&r);

    let zahl = |sql: &str| r.sql_zahl(sql).unwrap_or(-1) as f64;
    let seiten = zahl("SELECT count(*) FROM seite_text WHERE text <> ''");
    let dateien = zahl("SELECT count(*) FROM datei");
    let text_bytes = zahl("SELECT coalesce(sum(length(CAST(text AS BLOB))), 0) FROM seite_text");
    let woerter = zahl("SELECT count(*) FROM suche_wortliste");
    let faktor = if seiten > 0.0 { ARCHIV_SEITEN / seiten } else { 0.0 };

    println!();
    println!("== Aufbau");
    println!("Dateien:               {dateien}");
    println!("Seiten mit Text:       {seiten}");
    println!("Text (UTF-8):          {}  ({:.0} Byte je Seite)", mb(text_bytes), text_bytes / seiten.max(1.0));
    println!("Wörter in der Liste:   {woerter}");
    println!("Dauer Aufbau:          {:.1} s  ({:.2} ms je Seite, {} Dateien, {} Seiten)",
        dauer_aufbau, dauer_aufbau * 1000.0 / seiten.max(1.0), aufbau.dateien, aufbau.seiten);
    println!("Register ohne Index:   {}", mb(vorher));
    println!("Register mit Index:    {}", mb(nachher));
    println!("Suchindex gesamt:      {}  ({:.0} Byte je Seite, {:.1}-mal der Text)",
        mb(nachher - vorher), (nachher - vorher) / seiten.max(1.0), (nachher - vorher) / text_bytes.max(1.0));

    println!();
    println!("== Größe je Tabelle (mit Hochrechnung auf {:.1} Mio. Seiten, Faktor {:.0})", ARCHIV_SEITEN / 1e6, faktor);
    let mut summe = 0.0;
    for t in ["suche_datei", "suche_kompakt", "suche_wort"] {
        let g = r.sql_zahl(&format!("SELECT sum(pgsize) FROM dbstat WHERE name LIKE '{t}%'")).map(|v| v as f64)
            .or_else(|_| r.sql_zahl(&format!("SELECT sum(length(block)) FROM {t}_data")).map(|v| v as f64))
            .unwrap_or(0.0);
        summe += g;
        let hoch = if t == "suche_datei" { "(je Datei)".to_string() } else { format!("{:.1} GB", g * faktor / 1e9) };
        println!("{t:<15} {:>10}   Archiv {hoch}", mb(g));
    }
    println!("{:<15} {:>10}   Archiv {:.1} GB (ohne suche_datei)", "zusammen", mb(summe), (summe) * faktor / 1e9);

    // Wortliste
    let start = Instant::now();
    let liste = r.wortliste().expect("Wortliste");
    println!();
    println!("== Wortliste");
    println!("Aufbau im Speicher:    {:.0} ms, {} Wörter ab 3 Zeichen, {}",
        start.elapsed().as_secs_f64() * 1000.0, liste.len(), mb(liste.as_fst().size() as f64));

    // Anfragen
    let gruppen = anfragen_ziehen(&r);
    println!();
    println!("== Antwortzeiten (je Anfrage 3 Läufe, Median; Zeiten in ms)");
    println!("{:<14} {:>4} {:>8} {:>8} {:>8} {:>10}", "Art", "Anz.", "Median", "95 %", "max", "Treffer Ø");
    let mut beispiele = Vec::new();
    for (art, liste) in &gruppen {
        let mut zeiten = Vec::new();
        let mut treffer = 0usize;
        for q in liste {
            let mut laeufe = Vec::new();
            let mut gesamt = 0;
            for _ in 0..3 {
                let t = Instant::now();
                let e = r.suchen(&Anfrage { text: q.clone(), ..Default::default() }).expect("Suche");
                laeufe.push(t.elapsed().as_secs_f64() * 1000.0);
                gesamt = e.gesamt;
            }
            laeufe.sort_by(f64::total_cmp);
            zeiten.push(laeufe[1]);
            treffer += gesamt;
            beispiele.push((art.to_string(), q.clone(), gesamt, laeufe[1]));
        }
        zeiten.sort_by(f64::total_cmp);
        if zeiten.is_empty() {
            continue;
        }
        let p = |q: f64| zeiten[((zeiten.len() - 1) as f64 * q).round() as usize];
        println!("{art:<14} {:>4} {:>8.1} {:>8.1} {:>8.1} {:>10.0}",
            zeiten.len(), p(0.5), p(0.95), zeiten[zeiten.len() - 1], treffer as f64 / zeiten.len() as f64);
    }
    println!();
    println!("== Anfragen im Einzelnen");
    for (art, q, gesamt, ms) in beispiele {
        println!("{art:<14} {ms:>7.1} ms {gesamt:>6} Treffer  {q}");
    }
    println!();
    println!("Hinweis: Antwortzeiten wachsen mit der Zahl der Trefferseiten. Häufige Wörter");
    println!("im ganzen Archiv grob um den Faktor {faktor:.0} langsamer, seltene kaum.");
}

/// Einfacher Zufall mit festem Startwert, damit Messungen vergleichbar bleiben.
struct Zufall(u64);
impl Zufall {
    fn zahl(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

fn anfragen_ziehen(r: &Register) -> Vec<(&'static str, Vec<String>)> {
    let mut z = Zufall(20261119);
    // Wörter aus Buchstaben, ab 5 Zeichen, nach Häufigkeit
    let mut woerter: Vec<(String, i64)> = r
        .sql_liste("SELECT term, doc FROM suche_wortliste WHERE length(term) >= 5 AND term NOT GLOB '*[0-9]*' ORDER BY doc DESC, term")
        .unwrap_or_default();
    woerter.retain(|(w, _)| w.chars().all(char::is_alphabetic));
    let n = woerter.len();
    let mut gruppen: Vec<(&'static str, Vec<String>)> = Vec::new();
    if n == 0 {
        return gruppen;
    }
    let haeufig: Vec<String> = woerter.iter().take(20).map(|w| w.0.clone()).collect();
    let mit_mehreren: Vec<&(String, i64)> = woerter.iter().filter(|w| w.1 >= 5).collect();
    let mittel: Vec<String> = (0..20)
        .filter_map(|i| mit_mehreren.get(mit_mehreren.len() / 4 + i * mit_mehreren.len() / 60).map(|w| w.0.clone()))
        .collect();
    let einmalig: Vec<&(String, i64)> = woerter.iter().filter(|w| w.1 == 1 && w.0.chars().count() >= 6).collect();
    let selten: Vec<String> = (0..20).filter_map(|_| einmalig.get(z.zahl(einmalig.len())).map(|w| w.0.clone())).collect();
    let quelle: Vec<String> = mittel.iter().chain(haeufig.iter()).filter(|w| w.chars().count() >= 6).cloned().collect();
    let tippfehler: Vec<String> = quelle
        .iter()
        .take(20)
        .map(|w| {
            let c: Vec<char> = w.chars().collect();
            let i = 1 + z.zahl(c.len() - 2);
            c.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| c).collect()
        })
        .collect();
    let getrennt: Vec<String> = quelle
        .iter()
        .take(20)
        .map(|w| {
            let c: Vec<char> = w.chars().collect();
            let i = 3.min(c.len() - 3) + z.zahl(c.len().saturating_sub(5).max(1));
            let (a, b) = c.split_at(i.min(c.len() - 3));
            format!("{} {}", a.iter().collect::<String>(), b.iter().collect::<String>())
        })
        .collect();
    // zwei Wörter derselben Seite
    let seiten: Vec<(String, i64)> = r
        .sql_liste("SELECT text, 0 FROM seite_text WHERE length(text) > 300 LIMIT 5000")
        .unwrap_or_default();
    let mut zwei = Vec::new();
    for _ in 0..200 {
        if zwei.len() >= 20 || seiten.is_empty() {
            break;
        }
        let text = &seiten[z.zahl(seiten.len())].0;
        let w: Vec<&str> = text
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| w.chars().count() >= 5)
            .collect();
        if w.len() >= 2 {
            let (a, b) = (w[z.zahl(w.len())], w[z.zahl(w.len())]);
            if a != b {
                zwei.push(format!("{a} {b}"));
            }
        }
    }
    gruppen.push(("häufig", haeufig));
    gruppen.push(("mittel", mittel));
    gruppen.push(("selten", selten));
    gruppen.push(("Tippfehler", tippfehler));
    gruppen.push(("getrennt", getrennt));
    gruppen.push(("zwei Wörter", zwei));
    gruppen
}

fn anfrage_zeigen(r: &Register, text: &str) {
    let e = match r.suchen(&Anfrage { text: text.into(), ..Default::default() }) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Suche: {e}");
            return;
        }
    };
    println!();
    println!("== Anfrage \"{text}\": {} Treffer in {:.1} ms", e.gesamt, e.dauer_ms);
    if !e.uebergangen.is_empty() {
        println!("   übergangen (zu kurz): {}", e.uebergangen.join(", "));
    }
    for (w, v) in &e.aehnliche {
        println!("   ähnlich zu {w}: {}", v.join(", "));
    }
    for t in e.treffer.iter().take(15) {
        println!(
            "{:>6.2} {}{} {:<10} S. {:<12} {}",
            t.punkte,
            if t.im_namen { "N" } else { " " },
            if t.unscharf { "~" } else { " " },
            t.datum.as_deref().unwrap_or("-"),
            t.trefferseiten.iter().take(5).map(|s| s.to_string()).collect::<Vec<_>>().join(","),
            t.pfad
        );
    }
}
