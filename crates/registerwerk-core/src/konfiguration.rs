//! Konfiguration dieses Geräts aus einer lesbaren TOML-Datei.
//!
//! Die Datei heißt `config.local.toml` und wird nicht versioniert. Eine Vorlage
//! ohne echte Pfade liegt als `config.beispiel.toml` im Repo.
//!
//! Seit v0.0.7 nennt sie die **Äste**: Ordner dieses Geräts, aus denen
//! Unterlagen ins Register kommen können, jeder mit einem Namen. Im Register
//! und in den Ordnerregeln beginnt jeder Pfad mit dem Namen des Astes
//! (`Dropbox/Archiv/…`). Wo der Ast liegt, steht nur hier. So gelten Register
//! und Regeln auf jedem Gerät, auch wenn die Ordner dort woanders liegen oder
//! ein Ast dort fehlt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::{Ergebnis, Fehler};

pub const DATEINAME: &str = "config.local.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Konfiguration {
    /// Name dieses Geräts. Das Register merkt sich damit, welches Gerät einen
    /// Ordner gesehen hat.
    #[serde(default)]
    pub geraet: String,
    /// Ordner für das Register. Ohne Angabe nimmt die Anwendung ihren
    /// Datenordner im Benutzerprofil.
    #[serde(default)]
    pub register_ordner: Option<PathBuf>,
    /// Äste: Name und Ort auf diesem Gerät.
    #[serde(default)]
    pub aeste: BTreeMap<String, PathBuf>,
    /// Bis v0.0.6: der eine Cloud-Ordner. Wird beim Laden zu einem Ast mit dem
    /// Namen des Ordners.
    #[serde(default, skip_serializing)]
    cloud_ordner: Option<PathBuf>,
    /// Bis v0.0.6: zu indizierende Verzeichnisse, relativ zum Cloud-Ordner.
    /// Daraus entstehen beim ersten Start die Ordnerregeln.
    #[serde(default, skip_serializing)]
    pub verzeichnisse: Vec<String>,
    /// Der Ast, der aus `cloud_ordner` entstanden ist. Pfade eines Registers
    /// und Regeln aus der Zeit davor gehören zu ihm (`umstellen_auf_aeste`).
    #[serde(skip)]
    pub alter_ast: Option<String>,
}

/// Prüft und vereinheitlicht den Namen eines Astes: ein Ordnername ohne
/// Trenner, Unicode in NFC.
pub fn ast_name(name: &str) -> Ergebnis<String> {
    let name: String = name.trim().nfc().collect();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(Fehler::Konfiguration(format!("\"{name}\" ist kein gültiger Name für einen Ast")));
    }
    Ok(name)
}

impl Konfiguration {
    pub fn laden(pfad: &Path) -> Ergebnis<Self> {
        let text = std::fs::read_to_string(pfad)
            .map_err(|e| Fehler::Konfiguration(format!("{} nicht lesbar: {e}", pfad.display())))?;
        Self::aus_text(&text)
    }

    pub fn aus_text(text: &str) -> Ergebnis<Self> {
        let mut konfig: Konfiguration = toml::from_str(text).map_err(|e| Fehler::Konfiguration(e.to_string()))?;
        konfig.pruefen()?;
        Ok(konfig)
    }

    /// Eine Konfiguration mit einem einzigen Ast. Für Tests.
    pub fn mit_ast(name: &str, ort: &Path) -> Self {
        Self { aeste: BTreeMap::from([(name.to_string(), ort.to_path_buf())]), ..Default::default() }
    }

    fn pruefen(&mut self) -> Ergebnis<()> {
        let mut aeste = BTreeMap::new();
        for (name, ort) in std::mem::take(&mut self.aeste) {
            if !ort.is_absolute() {
                return Err(Fehler::Konfiguration(format!("Der Ort des Astes \"{name}\" muss ein absoluter Pfad sein")));
            }
            aeste.insert(ast_name(&name)?, ort);
        }
        self.aeste = aeste;
        if let Some(cloud) = self.cloud_ordner.clone() {
            if !cloud.is_absolute() {
                return Err(Fehler::Konfiguration("cloud_ordner muss ein absoluter Pfad sein".into()));
            }
            // Der frühere Cloud-Ordner wird ein Ast mit dem Namen des Ordners
            let name = match self.aeste.iter().find(|(_, ort)| **ort == cloud) {
                Some((name, _)) => name.clone(),
                None => {
                    let name = ast_name(&cloud.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
                        .unwrap_or_else(|_| "Cloud".into());
                    self.aeste.insert(name.clone(), cloud);
                    name
                }
            };
            self.alter_ast = Some(name);
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
                return Err(Fehler::Konfiguration("register_ordner muss ein absoluter Pfad sein".into()));
            }
        }
        Ok(())
    }

    /// Lesbare Fassung mit Erläuterung. Die früheren Angaben `cloud_ordner`
    /// und `verzeichnisse` entfallen dabei.
    pub fn als_text(&self) -> String {
        let wert = |s: &str| toml::Value::String(s.to_string()).to_string();
        let mut text = String::from(
            "# Registerwerk: Konfiguration dieses Geräts (nicht versioniert)\n\
             #\n\
             # Die Anwendung schreibt diese Datei, sie lässt sich aber auch von Hand ändern.\n\
             \n\
             # Name dieses Geräts. Das Register merkt sich damit, welches Gerät einen\n\
             # Ordner gesehen hat.\n",
        );
        text.push_str(&format!("geraet = {}\n\n", wert(&self.geraet)));
        text.push_str(
            "# Ordner für das Register (absoluter Pfad, optional). Ohne Angabe liegt es\n\
             # im Datenordner der Anwendung im Benutzerprofil.\n",
        );
        match &self.register_ordner {
            Some(ordner) => text.push_str(&format!("register_ordner = {}\n", wert(&ordner.to_string_lossy()))),
            None => text.push_str("# register_ordner = \"/pfad/zum/register\"\n"),
        }
        text.push_str(
            "\n# Äste: Ordner dieses Geräts, aus denen Unterlagen ins Register kommen können.\n\
             # Im Register beginnt jeder Pfad mit dem Namen des Astes. Welche Unterordner\n\
             # gelesen werden, steht in \"regeln.toml\" beim Register.\n\
             [aeste]\n",
        );
        for (name, ort) in &self.aeste {
            text.push_str(&format!("{} = {}\n", wert(name), wert(&ort.to_string_lossy())));
        }
        text
    }

    /// Schreibt die Konfiguration. Erst als neue Datei, dann in einem Schritt
    /// an die Stelle der alten.
    pub fn speichern(&self, pfad: &Path) -> Ergebnis<()> {
        let neu = pfad.with_extension("toml.neu");
        std::fs::write(&neu, self.als_text())?;
        std::fs::rename(&neu, pfad)?;
        Ok(())
    }

    /// Enthält die geladene Datei noch Angaben aus der Zeit vor den Ästen?
    pub fn ist_alt(&self) -> bool {
        self.cloud_ordner.is_some() || !self.verzeichnisse.is_empty()
    }

    /// Der Ort eines Pfads aus dem Register (`Ast/Ordner/Datei`) auf diesem
    /// Gerät. `None`, wenn es den Ast hier nicht gibt.
    pub fn ort(&self, pfad: &str) -> Option<PathBuf> {
        let (ast, rest) = pfad.split_once('/').unwrap_or((pfad, ""));
        let wurzel = self.aeste.get(ast)?;
        Some(if rest.is_empty() { wurzel.clone() } else { wurzel.join(rest) })
    }

    /// Wie [`Konfiguration::ort`]. Fehlt der Ast, kommt ein Pfad zurück, den
    /// es nicht gibt: Die Datei gilt dann wie eine nicht vorhandene.
    pub fn ort_oder_nichts(&self, pfad: &str) -> PathBuf {
        self.ort(pfad).unwrap_or_default()
    }

    /// Der Ast eines Pfads aus dem Register.
    pub fn ast_von(pfad: &str) -> &str {
        pfad.split_once('/').map(|(ast, _)| ast).unwrap_or(pfad)
    }

    /// Nimmt einen Ordner dieses Geräts als Ast auf. Ohne Namen heißt der Ast
    /// wie der Ordner. Gibt es den Namen schon für einen anderen Ort, ist das
    /// ein Fehler: Der Name steht im Register für genau einen Ordner.
    pub fn ast_hinzufuegen(&mut self, ort: &Path, name: Option<&str>) -> Ergebnis<String> {
        if !ort.is_absolute() {
            return Err(Fehler::Konfiguration("Der Ordner muss mit absolutem Pfad angegeben sein".into()));
        }
        if let Some((vorhanden, _)) = self.aeste.iter().find(|(_, o)| o.as_path() == ort) {
            return Ok(vorhanden.clone());
        }
        let name = ast_name(match name {
            Some(n) => n.to_string(),
            None => ort.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
        .as_str())?;
        if self.aeste.contains_key(&name) {
            return Err(Fehler::Konfiguration(format!(
                "Einen Ast \"{name}\" gibt es schon. Bitte einen anderen Namen wählen."
            )));
        }
        self.aeste.insert(name.clone(), ort.to_path_buf());
        Ok(name)
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
    fn liest_aeste_und_findet_orte() {
        let k = Konfiguration::aus_text(
            "geraet = \"MacBook\"\n[aeste]\nDropbox = \"/c/Dropbox\"\n\"Dokumente\" = \"/home/x/Documents\"\n",
        )
        .unwrap();
        assert_eq!(k.geraet, "MacBook");
        assert_eq!(k.ort("Dropbox/Archiv/a.pdf"), Some(PathBuf::from("/c/Dropbox/Archiv/a.pdf")));
        assert_eq!(k.ort("Dokumente"), Some(PathBuf::from("/home/x/Documents")));
        assert_eq!(k.ort("Fremd/a.pdf"), None);
        assert!(!k.ort_oder_nichts("Fremd/a.pdf").exists());
        assert_eq!(Konfiguration::ast_von("Dropbox/Archiv/a.pdf"), "Dropbox");
        assert!(!k.ist_alt() && k.alter_ast.is_none());
        // hin und zurück
        assert_eq!(Konfiguration::aus_text(&k.als_text()).unwrap(), k);
    }

    #[test]
    fn frueherer_cloud_ordner_wird_ein_ast() {
        let k = Konfiguration::aus_text(
            "cloud_ordner = \"/tmp/Meine Cloud\"\nverzeichnisse = [\"Archiv/Alles/\", \"Archiv\\\\Belege\"]\n",
        )
        .unwrap();
        assert_eq!(k.aeste, BTreeMap::from([("Meine Cloud".to_string(), PathBuf::from("/tmp/Meine Cloud"))]));
        assert_eq!(k.alter_ast.as_deref(), Some("Meine Cloud"));
        assert_eq!(k.verzeichnisse, vec!["Archiv/Alles", "Archiv/Belege"]);
        assert!(k.ist_alt());
        // gespeichert bleiben nur die Äste
        let neu = Konfiguration::aus_text(&k.als_text()).unwrap();
        assert!(!neu.ist_alt() && neu.aeste == k.aeste && neu.alter_ast.is_none());
    }

    #[test]
    fn nimmt_aeste_auf() {
        let mut k = Konfiguration::default();
        assert_eq!(k.ast_hinzufuegen(Path::new("/x/Documents"), None).unwrap(), "Documents");
        // derselbe Ordner noch einmal: derselbe Ast
        assert_eq!(k.ast_hinzufuegen(Path::new("/x/Documents"), Some("Anders")).unwrap(), "Documents");
        // gleicher Name für einen anderen Ordner
        assert!(k.ast_hinzufuegen(Path::new("/y/Documents"), None).is_err());
        assert_eq!(k.ast_hinzufuegen(Path::new("/y/Documents"), Some("Dokumente alt")).unwrap(), "Dokumente alt");
        assert!(k.ast_hinzufuegen(Path::new("relativ"), None).is_err());
        assert!(k.ast_hinzufuegen(Path::new("/z"), Some("a/b")).is_err());
    }

    #[test]
    fn speichert_und_lehnt_ungueltiges_ab() {
        let tmp = tempfile::tempdir().unwrap();
        let pfad = tmp.path().join(DATEINAME);
        let mut k = Konfiguration::mit_ast("Dropbox", Path::new("/c/Dropbox"));
        k.geraet = "Mac \"mini\"".into();
        k.register_ordner = Some(PathBuf::from("/r"));
        k.speichern(&pfad).unwrap();
        assert_eq!(Konfiguration::laden(&pfad).unwrap(), k);

        assert!(Konfiguration::aus_text("cloud_ordner = \"cloud\"").is_err());
        assert!(Konfiguration::aus_text("[aeste]\nA = \"relativ\"").is_err());
        assert!(Konfiguration::aus_text("[aeste]\n\"a/b\" = \"/x\"").is_err());
        assert!(Konfiguration::aus_text("cloud_ordner = \"/c\"\nverzeichnisse = [\"../x\"]").is_err());
        assert!(Konfiguration::aus_text("tippfehler = 1").is_err());
        assert!(Konfiguration::aus_text("").unwrap().aeste.is_empty());
    }

    /// Laden, Suchen und der frühere Cloud-Ordner als Ast (v0.1.2).
    #[test]
    fn laden_suchen_und_cloud_ordner() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(Konfiguration::laden(&tmp.path().join("fehlt.toml")), Err(Fehler::Konfiguration(_))));
        let unten = tmp.path().join("a/b");
        std::fs::create_dir_all(&unten).unwrap();
        assert_eq!(Konfiguration::suchen_ab(&unten), None);
        std::fs::write(tmp.path().join("a").join(DATEINAME), "geraet = \"Test\"\n").unwrap();
        let gefunden = Konfiguration::suchen_ab(&unten).unwrap();
        assert_eq!(gefunden, tmp.path().join("a").join(DATEINAME));
        assert_eq!(Konfiguration::laden(&gefunden).unwrap().geraet, "Test");

        // Ein Ast mit dem Ort des Cloud-Ordners behält seinen Namen
        let k = Konfiguration::aus_text("cloud_ordner = \"/c/Dropbox\"\n[aeste]\nMeins = \"/c/Dropbox\"\n").unwrap();
        assert_eq!((k.alter_ast.as_deref(), k.aeste.len()), (Some("Meins"), 1));
        // Ohne brauchbaren Ordnernamen heißt der Ast "Cloud"
        let k = Konfiguration::aus_text("cloud_ordner = \"/\"\n").unwrap();
        assert_eq!(k.alter_ast.as_deref(), Some("Cloud"));
    }

}
