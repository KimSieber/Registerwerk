// Unter Windows kein zusätzliches Konsolenfenster in der Release-Fassung.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri-Hülle um den Kern. Enthält keine Fachlogik, nur die Befehle für die
//! Oberfläche und die Suche nach Konfiguration und Register-Ordner.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use registerwerk_core::{
    Anfrage, DateiDetails, DateiVorschau, Fortschritt, InhaltErgebnis, Konfiguration, LaufErgebnis, OcrErgebnis,
    OnlineOrdner, OrdnerPruefung, Regeln, Register, RegisterStand, SeitenText, Stufe, SuchErgebnis, SucheAngaben, SucheAufbau,
    TextebeneErgebnis, VorschauErgebnis,
};
use serde::{Deserialize, Serialize};
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

/// Abbruch einer laufenden Ordnerprüfung.
#[derive(Default)]
struct Pruefen(AtomicBool);

/// Eine Ordnerregel für die Oberfläche.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegelZeile {
    /// Pfad relativ zum Cloud-Ordner, leer für den Cloud-Ordner selbst.
    ordner: String,
    stufe: Stufe,
}

/// Ein Ordner im Baum der Regeln.
#[derive(Serialize)]
struct BaumEintrag {
    name: String,
    pfad: String,
    hat_unterordner: bool,
    bibliothek: bool,
    /// Dateien im Register in diesem Ordner samt Unterordnern.
    im_register: u64,
    /// Davon ausstehend und nur online.
    nur_online: u64,
    /// Nur bei Ästen: Ort auf diesem Gerät, wenn hier einer zugewiesen ist.
    ort: Option<String>,
    /// Nur bei Ästen: Den Ordner gibt es auf diesem Gerät nicht.
    fehlt: bool,
    /// Geräte, die den Ordner gesehen haben.
    geraete: Vec<String>,
}

/// Geöffnetes Register zum Lesen der Vorschauen, mit seinem Ordner.
#[derive(Default)]
struct Leser(Mutex<Option<(PathBuf, Register)>>);

#[derive(Serialize)]
struct AstInfo {
    name: String,
    ort: String,
    fehlt: bool,
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    /// Pfad der gefundenen Konfigurationsdatei.
    konfig_pfad: Option<String>,
    konfig_fehler: Option<String>,
    /// Name dieses Geräts.
    geraet: String,
    /// Äste dieses Geräts: Name, Ort, vorhanden.
    aeste: Vec<AstInfo>,
    /// Ordnerregeln aus `regeln.toml` im Register-Ordner.
    regeln: Vec<RegelZeile>,
    regeln_fehler: Option<String>,
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

/// Name dieses Geräts, wie ihn das System nennt.
fn geraetename() -> String {
    use std::process::Command;
    let aus = |programm: &str, args: &[&str]| {
        Command::new(programm)
            .args(args)
            .output()
            .ok()
            .map(|a| String::from_utf8_lossy(&a.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let name = if cfg!(target_os = "macos") { aus("scutil", &["--get", "ComputerName"]) } else { None };
    name.or_else(|| std::env::var("COMPUTERNAME").ok().filter(|s| !s.is_empty()))
        .or_else(|| aus("hostname", &[]))
        .unwrap_or_else(|| "Gerät".into())
}

/// Schützt das Schreiben der Konfiguration und die einmalige Umstellung auf
/// Äste: Die Oberfläche fragt beim Start mehrfach gleichzeitig an.
static KONFIG_SCHREIBEN: Mutex<()> = Mutex::new(());

/// Lädt die Konfiguration dieses Geräts. Gibt es noch keine, entsteht eine
/// leere im Konfigurationsordner der Anwendung. Dabei werden einmalig
/// erledigt: der Gerätename und die Umstellung einer Konfiguration aus der
/// Zeit vor den Ästen samt Regeln und Register (`umstellen_auf_aeste`).
fn konfig_laden(app: &AppHandle) -> Result<(PathBuf, Konfiguration), String> {
    let fehler = |e: registerwerk_core::Fehler| e.to_string();
    let pfad = match konfig_pfad(app) {
        Some(pfad) => pfad,
        None => {
            let _sperre = KONFIG_SCHREIBEN.lock().map_err(|e| e.to_string())?;
            let ordner = app.path().app_config_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
            let pfad = ordner.join(registerwerk_core::konfiguration::DATEINAME);
            if !pfad.is_file() {
                let mut neu = Konfiguration::default();
                neu.geraet = geraetename();
                neu.speichern(&pfad).map_err(fehler)?;
            }
            pfad
        }
    };
    let konfig = Konfiguration::laden(&pfad).map_err(fehler)?;
    if !konfig.ist_alt() && !konfig.geraet.trim().is_empty() {
        return Ok((pfad, konfig));
    }

    let _sperre = KONFIG_SCHREIBEN.lock().map_err(|e| e.to_string())?;
    // Ein anderer Aufruf kann inzwischen fertig sein
    let mut konfig = Konfiguration::laden(&pfad).map_err(fehler)?;
    if konfig.geraet.trim().is_empty() {
        konfig.geraet = geraetename();
    }
    if konfig.ist_alt() {
        let ordner = register_ordner(app, &konfig)?;
        if registerwerk_core::umstellen_auf_aeste(&konfig, &ordner).map_err(fehler)? {
            // Namen und Ordner stehen im Suchindex: einmal neu aufbauen
            let mut register = Register::oeffnen(&ordner).map_err(fehler)?;
            register.suche_aktualisieren(|_| {}, &AtomicBool::new(false)).map_err(fehler)?;
        }
        // legt die Regeln aus der früheren Liste an, falls es noch keine gibt
        registerwerk_core::regeln_laden(&konfig, &ordner).map_err(fehler)?;
        leser_zuruecksetzen(app);
    }
    // Ab jetzt stehen nur noch Gerät und Äste in der Datei
    konfig.speichern(&pfad).map_err(fehler)?;
    let konfig = Konfiguration::laden(&pfad).map_err(fehler)?;
    Ok((pfad, konfig))
}

/// Ändert die Konfiguration dieses Geräts und schreibt sie.
fn konfig_aendern<T>(
    app: &AppHandle,
    aenderung: impl FnOnce(&mut Konfiguration) -> Result<T, String>,
) -> Result<T, String> {
    let (pfad, _) = konfig_laden(app)?;
    let _sperre = KONFIG_SCHREIBEN.lock().map_err(|e| e.to_string())?;
    let mut konfig = Konfiguration::laden(&pfad).map_err(|e| e.to_string())?;
    let ergebnis = aenderung(&mut konfig)?;
    konfig.speichern(&pfad).map_err(|e| e.to_string())?;
    Ok(ergebnis)
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

fn regeln_laden(konfig: &Konfiguration, ordner: &std::path::Path) -> Result<Regeln, String> {
    registerwerk_core::regeln_laden(konfig, ordner).map_err(|e| e.to_string())
}

fn zeilen(regeln: &Regeln) -> Vec<RegelZeile> {
    regeln.eintraege().map(|(ordner, stufe)| RegelZeile { ordner: ordner.to_string(), stufe }).collect()
}

#[tauri::command]
fn app_info(app: AppHandle) -> AppInfo {
    let mut info = AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        konfig_pfad: None,
        konfig_fehler: None,
        geraet: String::new(),
        aeste: Vec::new(),
        regeln: Vec::new(),
        regeln_fehler: None,
        register_ordner: None,
        pdfium_ordner: pdfium_ordner(&app).map(|p| p.display().to_string()),
        tessdata_ordner: tessdata_ordner(&app).map(|p| p.display().to_string()),
        tesseract_version: registerwerk_core::ocr_version(),
    };
    match konfig_laden(&app) {
        Ok((pfad, konfig)) => {
            info.konfig_pfad = Some(pfad.display().to_string());
            let ordner = register_ordner(&app, &konfig);
            match ordner.as_ref().map_err(|e| e.clone()).and_then(|o| regeln_laden(&konfig, o)) {
                Ok(regeln) => info.regeln = zeilen(&regeln),
                Err(fehler) => info.regeln_fehler = Some(fehler),
            }
            info.register_ordner = ordner.ok().map(|p| p.display().to_string());
            info.aeste = konfig
                .aeste
                .iter()
                .map(|(name, ort)| AstInfo { name: name.clone(), ort: ort.display().to_string(), fehlt: !ort.is_dir() })
                .collect();
            info.geraet = konfig.geraet;
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

/// Liest den Dateibaum der Ordner mit Regel ins Register.
///
/// Würde der Abgleich wegen der Regeln Text und Vorschauen entfernen, ändert
/// er ohne `bestaetigt` nichts. Das Ergebnis trägt dann `probe` und nennt, was
/// entfernt würde. Die Oberfläche fragt nach und ruft erneut mit `bestaetigt`.
#[tauri::command]
async fn dateibaum_einlesen(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    bestaetigt: bool,
) -> Result<LaufErgebnis, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es läuft bereits ein Vorgang.".into());
    }
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let regeln = regeln_laden(&konfig, &ordner)?;
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        let lauf = registerwerk_core::dateibaum_einlesen(&konfig, &regeln, &mut register, bestaetigt)
            .map_err(|e| e.to_string())?;
        if !lauf.probe {
            // Neue Dateien sind sofort über ihren Namen auffindbar
            register.suche_aktualisieren(|_| {}, &AtomicBool::new(false)).map_err(|e| e.to_string())?;
        }
        Ok(lauf)
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
    ergebnis?
}

/// Die Ordnerregeln aus `regeln.toml`.
#[tauri::command]
async fn regeln_lesen(app: AppHandle) -> Result<Vec<RegelZeile>, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || regeln_laden(&konfig, &ordner).map(|r| zeilen(&r)))
        .await
        .map_err(|e| e.to_string())?
}

/// Schreibt die Ordnerregeln nach `regeln.toml`. Sie wirken beim nächsten
/// Einlesen des Dateibaums.
#[tauri::command]
async fn regeln_speichern(app: AppHandle, regeln: Vec<RegelZeile>) -> Result<Vec<RegelZeile>, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut neu = Regeln::default();
        for zeile in &regeln {
            neu.setzen(&zeile.ordner, Some(zeile.stufe)).map_err(|e| e.to_string())?;
        }
        neu.speichern(&ordner).map_err(|e| e.to_string())?;
        Ok(zeilen(&neu))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Die direkten Unterordner eines Ordners (Pfad im Register, `Ast/…`), mit
/// der Zahl der Dateien, die das Register dort kennt. Der leere Pfad liefert
/// die Äste: die dieses Geräts und die, die nur Regeln oder Register kennen.
#[tauri::command]
async fn ordner_liste(app: AppHandle, ordner: String) -> Result<Vec<BaumEintrag>, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let register = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut liste = registerwerk_core::dateibaum::unterordner(&konfig, &ordner).map_err(|e| e.to_string())?;
        let r = if register.join(registerwerk_core::register::DATEINAME).is_file() {
            Some(Register::oeffnen(&register).map_err(|e| e.to_string())?)
        } else {
            None
        };
        if ordner.is_empty() {
            // Äste, die es hier nicht gibt: aus den Regeln und dem Register
            let mut weitere: Vec<String> = regeln_laden(&konfig, &register)?
                .eintraege()
                .map(|(pfad, _)| Konfiguration::ast_von(pfad).to_string())
                .collect();
            if let Some(r) = &r {
                weitere.extend(r.gesehene_aeste().map_err(|e| e.to_string())?);
            }
            weitere.sort();
            weitere.dedup();
            for name in weitere {
                if !name.is_empty() && !liste.iter().any(|e| e.name == name) {
                    liste.push(registerwerk_core::OrdnerEintrag {
                        pfad: name.clone(),
                        name,
                        hat_unterordner: false,
                        bibliothek: false,
                        ort: None,
                        fehlt: true,
                    });
                }
            }
            liste.sort_by_key(|e| e.name.to_lowercase());
        }
        let pfade: Vec<String> = liste.iter().map(|e| e.pfad.clone()).collect();
        let n = pfade.len();
        let (zahlen, online, geraete) = match &r {
            Some(r) => (
                r.dateien_je_ordner(&pfade).map_err(|e| e.to_string())?,
                r.nur_online_je_ordner(&pfade).map_err(|e| e.to_string())?,
                r.geraete_je_ordner(&pfade).map_err(|e| e.to_string())?,
            ),
            None => (vec![0; n], vec![0; n], vec![Vec::new(); n]),
        };
        Ok(liste
            .into_iter()
            .zip(zahlen.into_iter().zip(online).zip(geraete))
            .map(|(e, ((im_register, nur_online), geraete))| BaumEintrag {
                name: e.name,
                pfad: e.pfad,
                hat_unterordner: e.hat_unterordner,
                bibliothek: e.bibliothek,
                im_register,
                nur_online,
                ort: e.ort,
                fehlt: e.fehlt,
                geraete,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Öffnet den Auswahldialog des Systems für einen Ordner. `None`, wenn
/// abgebrochen wurde.
#[tauri::command]
async fn ordner_waehlen(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let wahl = app.dialog().file().set_title("Ordner für Registerwerk wählen").blocking_pick_folder();
        match wahl {
            Some(ordner) => ordner.into_path().map(|p| Some(p.display().to_string())).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Nimmt einen Ordner dieses Geräts als Ast auf. Ohne Namen heißt der Ast wie
/// der Ordner. Mit `zuweisen` bekommt ein Ast, den es schon gibt, auf diesem
/// Gerät diesen Ort.
#[tauri::command]
async fn ast_hinzufuegen(app: AppHandle, ort: String, name: Option<String>, zuweisen: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ort = PathBuf::from(ort);
        if !ort.is_dir() {
            return Err(format!("Ordner nicht gefunden: {}", ort.display()));
        }
        konfig_aendern(&app, |konfig| {
            if zuweisen {
                let name = registerwerk_core::konfiguration::ast_name(name.as_deref().unwrap_or_default())
                    .map_err(|e| e.to_string())?;
                konfig.aeste.insert(name.clone(), ort);
                Ok(name)
            } else {
                konfig.ast_hinzufuegen(&ort, name.as_deref()).map_err(|e| e.to_string())
            }
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Löst einen Ast von diesem Gerät: Sein Ort steht nicht mehr in der
/// Konfiguration. Regeln und Register bleiben unberührt.
#[tauri::command]
async fn ast_loesen(app: AppHandle, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        konfig_aendern(&app, |konfig| {
            konfig.aeste.remove(&name);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Entfernt einen Ordner, den es nicht mehr gibt, aus dem Register: seinen
/// Bestand samt Unterordnern und seine Regeln. Liefert die Zahl der
/// entfernten Dateien.
#[tauri::command]
async fn ordner_entfernen(app: AppHandle, zustand: State<'_, Lesen>, ordner: String) -> Result<u64, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let register = register_ordner(&app, &konfig)?;
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es läuft bereits ein Vorgang.".into());
    }
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let ordner = registerwerk_core::dateibaum::relativ_normalisieren(&ordner);
        if ordner.is_empty() {
            return Err("Kein Ordner angegeben.".to_string());
        }
        let regeln = regeln_laden(&konfig, &register)?;
        let mut neu = Regeln::default();
        for (pfad, stufe) in regeln.eintraege() {
            if pfad != ordner && !pfad.starts_with(&format!("{ordner}/")) {
                neu.setzen(pfad, Some(stufe)).map_err(|e| e.to_string())?;
            }
        }
        neu.speichern(&register).map_err(|e| e.to_string())?;
        let mut r = Register::oeffnen(&register).map_err(|e| e.to_string())?;
        let dateien = r.ordner_entfernen(&ordner).map_err(|e| e.to_string())?;
        r.suche_aktualisieren(|_| {}, &AtomicBool::new(false)).map_err(|e| e.to_string())?;
        Ok(dateien)
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
    ergebnis?
}

/// Zählt die Dateien eines Ordners samt Unterordnern nach Typ. Liest nur
/// Verzeichnisse, lädt nichts herunter und schreibt nichts.
#[tauri::command]
async fn ordner_pruefen(app: AppHandle, ordner: String) -> Result<OrdnerPruefung, String> {
    let (_, konfig) = konfig_laden(&app)?;
    app.state::<Pruefen>().0.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        registerwerk_core::dateibaum::ordner_pruefen(&konfig, &ordner, &app2.state::<Pruefen>().0)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn ordner_pruefen_abbrechen(zustand: State<'_, Pruefen>) {
    zustand.0.store(true, Ordering::SeqCst);
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
        let e = registerwerk_core::inhalte_lesen(
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
        .map_err(|e| e.to_string())?;
        index_nachfuehren(&mut register, &zustand.abbrechen)?;
        Ok(e)
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
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
        let e = registerwerk_core::vorschauen_erzeugen(
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
        .map_err(|e| e.to_string())?;
        index_nachfuehren(&mut register, &zustand.abbrechen)?;
        Ok(e)
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
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
        let e = registerwerk_core::ocr_ausfuehren(
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
        .map_err(|e| e.to_string())?;
        index_nachfuehren(&mut register, &zustand.abbrechen)?;
        Ok(e)
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
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
        let e = registerwerk_core::textebene_schreiben(
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
        .map_err(|e| e.to_string())?;
        index_nachfuehren(&mut register, &zustand.abbrechen)?;
        Ok(e)
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
    ergebnis?
}

/// Führt den Suchindex nach einem Lauf nach, außer der Lauf wurde abgebrochen.
fn index_nachfuehren(register: &mut Register, abbrechen: &AtomicBool) -> Result<(), String> {
    if abbrechen.load(Ordering::SeqCst) {
        return Ok(());
    }
    register.suche_aktualisieren(|_| {}, abbrechen).map(|_| ()).map_err(|e| e.to_string())
}

/// Schließt das Register zum Lesen. Der nächste Zugriff öffnet es neu und
/// sieht damit auch eine neue Wortliste.
fn leser_zuruecksetzen(app: &AppHandle) {
    if let Ok(mut offen) = app.state::<Leser>().0.lock() {
        *offen = None;
    }
}

/// Baut den Suchindex für alle Dateien auf, deren Index fehlt oder veraltet
/// ist. Meldet den Fortschritt über `kanal`. Abbrechen über `inhalte_abbrechen`.
#[tauri::command]
async fn suche_aufbauen(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    kanal: Channel<Fortschritt>,
) -> Result<SucheAufbau, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    if zustand.laeuft.swap(true, Ordering::SeqCst) {
        return Err("Es läuft bereits ein Vorgang.".into());
    }
    zustand.abbrechen.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let ergebnis = tauri::async_runtime::spawn_blocking(move || {
        let zustand = app2.state::<Lesen>();
        let mut register = Register::oeffnen(&ordner).map_err(|e| e.to_string())?;
        register
            .suche_aktualisieren(
                |f| {
                    let _ = kanal.send(f.clone());
                },
                &zustand.abbrechen,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string());
    zustand.laeuft.store(false, Ordering::SeqCst);
    leser_zuruecksetzen(&app);
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

/// Sucht Dokumente (Stufe 2).
#[tauri::command]
async fn suchen(app: AppHandle, anfrage: Anfrage) -> Result<SuchErgebnis, String> {
    tauri::async_runtime::spawn_blocking(move || mit_leser(&app, |r| r.suchen(&anfrage).map_err(|e| e.to_string())))
        .await
        .map_err(|e| e.to_string())?
}

/// Ordner, Dateitypen und Jahre für die Filter der Suche.
#[tauri::command]
async fn suche_angaben(app: AppHandle) -> Result<SucheAngaben, String> {
    tauri::async_runtime::spawn_blocking(move || mit_leser(&app, |r| r.suche_angaben().map_err(|e| e.to_string())))
        .await
        .map_err(|e| e.to_string())?
}

/// Angaben zu einer Datei für den Detailbereich.
#[tauri::command]
async fn datei_details(app: AppHandle, datei: i64) -> Result<Option<DateiDetails>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mit_leser(&app, |r| r.datei_details(datei).map_err(|e| e.to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Voller Pfad einer Datei im Cloud-Ordner dieses Geräts.
fn original_pfad(app: &AppHandle, datei: i64) -> Result<PathBuf, String> {
    let (_, konfig) = konfig_laden(app)?;
    let d = mit_leser(app, |r| r.datei_details(datei).map_err(|e| e.to_string()))?
        .ok_or_else(|| "Die Datei steht nicht mehr im Register.".to_string())?;
    let Some(pfad) = konfig.ort(&d.pfad) else {
        return Err(format!(
            "Das Original liegt auf diesem Gerät nicht vor: Den Ast „{}“ gibt es hier nicht.",
            Konfiguration::ast_von(&d.pfad)
        ));
    };
    if !pfad.exists() {
        return Err(format!("Datei nicht gefunden: {}", pfad.display()));
    }
    Ok(pfad)
}

/// Öffnet eine Datei mit dem Standardprogramm des Systems oder zeigt sie im
/// Dateimanager. Eine "nur online"-Datei lädt der Cloud-Client dabei.
fn im_system(pfad: &std::path::Path, zeigen: bool) -> Result<(), String> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let mut befehl = {
        let mut b = Command::new("open");
        if zeigen {
            b.arg("-R");
        }
        b.arg(pfad);
        b
    };
    #[cfg(target_os = "windows")]
    let mut befehl = {
        let mut b = Command::new("explorer");
        if zeigen {
            b.arg(format!("/select,{}", pfad.display()));
        } else {
            b.arg(pfad);
        }
        b
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut befehl = {
        let mut b = Command::new("xdg-open");
        b.arg(if zeigen { pfad.parent().unwrap_or(pfad) } else { pfad });
        b
    };
    befehl.spawn().map(|_| ()).map_err(|e| format!("Öffnen: {e}"))
}

/// Öffnet das Original mit dem Standardprogramm.
#[tauri::command]
async fn original_oeffnen(app: AppHandle, datei: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || im_system(&original_pfad(&app, datei)?, false))
        .await
        .map_err(|e| e.to_string())?
}

/// Zeigt das Original im Finder bzw. Dateimanager.
#[tauri::command]
async fn im_finder_zeigen(app: AppHandle, datei: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || im_system(&original_pfad(&app, datei)?, true))
        .await
        .map_err(|e| e.to_string())?
}

/// Kennzahlen des Registers für das Datenbank-Symbol.
#[derive(Serialize)]
struct RegisterInfo {
    ordner: String,
    /// In diesem Ordner liegt ein Register.
    vorhanden: bool,
    /// Der Ordner steht in der Konfiguration. Sonst ist es der Datenordner
    /// der Anwendung im Benutzerprofil.
    gewaehlt: bool,
    index_bytes: u64,
    scheiben: u64,
    scheiben_bytes: u64,
    dateien: u64,
    seiten: u64,
    vorschauen: u64,
    /// Geräte mit dem Zeitpunkt ihres letzten Durchlaufs.
    geraete: Vec<(String, Option<i64>)>,
    geraet: String,
    regeln: usize,
}

fn register_info_von(konfig: &Konfiguration, ordner: &std::path::Path) -> Result<RegisterInfo, String> {
    let mut info = RegisterInfo {
        ordner: ordner.display().to_string(),
        vorhanden: ordner.join(registerwerk_core::register::DATEINAME).is_file(),
        gewaehlt: konfig.register_ordner.is_some(),
        index_bytes: 0,
        scheiben: 0,
        scheiben_bytes: 0,
        dateien: 0,
        seiten: 0,
        vorschauen: 0,
        geraete: Vec::new(),
        geraet: konfig.geraet.clone(),
        regeln: 0,
    };
    if let Ok(eintraege) = std::fs::read_dir(ordner) {
        for e in eintraege.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            if name.starts_with("register.db") {
                info.index_bytes += bytes;
            } else if name.starts_with("vorschau-") && name.contains(".db") {
                info.scheiben_bytes += bytes;
                info.scheiben += name.ends_with(".db") as u64;
            }
        }
    }
    if info.vorhanden {
        let r = Register::oeffnen(ordner).map_err(|e| e.to_string())?;
        let stand = r.stand().map_err(|e| e.to_string())?;
        info.dateien = stand.dateien;
        info.seiten = stand.inhalt.seiten;
        info.vorschauen = stand.vorschau.seiten;
        info.geraete = r.geraete().map_err(|e| e.to_string())?;
    }
    if let Ok(Some(regeln)) = Regeln::laden(ordner) {
        info.regeln = regeln.eintraege().count();
    }
    Ok(info)
}

#[tauri::command]
async fn register_info(app: AppHandle) -> Result<RegisterInfo, String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || register_info_von(&konfig, &ordner))
        .await
        .map_err(|e| e.to_string())?
}

/// Wählt einen anderen Ordner für das Register, etwa nachdem es verschoben
/// wurde. Liegt dort noch keines, geschieht ohne `bestaetigt` nichts: Die
/// Oberfläche fragt erst nach, ob dort ein neues, leeres entstehen soll.
/// Liefert die Kennzahlen des Ordners und ob er jetzt gilt.
#[tauri::command]
async fn register_waehlen(
    app: AppHandle,
    zustand: State<'_, Lesen>,
    ordner: String,
    bestaetigt: bool,
) -> Result<(RegisterInfo, bool), String> {
    if zustand.laeuft.load(Ordering::SeqCst) {
        return Err("Es läuft gerade ein Vorgang. Bitte erst abwarten.".into());
    }
    let (_, konfig) = konfig_laden(&app)?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ordner = PathBuf::from(ordner);
        if !ordner.is_dir() {
            return Err(format!("Ordner nicht gefunden: {}", ordner.display()));
        }
        let info = register_info_von(&konfig, &ordner)?;
        if !info.vorhanden && !bestaetigt {
            return Ok((info, false));
        }
        konfig_aendern(&app2, |k| {
            k.register_ordner = Some(ordner.clone());
            Ok(())
        })?;
        leser_zuruecksetzen(&app2);
        Ok((info, true))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Zeigt den Ordner des Registers im Finder bzw. Dateimanager.
#[tauri::command]
async fn register_im_finder(app: AppHandle) -> Result<(), String> {
    let (_, konfig) = konfig_laden(&app)?;
    let ordner = register_ordner(&app, &konfig)?;
    tauri::async_runtime::spawn_blocking(move || im_system(&ordner, true))
        .await
        .map_err(|e| e.to_string())?
}

/// Ordner mit Dateien, die nur online liegen und deren Inhalt aussteht, mit
/// `vorschau` deren Vorschau aussteht.
#[tauri::command]
async fn nur_online_ordner(app: AppHandle, vorschau: bool) -> Result<Vec<OnlineOrdner>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        mit_leser(&app, |r| {
            if vorschau { r.nur_online_ordner_vorschau() } else { r.nur_online_ordner() }.map_err(|e| e.to_string())
        })
    })
        .await
        .map_err(|e| e.to_string())?
}

/// Zeigt einen Ordner des Cloud-Ordners im Finder bzw. Dateimanager, etwa um
/// ihn dort offline verfügbar zu machen.
#[tauri::command]
async fn ordner_im_finder(app: AppHandle, ordner: String) -> Result<(), String> {
    let (_, konfig) = konfig_laden(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let relativ = registerwerk_core::dateibaum::relativ_normalisieren(&ordner);
        if relativ.split('/').any(|teil| teil == "..") {
            return Err(format!("Ordner \"{ordner}\" ist ungültig"));
        }
        let pfad = konfig.ort(&relativ).unwrap_or_default();
        if !pfad.is_dir() {
            return Err(format!("Ordner nicht gefunden: {}", pfad.display()));
        }
        im_system(&pfad, true)
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
        .manage(Pruefen::default())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("vorschau", |ctx, anfrage, antwort| {
            vorschau_protokoll(ctx.app_handle().clone(), anfrage, antwort)
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            register_stand,
            dateibaum_einlesen,
            regeln_lesen,
            regeln_speichern,
            ordner_liste,
            ordner_pruefen,
            ordner_pruefen_abbrechen,
            nur_online_ordner,
            ordner_im_finder,
            register_info,
            register_waehlen,
            register_im_finder,
            ordner_waehlen,
            ast_hinzufuegen,
            ast_loesen,
            ordner_entfernen,
            inhalte_lesen,
            inhalte_abbrechen,
            vorschauen_erzeugen,
            ocr_ausfuehren,
            textebene_schreiben,
            dateien_mit_vorschau,
            seitentexte,
            suche_aufbauen,
            suchen,
            suche_angaben,
            datei_details,
            original_oeffnen,
            im_finder_zeigen
        ])
        .run(tauri::generate_context!())
        .expect("Registerwerk konnte nicht gestartet werden");
}
