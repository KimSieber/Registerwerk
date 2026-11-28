//! Verschlüsselung des Registers (v0.1.0, Stufe 3).
//!
//! Index-Datenbank und Scheiben sind SQLCipher-Datenbanken (AES-256, je
//! Seite der Datenbank mit eigenem Zufallswert und Prüfsumme). Der Schlüssel
//! ist ein fertiger Schlüssel mit 256 Bit, den Kim selbst erzeugt und im
//! Passwortsafe aufbewahrt. Kein Kennwort, keine Schlüsselableitung, kein
//! Salt in Registerwerk (Kim, 24.11.2026). Auf jedem Gerät hält der lokale
//! Schlüsselbund den Schlüssel, das erledigt die Hülle.
//!
//! Der Kern merkt sich den Schlüssel je Register-Ordner im Arbeitsspeicher
//! ([`hinterlegen`]). Jede Verbindung zu einer Datei des Registers läuft über
//! [`verbinden`]: Ob eine Datei verschlüsselt ist, zeigt ihr Kopf. Eine
//! unverschlüsselte SQLite-Datei beginnt mit `SQLite format 3`.
//!
//! Neue Dateien werden verschlüsselt angelegt, sobald für den Ordner ein
//! Schlüssel hinterlegt ist. Ist ein Schlüssel hinterlegt, öffnet
//! [`verbinden`] keine unverschlüsselte Datei mehr (v0.1.4). Sonst würde eine
//! zurückgespielte ältere Fassung oder eine ausgetauschte Datei still
//! unverschlüsselt weiterbeschrieben, und ihr Inhalt (etwa das gespeicherte
//! Schema) stammte nicht sicher von Registerwerk. Eine angehaltene Umstellung
//! gilt damit ebenfalls als gesperrt, bis [`umstellen`] sie beendet. Das
//! Umstellen selbst öffnet die Dateien ohne [`verbinden`].

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::inhalte::Fortschritt;
use crate::register::DATEINAME;
use crate::{Ergebnis, Fehler};

/// Kopf einer unverschlüsselten SQLite-Datei.
const KOPF_KLAR: &[u8; 16] = b"SQLite format 3\0";

/// Endung der Arbeitskopie beim Umstellen.
const ARBEITSKOPIE: &str = ".verschluesseln";

/// Ein Schlüssel mit 256 Bit. Wird beim Verwerfen überschrieben.
#[derive(Clone)]
pub struct Schluessel([u8; 32]);

/// Vergleich in konstanter Zeit (v0.1.4): Die Dauer verrät nicht, ab welchem
/// Byte zwei Schlüssel abweichen.
impl PartialEq for Schluessel {
    fn eq(&self, anderer: &Self) -> bool {
        let mut unterschied = 0u8;
        for (a, b) in self.0.iter().zip(anderer.0.iter()) {
            unterschied |= a ^ b;
        }
        std::hint::black_box(unterschied) == 0
    }
}

impl Eq for Schluessel {}

impl Schluessel {
    /// Liest einen Schlüssel aus 64 Hex-Zeichen. Leerzeichen, Bindestriche und
    /// Zeilenumbrüche zur Gliederung werden übergangen, Groß- und
    /// Kleinschreibung zählt nicht.
    pub fn aus_text(text: &str) -> Ergebnis<Self> {
        let mut ziffern = Vec::with_capacity(64);
        for z in text.chars() {
            if z.is_whitespace() || z == '-' {
                continue;
            }
            match z.to_digit(16) {
                Some(w) => ziffern.push(w as u8),
                None => {
                    ziffern.fill(0);
                    return Err(Fehler::SchluesselUngueltig(
                        "Erlaubt sind nur die Zeichen 0–9 und a–f.".into(),
                    ));
                }
            }
        }
        if ziffern.len() != 64 {
            let n = ziffern.len();
            ziffern.fill(0);
            return Err(Fehler::SchluesselUngueltig(format!(
                "Der Schlüssel braucht genau 64 Zeichen, eingegeben sind {n}."
            )));
        }
        let mut bytes = [0u8; 32];
        for (i, paar) in ziffern.chunks(2).enumerate() {
            bytes[i] = paar[0] << 4 | paar[1];
        }
        ziffern.fill(0);
        if bytes.iter().all(|&b| b == bytes[0]) {
            bytes.fill(0);
            return Err(Fehler::SchluesselUngueltig(
                "Der Schlüssel besteht aus einem einzigen wiederholten Wert. Bitte einen zufälligen Schlüssel erzeugen.".into(),
            ));
        }
        Ok(Self(bytes))
    }

    /// 64 Hex-Zeichen in Kleinbuchstaben, zum Ablegen im Schlüsselbund.
    pub fn als_text(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl Drop for Schluessel {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            // volatile, damit der Übersetzer das Überschreiben nicht weglässt
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

impl std::fmt::Debug for Schluessel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Schluessel(…)")
    }
}

fn ablage() -> &'static Mutex<HashMap<PathBuf, Schluessel>> {
    static ABLAGE: std::sync::OnceLock<Mutex<HashMap<PathBuf, Schluessel>>> = std::sync::OnceLock::new();
    ABLAGE.get_or_init(Default::default)
}

/// Ordner als Schlüssel der Ablage. Ohne Auflösen von Verknüpfungen, damit
/// ein noch nicht angelegter Ordner denselben Eintrag hat wie später. Bis
/// v0.1.3 wurde der Ordner hier trotzdem aufgelöst: Ein Schlüssel, der vor dem
/// Anlegen des Ordners hinterlegt wurde, galt danach unter einem anderen Pfad,
/// und ein neues Register entstand unverschlüsselt (Finding S9-0a67a1af).
/// Jetzt nur noch rein lexikalisch: `.` und ein `/` am Ende zählen nicht.
fn kennung(ordner: &Path) -> PathBuf {
    ordner.components().collect()
}

/// Hinterlegt den Schlüssel für das Register in `ordner`, bis das Programm
/// endet oder [`vergessen`] ihn entfernt.
pub fn hinterlegen(ordner: &Path, schluessel: Schluessel) {
    ablage().lock().expect("Ablage der Schlüssel").insert(kennung(ordner), schluessel);
}

/// Entfernt den Schlüssel für `ordner` aus dem Arbeitsspeicher.
pub fn vergessen(ordner: &Path) {
    ablage().lock().expect("Ablage der Schlüssel").remove(&kennung(ordner));
}

/// Ist für `ordner` ein Schlüssel hinterlegt?
pub fn hinterlegt(ordner: &Path) -> bool {
    ablage().lock().expect("Ablage der Schlüssel").contains_key(&kennung(ordner))
}

/// Ist für `ordner` genau dieser Schlüssel hinterlegt?
pub fn stimmt_ueberein(ordner: &Path, schluessel: &Schluessel) -> bool {
    ablage().lock().expect("Ablage der Schlüssel").get(&kennung(ordner)) == Some(schluessel)
}

fn schluessel_fuer(ordner: &Path) -> Option<Schluessel> {
    ablage().lock().expect("Ablage der Schlüssel").get(&kennung(ordner)).cloned()
}

/// Zustand einer Datei des Registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Zustand {
    /// Gibt es nicht oder ist leer.
    Fehlt,
    Unverschluesselt,
    Verschluesselt,
}

/// Liest den Kopf der Datei. Ändert nichts.
pub fn zustand(pfad: &Path) -> Ergebnis<Zustand> {
    let mut datei = match std::fs::File::open(pfad) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Zustand::Fehlt),
        Err(e) => return Err(e.into()),
    };
    let mut kopf = [0u8; 16];
    let mut gelesen = 0;
    while gelesen < kopf.len() {
        match datei.read(&mut kopf[gelesen..])? {
            0 => break,
            n => gelesen += n,
        }
    }
    Ok(match gelesen {
        0 => Zustand::Fehlt,
        16 if &kopf == KOPF_KLAR => Zustand::Unverschluesselt,
        _ => Zustand::Verschluesselt,
    })
}

/// Schlüssel als Anweisung für SQLCipher: fertiger Schlüssel, keine Ableitung.
fn anweisung(s: &Schluessel) -> String {
    format!("PRAGMA key = \"x'{}'\";", s.als_text())
}

/// Führt eine Anweisung mit dem Schlüssel aus und überschreibt den Text danach.
fn mit_schluessel(db: &Connection, sql: String) -> rusqlite::Result<()> {
    let ergebnis = db.execute_batch(&sql);
    let mut bytes = sql.into_bytes();
    for b in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    ergebnis
}

/// Muss direkt nach dem Öffnen jeder Datei des Registers laufen, vor jeder
/// anderen Anweisung. Ist die Datei verschlüsselt, wird der hinterlegte
/// Schlüssel gesetzt und geprüft. Eine neue Datei wird verschlüsselt, wenn
/// ein Schlüssel hinterlegt ist. Eine unverschlüsselte Datei öffnet nur ohne
/// hinterlegten Schlüssel, sonst [`Fehler::Unverschluesselt`] (v0.1.4).
pub fn verbinden(db: &Connection, pfad: &Path) -> Ergebnis<()> {
    let ordner = pfad.parent().unwrap_or(Path::new("."));
    match zustand(pfad)? {
        Zustand::Unverschluesselt if hinterlegt(ordner) => {
            let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Err(Fehler::Unverschluesselt(name))
        }
        Zustand::Unverschluesselt => Ok(()),
        Zustand::Fehlt => {
            if let Some(s) = schluessel_fuer(ordner) {
                mit_schluessel(db, anweisung(&s))?;
            }
            Ok(())
        }
        Zustand::Verschluesselt => {
            let s = schluessel_fuer(ordner).ok_or(Fehler::SchluesselFehlt)?;
            mit_schluessel(db, anweisung(&s))?;
            pruefen(db)
        }
    }
}

/// Liest einmal aus der Datei. Ein falscher Schlüssel zeigt sich hier.
fn pruefen(db: &Connection) -> Ergebnis<()> {
    match db.query_row("SELECT count(*) FROM sqlite_master", [], |z| z.get::<_, i64>(0)) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(f, _)) if f.code == rusqlite::ErrorCode::NotADatabase => {
            Err(Fehler::SchluesselFalsch)
        }
        Err(e) => Err(e.into()),
    }
}

/// Passt `schluessel` zum Register in `ordner`? Öffnet die Index-Datenbank
/// nur lesend. Ist sie noch unverschlüsselt oder fehlt sie, lässt sich das
/// nicht prüfen: dann `None`.
pub fn schluessel_pruefen(ordner: &Path, schluessel: &Schluessel) -> Ergebnis<Option<bool>> {
    let pfad = ordner.join(DATEINAME);
    if zustand(&pfad)? != Zustand::Verschluesselt {
        return Ok(None);
    }
    let db = Connection::open_with_flags(&pfad, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    mit_schluessel(&db, anweisung(schluessel))?;
    match pruefen(&db) {
        Ok(()) => Ok(Some(true)),
        Err(Fehler::SchluesselFalsch) => Ok(Some(false)),
        Err(e) => Err(e),
    }
}

/// Lässt sich die Index-Datenbank in `ordner` mit dem hinterlegten Schlüssel
/// lesen? Öffnet nur lesend und liest eine Seite. Ohne Register: `Ok`.
pub fn index_lesbar(ordner: &Path) -> Ergebnis<()> {
    let pfad = ordner.join(DATEINAME);
    if zustand(&pfad)? == Zustand::Fehlt {
        return Ok(());
    }
    let db = Connection::open_with_flags(&pfad, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    verbinden(&db, &pfad)?;
    pruefen(&db)
}

/// Dateien des Registers im Ordner: die Index-Datenbank und alle Scheiben.
/// Aus dem Ordner gelesen, nicht aus dem Register, damit es auch ohne
/// Schlüssel geht.
fn dateien(ordner: &Path) -> Ergebnis<Vec<PathBuf>> {
    let mut scheiben = Vec::new();
    for e in std::fs::read_dir(ordner)? {
        let e = e?;
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        let ist_scheibe = name.len() == "vorschau-0001.db".len()
            && name.starts_with("vorschau-")
            && name.ends_with(".db")
            && name[9..13].bytes().all(|b| b.is_ascii_digit());
        if ist_scheibe && e.file_type()?.is_file() {
            scheiben.push(e.path());
        }
    }
    scheiben.sort();
    // Die Index-Datenbank zuletzt: Ist sie verschlüsselt, ist die Umstellung fertig
    let index = ordner.join(DATEINAME);
    if index.is_file() {
        scheiben.push(index);
    }
    Ok(scheiben)
}

/// Stand der Verschlüsselung eines Registers.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct VerschluesselungStand {
    /// Zustand der Index-Datenbank. Sie wird als Letztes umgestellt.
    pub index: Option<Zustand>,
    pub dateien: u64,
    pub verschluesselt: u64,
    pub unverschluesselt: u64,
    /// Bytes der noch unverschlüsselten Dateien.
    pub unverschluesselt_bytes: u64,
    /// Namen der noch unverschlüsselten Dateien.
    pub unverschluesselt_liste: Vec<String>,
    /// Ist für den Ordner ein Schlüssel hinterlegt?
    pub schluessel_hinterlegt: bool,
    /// Liegt eine Arbeitskopie einer abgebrochenen Umstellung im Ordner?
    pub arbeitskopien: Vec<String>,
    /// Schlüssel hinterlegt, aber mindestens eine Datei unverschlüsselt: Das
    /// Register öffnet nicht, bis die Umstellung fertig ist (v0.1.4).
    pub gesperrt: bool,
}

impl VerschluesselungStand {
    /// Alle Dateien verschlüsselt.
    pub fn vollstaendig(&self) -> bool {
        self.dateien > 0 && self.unverschluesselt == 0
    }
}

/// Liest nur die Köpfe der Dateien. Ändert nichts.
pub fn stand(ordner: &Path) -> Ergebnis<VerschluesselungStand> {
    let mut s = VerschluesselungStand { schluessel_hinterlegt: hinterlegt(ordner), ..Default::default() };
    if !ordner.is_dir() {
        return Ok(s);
    }
    s.index = Some(zustand(&ordner.join(DATEINAME))?);
    for pfad in dateien(ordner)? {
        let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match zustand(&pfad)? {
            Zustand::Fehlt => continue,
            Zustand::Verschluesselt => s.verschluesselt += 1,
            Zustand::Unverschluesselt => {
                s.unverschluesselt += 1;
                s.unverschluesselt_bytes += std::fs::metadata(&pfad)?.len();
                s.unverschluesselt_liste.push(name);
            }
        }
        s.dateien += 1;
    }
    for e in std::fs::read_dir(ordner)? {
        let name = e?.file_name().to_string_lossy().into_owned();
        if name.ends_with(ARBEITSKOPIE) || name.ends_with(&format!("{ARBEITSKOPIE}-journal")) {
            s.arbeitskopien.push(name);
        }
    }
    s.arbeitskopien.sort();
    s.gesperrt = s.schluessel_hinterlegt && s.unverschluesselt > 0;
    Ok(s)
}

/// Ergebnis von [`umstellen`].
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct UmstellErgebnis {
    pub dauer_ms: u64,
    /// In diesem Lauf verschlüsselte Dateien und ihre Größe vorher.
    pub dateien: u64,
    pub bytes: u64,
    /// Angehalten. Der nächste Lauf macht mit der nächsten Datei weiter.
    pub abgebrochen: bool,
}

/// Verschlüsselt alle noch unverschlüsselten Dateien des Registers in
/// `ordner` mit dem hinterlegten Schlüssel, eine nach der anderen. Die
/// Index-Datenbank kommt zuletzt.
///
/// Je Datei: verschlüsselte Arbeitskopie anlegen (`sqlcipher_export`),
/// prüfen (Schlüssel, `quick_check`, Zeilen je Tabelle, Schema-Version), dann
/// das Original in einem Schritt ersetzen. Besteht die Kopie die Prüfung
/// nicht, wird sie verworfen und das Original bleibt unberührt. Keine
/// Sicherungskopie (Grundsatz 6). Angehalten wird nur zwischen zwei Dateien.
///
/// `arbeitsordner`: Ort der Arbeitskopie, etwa der Datenordner der Anwendung.
/// So sieht der Cloud-Client die halbfertige Kopie nicht. Liegt er auf einem
/// anderen Laufwerk, entsteht die Kopie im Register-Ordner.
///
/// Während der Umstellung darf keine andere Verbindung das Register offen
/// halten.
pub fn umstellen(
    ordner: &Path,
    arbeitsordner: Option<&Path>,
    abbrechen: &AtomicBool,
    mut fortschritt: impl FnMut(&Fortschritt),
) -> Ergebnis<UmstellErgebnis> {
    let start = Instant::now();
    let schluessel = schluessel_fuer(ordner).ok_or(Fehler::SchluesselFehlt)?;
    let offen: Vec<PathBuf> = dateien(ordner)?
        .into_iter()
        .filter(|p| matches!(zustand(p), Ok(Zustand::Unverschluesselt)))
        .collect();
    let gesamt = offen.len() as u64;
    let mut e = UmstellErgebnis::default();
    for (i, pfad) in offen.iter().enumerate() {
        if abbrechen.load(Ordering::SeqCst) {
            e.abgebrochen = true;
            break;
        }
        let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        fortschritt(&Fortschritt { erledigt: i as u64, gesamt, datei: name.clone() });
        let bytes = std::fs::metadata(pfad)?.len();
        datei_umstellen(pfad, arbeitsordner, &schluessel)?;
        e.dateien += 1;
        e.bytes += bytes;
    }
    if !e.abgebrochen {
        fortschritt(&Fortschritt { erledigt: gesamt, gesamt, datei: String::new() });
    }
    e.dauer_ms = start.elapsed().as_millis() as u64;
    Ok(e)
}

/// Verschlüsselt eine Datei. Siehe [`umstellen`].
fn datei_umstellen(pfad: &Path, arbeitsordner: Option<&Path>, schluessel: &Schluessel) -> Ergebnis<()> {
    let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ordner = pfad.parent().unwrap_or(Path::new("."));
    let ziel_im_ordner = ordner.join(format!("{name}{ARBEITSKOPIE}"));
    let ziel = match arbeitsordner {
        Some(a) => {
            std::fs::create_dir_all(a)?;
            a.join(format!("{name}{ARBEITSKOPIE}"))
        }
        None => ziel_im_ordner.clone(),
    };
    match kopie_anlegen(pfad, &ziel, schluessel).and_then(|_| ersetzen(&ziel, pfad)) {
        Ok(()) => Ok(()),
        Err(Fehler::EinAusgabe(f)) if ziel != ziel_im_ordner && f.kind() == std::io::ErrorKind::CrossesDevices => {
            // Arbeitsordner auf einem anderen Laufwerk: im Register-Ordner wiederholen
            weg(&ziel);
            kopie_anlegen(pfad, &ziel_im_ordner, schluessel).and_then(|_| ersetzen(&ziel_im_ordner, pfad)).map_err(|f| {
                weg(&ziel_im_ordner);
                f
            })
        }
        Err(f) => {
            weg(&ziel);
            Err(f)
        }
    }
}

/// Entfernt die eigene Arbeitskopie samt Journal. Fehler sind hier egal.
fn weg(kopie: &Path) {
    let _ = std::fs::remove_file(kopie);
    let mut journal = kopie.as_os_str().to_owned();
    journal.push("-journal");
    let _ = std::fs::remove_file(PathBuf::from(journal));
}

/// Schreibt die verschlüsselte Kopie von `quelle` nach `ziel` und prüft sie.
fn kopie_anlegen(quelle: &Path, ziel: &Path, schluessel: &Schluessel) -> Ergebnis<()> {
    // Reste eines abgebrochenen Laufs
    weg(ziel);
    // Lesen, schreiben und anlegen: ATTACH erbt die Rechte der Verbindung und
    // legt damit die Kopie an. Die Quelle gibt es (geprüft vom Aufrufer) und
    // sie wird nicht verändert.
    let db = Connection::open_with_flags(
        quelle,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(std::time::Duration::from_secs(15))?;
    if zustand(quelle)? != Zustand::Unverschluesselt {
        return Err(Fehler::Konfiguration(format!("{} ist nicht unverschlüsselt", quelle.display())));
    }
    let version: i64 = db.query_row("PRAGMA user_version", [], |z| z.get(0))?;
    let zeilen_vorher = zeilen_je_tabelle(&db, "main")?;
    let mut roh = format!("x'{}'", schluessel.als_text());
    let angehaengt = db.execute("ATTACH DATABASE ?1 AS ziel KEY ?2", rusqlite::params![ziel.to_string_lossy(), roh]);
    for b in unsafe { roh.as_bytes_mut() } {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    angehaengt?;
    let ergebnis = (|| -> Ergebnis<()> {
        db.query_row("SELECT sqlcipher_export('ziel')", [], |_| Ok(()))?;
        db.execute_batch(&format!("PRAGMA ziel.user_version = {version};"))?;
        Ok(())
    })();
    db.execute_batch("DETACH DATABASE ziel")?;
    ergebnis?;
    drop(db);

    // Prüfen: mit dem Schlüssel lesbar, unbeschädigt, gleicher Inhalt
    if zustand(ziel)? != Zustand::Verschluesselt {
        return Err(Fehler::Konfiguration("Die Kopie ist nicht verschlüsselt".into()));
    }
    let kopie = Connection::open_with_flags(ziel, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    mit_schluessel(&kopie, anweisung(schluessel))?;
    pruefen(&kopie)?;
    let ok: String = kopie.query_row("PRAGMA quick_check", [], |z| z.get(0))?;
    if ok != "ok" {
        return Err(Fehler::Konfiguration(format!("Prüfung der verschlüsselten Kopie: {ok}")));
    }
    let version_kopie: i64 = kopie.query_row("PRAGMA user_version", [], |z| z.get(0))?;
    let zeilen_nachher = zeilen_je_tabelle(&kopie, "main")?;
    if version_kopie != version || zeilen_nachher != zeilen_vorher {
        return Err(Fehler::Konfiguration(format!(
            "Die verschlüsselte Kopie von {} weicht vom Original ab",
            quelle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        )));
    }
    Ok(())
}

/// Zeilen je Tabelle, für den Vergleich von Original und Kopie. Virtuelle
/// Tabellen zählen über ihre Schattentabellen mit.
fn zeilen_je_tabelle(db: &Connection, schema: &str) -> Ergebnis<Vec<(String, i64)>> {
    let namen: Vec<String> = db
        .prepare(&format!(
            "SELECT name FROM {schema}.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             AND sql NOT LIKE 'CREATE VIRTUAL TABLE%' ORDER BY name"
        ))?
        .query_map([], |z| z.get(0))?
        .collect::<Result<_, _>>()?;
    let mut liste = Vec::with_capacity(namen.len());
    for name in namen {
        let n: i64 = db.query_row(
            &format!("SELECT count(*) FROM {schema}.\"{}\"", name.replace('"', "\"\"")),
            [],
            |z| z.get(0),
        )?;
        liste.push((name, n));
    }
    Ok(liste)
}

/// Ersetzt `original` in einem Schritt durch die geprüfte `kopie`.
fn ersetzen(kopie: &Path, original: &Path) -> Ergebnis<()> {
    std::fs::File::open(kopie)?.sync_all()?;
    std::fs::rename(kopie, original)?;
    if let Some(ordner) = original.parent() {
        // Den Eintrag im Ordner sichern. Unter Windows nicht möglich, dort egal.
        if let Ok(o) = std::fs::File::open(ordner) {
            let _ = o.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "00112233 44556677 8899aabb ccddeeff 00112233 44556677 8899AABB CCDDEEF0";

    fn schluessel() -> Schluessel {
        Schluessel::aus_text(TEXT).unwrap()
    }

    #[test]
    fn schluessel_aus_text() {
        let s = schluessel();
        assert_eq!(s.als_text(), "00112233445566778899aabbccddeeff00112233445566778899aabbccddeef0");
        assert_eq!(Schluessel::aus_text(&s.als_text()).unwrap(), s);
        assert_eq!(Schluessel::aus_text(&TEXT.replace(' ', "-")).unwrap(), s);
        assert!(matches!(Schluessel::aus_text("abc"), Err(Fehler::SchluesselUngueltig(_))));
        assert!(matches!(Schluessel::aus_text(&format!("{}0", s.als_text())), Err(Fehler::SchluesselUngueltig(_))));
        assert!(matches!(Schluessel::aus_text(&"g".repeat(64)), Err(Fehler::SchluesselUngueltig(_))));
        assert!(matches!(Schluessel::aus_text(&"0".repeat(64)), Err(Fehler::SchluesselUngueltig(_))));
        assert_eq!(format!("{s:?}"), "Schluessel(…)");
    }

    /// Legt eine unverschlüsselte SQLite-Datei mit einer Tabelle an.
    fn klar_anlegen(pfad: &Path, text: &str) {
        let db = Connection::open(pfad).unwrap();
        db.execute_batch(&format!("CREATE TABLE t(x); INSERT INTO t VALUES ('{text}'); PRAGMA user_version = 7;")).unwrap();
    }

    /// Umstellung eines Registers mit Index-Datenbank und Scheibe, über einen
    /// Arbeitsordner, mit Anhalten und Prüfen des Schlüssels (v0.1.2).
    #[test]
    fn stellt_ein_register_um() {
        let tmp = tempfile::tempdir().unwrap();
        let ordner = tmp.path().join("register");
        std::fs::create_dir_all(&ordner).unwrap();
        klar_anlegen(&ordner.join(DATEINAME), "Index");
        klar_anlegen(&ordner.join("vorschau-0001.db"), "Scheibe");
        // Kein Teil des Registers
        std::fs::write(ordner.join("regeln.toml"), "x").unwrap();
        std::fs::write(ordner.join("vorschau-x.db"), "x").unwrap();
        let nein = AtomicBool::new(false);

        // Ohne Schlüssel geht nichts
        assert!(matches!(umstellen(&ordner, None, &nein, |_| {}), Err(Fehler::SchluesselFehlt)));
        let s = stand(&ordner).unwrap();
        assert_eq!((s.index, s.dateien, s.unverschluesselt, s.verschluesselt), (Some(Zustand::Unverschluesselt), 2, 2, 0));
        assert_eq!(s.unverschluesselt_liste, ["vorschau-0001.db", DATEINAME]);
        assert!(!s.vollstaendig() && !s.schluessel_hinterlegt && s.unverschluesselt_bytes > 0);
        assert_eq!(schluessel_pruefen(&ordner, &schluessel()).unwrap(), None);

        hinterlegen(&ordner, schluessel());
        assert!(stand(&ordner).unwrap().gesperrt);
        // Mit hinterlegtem Schlüssel öffnet keine unverschlüsselte Datei (v0.1.4)
        let db = Connection::open(ordner.join(DATEINAME)).unwrap();
        assert!(matches!(verbinden(&db, &ordner.join(DATEINAME)), Err(Fehler::Unverschluesselt(n)) if n == DATEINAME));
        drop(db);
        assert!(matches!(index_lesbar(&ordner), Err(Fehler::Unverschluesselt(_))));
        assert!(stimmt_ueberein(&ordner, &schluessel()));
        assert!(!stimmt_ueberein(&ordner, &Schluessel::aus_text(&"1234".repeat(16)).unwrap()));
        // Angehalten vor der ersten Datei
        let e = umstellen(&ordner, None, &AtomicBool::new(true), |_| {}).unwrap();
        assert_eq!((e.abgebrochen, e.dateien), (true, 0));

        let arbeit = tmp.path().join("arbeit");
        let mut meldungen = Vec::new();
        let e = umstellen(&ordner, Some(&arbeit), &nein, |f| meldungen.push((f.erledigt, f.datei.clone()))).unwrap();
        assert_eq!((e.abgebrochen, e.dateien), (false, 2));
        assert_eq!(meldungen, [(0, "vorschau-0001.db".to_string()), (1, DATEINAME.to_string()), (2, String::new())]);
        let s = stand(&ordner).unwrap();
        assert!(s.vollstaendig() && s.schluessel_hinterlegt && s.arbeitskopien.is_empty() && !s.gesperrt);
        assert!(std::fs::read_dir(&arbeit).unwrap().next().is_none(), "Arbeitskopie liegt noch");

        // Inhalt, Schema-Version und Schlüssel
        let db = Connection::open(ordner.join("vorschau-0001.db")).unwrap();
        verbinden(&db, &ordner.join("vorschau-0001.db")).unwrap();
        assert_eq!(db.query_row("SELECT x FROM t", [], |z| z.get::<_, String>(0)).unwrap(), "Scheibe");
        assert_eq!(db.query_row("PRAGMA user_version", [], |z| z.get::<_, i64>(0)).unwrap(), 7);
        assert_eq!(schluessel_pruefen(&ordner, &schluessel()).unwrap(), Some(true));
        assert_eq!(schluessel_pruefen(&ordner, &Schluessel::aus_text(&"1234".repeat(16)).unwrap()).unwrap(), Some(false));
        index_lesbar(&ordner).unwrap();
        // Ein zweiter Lauf hat nichts zu tun
        assert_eq!(umstellen(&ordner, None, &nein, |_| {}).unwrap().dateien, 0);

        // Reste einer abgebrochenen Umstellung werden gemeldet
        std::fs::write(ordner.join(format!("{DATEINAME}{ARBEITSKOPIE}")), "x").unwrap();
        std::fs::write(ordner.join(format!("vorschau-0001.db{ARBEITSKOPIE}-journal")), "x").unwrap();
        assert_eq!(stand(&ordner).unwrap().arbeitskopien.len(), 2);

        vergessen(&ordner);
        assert!(!hinterlegt(&ordner));
        assert!(matches!(index_lesbar(&ordner), Err(Fehler::SchluesselFehlt)));
    }

    /// Schlüssel hinterlegt, bevor der Ordner existiert, über eine
    /// Verknüpfung im Pfad: Das Register entsteht trotzdem verschlüsselt
    /// (S9-0a67a1af, v0.1.4).
    #[cfg(unix)]
    #[test]
    fn schluessel_fuer_noch_nicht_angelegten_ordner() {
        let tmp = tempfile::tempdir().unwrap();
        let echt = tmp.path().join("echt");
        std::fs::create_dir(&echt).unwrap();
        std::os::unix::fs::symlink(&echt, tmp.path().join("verweis")).unwrap();
        let ordner = tmp.path().join("verweis").join("register");
        hinterlegen(&ordner, schluessel());
        std::fs::create_dir(&ordner).unwrap();
        assert!(hinterlegt(&ordner));
        assert!(hinterlegt(&tmp.path().join("verweis/./register/")));
        let pfad = ordner.join(DATEINAME);
        let db = Connection::open(&pfad).unwrap();
        verbinden(&db, &pfad).unwrap();
        db.execute_batch("CREATE TABLE t(x)").unwrap();
        drop(db);
        assert_eq!(zustand(&pfad).unwrap(), Zustand::Verschluesselt);
        vergessen(&ordner);
    }

    #[test]
    fn zustand_und_stand_ohne_register() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(zustand(&tmp.path().join("fehlt.db")).unwrap(), Zustand::Fehlt);
        let leer = tmp.path().join("leer.db");
        std::fs::write(&leer, b"").unwrap();
        assert_eq!(zustand(&leer).unwrap(), Zustand::Fehlt);
        let kurz = tmp.path().join("kurz.db");
        std::fs::write(&kurz, b"SQLite").unwrap();
        assert_eq!(zustand(&kurz).unwrap(), Zustand::Verschluesselt);
        assert!(zustand(tmp.path()).is_err());

        let s = stand(&tmp.path().join("gibt es nicht")).unwrap();
        assert_eq!((s.index, s.dateien), (None, 0));
        let s = stand(tmp.path()).unwrap();
        assert_eq!((s.index, s.dateien), (Some(Zustand::Fehlt), 0));
        assert!(!s.vollstaendig());
        index_lesbar(tmp.path()).unwrap();
    }

    /// Eine Quelle, die schon verschlüsselt ist, wird nicht noch einmal kopiert.
    #[test]
    fn kopie_nur_von_unverschluesselten_dateien() {
        let tmp = tempfile::tempdir().unwrap();
        let quelle = tmp.path().join("q.db");
        std::fs::write(&quelle, [7u8; 64]).unwrap();
        assert!(datei_umstellen(&quelle, None, &schluessel()).is_err());
        // Die Arbeitskopie ist wieder weg, das Original unverändert
        assert!(!tmp.path().join(format!("q.db{ARBEITSKOPIE}")).exists());
        assert_eq!(std::fs::read(&quelle).unwrap(), [7u8; 64]);
    }

    #[test]
    fn neue_datei_mit_und_ohne_schluessel() {
        let tmp = tempfile::tempdir().unwrap();
        let klar = tmp.path().join("klar.db");
        let db = Connection::open(&klar).unwrap();
        verbinden(&db, &klar).unwrap();
        db.execute_batch("CREATE TABLE t(x)").unwrap();
        drop(db);
        assert_eq!(zustand(&klar).unwrap(), Zustand::Unverschluesselt);

        hinterlegen(tmp.path(), schluessel());
        let geheim = tmp.path().join("geheim.db");
        let db = Connection::open(&geheim).unwrap();
        verbinden(&db, &geheim).unwrap();
        db.execute_batch("CREATE TABLE t(x); INSERT INTO t VALUES ('Kontoauszug')").unwrap();
        drop(db);
        assert_eq!(zustand(&geheim).unwrap(), Zustand::Verschluesselt);
        let roh = std::fs::read(&geheim).unwrap();
        assert!(!roh.windows(11).any(|w| w == b"Kontoauszug"));
        // Mit hinterlegtem Schlüssel öffnet die unverschlüsselte Datei nicht
        // mehr, etwa eine ausgetauschte Scheibe neben einem verschlüsselten
        // Index (v0.1.4)
        let db = Connection::open(&klar).unwrap();
        assert!(matches!(verbinden(&db, &klar), Err(Fehler::Unverschluesselt(n)) if n == "klar.db"));
        drop(db);

        vergessen(tmp.path());
        let db = Connection::open(&geheim).unwrap();
        assert!(matches!(verbinden(&db, &geheim), Err(Fehler::SchluesselFehlt)));

        hinterlegen(tmp.path(), Schluessel::aus_text(&"ab".repeat(31).chars().chain("cd".chars()).collect::<String>()).unwrap());
        let db = Connection::open(&geheim).unwrap();
        assert!(matches!(verbinden(&db, &geheim), Err(Fehler::SchluesselFalsch)));
    }
}
