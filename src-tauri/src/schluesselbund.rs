//! Schlüssel des Registers im lokalen Schlüsselbund des Geräts (v0.1.0).
//!
//! Unter macOS ein Eintrag „Kennwort eines Programms“ im Anmelde-Schlüsselbund,
//! ausdrücklich ohne Abgleich über die iCloud (Kim: keine Bindung an einen
//! Cloud-Schlüsselbund). Ein Eintrag je Register-Ordner. Auf anderen Systemen
//! noch nicht umgesetzt: Der Schlüssel gilt dann nur, solange das Programm
//! läuft.

use std::path::Path;

/// Name des Dienstes im Schlüsselbund.
#[cfg(target_os = "macos")]
const DIENST: &str = "Registerwerk: Register-Schlüssel";

fn konto(ordner: &Path) -> String {
    ordner.display().to_string()
}

/// Liest den Schlüssel für das Register in `ordner`. `Ok(None)`, wenn es
/// keinen Eintrag gibt.
#[cfg(target_os = "macos")]
pub fn lesen(ordner: &Path) -> Result<Option<String>, String> {
    use security_framework::passwords::get_generic_password;
    match get_generic_password(DIENST, &konto(ordner)) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| "Eintrag im Schlüsselbund ist beschädigt".into()),
        Err(f) if f.code() == ERR_NICHT_GEFUNDEN => Ok(None),
        Err(f) => Err(format!("Schlüsselbund: {f}")),
    }
}

/// Legt den Schlüssel ab oder ersetzt einen vorhandenen Eintrag.
#[cfg(target_os = "macos")]
pub fn speichern(ordner: &Path, schluessel: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(DIENST, &konto(ordner), schluessel.as_bytes())
        .map_err(|f| format!("Schlüsselbund: {f}"))
}

/// Entfernt den Eintrag. Kein Fehler, wenn es keinen gibt.
#[cfg(target_os = "macos")]
pub fn entfernen(ordner: &Path) -> Result<(), String> {
    match security_framework::passwords::delete_generic_password(DIENST, &konto(ordner)) {
        Ok(()) => Ok(()),
        Err(f) if f.code() == ERR_NICHT_GEFUNDEN => Ok(()),
        Err(f) => Err(format!("Schlüsselbund: {f}")),
    }
}

/// `errSecItemNotFound`
#[cfg(target_os = "macos")]
const ERR_NICHT_GEFUNDEN: i32 = -25300;

#[cfg(not(target_os = "macos"))]
pub fn lesen(ordner: &Path) -> Result<Option<String>, String> {
    let _ = konto(ordner);
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
pub fn speichern(_ordner: &Path, _schluessel: &str) -> Result<(), String> {
    Err("Der Schlüsselbund ist auf diesem System noch nicht angebunden. Der Schlüssel gilt nur bis zum Beenden.".into())
}

#[cfg(not(target_os = "macos"))]
pub fn entfernen(_ordner: &Path) -> Result<(), String> {
    Ok(())
}

/// Wird der Schlüssel auf diesem System dauerhaft gespeichert?
pub const DAUERHAFT: bool = cfg!(target_os = "macos");
