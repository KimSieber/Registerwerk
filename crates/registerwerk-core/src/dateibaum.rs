//! Durchlauf des Dateibaums.
//!
//! Liest ausschließlich Metadaten (Name, Größe, Änderungsdatum). Es wird keine
//! Datei geöffnet, damit der Cloud-Client nichts herunterlädt (Konzept,
//! Abschnitt 5).
//!
//! Seit v0.0.7 bestimmen die Ordnerregeln (`regeln.rs`), welche Ordner gelesen
//! werden. Ordner fremder Bibliotheken in Entwicklungsprojekten werden
//! übergangen.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use unicode_normalization::UnicodeNormalization;

use crate::formate::{self, Klasse};
use crate::regeln::{Regeln, Stufe};
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

/// Ordner fremder Bibliotheken und Bauergebnisse, die überall übergangen
/// werden. Die Namen sind eindeutig. Dazu kommen alle Ordner, deren Name mit
/// einem Punkt beginnt (`.git`, `.venv`, `.pio`).
const BIBLIOTHEKEN_IMMER: &[&str] =
    &["node_modules", "__pycache__", "site-packages", "venv", "Pods", "DerivedData", "bower_components"];

/// Ordner, die nur in einem Entwicklungsprojekt übergangen werden: wenn im
/// selben Ordner eine der [`PROJEKTDATEIEN`] liegt. Die Namen sind mehrdeutig,
/// ein Archivordner "build" oder "vendor" bleibt so erhalten.
const BIBLIOTHEKEN_IM_PROJEKT: &[&str] = &["target", "build", "dist", "vendor"];

const PROJEKTDATEIEN: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "composer.json",
    "platformio.ini",
    "pyproject.toml",
    "setup.py",
    "requirements.txt",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "CMakeLists.txt",
    "Makefile",
    "go.mod",
    "Package.swift",
    "Gemfile",
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
    /// Stufe des Ordners: Voll oder Metadaten.
    pub stufe: Stufe,
}

/// Kennzeichen für Dateien ohne lokalen Inhalt (`SF_DATALESS` aus
/// `<sys/stat.h>`): Platzhalter des File Providers von macOS (Google Drive,
/// OneDrive, iCloud, Dropbox auf File Provider). Sie melden die echte Größe,
/// nicht 0 Byte wie die klassische Dropbox.
#[cfg(target_os = "macos")]
const SF_DATALESS: u32 = 0x4000_0000;

/// Hat die Datei keinen lokalen Inhalt, also nur online (v0.0.8)?
#[cfg(target_os = "macos")]
pub fn ohne_inhalt(meta: &std::fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    meta.st_flags() & SF_DATALESS != 0
}

#[cfg(not(target_os = "macos"))]
pub fn ohne_inhalt(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Größe der lokal vorhandenen Datei: 0 für Platzhalter, auch für solche des
/// File Providers, die sonst ihre echte Größe melden. Ein Platzhalter mit
/// Größe würde beim Lesen still heruntergeladen (v0.0.8).
pub fn lokale_groesse(meta: &std::fs::Metadata) -> u64 {
    if ohne_inhalt(meta) {
        0
    } else {
        meta.len()
    }
}

impl DateiEintrag {
    /// Platzhalter melden 0 Byte und sind von einer leeren Datei nicht zu
    /// unterscheiden. Beides gilt als "nur online oder leer". Platzhalter des
    /// File Providers liefert der Durchlauf ebenfalls mit 0 Byte
    /// ([`lokale_groesse`]).
    pub fn nur_online_oder_leer(&self) -> bool {
        self.groesse == 0
    }
}

#[derive(Debug, Default)]
pub struct Durchlauf {
    pub dateien: Vec<DateiEintrag>,
    /// Name des Geräts, das gelesen hat (`Konfiguration::geraet`).
    pub geraet: String,
    /// Alle Ordner, die es auf diesem Gerät gibt und die betreten wurden,
    /// als Pfad im Register (`Ast/Ordner`). Fehlt der Ordner einer Datei
    /// hier, sagt das Fehlen der Datei nichts: Der Ordner liegt dann auf
    /// einem anderen Gerät oder ist gerade nicht erreichbar.
    pub ordner: std::collections::BTreeSet<String>,
    /// Die Regeln, nach denen gelesen wurde. Das Register entscheidet damit,
    /// welche nicht mehr gesehenen Einträge es entfernt.
    pub regeln: Regeln,
    /// Ordner mit Regel, die vollständig gelesen wurden.
    pub gelesene_verzeichnisse: Vec<String>,
    /// Ordner mit Regel, die es auf diesem Gerät nicht gibt, auch weil der
    /// ganze Ast hier fehlt. Ihr Bestand im Register bleibt unangetastet.
    pub fehlende_verzeichnisse: Vec<String>,
    /// Ordner mit Regel, in denen es Lesefehler gab. Sie gelten nicht als
    /// vollständig gelesen, ihr Bestand im Register bleibt unangetastet.
    pub unvollstaendige_verzeichnisse: Vec<String>,
    /// Einträge, deren Metadaten sich nicht lesen ließen.
    pub nicht_lesbar: Vec<String>,
    /// Dateien in Ordnern der Stufe Voll, die wegen ihres Typs nicht ins
    /// Register kommen (`formate::Klasse::Ignoriert`).
    pub ignoriert: u64,
    /// Übergangene Ordner fremder Bibliotheken.
    pub uebergangene_ordner: u64,
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

/// Ist `name` im Ordner `eltern` ein Ordner fremder Bibliotheken?
fn ist_bibliothek(name: &str, eltern: &Path) -> bool {
    if name.starts_with('.') || BIBLIOTHEKEN_IMMER.contains(&name) {
        return true;
    }
    BIBLIOTHEKEN_IM_PROJEKT.contains(&name) && PROJEKTDATEIEN.iter().any(|p| eltern.join(p).is_file())
}

/// Wäre ein Ordner mit diesem Namen übergangen worden? Ohne Blick ins
/// Dateisystem, für die Frage, warum ein Eintrag aus dem Register fällt.
pub fn name_wie_bibliothek(name: &str) -> bool {
    name.starts_with('.') || BIBLIOTHEKEN_IMMER.contains(&name) || BIBLIOTHEKEN_IM_PROJEKT.contains(&name)
}

/// Endung in Kleinbuchstaben ohne Punkt, leer wenn keine vorhanden.
pub fn endung_von(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((vorn, hinten)) if !vorn.is_empty() && !hinten.is_empty() && !hinten.contains(' ') => {
            hinten.to_lowercase()
        }
        _ => String::new(),
    }
}

/// Was der Lauf durch einen Ordner findet.
enum Fund<'a> {
    /// Ein Ordner wird betreten.
    Ordner(&'a Path),
    Datei { pfad: &'a Path, meta: std::fs::Metadata },
    /// Ordner fremder Bibliotheken, nicht betreten.
    Bibliothek(&'a Path),
    NichtLesbar(String),
}

/// Läuft durch `ordner` und alle Unterordner. Liest nur Verzeichnisse und
/// Metadaten. Verknüpfungen wird nicht gefolgt. `eigene_regel` nennt
/// Unterordner, an denen der Lauf endet, weil für sie eine eigene Regel gilt.
fn baum_lesen(
    ordner: &Path,
    eigene_regel: &dyn Fn(&Path) -> bool,
    fund: &mut dyn FnMut(Fund),
    abbrechen: &AtomicBool,
) {
    if abbrechen.load(Ordering::Relaxed) {
        return;
    }
    let eintraege = match std::fs::read_dir(ordner) {
        Ok(e) => e,
        Err(_) => return fund(Fund::NichtLesbar(ordner.display().to_string())),
    };
    fund(Fund::Ordner(ordner));
    for eintrag in eintraege {
        let eintrag = match eintrag {
            Ok(e) => e,
            Err(_) => {
                fund(Fund::NichtLesbar(ordner.display().to_string()));
                continue;
            }
        };
        let name = eintrag.file_name();
        let name = name.to_string_lossy();
        if uebergehen(&name) {
            continue;
        }
        let pfad = eintrag.path();
        let Ok(typ) = eintrag.file_type() else {
            fund(Fund::NichtLesbar(pfad.display().to_string()));
            continue;
        };
        if typ.is_dir() {
            if eigene_regel(&pfad) {
                continue;
            }
            if ist_bibliothek(&name, ordner) {
                fund(Fund::Bibliothek(&pfad));
                continue;
            }
            baum_lesen(&pfad, eigene_regel, fund, abbrechen);
        } else if typ.is_file() {
            match eintrag.metadata() {
                Ok(meta) => fund(Fund::Datei { pfad: &pfad, meta }),
                Err(_) => fund(Fund::NichtLesbar(pfad.display().to_string())),
            }
        }
    }
}

/// Pfad unterhalb von `wurzel`, vereinheitlicht. Leer für die Wurzel selbst.
fn relativ(wurzel: &Path, pfad: &Path) -> Option<String> {
    pfad.strip_prefix(wurzel).ok().map(|r| relativ_normalisieren(&r.to_string_lossy()))
}

/// Pfad im Register: der Ordner der Regel (`Ast/…`) und der Rest darunter.
fn im_register(wurzel: &Path, verzeichnis: &str, pfad: &Path) -> Option<String> {
    let rest = relativ(wurzel, pfad)?;
    Some(if rest.is_empty() { verzeichnis.to_string() } else { format!("{verzeichnis}/{rest}") })
}

fn geaendert_von(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .map(|zeit| match zeit.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(e) => -(e.duration().as_secs() as i64),
        })
        .unwrap_or(0)
}

/// Liest alle Ordner, für die eine Regel außer Ausgeschlossen gilt. Der erste
/// Teil jedes Pfads ist der Ast, sein Ort steht in der Konfiguration.
pub fn durchlaufen(konfig: &Konfiguration, regeln: &Regeln) -> Durchlauf {
    let mut ergebnis = Durchlauf { regeln: regeln.clone(), geraet: konfig.geraet.clone(), ..Default::default() };
    let nie = AtomicBool::new(false);
    for (verzeichnis, stufe) in regeln.wurzeln() {
        let Some(wurzel) = konfig.ort(verzeichnis).filter(|w| w.is_dir()) else {
            ergebnis.fehlende_verzeichnisse.push(verzeichnis.to_string());
            continue;
        };
        // Gibt es den Ordner, gibt es auch seine Oberordner bis zum Ast
        let mut ober = verzeichnis;
        while let Some((vorn, _)) = ober.rsplit_once('/') {
            ergebnis.ordner.insert(vorn.to_string());
            ober = vorn;
        }
        let fehler_vorher = ergebnis.nicht_lesbar.len();
        baum_lesen(
            &wurzel,
            &|ordner| im_register(&wurzel, verzeichnis, ordner).is_some_and(|r| regeln.eigene(&r).is_some()),
            &mut |fund| match fund {
                Fund::NichtLesbar(pfad) => ergebnis.nicht_lesbar.push(pfad),
                Fund::Bibliothek(_) => ergebnis.uebergangene_ordner += 1,
                Fund::Ordner(pfad) => {
                    if let Some(ordner) = im_register(&wurzel, verzeichnis, pfad) {
                        ergebnis.ordner.insert(ordner);
                    }
                }
                Fund::Datei { pfad, meta } => {
                    let Some(pfad) = im_register(&wurzel, verzeichnis, pfad) else { return };
                    let name = pfad.rsplit('/').next().unwrap_or_default().to_string();
                    let endung = endung_von(&name);
                    if stufe == Stufe::Voll && formate::klasse(&endung) == Klasse::Ignoriert {
                        ergebnis.ignoriert += 1;
                        return;
                    }
                    ergebnis.dateien.push(DateiEintrag {
                        pfad,
                        name,
                        endung,
                        groesse: lokale_groesse(&meta),
                        geaendert: geaendert_von(&meta),
                        stufe,
                    });
                }
            },
            &nie,
        );
        // Ein Ordner mit Lesefehlern gilt nicht als vollständig gelesen.
        // Sonst würden die Einträge eines kurzzeitig unlesbaren Unterordners
        // aus dem Register entfernt.
        if ergebnis.nicht_lesbar.len() == fehler_vorher {
            ergebnis.gelesene_verzeichnisse.push(verzeichnis.to_string());
        } else {
            ergebnis.unvollstaendige_verzeichnisse.push(verzeichnis.to_string());
        }
    }
    ergebnis
}

/// Zählung je Endung in einem geprüften Ordner.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EndungZeile {
    pub endung: String,
    pub klasse: Klasse,
    pub anzahl: u64,
    /// Summe der Größen der lokal vorhandenen Dateien.
    pub bytes: u64,
    pub nur_online: u64,
}

/// Zählung je direktem Unterordner. Der leere Name steht für die Dateien
/// direkt im geprüften Ordner.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct UnterordnerZeile {
    pub name: String,
    pub gelesen: u64,
    pub nur_name: u64,
    pub ignoriert: u64,
    pub bytes: u64,
    pub nur_online: u64,
}

/// Was in einem Ordner samt Unterordnern liegt und wie es in der Stufe Voll
/// behandelt würde (v0.0.7). Hilft vor dem Einlesen bei der Wahl der Regeln.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct OrdnerPruefung {
    pub ordner: String,
    pub dauer_ms: u64,
    pub abgebrochen: bool,
    /// Alle gefundenen Dateien, ohne die übergangenen Ordner.
    pub dateien: u64,
    pub gelesen: u64,
    pub nur_name: u64,
    pub ignoriert: u64,
    /// Dateien mit 0 Byte ("nur online oder leer"). Ihre Größe ist unbekannt.
    pub nur_online: u64,
    /// Summe der Größen der lokal vorhandenen Dateien.
    pub bytes: u64,
    /// Nach Anzahl, häufigste zuerst.
    pub endungen: Vec<EndungZeile>,
    /// Nach Zahl der Dateien, die ins Register kämen, größte zuerst.
    pub unterordner: Vec<UnterordnerZeile>,
    /// Übergangene Ordner fremder Bibliotheken, höchstens [`PRUEFUNG_BIBLIOTHEKEN`].
    pub bibliotheken: Vec<String>,
    pub bibliotheken_anzahl: u64,
    pub nicht_lesbar: u64,
}

pub const PRUEFUNG_BIBLIOTHEKEN: usize = 200;

/// Zählt die Dateien in `ordner` (Pfad im Register, `Ast/…`) samt Unterordnern
/// nach Typ. Liest nur Verzeichnisse und Metadaten, lädt nichts herunter und
/// schreibt nichts. Die Ordnerregeln spielen keine Rolle.
pub fn ordner_pruefen(konfig: &Konfiguration, ordner: &str, abbrechen: &AtomicBool) -> crate::Ergebnis<OrdnerPruefung> {
    let start = std::time::Instant::now();
    let ordner = relativ_normalisieren(ordner);
    let fehlt = || crate::Fehler::Konfiguration(format!("Ordner \"{ordner}\" gibt es auf diesem Gerät nicht"));
    if ordner.split('/').any(|teil| teil == "..") {
        return Err(fehlt());
    }
    let wurzel = konfig.ort(&ordner).filter(|w| w.is_dir()).ok_or_else(fehlt)?;
    let mut p = OrdnerPruefung { ordner, ..Default::default() };
    let mut endungen: std::collections::HashMap<String, EndungZeile> = Default::default();
    let mut unterordner: std::collections::HashMap<String, UnterordnerZeile> = Default::default();
    baum_lesen(
        &wurzel,
        &|_| false,
        &mut |fund| match fund {
            Fund::NichtLesbar(_) => p.nicht_lesbar += 1,
            Fund::Ordner(_) => {}
            Fund::Bibliothek(pfad) => {
                p.bibliotheken_anzahl += 1;
                if p.bibliotheken.len() < PRUEFUNG_BIBLIOTHEKEN {
                    if let Some(r) = relativ(&wurzel, pfad) {
                        p.bibliotheken.push(r);
                    }
                }
            }
            Fund::Datei { pfad, meta } => {
                let Some(rel) = relativ(&wurzel, pfad) else { return };
                let (oben, name) = match rel.split_once('/') {
                    Some((oben, rest)) => (oben, rest.rsplit('/').next().unwrap_or(rest)),
                    None => ("", rel.as_str()),
                };
                let endung = endung_von(name);
                let klasse = formate::klasse(&endung);
                let bytes = meta.len();
                let online = (lokale_groesse(&meta) == 0) as u64;
                p.dateien += 1;
                p.bytes += bytes;
                p.nur_online += online;
                let u = unterordner
                    .entry(oben.to_string())
                    .or_insert_with(|| UnterordnerZeile { name: oben.to_string(), ..Default::default() });
                match klasse {
                    Klasse::Gelesen => (p.gelesen += 1, u.gelesen += 1),
                    Klasse::NurName => (p.nur_name += 1, u.nur_name += 1),
                    Klasse::Ignoriert => (p.ignoriert += 1, u.ignoriert += 1),
                };
                u.bytes += bytes;
                u.nur_online += online;
                let e = endungen.entry(endung.clone()).or_insert_with(|| EndungZeile {
                    endung,
                    klasse,
                    anzahl: 0,
                    bytes: 0,
                    nur_online: 0,
                });
                e.anzahl += 1;
                e.bytes += bytes;
                e.nur_online += online;
            }
        },
        abbrechen,
    );
    p.abgebrochen = abbrechen.load(Ordering::Relaxed);
    p.endungen = endungen.into_values().collect();
    p.endungen.sort_by(|a, b| b.anzahl.cmp(&a.anzahl).then_with(|| a.endung.cmp(&b.endung)));
    p.unterordner = unterordner.into_values().collect();
    p.unterordner.sort_by(|a, b| {
        (b.gelesen + b.nur_name).cmp(&(a.gelesen + a.nur_name)).then_with(|| a.name.cmp(&b.name))
    });
    p.bibliotheken.sort();
    p.dauer_ms = start.elapsed().as_millis() as u64;
    Ok(p)
}

/// Ein Ordner für den Ordnerbaum der Regeln.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct OrdnerEintrag {
    pub name: String,
    /// Pfad im Register (`Ast/…`).
    pub pfad: String,
    pub hat_unterordner: bool,
    /// Ordner fremder Bibliotheken, wird ohne eigene Regel übergangen.
    pub bibliothek: bool,
    /// Nur bei Ästen: Ort auf diesem Gerät.
    pub ort: Option<String>,
    /// Nur bei Ästen: Den Ordner gibt es auf diesem Gerät gerade nicht.
    pub fehlt: bool,
}

fn ist_ordner(e: &std::fs::DirEntry) -> bool {
    // Auch Verknüpfungen auf Ordner zählen nicht: Der Durchlauf folgt ihnen nicht
    e.file_type().is_ok_and(|t| t.is_dir())
}

fn hat_unterordner(ordner: &Path) -> bool {
    std::fs::read_dir(ordner)
        .map(|inhalt| inhalt.flatten().any(|e| ist_ordner(&e) && !uebergehen(&e.file_name().to_string_lossy())))
        .unwrap_or(false)
}

/// Die Äste dieses Geräts, nach Name. Sie sind die oberste Ebene des
/// Ordnerbaums.
pub fn aeste(konfig: &Konfiguration) -> Vec<OrdnerEintrag> {
    konfig
        .aeste
        .iter()
        .map(|(name, ort)| OrdnerEintrag {
            name: name.clone(),
            pfad: name.clone(),
            hat_unterordner: hat_unterordner(ort),
            bibliothek: false,
            ort: Some(ort.display().to_string()),
            fehlt: !ort.is_dir(),
        })
        .collect()
}

/// Die direkten Unterordner von `ordner` (Pfad im Register, `Ast/…`), nach
/// Name. Der leere Pfad liefert die Äste.
pub fn unterordner(konfig: &Konfiguration, ordner: &str) -> crate::Ergebnis<Vec<OrdnerEintrag>> {
    let ordner = relativ_normalisieren(ordner);
    if ordner.is_empty() {
        return Ok(aeste(konfig));
    }
    if ordner.split('/').any(|teil| teil == "..") {
        return Err(crate::Fehler::Konfiguration(format!("Ordner \"{ordner}\" ist ungültig")));
    }
    let wurzel = konfig
        .ort(&ordner)
        .ok_or_else(|| crate::Fehler::Konfiguration(format!("Ordner \"{ordner}\" gibt es auf diesem Gerät nicht")))?;
    let mut liste = Vec::new();
    for eintrag in std::fs::read_dir(&wurzel)?.flatten() {
        let name: String = eintrag.file_name().to_string_lossy().nfc().collect();
        if !ist_ordner(&eintrag) || uebergehen(&name) {
            continue;
        }
        liste.push(OrdnerEintrag {
            pfad: format!("{ordner}/{name}"),
            bibliothek: ist_bibliothek(&eintrag.file_name().to_string_lossy(), &wurzel),
            hat_unterordner: hat_unterordner(&eintrag.path()),
            ort: None,
            fehlt: false,
            name,
        });
    }
    liste.sort_by_key(|e| e.name.to_lowercase());
    Ok(liste)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn anlegen(wurzel: &Path, dateien: &[(&str, &[u8])]) {
        for (pfad, inhalt) in dateien {
            let ziel = wurzel.join(pfad);
            fs::create_dir_all(ziel.parent().unwrap()).unwrap();
            fs::write(ziel, inhalt).unwrap();
        }
    }

    fn konfig(ort: &Path) -> Konfiguration {
        let mut k = Konfiguration::mit_ast("C", ort);
        k.geraet = "Test".into();
        k
    }

    fn pfade(d: &Durchlauf) -> Vec<&str> {
        let mut pfade: Vec<_> = d.dateien.iter().map(|e| e.pfad.as_str()).collect();
        pfade.sort();
        pfade
    }

    #[test]
    fn liest_dateien_mit_relativen_pfaden() {
        let tmp = tempfile::tempdir().unwrap();
        anlegen(
            tmp.path(),
            &[
                ("Archiv/Alles/2020-01-02 Brief.PDF", b"abc"),
                ("Archiv/Alles/Unterordner/Platzhalter.pdf", b""),
                ("Archiv/Alles/Unterordner/ohne endung", b"x"),
                ("Archiv/Alles/.DS_Store", b"x"),
                ("Archiv/Alles/.dropbox.cache/rest.pdf", b"x"),
                ("Archiv/ausserhalb.pdf", b"x"),
            ],
        );
        let d = durchlaufen(&konfig(tmp.path()), &Regeln::voll(&["C/Archiv/Alles", "C/Archiv/Fehlt", "Fremd/X"]));
        // "ohne endung" ist kein bekanntes Format und wird ignoriert
        assert_eq!(pfade(&d), vec!["C/Archiv/Alles/2020-01-02 Brief.PDF", "C/Archiv/Alles/Unterordner/Platzhalter.pdf"]);
        // betretene Ordner, ohne Verwaltungsordner
        let ordner: Vec<_> = d.ordner.iter().map(String::as_str).collect();
        assert_eq!(ordner, vec!["C", "C/Archiv", "C/Archiv/Alles", "C/Archiv/Alles/Unterordner"]);
        assert_eq!(d.geraet, "Test");
        assert_eq!(d.ignoriert, 1);
        let brief = d.dateien.iter().find(|e| e.name.ends_with("Brief.PDF")).unwrap();
        assert_eq!((brief.endung.as_str(), brief.groesse, brief.stufe), ("pdf", 3, Stufe::Voll));
        assert!(brief.geaendert > 1_600_000_000);
        assert!(d.dateien.iter().find(|e| e.name == "Platzhalter.pdf").unwrap().nur_online_oder_leer());
        assert_eq!(d.gelesene_verzeichnisse, vec!["C/Archiv/Alles"]);
        // ein fehlender Ordner und ein Ast, den es auf diesem Gerät nicht gibt
        assert_eq!(d.fehlende_verzeichnisse, vec!["C/Archiv/Fehlt", "Fremd/X"]);
    }

    #[test]
    fn regeln_bestimmen_ordner_und_stufe() {
        let tmp = tempfile::tempdir().unwrap();
        anlegen(
            tmp.path(),
            &[
                ("Archiv/a.pdf", b"x"),
                ("Archiv/quelle.rs", b"x"),
                ("Archiv/alt.zip", b"x"),
                ("Archiv/Bilder/foto.jpg", b"x"),
                ("Archiv/Bilder/Belege/beleg.jpg", b"x"),
                ("Musik/A/lied.mp3", b"x"),
                ("Musik/A/cover.jpg", b"x"),
                ("Musik/A/liesmich", b"x"),
                ("Sonst/b.pdf", b"x"),
            ],
        );
        let mut regeln = Regeln::voll(&["C/Archiv", "C/Archiv/Bilder/Belege"]);
        regeln.setzen("C/Archiv/Bilder", Some(Stufe::Ausgeschlossen)).unwrap();
        regeln.setzen("C/Musik", Some(Stufe::Metadaten)).unwrap();
        let d = durchlaufen(&konfig(tmp.path()), &regeln);
        assert_eq!(
            pfade(&d),
            vec![
                "C/Archiv/Bilder/Belege/beleg.jpg",
                "C/Archiv/a.pdf",
                "C/Archiv/alt.zip",
                "C/Musik/A/cover.jpg",
                "C/Musik/A/lied.mp3",
                "C/Musik/A/liesmich",
            ]
        );
        // In der Stufe Metadaten kommt jede Datei mit Namen ins Register
        assert!(d.dateien.iter().filter(|e| e.pfad.starts_with("C/Musik/")).all(|e| e.stufe == Stufe::Metadaten));
        assert!(d.ordner.contains("C/Archiv") && d.ordner.contains("C/Archiv/Bilder/Belege"));
        assert_eq!(d.ignoriert, 1);
        assert_eq!(d.gelesene_verzeichnisse, vec!["C/Archiv", "C/Archiv/Bilder/Belege", "C/Musik"]);
    }

    #[test]
    fn uebergeht_bibliotheken() {
        let tmp = tempfile::tempdir().unwrap();
        anlegen(
            tmp.path(),
            &[
                ("Dev/App/README.md", b"x"),
                ("Dev/App/package.json", b"x"),
                ("Dev/App/node_modules/x/README.md", b"x"),
                ("Dev/App/dist/index.txt", b"x"),
                ("Dev/App/.git/beschreibung.txt", b"x"),
                ("Dev/App/docs/build/plan.md", b"x"),
                ("Dev/Rust/Cargo.toml", b"x"),
                ("Dev/Rust/target/doc/a.md", b"x"),
                ("Archiv/build/Bauplan.pdf", b"x"),
                ("Archiv/vendor/Rechnung.pdf", b"x"),
            ],
        );
        let d = durchlaufen(&konfig(tmp.path()), &Regeln::voll(&["C/Dev", "C/Archiv"]));
        assert_eq!(
            pfade(&d),
            vec![
                "C/Archiv/build/Bauplan.pdf",
                "C/Archiv/vendor/Rechnung.pdf",
                "C/Dev/App/README.md",
                "C/Dev/App/docs/build/plan.md",
            ]
        );
        assert_eq!(d.uebergangene_ordner, 4);

        // Eine eigene Regel schließt einen übergangenen Ordner ausdrücklich ein
        let d = durchlaufen(&konfig(tmp.path()), &Regeln::voll(&["C/Dev", "C/Dev/App/node_modules"]));
        assert!(pfade(&d).contains(&"C/Dev/App/node_modules/x/README.md"));

        // Ein ganzer Ast als Regel
        let d = durchlaufen(&konfig(&tmp.path().join("Archiv")), &Regeln::voll(&["C"]));
        assert_eq!(pfade(&d), vec!["C/build/Bauplan.pdf", "C/vendor/Rechnung.pdf"]);
        assert!(d.ordner.contains("C") && d.ordner.contains("C/build"));
    }

    #[test]
    fn prueft_einen_ordner() {
        let tmp = tempfile::tempdir().unwrap();
        anlegen(
            tmp.path(),
            &[
                ("Dev/liesmich.md", b"12"),
                ("Dev/App/a.pdf", b"123"),
                ("Dev/App/b.PDF", b""),
                ("Dev/App/main.rs", b"1"),
                ("Dev/App/package.json", b"1"),
                ("Dev/App/node_modules/x/README.md", b"1"),
                ("Dev/App/tief/unten/alt.zip", b"1234"),
                ("Dev/Leer/x.rs", b"1"),
            ],
        );
        let k = konfig(tmp.path());
        let p = ordner_pruefen(&k, "C/Dev/", &AtomicBool::new(false)).unwrap();
        assert_eq!(p.ordner, "C/Dev");
        assert_eq!((p.dateien, p.gelesen, p.nur_name, p.ignoriert), (7, 3, 1, 3));
        assert_eq!((p.nur_online, p.bytes), (1, 12));
        assert_eq!((p.bibliotheken.clone(), p.bibliotheken_anzahl), (vec!["App/node_modules".to_string()], 1));
        let pdf = p.endungen.iter().find(|e| e.endung == "pdf").unwrap();
        assert_eq!((pdf.klasse, pdf.anzahl, pdf.bytes, pdf.nur_online), (Klasse::Gelesen, 2, 3, 1));
        assert_eq!(p.endungen[0].endung, "pdf");
        let namen: Vec<_> = p.unterordner.iter().map(|u| (u.name.as_str(), u.gelesen + u.nur_name, u.ignoriert)).collect();
        assert_eq!(namen, vec![("App", 3, 2), ("", 1, 0), ("Leer", 0, 1)]);
        assert!(ordner_pruefen(&k, "C/Fehlt", &AtomicBool::new(false)).is_err());
        assert!(ordner_pruefen(&k, "Fremd/Dev", &AtomicBool::new(false)).is_err());
        assert!(ordner_pruefen(&k, "C/../x", &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn listet_unterordner() {
        let tmp = tempfile::tempdir().unwrap();
        anlegen(
            tmp.path(),
            &[("Archiv/Alles/2020/a.pdf", b"x"), ("Archiv/belege/b.pdf", b"x"), ("Archiv/.dropbox.cache/c", b"x"),
              ("Archiv/node_modules/d", b"x"), ("Archiv/datei.pdf", b"x")],
        );
        let mut k = konfig(tmp.path());
        let liste = unterordner(&k, "C/Archiv").unwrap();
        let kurz: Vec<_> = liste.iter().map(|e| (e.pfad.as_str(), e.hat_unterordner, e.bibliothek)).collect();
        assert_eq!(
            kurz,
            vec![("C/Archiv/Alles", true, false), ("C/Archiv/belege", false, false), ("C/Archiv/node_modules", false, true)]
        );
        // Oberste Ebene sind die Äste, auch die auf diesem Gerät fehlenden
        k.aeste.insert("Stick".into(), tmp.path().join("nicht da"));
        let oben: Vec<_> = unterordner(&k, "").unwrap().into_iter().map(|e| (e.pfad, e.hat_unterordner, e.fehlt)).collect();
        assert_eq!(oben, vec![("C".to_string(), true, false), ("Stick".to_string(), false, true)]);
        assert_eq!(unterordner(&k, "C").unwrap()[0].pfad, "C/Archiv");
        assert!(unterordner(&k, "C/..").is_err() && unterordner(&k, "Fremd").is_err());
    }

    #[test]
    fn vereinheitlicht_umlaute() {
        // "ä" zerlegt (a + Trema) wird zu einem Zeichen
        assert_eq!(relativ_normalisieren("Archiv/Scans fu\u{0308}r/"), "Archiv/Scans f\u{00fc}r");
    }
}
