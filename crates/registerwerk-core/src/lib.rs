//! Kern von Registerwerk.
//!
//! Enthält alles, was keine Oberfläche braucht: Konfiguration, Durchlauf des
//! Dateibaums und das Register (SQLite). Die Tauri-Anwendung ist nur eine Hülle
//! darüber. Der lesende Teil soll sich später für eine Web-Oberfläche
//! wiederverwenden lassen (Konzept, Abschnitt 4).

pub mod dateibaum;
pub mod datum;
pub mod konfiguration;
pub mod register;

pub use konfiguration::Konfiguration;
pub use register::{LaufErgebnis, Register, RegisterStand};

/// Fehler des Kerns.
#[derive(Debug, thiserror::Error)]
pub enum Fehler {
    #[error("Konfiguration: {0}")]
    Konfiguration(String),
    #[error("Dateizugriff: {0}")]
    EinAusgabe(#[from] std::io::Error),
    #[error("Register: {0}")]
    Datenbank(#[from] rusqlite::Error),
}

pub type Ergebnis<T> = Result<T, Fehler>;

/// Liest den Dateibaum der konfigurierten Verzeichnisse und gleicht ihn mit dem
/// Register ab. Liest nur Metadaten, öffnet keine Datei und löst damit keinen
/// Download von "nur online"-Dateien aus.
pub fn dateibaum_einlesen(konfig: &Konfiguration, register: &mut Register) -> Ergebnis<LaufErgebnis> {
    let durchlauf = dateibaum::durchlaufen(konfig);
    register.abgleichen(&durchlauf)
}
