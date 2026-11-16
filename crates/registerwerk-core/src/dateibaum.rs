//! Durchlauf des Dateibaums.
//!
//! Liest ausschließlich Metadaten (Name, Größe, Änderungsdatum). Es wird keine
//! Datei geöffnet, damit der Cloud-Client nichts herunterlädt (Konzept,
//! Abschnitt 5).

use std::path::Path;
use std::time::UNIX_EPOCH;

use unicode_normalization::UnicodeNormalization;
use walkdir::WalkDir;

use crate::Konfiguration;

/// Verwaltungsdateien von Cloud-Client und Betriebssystem, die nie ins Register
/// gehören.
const UEBERGANGENE_NAMEN: &[&str] = &[
    ".DS_Store",
    ".dropbox",
    ".dropbox.attr",
    ".dropbox.cache",
    "Icon\r",
    "desktop.ini",
    "Thumbs.db",
];

#[derive(Debug, Clone, PartialEq)]
pub struct DateiEintrag {
    /// Pfad relativ zum Cloud-Ordner, mit `/` getrennt, Unicode in NFC.
    pub pfad: String,
    pub name: String,
    /// Endung in Kleinbuchstaben ohne Punkt, leer wenn keine vorhanden.
    pub endung: String,
    pub groesse: u64,
    /// Änderungsdatum in Sekunden seit 1970 (UTC).
    pub geaendert: i64,
}

impl DateiEintrag {
    /// Platzhalter melden 0 Byte und sind von einer leeren Datei nicht zu
    /// unterscheiden. Beides gilt als "nur online oder leer".
    pub fn nur_online_oder_leer(&self) -> bool {
        self.groesse == 0
    }
}

#[derive(Debug, Default)]
pub struct Durchlauf {
    pub dateien: Vec<DateiEintrag>,
    /// Verzeichnisse, die vollständig gelesen wurden. Nur für sie darf das
    /// Register Einträge als entfernt behandeln.
    pub gelesene_verzeichnisse: Vec<String>,
    /// Konfigurierte Verzeichnisse, die es auf diesem Gerät nicht gibt.
    pub fehlende_verzeichnisse: Vec<String>,
    /// Verzeichnisse mit Lesefehlern. Sie gelten nicht als vollständig gelesen.
    pub unvollstaendige_verzeichnisse: Vec<String>,
    /// Einträge, deren Metadaten sich nicht lesen ließen.
    pub nicht_lesbar: Vec<String>,
}

/// Vereinheitlicht einen relativen Pfad: `/` als Trenner, keine führenden oder
/// abschließenden Trenner, Unicode in NFC. macOS liefert Umlaute zerlegt (NFD),
/// andere Systeme zusammengesetzt. Ohne Vereinheitlichung wäre dieselbe Datei
/// je nach Gerät ein anderer Eintrag.
pub fn relativ_normalisieren(pfad: &str) -> String {
    pfad.replace('\\', "/")
        .split('/')
        .filter(|teil| !teil.is_empty() && *teil != ".")
        .collect::<Vec<_>>()
        .join("/")
        .nfc()
        .collect()
}

fn uebergehen(name: &str) -> bool {
    UEBERGANGENE_NAMEN.contains(&name) || name.starts_with("._") || name.starts_with(".~lock.")
}

pub fn durchlaufen(konfig: &Konfiguration) -> Durchlauf {
    let mut ergebnis = Durchlauf::default();
    for verzeichnis in &konfig.verzeichnisse {
        let wurzel = konfig.cloud_ordner.join(verzeichnis);
        if !wurzel.is_dir() {
            ergebnis.fehlende_verzeichnisse.push(verzeichnis.clone());
            continue;
        }
        let fehler_vorher = ergebnis.nicht_lesbar.len();
        verzeichnis_lesen(&konfig.cloud_ordner, &wurzel, &mut ergebnis);
        // Ein Verzeichnis mit Lesefehlern gilt nicht als vollständig gelesen.
        // Sonst würden die Einträge eines kurzzeitig unlesbaren Unterordners
        // aus dem Register entfernt.
        if ergebnis.nicht_lesbar.len() == fehler_vorher {
            ergebnis.gelesene_verzeichnisse.push(verzeichnis.clone());
        } else {
            ergebnis.unvollstaendige_verzeichnisse.push(verzeichnis.clone());
        }
    }
    ergebnis
}

fn verzeichnis_lesen(cloud_ordner: &Path, wurzel: &Path, ergebnis: &mut Durchlauf) {
    let lauf = WalkDir::new(wurzel)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !uebergehen(&e.file_name().to_string_lossy()));
    for eintrag in lauf {
        let eintrag = match eintrag {
            Ok(e) => e,
            Err(e) => {
                ergebnis.nicht_lesbar.push(
                    e.path().map(|p| p.display().to_string()).unwrap_or_else(|| e.to_string()),
                );
                continue;
            }
        };
        if !eintrag.file_type().is_file() {
            continue;
        }
        let meta = match eintrag.metadata() {
            Ok(m) => m,
            Err(_) => {
                ergebnis.nicht_lesbar.push(eintrag.path().display().to_string());
                continue;
            }
        };
        let Ok(relativ) = eintrag.path().strip_prefix(cloud_ordner) else {
            continue;
        };
        let pfad = relativ_normalisieren(&relativ.to_string_lossy());
        let name = pfad.rsplit('/').next().unwrap_or_default().to_string();
        let endung = match name.rsplit_once('.') {
            Some((vorn, hinten)) if !vorn.is_empty() && !hinten.is_empty() && !hinten.contains(' ') => {
                hinten.to_lowercase()
            }
            _ => String::new(),
        };
        let geaendert = meta
            .modified()
            .ok()
            .map(|zeit| match zeit.duration_since(UNIX_EPOCH) {
                Ok(d) => d.as_secs() as i64,
                Err(e) => -(e.duration().as_secs() as i64),
            })
            .unwrap_or(0);
        ergebnis.dateien.push(DateiEintrag {
            pfad,
            name,
            endung,
            groesse: meta.len(),
            geaendert,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn konfig(cloud: &Path, verzeichnisse: &[&str]) -> Konfiguration {
        Konfiguration {
            cloud_ordner: cloud.to_path_buf(),
            verzeichnisse: verzeichnisse.iter().map(|s| s.to_string()).collect(),
            register_ordner: None,
        }
    }

    #[test]
    fn liest_dateien_mit_relativen_pfaden() {
        let tmp = tempfile::tempdir().unwrap();
        let alles = tmp.path().join("Archiv/Alles/Unterordner");
        fs::create_dir_all(&alles).unwrap();
        fs::write(tmp.path().join("Archiv/Alles/2020-01-02 Brief.PDF"), b"abc").unwrap();
        fs::write(alles.join("Platzhalter.pdf"), b"").unwrap();
        fs::write(alles.join("ohne endung"), b"x").unwrap();
        fs::write(tmp.path().join("Archiv/Alles/.DS_Store"), b"x").unwrap();
        fs::create_dir_all(tmp.path().join("Archiv/Alles/.dropbox.cache")).unwrap();
        fs::write(tmp.path().join("Archiv/Alles/.dropbox.cache/rest.pdf"), b"x").unwrap();
        fs::write(tmp.path().join("Archiv/ausserhalb.pdf"), b"x").unwrap();

        let d = durchlaufen(&konfig(tmp.path(), &["Archiv/Alles", "Archiv/Fehlt"]));
        let mut pfade: Vec<_> = d.dateien.iter().map(|e| e.pfad.as_str()).collect();
        pfade.sort();
        assert_eq!(
            pfade,
            vec![
                "Archiv/Alles/2020-01-02 Brief.PDF",
                "Archiv/Alles/Unterordner/Platzhalter.pdf",
                "Archiv/Alles/Unterordner/ohne endung",
            ]
        );
        let brief = d.dateien.iter().find(|e| e.name.ends_with("Brief.PDF")).unwrap();
        assert_eq!((brief.endung.as_str(), brief.groesse), ("pdf", 3));
        assert!(brief.geaendert > 1_600_000_000);
        let leer = d.dateien.iter().find(|e| e.name == "Platzhalter.pdf").unwrap();
        assert!(leer.nur_online_oder_leer());
        assert_eq!(d.dateien.iter().find(|e| e.name == "ohne endung").unwrap().endung, "");
        assert_eq!(d.gelesene_verzeichnisse, vec!["Archiv/Alles"]);
        assert_eq!(d.fehlende_verzeichnisse, vec!["Archiv/Fehlt"]);
    }

    #[test]
    fn vereinheitlicht_umlaute() {
        // "ä" zerlegt (a + Trema) wird zu einem Zeichen
        assert_eq!(relativ_normalisieren("Archiv/Scans fu\u{0308}r/"), "Archiv/Scans f\u{00fc}r");
    }
}
