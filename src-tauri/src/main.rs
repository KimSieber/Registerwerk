// Unter Windows kein zusätzliches Konsolenfenster in der Release-Fassung.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri-Hülle um den Kern. Enthält keine Fachlogik, nur die Befehle für die
//! Oberfläche und die Suche nach Konfiguration und Register-Ordner.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use registerwerk_core::{
    DateiVorschau, Fortschritt, InhaltErgebnis, Konfiguration, LaufErgebnis, OcrErgebnis, Register, RegisterStand,
    SeitenText, TextebeneErgebnis, VorschauErgebnis,
};
use serde::Serialize;
use tauri::http::{Request, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, UriSchemeResponder};

/// Zustand eines laufenden Vorgangs, der ins Register schreibt (Inhalte lesen,
/// Vorschauen erzeugen oder Text erkennen). Es läuft höchstens einer.
#[derive(Default)]
struct Lesen {
    laeuft: AtomicBool,
    abbrechen: AtomicBool,
}

/// Geöffnetes Register zum Lesen der Vorschauen, mit seinem Ordner.
#[derive(Default)]
struct Leser(Mutex<Option<(PathBuf, Register)>>);

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
    /// Ordner mit den Sprachdaten für die OCR.
    tessdata_ordner: Option<String>,
    /// Version der eingebauten Tesseract-Bibliothek.
    tesseract_version: String,
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

/// Sucht den Ordner mit den Sprachdaten (Satz `registerwerk_core::ocr::MODELLE`) in dieser Reihenfolge:
/// 1. Umgebungsvariable `REGISTERWERK_TESSDATA`, darin der Unterordner des Satzes
/// 2. `tessdata/<Satz>` in den Ressourcen des Programmpakets
/// 3. `bibliotheken/tesseract/tessdata/<Satz>` im Arbeitsverzeichnis oder darüber (Entwicklung)
/// 4. dasselbe neben dem Programm oder darüber
fn tessdata_ordner(app: &AppHandle) -> Option<PathBuf> {
    use registerwerk_core::ocr;
    if let Some(basis) = std::env::var_os("REGISTERWERK_TESSDATA").map(PathBuf::from) {
        return Some(basis.join(ocr::MODELLE));
    }
    if let Ok(ressourcen) = app.path().resource_dir() {
        let ordner = ressourcen.join("tessdata").join(ocr::MODELLE);
        if ocr::tessdata_vollstaendig(&ordner) {
            return Some(ordner);
        }
    }
    let start_orte = [std::env::current_dir().ok(), std::env::current_exe().ok()];
    start_orte.into_iter().flatten().find_map(|ort| ocr::tessdata_suchen_ab(&ort, ocr::MODELLE))
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
        tessdata_ordner: tessdata_ordner(&app).map(|p| p.display().to_string()),
        tesseract_version: registerwerk_core::ocr_version(),
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
        return Err("Es läuft bereits ein Vorgang.".into());
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

/// Bittet einen laufenden Vorgang, nach der aktuellen Datei anzuhalten.
#[tauri::command]
fn inhalte_abbrechen(zustand: State<'_, Lesen>) {
    zustand.abbrechen.store(true, Ordering::SeqCst);
}

/// Erzeugt die Vorschauen aller ausstehenden PDFs. Meldet den Fortschritt
/// über `kanal`, höchstens etwa zehnmal je Sekunde. Abbrechen über
/// `inhalte_abbrechen`.
#[tauri::command]
async fn vorschauen_erzeugen(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    kanal: Channel<Fortschritt>,
) -> Result<VorschauErgebnis, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    let pdfium = pdfium_ordner(&app)
        .ok_or_else(|| "PDFium nicht gefunden. Bitte im Projektordner ausführen: bash tools/pdfium_holen.sh".to_string())?;
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es läuft bereits ein Vorgang.".into());
    }
    zustand.abbrechen.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let zustand = app2.state::<Lesen>();
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        let mut zuletzt = Instant::now() - Duration::from_secs(1);
        registerwerk_core::vorschauen_erzeugen(
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

/// Erkennt den Text aller Seiten ohne Text samt Lage der Wörter. Meldet den
/// Fortschritt über `kanal`, höchstens etwa zehnmal je Sekunde. Abbrechen über
/// `inhalte_abbrechen`. Schreibt nur ins Register, die PDFs bleiben unverändert.
#[tauri::command]
async fn ocr_ausfuehren(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    kanal: Channel<Fortschritt>,
) -> Result<OcrErgebnis, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    let pdfium = pdfium_ordner(&app)
        .ok_or_else(|| "PDFium nicht gefunden. Bitte im Projektordner ausführen: bash tools/pdfium_holen.sh".to_string())?;
    let tessdata = tessdata_ordner(&app).ok_or_else(|| {
        "Sprachdaten für die OCR nicht gefunden. Bitte im Projektordner ausführen: bash tools/tesseract_holen.sh".to_string()
    })?;
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es läuft bereits ein Vorgang.".into());
    }
    zustand.abbrechen.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let zustand = app2.state::<Lesen>();
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        let mut zuletzt = Instant::now() - Duration::from_secs(1);
        registerwerk_core::ocr_ausfuehren(
            &konfig,
            &mut register,
            &pdfium,
            &tessdata,
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

/// Ordner für die Kopien des Probelaufs: `target/textebene_probelauf` im
/// Projektordner (Entwicklung), sonst im Datenordner der Anwendung. Der
/// Projektordner ist der erste Ordner ab Arbeitsverzeichnis oder Programm
/// aufwärts, der `Cargo.lock` und `bibliotheken` enthält.
fn probelauf_ordner(app: &AppHandle) -> Result<PathBuf, String> {
    let start_orte = [std::env::current_dir().ok(), std::env::current_exe().ok()];
    let projekt = start_orte
        .into_iter()
        .flatten()
        .find_map(|ort| ort.ancestors().find(|o| o.join("Cargo.lock").is_file() && o.join("bibliotheken").is_dir()).map(PathBuf::from));
    match projekt {
        Some(p) => Ok(p.join("target").join("textebene_probelauf")),
        None => app.path().app_data_dir().map(|d| d.join("textebene_probelauf")).map_err(|e| e.to_string()),
    }
}

/// Schreibt den erkannten Text als unsichtbare Textebene in die Scans. Mit
/// `probelauf` entstehen nur Kopien (siehe `probelauf_ordner`), Originale
/// und Register bleiben unberührt. Mit `eine` endet der Lauf nach der ersten
/// geschriebenen Datei. Meldet den Fortschritt über `kanal`. Abbrechen über
/// `inhalte_abbrechen`.
#[tauri::command]
async fn textebene_schreiben(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    probelauf: bool,
    eine: bool,
    kanal: Channel<Fortschritt>,
) -> Result<TextebeneErgebnis, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    let pdfium = pdfium_ordner(&app)
        .ok_or_else(|| "PDFium nicht gefunden. Bitte im Projektordner ausführen: bash tools/pdfium_holen.sh".to_string())?;
    let ziel = if probelauf { Some(probelauf_ordner(&app)?) } else { None };
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es läuft bereits ein Vorgang.".into());
    }
    zustand.abbrechen.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let zustand = app2.state::<Lesen>();
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        let mut zuletzt = Instant::now() - Duration::from_secs(1);
        registerwerk_core::textebene_schreiben(
            &konfig,
            &mut register,
            &pdfium,
            ziel.as_deref(),
            eine.then_some(1),
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

/// Führt `f` mit dem Register zum Lesen aus. Öffnet es beim ersten Aufruf und
/// erneut, wenn sich der Register-Ordner geändert hat.
fn mit_leser<T>(app: &AppHandle, f: impl FnOnce(&Register) -> Result<T, String>) -> Result<T, String> {
    let (_, konfig) = konfig_laden(app)?;
    let ordner = register_ordner(app, &konfig)?;
    let leser = app.state::<Leser>();
    let mut offen = leser.0.lock().map_err(|e| e.to_string())?;
    if offen.as_ref().map(|(o, _)| o != &ordner).unwrap_or(true) {
        if !ordner.join(registerwerk_core::register::DATEINAME).is_file() {
            return Err("Noch kein Register vorhanden.".into());
        }
        let register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        *offen = Some((ordner, register));
    }
    f(&offen.as_ref().expect("eben geöffnet").1)
}

/// Dateien mit Vorschauen, deren Pfad alle Wörter von `muster` enthält,
/// mit `nur_ocr` nur solche mit erkanntem Text. Nur zum Prüfen, die Suche
/// folgt in Stufe 2.
#[tauri::command]
async fn dateien_mit_vorschau(app: AppHandle, muster: String, nur_ocr: bool) -> Result<Vec<DateiVorschau>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mit_leser(&app, |r| r.dateien_mit_vorschau(&muster, nur_ocr, 100).map_err(|e| e.to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Eingebetteter und erkannter Text aller Seiten einer Datei.
#[tauri::command]
async fn seitentexte(app: AppHandle, datei: i64) -> Result<Vec<SeitenText>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mit_leser(&app, |r| r.seitentexte(datei).map_err(|e| e.to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Liefert eine Vorschau für `vorschau://localhost/<datei>/<seite>`.
/// Läuft außerhalb des Fensters, damit das Lesen die Oberfläche nicht aufhält.
fn vorschau_protokoll(app: AppHandle, anfrage: Request<Vec<u8>>, antwort: UriSchemeResponder) {
    tauri::async_runtime::spawn_blocking(move || {
        let teile: Vec<&str> = anfrage.uri().path().trim_matches('/').split('/').collect();
        let schluessel = match teile.as_slice() {
            [datei, seite] => datei.parse::<i64>().ok().zip(seite.parse::<u32>().ok()),
            _ => None,
        };
        let ergebnis = match schluessel {
            Some((datei, seite)) => mit_leser(&app, |r| r.vorschau(datei, seite).map_err(|e| e.to_string())),
            None => Ok(None),
        };
        let fertig = match ergebnis {
            Ok(Some(avif)) => Response::builder()
                .header("Content-Type", "image/avif")
                .header("Cache-Control", "max-age=3600")
                .body(avif),
            Ok(None) => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
            Err(fehler) => Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(fehler.into_bytes()),
        };
        antwort.respond(fertig.expect("gültige Antwort"));
    });
}

fn main() {
    tauri::Builder::default()
        .manage(Lesen::default())
        .manage(Leser::default())
        .register_asynchronous_uri_scheme_protocol("vorschau", |ctx, anfrage, antwort| {
            vorschau_protokoll(ctx.app_handle().clone(), anfrage, antwort)
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            register_stand,
            dateibaum_einlesen,
            inhalte_lesen,
            inhalte_abbrechen,
            vorschauen_erzeugen,
            ocr_ausfuehren,
            textebene_schreiben,
            dateien_mit_vorschau,
            seitentexte
        ])
        .run(tauri::generate_context!())
        .expect("Registerwerk konnte nicht gestartet werden");
}
