// Unter Windows kein zusätzliches Konsolenfenster in der Release-Fassung.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri-Hülle um den Kern. Enthält keine Fachlogik, nur die Befehle für die
//! Oberfläche und die Suche nach Konfiguration und Register-Ordner.

use std::path::PathBuf;

use registerwerk_core::{Konfiguration, LaufErgebnis, Register, RegisterStand};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    /// Pfad der gefundenen Konfigurationsdatei.
    konfig_pfad: Option<String>,
    konfig_fehler: Option<String>,
    cloud_ordner: Option<String>,
    verzeichnisse: Vec<String>,
    register_ordner: Option<String>,
}

/// Sucht die Konfiguration in dieser Reihenfolge:
/// 1. Umgebungsvariable `REGISTERWERK_CONFIG`
/// 2. `config.local.toml` im Arbeitsverzeichnis oder darüber (Entwicklung)
/// 3. `config.local.toml` neben dem Programm oder darüber
/// 4. `config.local.toml` im Konfigurationsordner der Anwendung
fn konfig_pfad(app: &AppHandle) -> Option<PathBuf> {
    if let Some(pfad) = std::env::var_os("REGISTERWERK_CONFIG").map(PathBuf::from) {
        return Some(pfad);
    }
    let start_orte = [std::env::current_dir().ok(), std::env::current_exe().ok()];
    for ort in start_orte.into_iter().flatten() {
        if let Some(pfad) = Konfiguration::suchen_ab(&ort) {
            return Some(pfad);
        }
    }
    let pfad = app.path().app_config_dir().ok()?.join(registerwerk_core::konfiguration::DATEINAME);
    pfad.is_file().then_some(pfad)
}

fn konfig_laden(app: &AppHandle) -> Result<(PathBuf, Konfiguration), String> {
    let pfad = konfig_pfad(app).ok_or_else(|| {
        "Keine Konfiguration gefunden. Bitte config.beispiel.toml nach config.local.toml kopieren und die Pfade eintragen."
            .to_string()
    })?;
    let konfig = Konfiguration::laden(&pfad).map_err(|e| e.to_string())?;
    Ok((pfad, konfig))
}

fn register_ordner(app: &AppHandle, konfig: &Konfiguration) -> Result<PathBuf, String> {
    match &konfig.register_ordner {
        Some(ordner) => Ok(ordner.clone()),
        None => app.path().app_data_dir().map_err(|e| e.to_string()),
    }
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    let mut info = AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        konfig_pfad: konfig_pfad(&app).map(|p| p.display().to_string()),
        konfig_fehler: None,
        cloud_ordner: None,
        verzeichnisse: Vec::new(),
        register_ordner: None,
    };
    match konfig_laden(&app) {
        Ok((_, konfig)) => {
            info.register_ordner = register_ordner(&app, &konfig).ok().map(|p| p.display().to_string());
            info.cloud_ordner = Some(konfig.cloud_ordner.display().to_string());
            info.verzeichnisse = konfig.verzeichnisse;
        }
        Err(fehler) => info.konfig_fehler = Some(fehler),
    }
    info
}

/// Stand des Registers. `None`, solange noch kein Register angelegt ist.
#[tauri::command]
async fn register_stand(app: AppHandle) -> Result<Option<RegisterStand>, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || {
        if !ordner.join(registerwerk_core::register::DATEINAME).is_file() {
            return Ok(None);
        }
        let register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        register.stand().map(Some).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Liest den Dateibaum der konfigurierten Verzeichnisse ins Register.
#[tauri::command]
async fn dateibaum_einlesen(app: AppHandle) -> Result<LaufErgebnis, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        registerwerk_core::dateibaum_einlesen(&konfig, &mut register).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info, register_stand, dateibaum_einlesen])
        .run(tauri::generate_context!())
        .expect("Registerwerk konnte nicht gestartet werden");
}
