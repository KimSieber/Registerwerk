//! Konfiguration aus einer lesbaren TOML-Datei.
//!
//! Die Datei mit den echten Pfaden heißt `config.local.toml` und wird nicht
//! versioniert. Eine Vorlage ohne echte Pfade liegt als `config.beispiel.toml`
//! im Repo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Ergebnis, Fehler};

pub const DATEINAME: &str = "config.local.toml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Konfiguration {
    /// Wurzel des Cloud-Ordners auf diesem Gerät. Alle Pfade im Register sind
    /// relativ zu diesem Ordner. Das Register bleibt damit auf jedem Gerät
    /// gültig, auch wenn der Cloud-Ordner dort woanders liegt.
    pub cloud_ordner: PathBuf,
    /// Zu indizierende Verzeichnisse, relativ zum Cloud-Ordner.
    pub verzeichnisse: Vec<String>,
    /// Ordner für die Arbeitskopie des Registers. Ohne Angabe nimmt die
    /// Anwendung ihren Datenordner im Benutzerprofil.
    #[serde(default)]
    pub register_ordner: Option<PathBuf>,
}

impl Konfiguration {
    pub fn laden(pfad: &Path) -> Ergebnis<Self> {
        let text = std::fs::read_to_string(pfad)
            .map_err(|e| Fehler::Konfiguration(format!("{} nicht lesbar: {e}", pfad.display())))?;
        Self::aus_text(&text)
    }

    pub fn aus_text(text: &str) -> Ergebnis<Self> {
        let mut konfig: Konfiguration =
            toml::from_str(text).map_err(|e| Fehler::Konfiguration(e.to_string()))?;
        konfig.pruefen()?;
        Ok(konfig)
    }

    fn pruefen(&mut self) -> Ergebnis<()> {
        if !self.cloud_ordner.is_absolute() {
            return Err(Fehler::Konfiguration(
                "cloud_ordner muss ein absoluter Pfad sein".into(),
            ));
        }
        if self.verzeichnisse.is_empty() {
            return Err(Fehler::Konfiguration(
                "verzeichnisse enthält keinen Eintrag".into(),
            ));
        }
        for v in &mut self.verzeichnisse {
            let sauber = crate::dateibaum::relativ_normalisieren(v);
            if sauber.is_empty() || sauber.split('/').any(|teil| teil == "..") || Path::new(v).is_absolute() {
                return Err(Fehler::Konfiguration(format!(
                    "Verzeichnis \"{v}\" muss relativ zum cloud_ordner angegeben sein"
                )));
            }
            *v = sauber;
        }
        if let Some(r) = &self.register_ordner {
            if !r.is_absolute() {
                return Err(Fehler::Konfiguration(
                    "register_ordner muss ein absoluter Pfad sein".into(),
                ));
            }
        }
        Ok(())
    }

    /// Sucht `config.local.toml` in `start` und allen übergeordneten Ordnern.
    pub fn suchen_ab(start: &Path) -> Option<PathBuf> {
        start
            .ancestors()
            .map(|ordner| ordner.join(DATEINAME))
            .find(|kandidat| kandidat.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liest_gueltige_konfiguration() {
        let k = Konfiguration::aus_text(
            "cloud_ordner = \"/tmp/cloud\"\nverzeichnisse = [\"Archiv/Alles/\", \"Archiv\\\\Belege\"]\n",
        )
        .unwrap();
        assert_eq!(k.verzeichnisse, vec!["Archiv/Alles", "Archiv/Belege"]);
        assert_eq!(k.register_ordner, None);
    }

    #[test]
    fn lehnt_ungueltige_angaben_ab() {
        assert!(Konfiguration::aus_text("cloud_ordner = \"cloud\"\nverzeichnisse = [\"a\"]").is_err());
        assert!(Konfiguration::aus_text("cloud_ordner = \"/c\"\nverzeichnisse = []").is_err());
        assert!(Konfiguration::aus_text("cloud_ordner = \"/c\"\nverzeichnisse = [\"../x\"]").is_err());
        assert!(Konfiguration::aus_text("cloud_ordner = \"/c\"\nverzeichnisse = [\"/abs\"]").is_err());
        assert!(Konfiguration::aus_text("cloud_ordner = \"/c\"\nverzeichnisse = [\"a\"]\ntippfehler = 1").is_err());
    }
}
