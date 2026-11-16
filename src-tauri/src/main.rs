// Unter Windows kein zusätzliches Konsolenfenster in der Release-Fassung.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri-Hülle um den Kern. Enthält keine Fachlogik, nur die Befehle für die
//! Oberfläche und die Suche nach Konfiguration und Register-Ordner.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use registerwerk_core::{Fortschritt, InhaltErgebnis, Konfiguration, LaufErgebnis, Register, RegisterStand};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

/// Zustand eines laufenden Lesevorgangs. Es läuft höchstens einer.
#[derive(Default)]
struct Lesen {
    laeuft: AtomicBool,
    abbrechen: AtomicBool,
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    /// Pfad der gefundenen Konfigurationsdatei.
    konfig_pfad: Option<String>,
    konfig_fehler: Option<String>,
    cloud_ordner: Option<String>,
    verzeichnisse: Vec<String>,
    register_ordner: Option<String>,
    /// Ordner, aus dem PDFium geladen wird.
    pdfium_ordner: Option<String>,
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

/// Sucht den Ordner mit der PDFium-Bibliothek in dieser Reihenfolge:
/// 1. Umgebungsvariable `REGISTERWERK_PDFIUM`
/// 2. `pdfium` in den Ressourcen des Programmpakets
/// 3. `bibliotheken/pdfium/lib` im Arbeitsverzeichnis oder darüber (Entwicklung)
/// 4. dasselbe neben dem Programm oder darüber
fn pdfium_ordner(app: &AppHandle) -> Option<PathBuf> {
    if let Some(ordner) = std::env::var_os("REGISTERWERK_PDFIUM").map(PathBuf::from) {
        return Some(ordner);
    }
    if let Ok(ressourcen) = app.path().resource_dir() {
        let ordner = ressourcen.join("pdfium");
        if registerwerk_core::pdf::bibliothek_in(&ordner).is_file() {
            return Some(ordner);
        }
    }
    let start_orte = [std::env::current_dir().ok(), std::env::current_exe().ok()];
    start_orte.into_iter().flatten().find_map(|ort| registerwerk_core::pdf::suchen_ab(&ort))
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
        pdfium_ordner: pdfium_ordner(&app).map(|p| p.display().to_string()),
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

/// Liest die Inhalte aller ausstehenden PDFs ins Register. Meldet den
/// Fortschritt über `kanal`, höchstens etwa zehnmal je Sekunde.
#[tauri::command]
async fn inhalte_lesen(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    kanal: Channel<Fortschritt>,
) -> Result<InhaltErgebnis, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    let pdfium = pdfium_ordner(&app)
        .ok_or_else(|| "PDFium nicht gefunden. Bitte im Projektordner ausführen: bash tools/pdfium_holen.sh".to_string())?;
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es werden bereits Inhalte gelesen.".into());
    }
    zustand.abbrechen.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let zustand = app2.state::<Lesen>();
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        let mut zuletzt = Instant::now() - Duration::from_secs(1);
        registerwerk_core::inhalte_lesen(
            &konfig,
            &mut register,
            &pdfium,
            |f| {
                if f.erledigt == f.gesamt || zuletzt.elapsed() >= Duration::from_millis(100) {
                    zuletzt = Instant::now();
                    let _ = kanal.send(f.clone());
                }
            },
            &zustand.abbrechen,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    ergebnis?
}

/// Bittet einen laufenden Lesevorgang, nach der aktuellen Datei anzuhalten.
#[tauri::command]
fn inhalte_abbrechen(zustand: State<'_, Lesen>) {
    zustand.abbrechen.store(true, Ordering::SeqCst);
}

fn main() {
    tauri::Builder::default()
        .manage(Lesen::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            register_stand,
            dateibaum_einlesen,
            inhalte_lesen,
            inhalte_abbrechen
        ])
        .run(tauri::generate_context!())
        .expect("Registerwerk konnte nicht gestartet werden");
}
