//! Ordnerregeln (Stufe 3, v0.0.7).
//!
//! Die Regeln legen fest, welche Ordner des Cloud-Ordners ins Register kommen
//! und wie. Sie sind die einzige eigene Information des Programms und stehen
//! deshalb als lesbare Textdatei `regeln.toml` neben dem Register (Grundsatz 2).
//!
//! - Ohne Regel wird ein Ordner nicht gelesen.
//! - Eine Regel gilt für den Ordner und alle Unterordner.
//! - Die genaueste Regel gewinnt: Ein Unterordner kann ausgeschlossen oder
//!   anders behandelt werden als sein Oberordner.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dateibaum::relativ_normalisieren;
use crate::{Ergebnis, Fehler};

pub const DATEINAME: &str = "regeln.toml";

/// Behandlung eines Ordners (Konzept, Abschnitt 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stufe {
    /// Bekannte Formate mit Text, Vorschau und OCR. Alles andere wird
    /// ignoriert oder nur mit Namen erfasst (`formate.rs`).
    Voll,
    /// Alle Dateien nur mit Name, Ordner, Größe und Datum.
    Metadaten,
    /// Nichts.
    Ausgeschlossen,
}

impl Stufe {
    /// Wert der Spalte `datei.stufe`. Ausgeschlossene Dateien stehen nicht im
    /// Register.
    pub fn als_zahl(self) -> i64 {
        match self {
            Stufe::Voll => 1,
            Stufe::Metadaten => 2,
            Stufe::Ausgeschlossen => 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegelDatei {
    #[serde(default)]
    ordner: BTreeMap<String, Stufe>,
}

/// Regeln je Ordner. Schlüssel ist der Pfad relativ zum Cloud-Ordner, mit `/`
/// getrennt und Unicode in NFC. Der leere Pfad steht für den Cloud-Ordner
/// selbst.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Regeln {
    ordner: BTreeMap<String, Stufe>,
}

impl Regeln {
    /// Regeln aus einer Liste von Ordnern, alle mit der Stufe Voll. Für die
    /// Übernahme der früheren Liste `verzeichnisse` und für Tests.
    pub fn voll(ordner: &[impl AsRef<str>]) -> Self {
        let mut r = Self::default();
        for o in ordner {
            r.ordner.insert(relativ_normalisieren(o.as_ref()), Stufe::Voll);
        }
        r
    }

    pub fn aus_text(text: &str) -> Ergebnis<Self> {
        let datei: RegelDatei =
            toml::from_str(text).map_err(|e| Fehler::Konfiguration(format!("{DATEINAME}: {e}")))?;
        let mut r = Self::default();
        for (pfad, stufe) in datei.ordner {
            r.setzen(&pfad, Some(stufe))?;
        }
        Ok(r)
    }

    /// Lesbare Fassung mit Erläuterung, Ordner alphabetisch.
    pub fn als_text(&self) -> String {
        let mut text = String::from(
            "# Registerwerk: Ordnerregeln\n\
             #\n\
             # Welche Ordner des Cloud-Ordners ins Register kommen. Pfade relativ zum\n\
             # Cloud-Ordner. Die Anwendung schreibt diese Datei, sie lässt sich aber auch\n\
             # von Hand ändern.\n\
             #\n\
             #   \"voll\"            Dokumente und Bilder mit Text und Vorschau\n\
             #   \"metadaten\"       alle Dateien nur mit Name und Datum\n\
             #   \"ausgeschlossen\"  nichts\n\
             #\n\
             # Ohne Regel wird ein Ordner nicht gelesen. Eine Regel gilt auch für die\n\
             # Unterordner. Die genaueste Regel gewinnt.\n\
             \n\
             [ordner]\n",
        );
        for (pfad, stufe) in &self.ordner {
            let wert = match stufe {
                Stufe::Voll => "voll",
                Stufe::Metadaten => "metadaten",
                Stufe::Ausgeschlossen => "ausgeschlossen",
            };
            text.push_str(&format!("{} = \"{wert}\"\n", toml::Value::String(pfad.clone())));
        }
        text
    }

    /// Lädt `regeln.toml` aus dem Register-Ordner. `None`, wenn es die Datei
    /// noch nicht gibt.
    pub fn laden(register_ordner: &Path) -> Ergebnis<Option<Self>> {
        let pfad = register_ordner.join(DATEINAME);
        match std::fs::read_to_string(&pfad) {
            Ok(text) => Self::aus_text(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Fehler::Konfiguration(format!("{} nicht lesbar: {e}", pfad.display()))),
        }
    }

    /// Schreibt `regeln.toml` in den Register-Ordner. Erst als neue Datei,
    /// dann in einem Schritt an die Stelle der alten.
    pub fn speichern(&self, register_ordner: &Path) -> Ergebnis<()> {
        std::fs::create_dir_all(register_ordner)?;
        let ziel = register_ordner.join(DATEINAME);
        let neu = register_ordner.join(format!("{DATEINAME}.neu"));
        std::fs::write(&neu, self.als_text())?;
        std::fs::rename(&neu, &ziel)?;
        Ok(())
    }

    /// Setzt oder entfernt (`None`) die Regel eines Ordners.
    pub fn setzen(&mut self, ordner: &str, stufe: Option<Stufe>) -> Ergebnis<()> {
        let pfad = relativ_normalisieren(ordner);
        if Path::new(ordner).is_absolute() || pfad.split('/').any(|teil| teil == "..") {
            return Err(Fehler::Konfiguration(format!(
                "Ordner \"{ordner}\" muss relativ zum Cloud-Ordner angegeben sein"
            )));
        }
        match stufe {
            Some(s) => self.ordner.insert(pfad, s),
            None => self.ordner.remove(&pfad),
        };
        Ok(())
    }

    pub fn ist_leer(&self) -> bool {
        self.ordner.is_empty()
    }

    /// Alle Regeln, alphabetisch nach Ordner.
    pub fn eintraege(&self) -> impl Iterator<Item = (&str, Stufe)> {
        self.ordner.iter().map(|(p, s)| (p.as_str(), *s))
    }

    /// Ordner, die gelesen werden: jede Regel außer Ausgeschlossen. Der
    /// Durchlauf eines Ordners endet an Unterordnern mit eigener Regel.
    pub fn wurzeln(&self) -> impl Iterator<Item = (&str, Stufe)> {
        self.eintraege().filter(|(_, s)| *s != Stufe::Ausgeschlossen)
    }

    /// Die eigene Regel genau dieses Ordners.
    pub fn eigene(&self, ordner: &str) -> Option<Stufe> {
        self.ordner.get(ordner).copied()
    }

    /// Die Regel, die für einen Ordner gilt: seine eigene oder die des
    /// nächsten Oberordners mit Regel. Liefert den Ordner der Regel mit.
    pub fn fuer_ordner<'a>(&'a self, ordner: &str) -> Option<(&'a str, Stufe)> {
        let mut rest = ordner;
        loop {
            if let Some((pfad, stufe)) = self.ordner.get_key_value(rest) {
                return Some((pfad.as_str(), *stufe));
            }
            if rest.is_empty() {
                return None;
            }
            rest = rest.rsplit_once('/').map(|(vorn, _)| vorn).unwrap_or("");
        }
    }

    /// Die Regel, die für eine Datei gilt.
    pub fn fuer_datei<'a>(&'a self, pfad: &str) -> Option<(&'a str, Stufe)> {
        self.fuer_ordner(pfad.rsplit_once('/').map(|(vorn, _)| vorn).unwrap_or(""))
    }

    /// Gibt es unterhalb dieses Ordners Regeln? Für die Anzeige im Baum.
    pub fn hat_regeln_unter(&self, ordner: &str) -> bool {
        if ordner.is_empty() {
            return self.ordner.keys().any(|p| !p.is_empty());
        }
        let anfang = format!("{ordner}/");
        self.ordner.range(anfang.clone()..).next().is_some_and(|(p, _)| p.starts_with(&anfang))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel() -> Regeln {
        Regeln::aus_text(
            "[ordner]\n\"Archiv\" = \"voll\"\n\"Archiv/Bilder\" = \"ausgeschlossen\"\n\
             \"Archiv/Bilder/Belege\" = \"voll\"\n\"Musik/\" = \"metadaten\"\n",
        )
        .unwrap()
    }

    #[test]
    fn genaueste_regel_gewinnt() {
        let r = beispiel();
        assert_eq!(r.fuer_datei("Archiv/a.pdf"), Some(("Archiv", Stufe::Voll)));
        assert_eq!(r.fuer_datei("Archiv/Alles/2020/a.pdf"), Some(("Archiv", Stufe::Voll)));
        assert_eq!(r.fuer_datei("Archiv/Bilder/x/a.jpg"), Some(("Archiv/Bilder", Stufe::Ausgeschlossen)));
        assert_eq!(r.fuer_datei("Archiv/Bilder/Belege/a.jpg"), Some(("Archiv/Bilder/Belege", Stufe::Voll)));
        assert_eq!(r.fuer_datei("Musik/A/b.mp3"), Some(("Musik", Stufe::Metadaten)));
        // ohne Regel wird nichts gelesen, auch nicht bei ähnlichem Namen
        assert_eq!(r.fuer_datei("Archiv2/a.pdf"), None);
        assert_eq!(r.fuer_datei("a.pdf"), None);
        assert_eq!(r.fuer_ordner("Archiv/Bilder"), Some(("Archiv/Bilder", Stufe::Ausgeschlossen)));
        let wurzeln: Vec<_> = r.wurzeln().map(|(p, _)| p).collect();
        assert_eq!(wurzeln, vec!["Archiv", "Archiv/Bilder/Belege", "Musik"]);
        assert!(r.hat_regeln_unter("Archiv") && r.hat_regeln_unter("") && !r.hat_regeln_unter("Musik"));
        assert!(!r.hat_regeln_unter("Arch"));
    }

    #[test]
    fn regel_fuer_den_cloud_ordner_selbst() {
        let mut r = Regeln::default();
        r.setzen("", Some(Stufe::Metadaten)).unwrap();
        r.setzen("Archiv", Some(Stufe::Voll)).unwrap();
        assert_eq!(r.fuer_datei("a.txt"), Some(("", Stufe::Metadaten)));
        assert_eq!(r.fuer_datei("X/Y/a.txt"), Some(("", Stufe::Metadaten)));
        assert_eq!(r.fuer_datei("Archiv/a.txt"), Some(("Archiv", Stufe::Voll)));
        assert_eq!(Regeln::aus_text(&r.als_text()).unwrap(), r);
    }

    #[test]
    fn text_hin_und_zurueck() {
        let r = beispiel();
        let text = r.als_text();
        assert!(text.contains("\"Archiv/Bilder\" = \"ausgeschlossen\""));
        assert_eq!(Regeln::aus_text(&text).unwrap(), r);
        // Sonderzeichen im Ordnernamen
        let mut s = Regeln::default();
        s.setzen("A \"b\"/Größe = 1", Some(Stufe::Voll)).unwrap();
        assert_eq!(Regeln::aus_text(&s.als_text()).unwrap(), s);
        // leere Datei und nur Kommentar sind gültig
        assert!(Regeln::aus_text("").unwrap().ist_leer());
    }

    #[test]
    fn lehnt_ungueltige_angaben_ab() {
        assert!(Regeln::aus_text("[ordner]\n\"../x\" = \"voll\"").is_err());
        assert!(Regeln::aus_text("[ordner]\n\"/abs\" = \"voll\"").is_err());
        assert!(Regeln::aus_text("[ordner]\n\"A\" = \"halb\"").is_err());
        assert!(Regeln::aus_text("[sonst]\nx = 1").is_err());
    }

    #[test]
    fn speichert_und_laedt() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(Regeln::laden(tmp.path()).unwrap(), None);
        let mut r = beispiel();
        r.speichern(tmp.path()).unwrap();
        r.setzen("Musik", None).unwrap();
        r.speichern(tmp.path()).unwrap();
        assert_eq!(Regeln::laden(tmp.path()).unwrap(), Some(r));
        assert!(!tmp.path().join("regeln.toml.neu").exists());
    }
}
