//! Kern von Registerwerk.
//!
//! Enthält alles, was keine Oberfläche braucht: Konfiguration, Durchlauf des
//! Dateibaums, Lesen der PDFs, Vorschauen, OCR und das Register (SQLite). Die Tauri-Anwendung ist nur eine Hülle
//! darüber. Der lesende Teil soll sich später für eine Web-Oberfläche
//! wiederverwenden lassen (Konzept, Abschnitt 4).

pub mod altoffice;
pub mod bild;
pub mod dateibaum;
pub mod datum;
pub mod dokumente;
pub mod formate;
pub mod guete;
pub mod inhalte;
pub mod konfiguration;
pub mod ocr;
pub mod pdf;
pub mod regeln;
pub mod register;
pub mod scheiben;
pub mod suche;
pub mod vorschau;
pub mod textbild;
pub mod textebene;
pub mod vorschauen;
pub mod woerter;

use std::path::Path;
use std::sync::atomic::AtomicBool;

pub use inhalte::{Fortschritt, InhaltErgebnis};
pub use konfiguration::Konfiguration;
pub use ocr::{OcrErgebnis, OcrStand, SeitenText};
pub use dateibaum::{OrdnerEintrag, OrdnerPruefung};
pub use regeln::{Regeln, Stufe};
pub use register::{LaufErgebnis, OnlineOrdner, OrdnerBestand, Register, RegisterStand};
pub use scheiben::{DateiVorschau, VorschauStand};
pub use suche::{Anfrage, DateiDetails, SuchErgebnis, SucheAngaben, SucheAufbau, Treffer, Zaehler};
pub use textebene::{TextebeneErgebnis, TextebeneStand};
pub use vorschauen::{Messung, VorschauErgebnis};

/// Fehler des Kerns.
#[derive(Debug, thiserror::Error)]
pub enum Fehler {
    #[error("Konfiguration: {0}")]
    Konfiguration(String),
    #[error("Dateizugriff: {0}")]
    EinAusgabe(#[from] std::io::Error),
    #[error("Register: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("{0}")]
    Pdf(String),
}

pub type Ergebnis<T> = Result<T, Fehler>;

/// Lädt die Ordnerregeln aus dem Register-Ordner. Gibt es dort noch keine
/// `regeln.toml`, entsteht sie aus der früheren Liste `verzeichnisse` der
/// Konfiguration (bis v0.0.6), jeder Ordner mit der Stufe Voll.
pub fn regeln_laden(konfig: &Konfiguration, register_ordner: &Path) -> Ergebnis<Regeln> {
    if let Some(regeln) = Regeln::laden(register_ordner)? {
        return Ok(regeln);
    }
    let regeln = match &konfig.alter_ast {
        Some(ast) => Regeln::voll(&konfig.verzeichnisse.iter().map(|v| format!("{ast}/{v}")).collect::<Vec<_>>()),
        None => Regeln::default(),
    };
    regeln.speichern(register_ordner)?;
    Ok(regeln)
}

/// Einmalige Umstellung auf Äste (v0.0.7). Bis dahin gab es einen
/// Cloud-Ordner, und Regeln und Register nannten ihre Pfade relativ dazu.
/// Der Cloud-Ordner ist jetzt ein Ast (`Konfiguration::alter_ast`): Jeder
/// Pfad in `regeln.toml` und im Register bekommt dessen Namen davor. Nichts
/// wird neu gelesen. Liefert, ob das Register umgestellt wurde.
///
/// Darf beliebig oft laufen: Die Regeln werden nur ergänzt, wo der Name
/// fehlt, das Register merkt sich die Umstellung.
pub fn umstellen_auf_aeste(konfig: &Konfiguration, register_ordner: &Path) -> Ergebnis<bool> {
    let Some(ast) = &konfig.alter_ast else { return Ok(false) };
    if !register_ordner.join(register::DATEINAME).is_file() {
        // Noch kein Register: Die Regeln entstehen in `regeln_laden` gleich
        // mit dem Namen des Astes.
        return Ok(false);
    }
    let mut register = Register::oeffnen(register_ordner)?;
    if register.merkmal("pfade_mit_ast")?.is_some() {
        return Ok(false);
    }
    if let Some(alt) = Regeln::laden(register_ordner)? {
        let mut neu = Regeln::default();
        for (ordner, stufe) in alt.eintraege() {
            let schon = ordner == ast || ordner.starts_with(&format!("{ast}/"));
            let pfad = if schon { ordner.to_string() } else { format!("{ast}/{ordner}") };
            neu.setzen(&pfad, Some(stufe))?;
        }
        neu.speichern(register_ordner)?;
    }
    register.pfade_voranstellen(ast)
}

/// Liest den Dateibaum der Ordner mit Regel und gleicht ihn mit dem Register
/// ab. Liest nur Metadaten, öffnet keine Datei und löst damit keinen Download
/// von "nur online"-Dateien aus.
///
/// Würde der Abgleich Einträge entfernen oder auf den Namen kürzen, ändert er
/// ohne `bestaetigt` nichts und liefert nur, was er entfernen würde
/// (`LaufErgebnis::probe`).
pub fn dateibaum_einlesen(
    konfig: &Konfiguration,
    regeln: &Regeln,
    register: &mut Register,
    bestaetigt: bool,
) -> Ergebnis<LaufErgebnis> {
    let durchlauf = dateibaum::durchlaufen(konfig, regeln);
    if !bestaetigt {
        let probe = register.abgleich_pruefen(&durchlauf)?;
        if probe.bestaetigung_noetig() {
            return Ok(probe);
        }
    }
    register.abgleichen(&durchlauf)
}

/// Liest die Inhalte aller ausstehenden PDFs mit PDFium aus `pdfium_ordner`
/// ins Register. Öffnet nur lokal vorhandene Dateien und verändert keine.
pub fn inhalte_lesen(
    konfig: &Konfiguration,
    register: &mut Register,
    pdfium_ordner: &Path,
    fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<InhaltErgebnis> {
    let pdfium = pdf::einbinden(pdfium_ordner)?;
    inhalte::lesen(
        konfig,
        register,
        |pfad| {
            if formate::ist_dokument(pfad) {
                dokumente::lesen(pfad)
            } else if formate::ist_bild(pfad) {
                Ok(bild::inhalt())
            } else {
                pdf::lesen(pdfium, pfad)
            }
        },
        fortschritt,
        abbrechen,
    )
}

/// Erzeugt die Vorschauen aller ausstehenden PDFs mit PDFium aus
/// `pdfium_ordner`. Kodiert auf allen Kernen. Öffnet nur lokal vorhandene
/// Dateien und verändert keine.
pub fn vorschauen_erzeugen(
    konfig: &Konfiguration,
    register: &mut Register,
    pdfium_ordner: &Path,
    fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<VorschauErgebnis> {
    let pdfium = pdf::einbinden(pdfium_ordner)?;
    let faeden = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    vorschauen::erzeugen(
        konfig,
        register,
        |pfad, seite| {
            if formate::ist_bild(pfad) {
                // Ein Bild ist eine Seite
                let bild = bild::laden(pfad)?;
                if !seite(bild) {
                    return Err(vorschauen::ABGEBROCHEN.into());
                }
                Ok(1)
            } else if formate::ist_text(pfad) {
                // Eine Textdatei ist eine gezeichnete Seite
                let text = dokumente::lesen(pfad)?.seiten.into_iter().next().map(|s| s.text).unwrap_or_default();
                if !seite(textbild::zeichnen(&text)) {
                    return Err(vorschauen::ABGEBROCHEN.into());
                }
                Ok(1)
            } else {
                vorschau::pdf_rendern_je_seite(pdfium, pfad, seite)
            }
        },
        |bild| vorschau::erzeugen(bild, Some(1)),
        faeden,
        fortschritt,
        abbrechen,
    )
}

/// Version der eingebauten Tesseract-Bibliothek.
pub fn ocr_version() -> String {
    registerwerk_ocr::version()
}

/// Erkennt den Text aller Seiten ohne Text mit Tesseract, samt Lage der
/// Wörter. Rendert mit PDFium aus `pdfium_ordner`, die Sprachdaten liegen in
/// `tessdata`. Erkennt auf allen Kernen. Schreibt nur ins Register, die
/// PDFs bleiben unverändert.
pub fn ocr_ausfuehren(
    konfig: &Konfiguration,
    register: &mut Register,
    pdfium_ordner: &Path,
    tessdata: &Path,
    fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<OcrErgebnis> {
    let pdfium = pdf::einbinden(pdfium_ordner)?;
    let faeden = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    ocr::erkennen(
        konfig,
        register,
        |pfad, nummern, seite| {
            if formate::ist_bild(pfad) {
                // Ein Bild hat nur Seite 1, erkannt mit 300 dpi angenommen
                if nummern.contains(&1) && !seite(1, bild::fuer_ocr(pfad)?, ocr::DPI) {
                    return Err(vorschauen::ABGEBROCHEN.into());
                }
                Ok(())
            } else {
                ocr::pdf_rendern(pdfium, pfad, nummern, seite)
            }
        },
        || {
            let mut erkenner = registerwerk_ocr::Erkenner::neu(tessdata, ocr::SPRACHEN)?;
            Ok(Box::new(move |bild: &ocr::Graubild, dpi: u32| {
                erkenner
                    .erkennen(&bild.grau, bild.breite, bild.hoehe, dpi)
                    .map(|e| ocr::Erkannt {
                        woerter: woerter::aus_tesseract(&e.woerter, bild.breite, bild.hoehe),
                        text: e.text,
                        konfidenz: e.konfidenz,
                    })
            }) as ocr::ErkennerFn)
        },
        faeden,
        fortschritt,
        abbrechen,
    )
}

/// Schreibt den erkannten Text als unsichtbare Textebene in die Scans
/// (v0.0.5). Prüft jede neue Fassung mit PDFium aus `pdfium_ordner`, bevor
/// sie das Original ersetzt. Mit `probelauf` entstehen nur Kopien in diesem
/// Ordner, Originale und Register bleiben unberührt. Mit `hoechstens` endet
/// der Lauf nach so vielen geschriebenen Dateien. Das Protokoll liegt neben
/// dem Register.
pub fn textebene_schreiben(
    konfig: &Konfiguration,
    register: &mut Register,
    pdfium_ordner: &Path,
    probelauf: Option<&Path>,
    hoechstens: Option<u64>,
    fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<TextebeneErgebnis> {
    let pdfium = pdf::einbinden(pdfium_ordner)?;
    let protokoll = register.ordner().map(|o| o.join(textebene::PROTOKOLL));
    textebene::schreiben(
        konfig,
        register,
        &|alt: &Path, neu: &Path, seiten: &[u32]| pdf::textebene_pruefen(pdfium, alt, neu, seiten),
        probelauf,
        protokoll.as_deref(),
        hoechstens,
        fortschritt,
        abbrechen,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stellt_regeln_und_register_auf_aeste_um() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = tmp.path().join("Meine Cloud");
        std::fs::create_dir_all(cloud.join("Archiv/Alles")).unwrap();
        std::fs::write(cloud.join("Archiv/Alles/a.pdf"), b"x").unwrap();
        let reg = tmp.path().join("register");
        let alt = Konfiguration::aus_text(&format!(
            "cloud_ordner = {:?}\nverzeichnisse = [\"Archiv/Alles\"]\n",
            cloud.to_string_lossy()
        ))
        .unwrap();

        // Stand von v0.0.6: Regeln und Pfade ohne Ast
        Regeln::voll(&["Archiv/Alles"]).speichern(&reg).unwrap();
        {
            let mut register = Register::oeffnen(&reg).unwrap();
            let mut ohne_ast = Konfiguration::mit_ast("Archiv", &cloud.join("Archiv"));
            ohne_ast.geraet = "Notebook".into();
            dateibaum_einlesen(&ohne_ast, &Regeln::voll(&["Archiv/Alles"]), &mut register, true).unwrap();
            // als wäre die Umstellung noch nicht gelaufen
            register.sql_ausfuehren("DELETE FROM ordner_gesehen").unwrap();
        }

        assert!(umstellen_auf_aeste(&alt, &reg).unwrap());
        assert!(!umstellen_auf_aeste(&alt, &reg).unwrap());
        let regeln = regeln_laden(&alt, &reg).unwrap();
        assert_eq!(regeln.eintraege().collect::<Vec<_>>(), vec![("Meine Cloud/Archiv/Alles", Stufe::Voll)]);

        // Danach findet der Abgleich dieselbe Datei wieder: nichts neu, nichts entfernt
        let mut register = Register::oeffnen(&reg).unwrap();
        let e = dateibaum_einlesen(&alt, &regeln, &mut register, false).unwrap();
        assert_eq!((e.neu, e.entfernt, e.unveraendert, e.probe), (0, 0, 1, false));

        // Ohne Register entstehen die Regeln gleich mit dem Ast
        let neu = tmp.path().join("neu");
        assert!(!umstellen_auf_aeste(&alt, &neu).unwrap());
        let regeln = regeln_laden(&alt, &neu).unwrap();
        assert_eq!(regeln.eintraege().next().unwrap().0, "Meine Cloud/Archiv/Alles");
    }
}
