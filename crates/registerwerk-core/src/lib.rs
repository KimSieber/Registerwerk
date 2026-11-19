//! Kern von Registerwerk.
//!
//! Enthält alles, was keine Oberfläche braucht: Konfiguration, Durchlauf des
//! Dateibaums, Lesen der PDFs, Vorschauen, OCR und das Register (SQLite). Die Tauri-Anwendung ist nur eine Hülle
//! darüber. Der lesende Teil soll sich später für eine Web-Oberfläche
//! wiederverwenden lassen (Konzept, Abschnitt 4).

pub mod dateibaum;
pub mod datum;
pub mod guete;
pub mod inhalte;
pub mod konfiguration;
pub mod ocr;
pub mod pdf;
pub mod register;
pub mod scheiben;
pub mod vorschau;
pub mod textebene;
pub mod vorschauen;
pub mod woerter;

use std::path::Path;
use std::sync::atomic::AtomicBool;

pub use inhalte::{Fortschritt, InhaltErgebnis};
pub use konfiguration::Konfiguration;
pub use ocr::{OcrErgebnis, OcrStand, SeitenText};
pub use register::{LaufErgebnis, Register, RegisterStand};
pub use scheiben::{DateiVorschau, VorschauStand};
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

/// Liest den Dateibaum der konfigurierten Verzeichnisse und gleicht ihn mit dem
/// Register ab. Liest nur Metadaten, öffnet keine Datei und löst damit keinen
/// Download von "nur online"-Dateien aus.
pub fn dateibaum_einlesen(konfig: &Konfiguration, register: &mut Register) -> Ergebnis<LaufErgebnis> {
    let durchlauf = dateibaum::durchlaufen(konfig);
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
    inhalte::lesen(konfig, register, |pfad| pdf::lesen(pdfium, pfad), fortschritt, abbrechen)
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
        |pfad, seite| vorschau::pdf_rendern_je_seite(pdfium, pfad, seite),
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
        |pfad, nummern, seite| ocr::pdf_rendern(pdfium, pfad, nummern, seite),
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
