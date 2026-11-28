//! Delta-Lauf (v0.0.8, Stufe 4): zeigen, was ansteht, und es nach Auswahl
//! durch alle Schritte führen.
//!
//! 1. **Prüfen:** Der Dateibaum wird als Probe abgeglichen. In derselben,
//!    danach zurückgerollten Transaktion wird gelesen, bei welchen Dateien ein
//!    Schritt ansteht ([`Delta`]). Das Register bleibt unverändert.
//! 2. **Auswählen:** Die Oberfläche zeigt die Dateien als Ordnerbaum. Alles
//!    ist angehakt, Kim nimmt einzelne Ordner oder Dateien heraus.
//! 3. **Verarbeiten:** neue und geänderte Dateien aufnehmen (ohne zu
//!    entfernen), "nur online"-Dateien der Auswahl laden, dann Inhalte,
//!    Vorschauen, Texterkennung und Textebene, jeweils nur für die Auswahl.
//!
//! Seit v0.1.1 entfernt der Lauf auch, was wegfällt, außer dem Abgewählten
//! (Konzept, Abschnitt 5).

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;

use crate::inhalte::Fortschritt;
use crate::konfiguration::Konfiguration;
use crate::register::{LaufErgebnis, Register, INHALT_VERFAHREN, VORSCHAU_VERFAHREN};
use crate::Ergebnis;

/// Schritte, die bei einer Datei anstehen können, als Bits.
pub const INHALT: u8 = 1;
pub const VORSCHAU: u8 = 2;
pub const OCR: u8 = 4;
pub const TEXTEBENE: u8 = 8;

/// Warum eine Datei im Delta steht.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Art {
    /// Das Register kennt den Pfad noch nicht.
    Neu,
    /// Größe oder Änderungsdatum weichen ab.
    Geaendert,
    /// Unverändert, aber ein Schritt ist noch offen (abgebrochen, nur
    /// online, neues Verfahren).
    Offen,
    /// Fällt aus dem Register oder wird auf den Namen gekürzt (v0.1.1).
    Entfernen,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeltaDatei {
    pub pfad: String,
    pub art: Art,
    pub nur_online: bool,
    /// Anstehende Schritte ([`INHALT`], [`VORSCHAU`], [`OCR`], [`TEXTEBENE`]).
    /// Texterkennung und Textebene einer neuen Datei zeigen sich erst, wenn
    /// ihr Inhalt gelesen ist.
    pub schritte: u8,
    /// Bei [`Art::Entfernen`] der Grund (v0.1.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grund: Option<crate::register::Grund>,
}

/// Ergebnis der Prüfung.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Delta {
    /// Die Probe des Abgleichs: Zahlen, was zu entfernen wäre, vermisste Ordner.
    pub lauf: LaufErgebnis,
    /// Dateien mit anstehendem Schritt, nach Pfad.
    pub dateien: Vec<DeltaDatei>,
    /// Neue Dateien ohne anstehenden Schritt (nur mit Namen, nicht lesbar).
    /// Sie kommen beim Verarbeiten ohne Weiteres ins Register.
    pub nur_namen: u64,
    /// Diese Dateien, nach Pfad, zum Ansehen.
    pub nur_namen_dateien: Vec<DeltaDatei>,
    /// Was „Verarbeiten“ entfernt oder auf den Namen kürzt, nach Pfad
    /// (v0.1.1). Höchstens [`crate::register::BETROFFEN_MAX`], die Zahl steht in
    /// `lauf.entfernt` und `lauf.reduziert`.
    pub entfernen_dateien: Vec<DeltaDatei>,
}

/// Welche Dateien eine Liste zeigt.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    Alle,
    Neu,
    Geaendert,
    Offen,
    Online,
    /// Neue Dateien nur mit Namen
    Namen,
    /// Was entfernt würde (v0.1.1)
    Entfernen,
}

/// Höchstens so viele Dateien liefert [`Delta::liste`].
pub const LISTE_MAX: usize = 2000;

/// Ein Ordner im Baum des Deltas mit den Zahlen samt Unterordnern.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct OrdnerZahl {
    pub pfad: String,
    pub dateien: u64,
    pub neu: u64,
    pub geaendert: u64,
    pub nur_online: u64,
    /// Neue Dateien nur mit Namen darin samt Unterordnern (v0.1.1). Sie
    /// zählen nicht zu `dateien`, weil für sie kein Schritt ansteht.
    pub nur_namen: u64,
    /// Zu entfernende Dateien darin samt Unterordnern (v0.1.1)
    pub entfernen: u64,
}

/// Zahlen des Deltas für die Oberfläche, ohne die einzelnen Dateien.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DeltaUebersicht {
    pub dateien: u64,
    pub neu: u64,
    pub geaendert: u64,
    pub offen: u64,
    pub nur_online: u64,
    /// Dateien je Schritt: Inhalt, Vorschau, OCR, Textebene.
    pub schritte: [u64; 4],
    /// Alle Ordner, die Dateien des Deltas enthalten, nach Pfad.
    pub ordner: Vec<OrdnerZahl>,
}

fn eltern(pfad: &str) -> &str {
    pfad.rsplit_once('/').map(|(vorn, _)| vorn).unwrap_or("")
}

impl Delta {
    pub fn uebersicht(&self) -> DeltaUebersicht {
        let mut u = DeltaUebersicht { dateien: self.dateien.len() as u64, ..Default::default() };
        let mut ordner: BTreeMap<&str, OrdnerZahl> = BTreeMap::new();
        for d in &self.dateien {
            match d.art {
                Art::Neu => u.neu += 1,
                Art::Geaendert => u.geaendert += 1,
                Art::Offen => u.offen += 1,
                Art::Entfernen => {}
            }
            u.nur_online += d.nur_online as u64;
            for (i, bit) in [INHALT, VORSCHAU, OCR, TEXTEBENE].iter().enumerate() {
                u.schritte[i] += (d.schritte & bit != 0) as u64;
            }
            let mut o = eltern(&d.pfad);
            while !o.is_empty() {
                let z = ordner.entry(o).or_insert_with(|| OrdnerZahl { pfad: o.to_string(), ..Default::default() });
                z.dateien += 1;
                z.neu += (d.art == Art::Neu) as u64;
                z.geaendert += (d.art == Art::Geaendert) as u64;
                z.nur_online += d.nur_online as u64;
                o = eltern(o);
            }
        }
        // Neue Dateien nur mit Namen stehen auch im Baum (v0.1.1, Kim: sehen, was ins Register kommt)
        for d in &self.nur_namen_dateien {
            let mut o = eltern(&d.pfad);
            while !o.is_empty() {
                let z = ordner.entry(o).or_insert_with(|| OrdnerZahl { pfad: o.to_string(), ..Default::default() });
                z.nur_namen += 1;
                o = eltern(o);
            }
        }
        for d in &self.entfernen_dateien {
            let mut o = eltern(&d.pfad);
            while !o.is_empty() {
                let z = ordner.entry(o).or_insert_with(|| OrdnerZahl { pfad: o.to_string(), ..Default::default() });
                z.entfernen += 1;
                o = eltern(o);
            }
        }
        u.ordner = ordner.into_values().collect();
        u
    }

    /// Dateien nach `filter`, mit `ordner` nur darin samt Unterordnern.
    /// Höchstens [`LISTE_MAX`], dazu die Gesamtzahl.
    pub fn liste(&self, filter: Filter, ordner: Option<&str>) -> (Vec<DeltaDatei>, u64) {
        let quelle = match filter {
            Filter::Namen => &self.nur_namen_dateien,
            Filter::Entfernen => &self.entfernen_dateien,
            _ => &self.dateien,
        };
        let mut gesamt = 0u64;
        let mut liste = Vec::new();
        for d in quelle.iter().filter(|d| {
            let passt = match filter {
                Filter::Alle | Filter::Namen | Filter::Entfernen => true,
                Filter::Neu => d.art == Art::Neu,
                Filter::Geaendert => d.art == Art::Geaendert,
                Filter::Offen => d.art == Art::Offen,
                Filter::Online => d.nur_online,
            };
            passt && ordner.is_none_or(|o| d.pfad.len() > o.len() && d.pfad.starts_with(o) && d.pfad.as_bytes()[o.len()] == b'/')
        }) {
            gesamt += 1;
            if liste.len() < LISTE_MAX {
                liste.push(d.clone());
            }
        }
        (liste, gesamt)
    }

    /// Die Dateien direkt in `ordner`, ohne Unterordner, nach Pfad. Neue
    /// Dateien nur mit Namen sind dabei, erkennbar an `schritte == 0` (v0.1.1).
    pub fn dateien_in(&self, ordner: &str) -> Vec<DeltaDatei> {
        let mut liste: Vec<DeltaDatei> = self
            .dateien
            .iter()
            .chain(self.nur_namen_dateien.iter())
            .chain(self.entfernen_dateien.iter())
            .filter(|d| eltern(&d.pfad) == ordner)
            .cloned()
            .collect();
        liste.sort_by(|a, b| a.pfad.cmp(&b.pfad));
        liste
    }
}

/// Liegt `pfad` in der Auswahl? Ausgeschlossen sind die genannten Dateien
/// und alles in den genannten Ordnern.
pub fn ausgewaehlt(pfad: &str, ausgeschlossen: &[String]) -> bool {
    !ausgeschlossen.iter().any(|a| {
        pfad == a || (pfad.len() > a.len() && pfad.starts_with(a.as_str()) && pfad.as_bytes()[a.len()] == b'/')
    })
}

/// Je Datei-ID die anstehenden Schritte.
pub(crate) fn anstehend(db: &Connection) -> Ergebnis<BTreeMap<i64, u8>> {
    let mut alle: BTreeMap<i64, u8> = BTreeMap::new();
    let mut sammeln = |sql: String, p: &[&dyn rusqlite::ToSql], bit: u8| -> Ergebnis<()> {
        let mut abfrage = db.prepare(&sql)?;
        for id in abfrage.query_map(p, |z| z.get::<_, i64>(0))? {
            *alle.entry(id?).or_default() |= bit;
        }
        Ok(())
    };
    sammeln(format!("SELECT id FROM datei WHERE {}", Register::AUSSTEHEND), &[&INHALT_VERFAHREN], INHALT)?;
    sammeln(format!("SELECT id FROM datei WHERE {}", crate::scheiben::AUSSTEHEND), &[&VORSCHAU_VERFAHREN], VORSCHAU)?;
    sammeln(
        format!("SELECT d.id FROM datei d WHERE {} AND {}", crate::ocr::bedingung_datei(), crate::ocr::OCR_OFFEN),
        &[&INHALT_VERFAHREN, &crate::ocr::OCR_VERFAHREN],
        OCR,
    )?;
    sammeln(format!("SELECT d.id FROM datei d WHERE {}", crate::textebene::bedingung_datei()), &[], TEXTEBENE)?;
    Ok(alle)
}

/// Je Datei-ID die anstehenden Schritte, nur für Dateien, die der Abgleich
/// auf diesem Gerät gesehen hat (v0.0.9). Was er nicht gesehen hat, liegt in
/// einem vermissten Ordner oder auf einem anderen Gerät und lässt sich hier
/// weder laden noch lesen.
fn anstehend_gesehen(db: &Connection, gesehen: &HashSet<i64>) -> Ergebnis<BTreeMap<i64, u8>> {
    let mut offen = anstehend(db)?;
    offen.retain(|id, _| gesehen.contains(id));
    Ok(offen)
}

/// Liest das Delta innerhalb der Transaktion des Abgleichs. `gesehen` sind
/// die Dateien, die der Durchlauf gefunden hat.
pub(crate) fn delta_lesen(
    db: &Connection,
    gesehen: &HashSet<i64>,
    neu: &[i64],
    geaendert: &[i64],
) -> Ergebnis<(Vec<DeltaDatei>, Vec<DeltaDatei>)> {
    let offen = anstehend_gesehen(db, gesehen)?;
    let neu: HashSet<i64> = neu.iter().copied().collect();
    let geaendert: HashSet<i64> = geaendert.iter().copied().collect();
    let mut lesen = db.prepare("SELECT pfad, nur_online FROM datei WHERE id = ?1")?;
    let mut nur_namen = Vec::new();
    for id in neu.iter().filter(|id| !offen.contains_key(id)) {
        let (pfad, nur_online): (String, bool) = lesen.query_row([id], |z| Ok((z.get(0)?, z.get(1)?)))?;
        nur_namen.push(DeltaDatei { pfad, art: Art::Neu, nur_online, schritte: 0, grund: None });
    }
    nur_namen.sort_by(|a, b| a.pfad.cmp(&b.pfad));
    let mut dateien = Vec::with_capacity(offen.len());
    for (id, schritte) in offen {
        let (pfad, nur_online): (String, bool) = lesen.query_row([id], |z| Ok((z.get(0)?, z.get(1)?)))?;
        let art = if neu.contains(&id) {
            Art::Neu
        } else if geaendert.contains(&id) {
            Art::Geaendert
        } else {
            Art::Offen
        };
        dateien.push(DeltaDatei { pfad, art, nur_online, schritte, grund: None });
    }
    dateien.sort_by(|a, b| a.pfad.cmp(&b.pfad));
    Ok((dateien, nur_namen))
}

/// Ausgewählte Datei-IDs und davon die "nur online" liegenden mit Pfad.
pub type Auswahl = (HashSet<i64>, Vec<(i64, String)>);

impl Register {
    /// Bildet die Auswahl für einen Delta-Lauf: alle Dateien mit anstehendem
    /// Schritt außer den ausgeschlossenen, mit `ohne_online` auch ohne die
    /// "nur online"-Dateien. Dazu die "nur online" liegenden mit Pfad.
    ///
    /// Mit `gesehen` nur Dateien, die der Durchlauf gefunden hat (Pfade im
    /// Register). Dateien in vermissten Ordnern oder auf anderen Geräten
    /// gehören dann nicht dazu (v0.0.9).
    pub fn auswahl_bilden(
        &self,
        ausgeschlossen: &[String],
        ohne_online: bool,
        gesehen: Option<&HashSet<&str>>,
    ) -> Ergebnis<Auswahl> {
        let offen = anstehend(&self.db)?;
        let mut lesen = self.db.prepare("SELECT pfad, nur_online FROM datei WHERE id = ?1")?;
        let mut auswahl = HashSet::new();
        let mut online = Vec::new();
        for id in offen.into_keys() {
            let (pfad, nur_online): (String, bool) = lesen.query_row([id], |z| Ok((z.get(0)?, z.get(1)?)))?;
            let hier = gesehen.is_none_or(|g| g.contains(pfad.as_str()));
            if hier && ausgewaehlt(&pfad, ausgeschlossen) && !(ohne_online && nur_online) {
                auswahl.insert(id);
                if nur_online {
                    online.push((id, pfad));
                }
            }
        }
        online.sort_by(|a, b| a.1.cmp(&b.1));
        Ok((auswahl, online))
    }

    /// Dateien, die Registerwerk auf dem Gerät `geraet` aus der Cloud
    /// angefordert hat und die keinen Schritt mehr offen haben, nach Pfad
    /// (v0.1.2). Unabhängig von der Auswahl des Laufs.
    pub fn angefordert_fertig(&self, geraet: &str) -> Ergebnis<Vec<(i64, String)>> {
        let offen = anstehend(&self.db)?;
        let mut abfrage = self.db.prepare(
            "SELECT d.id, d.pfad FROM angefordert a JOIN datei d ON d.id = a.datei
             WHERE a.geraet = ?1 ORDER BY d.pfad",
        )?;
        let liste = abfrage
            .query_map([geraet], |z| Ok((z.get::<_, i64>(0)?, z.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste.into_iter().filter(|(id, _)| !offen.contains_key(id)).collect())
    }
}

/// Ergebnis des Ladens.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LadenErgebnis {
    pub geladen: u64,
    /// Dateien, die nicht ankamen: die Cloud lädt noch (Warteschlange,
    /// offline), die Datei ist wirklich leer, oder es wurde nicht weiter
    /// gewartet. Sie bleiben offen. Höchstens 50.
    pub nicht_geladen: Vec<String>,
    pub nicht_geladen_anzahl: u64,
    /// Es wurde auf Wunsch nicht weiter gewartet.
    pub uebersprungen: bool,
    pub abgebrochen: bool,
}

fn datei_stand(ort: &std::path::Path) -> Option<(u64, i64)> {
    let m = std::fs::metadata(ort).ok()?;
    if crate::dateibaum::platzhalter(ort, &m) {
        return None;
    }
    let geaendert =
        m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
    Some((m.len(), geaendert))
}

/// So viele Dateien werden gleichzeitig aus der Cloud angefordert.
const ANFORDERN_FAEDEN: usize = 4;

/// Fordert eine Datei beim Cloud-Client an. Unter macOS über die
/// Dateikoordination (`registerwerk-macos`): Nur so lädt die Dropbox (Messung
/// 22.11.2026). Danach wird ein Byte gelesen, was bei anderen Anbietern und
/// Systemen den Download auslösen kann.
fn anfordern(ort: &std::path::Path) {
    let _ = registerwerk_macos::anfordern(ort);
    if let Ok(mut f) = std::fs::File::open(ort) {
        let mut b = [0u8; 1];
        let _ = f.read(&mut b);
    }
}

/// Lädt "nur online"-Dateien aus der Cloud (v0.0.8).
///
/// Die Dateien werden im Hintergrund angefordert (unter macOS über die
/// Dateikoordination), gemeinsam wird gewartet, bis sie eine Größe haben. Größe und
/// Datum jeder angekommenen Datei kommen sofort ins Register. Das Warten
/// endet, wenn alle da sind, wenn `ohne_fortschritt` lang keine neue Datei
/// ankam, mit `weiter` (nicht weiter warten) oder mit `abbrechen`. Was nicht
/// ankam, bleibt offen für den nächsten Lauf.
///
/// Jede angeforderte Datei wird im Register vermerkt (v0.1.2). So stellt
/// [`auszulagern`] sie auch dann wieder auf "nur online", wenn sie erst nach
/// dem Warten ankommt oder der Lauf vor dem Auslagern endet.
///
/// `fortschritt` meldet angekommene von allen Dateien, als Datei die erste,
/// auf die noch gewartet wird.
pub fn laden(
    konfig: &Konfiguration,
    register: &mut Register,
    liste: &[(i64, String)],
    ohne_fortschritt: Duration,
    mut fortschritt: impl FnMut(&Fortschritt),
    weiter: &AtomicBool,
    abbrechen: &AtomicBool,
) -> Ergebnis<LadenErgebnis> {
    let mut e = LadenErgebnis::default();
    let gesamt = liste.len() as u64;
    if liste.is_empty() {
        return Ok(e);
    }
    let ids: Vec<i64> = liste.iter().map(|(id, _)| *id).collect();
    register.angefordert_eintragen(&ids, &konfig.geraet)?;
    let orte: Vec<_> = liste.iter().map(|(_, p)| konfig.ort_oder_nichts(p)).collect();
    fortschritt(&Fortschritt { erledigt: 0, gesamt, datei: liste[0].1.clone() });
    // Anfordern im Hintergrund: Die Dateikoordination wartet je Datei, bis
    // der Cloud-Client sie geliefert hat. Einige Fäden fordern gleichzeitig
    // an, das Warten unten prüft für alle, was angekommen ist. Endet das
    // Warten, nehmen die Fäden keine neue Datei mehr. Eine laufende Anforderung
    // endet von selbst, die Datei kommt dann beim nächsten Lauf ins Register.
    let stopp = std::sync::Arc::new(AtomicBool::new(false));
    let schlange = std::sync::Arc::new(std::sync::Mutex::new(orte.clone().into_iter().collect::<std::collections::VecDeque<_>>()));
    for _ in 0..ANFORDERN_FAEDEN.min(orte.len()) {
        let (stopp, schlange) = (stopp.clone(), schlange.clone());
        std::thread::spawn(move || loop {
            if stopp.load(Ordering::Relaxed) {
                break;
            }
            let Some(ort) = schlange.lock().ok().and_then(|mut s| s.pop_front()) else { break };
            anfordern(&ort);
        });
    }
    let mut offen: Vec<usize> = (0..liste.len()).collect();
    let mut zuletzt_neu = Instant::now();
    loop {
        let mut angekommen = false;
        let mut noch = Vec::with_capacity(offen.len());
        for i in offen {
            match datei_stand(&orte[i]) {
                Some((groesse, geaendert)) => {
                    register.geladen_eintragen(liste[i].0, groesse, geaendert)?;
                    e.geladen += 1;
                    angekommen = true;
                }
                None => noch.push(i),
            }
        }
        offen = noch;
        if angekommen {
            zuletzt_neu = Instant::now();
        }
        let datei = offen.first().map(|&i| liste[i].1.clone()).unwrap_or_default();
        fortschritt(&Fortschritt { erledigt: e.geladen, gesamt, datei });
        if offen.is_empty() {
            break;
        }
        if abbrechen.load(Ordering::Relaxed) {
            e.abgebrochen = true;
            break;
        }
        if weiter.load(Ordering::Relaxed) {
            e.uebersprungen = true;
            break;
        }
        if zuletzt_neu.elapsed() >= ohne_fortschritt {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    stopp.store(true, Ordering::Relaxed);
    for i in offen {
        e.nicht_geladen_anzahl += 1;
        if e.nicht_geladen.len() < 50 {
            e.nicht_geladen.push(liste[i].1.clone());
        }
    }
    Ok(e)
}

/// Ergebnis des Auslagerns.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AuslagernErgebnis {
    pub ausgelagert: u64,
    /// Dateien, die nicht zurückgestellt wurden, höchstens 50.
    pub nicht: Vec<String>,
    pub nicht_anzahl: u64,
    /// `onlineonly` ist nicht installiert, nichts wurde zurückgestellt.
    pub werkzeug_fehlt: bool,
    /// Meldung bei einem grundsätzlichen Fehler, etwa Dropbox nicht erreichbar.
    pub meldung: Option<String>,
}

/// So viele Dateien gehen in einem Aufruf an `onlineonly`.
const AUSLAGERN_JE_AUFRUF: usize = 50;
/// So lange wartet `onlineonly` je Aufruf höchstens auf die Dropbox.
const AUSLAGERN_WARTEN_S: u64 = 300;

/// Sucht das Werkzeug `onlineonly` an den üblichen Orten (Homebrew, eigene
/// Installation, `PATH`). Eine Anwendung aus dem Finder hat Homebrew nicht im
/// `PATH`, deshalb die festen Orte zuerst.
pub fn onlineonly() -> Option<std::path::PathBuf> {
    let mut orte: Vec<std::path::PathBuf> =
        vec!["/opt/homebrew/bin/onlineonly".into(), "/usr/local/bin/onlineonly".into()];
    if let Some(home) = std::env::var_os("HOME") {
        orte.push(std::path::Path::new(&home).join(".local/bin/onlineonly"));
    }
    if let Some(pfad) = std::env::var_os("PATH") {
        orte.extend(std::env::split_paths(&pfad).map(|d| d.join("onlineonly")));
    }
    orte.into_iter().find(|o| o.is_file())
}

/// Liest die Ausgabe von `onlineonly online -w`: je Zeile Zustand und Pfad,
/// etwa `online-only  /Users/…/Beleg.pdf`. Liefert die Pfade, die jetzt nur
/// online liegen.
fn ausgelagerte_pfade(ausgabe: &str) -> HashSet<String> {
    ausgabe
        .lines()
        .filter_map(|z| {
            let (zustand, pfad) = z.trim().split_once(char::is_whitespace)?;
            (zustand == "online-only").then(|| pfad.trim().to_string())
        })
        .collect()
}

/// Was wieder auf "nur online" soll (v0.1.2): alle Dateien, die Registerwerk
/// auf diesem Gerät angefordert hat, die jetzt lokal liegen und keinen Schritt
/// mehr offen haben, auch aus früheren Läufen. Liegt eine schon wieder nur
/// online (etwa von Hand zurückgestellt), fällt ihr Vermerk weg. Fehlt die
/// Datei, bleibt der Vermerk, bis der Abgleich den Eintrag entfernt.
pub fn auszulagern(konfig: &Konfiguration, register: &mut Register) -> Ergebnis<Vec<(i64, String)>> {
    let mut liste = Vec::new();
    for (id, pfad) in register.angefordert_fertig(&konfig.geraet)? {
        let ort = konfig.ort_oder_nichts(&pfad);
        match std::fs::metadata(&ort) {
            Ok(m) if crate::dateibaum::platzhalter(&ort, &m) => register.angefordert_loeschen(id, &konfig.geraet)?,
            Ok(_) => liste.push((id, pfad)),
            Err(_) => {}
        }
    }
    Ok(liste)
}

/// Stellt die Dateien aus `liste` ([`auszulagern`]) wieder auf "nur online"
/// (v0.0.8) und löscht ihren Vermerk "angefordert" (v0.1.2). Nutzt das Werkzeug `onlineonly` (MIT,
/// inoffiziell), das mit der Dropbox-App über die Schnittstelle ihres
/// Finder-Menüs spricht. Nur für die klassische Dropbox unter `~/Dropbox`.
/// Fehlt das Werkzeug, bleiben die Dateien lokal.
pub fn auslagern(
    konfig: &Konfiguration,
    register: &mut Register,
    liste: &[(i64, String)],
    fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<AuslagernErgebnis> {
    auslagern_mit(onlineonly(), konfig, register, liste, fortschritt, abbrechen)
}

/// Wie [`auslagern`], mit diesem Werkzeug. Tests geben ein Ersatzskript vor
/// (v0.1.2), damit die echte Dropbox unberührt bleibt.
fn auslagern_mit(
    werkzeug: Option<std::path::PathBuf>,
    konfig: &Konfiguration,
    register: &mut Register,
    liste: &[(i64, String)],
    mut fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<AuslagernErgebnis> {
    let mut e = AuslagernErgebnis::default();
    if liste.is_empty() {
        return Ok(e);
    }
    let Some(werkzeug) = werkzeug else {
        e.werkzeug_fehlt = true;
        e.nicht_anzahl = liste.len() as u64;
        return Ok(e);
    };
    let gesamt = liste.len() as u64;
    let mut erledigt = 0u64;
    for teil in liste.chunks(AUSLAGERN_JE_AUFRUF) {
        if abbrechen.load(Ordering::Relaxed) {
            break;
        }
        fortschritt(&Fortschritt { erledigt, gesamt, datei: teil[0].1.clone() });
        let orte: Vec<_> = teil.iter().map(|(_, p)| konfig.ort_oder_nichts(p)).collect();
        let lauf = std::process::Command::new(&werkzeug)
            .args(["online", "-w", "-t", &AUSLAGERN_WARTEN_S.to_string()])
            .args(&orte)
            .output();
        let ausgabe = match lauf {
            Ok(a) => {
                if a.status.code() == Some(69) {
                    e.meldung = Some("Die Dropbox-App ist nicht erreichbar.".into());
                }
                String::from_utf8_lossy(&a.stdout).into_owned()
            }
            Err(f) => {
                e.meldung = Some(format!("onlineonly ließ sich nicht starten: {f}"));
                String::new()
            }
        };
        let fertig = ausgelagerte_pfade(&ausgabe);
        for ((id, pfad), ort) in teil.iter().zip(&orte) {
            if fertig.contains(ort.to_string_lossy().as_ref()) {
                register.ausgelagert_eintragen(*id)?;
                register.angefordert_loeschen(*id, &konfig.geraet)?;
                e.ausgelagert += 1;
            } else {
                e.nicht_anzahl += 1;
                if e.nicht.len() < 50 {
                    e.nicht.push(pfad.clone());
                }
            }
        }
        erledigt += teil.len() as u64;
        fortschritt(&Fortschritt { erledigt, gesamt, datei: String::new() });
        if e.meldung.is_some() {
            break;
        }
    }
    // Was wegen Abbruch oder Fehler nicht versucht wurde
    e.nicht_anzahl = gesamt - e.ausgelagert;
    Ok(e)
}

/// Schritt des Delta-Laufs, für die Meldungen an die Oberfläche.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Schritt {
    Aufnehmen,
    Laden,
    Inhalte,
    Vorschauen,
    Erkennen,
    Textebene,
    Auslagern,
    Suche,
}

/// Fortschritt des Delta-Laufs.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Meldung {
    pub schritt: Schritt,
    pub erledigt: u64,
    pub gesamt: u64,
    pub datei: String,
}

/// Ergebnis des Delta-Laufs.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AblaufErgebnis {
    pub dauer_ms: u64,
    pub aufgenommen: LaufErgebnis,
    /// Dateien in der Auswahl.
    pub ausgewaehlt: u64,
    pub laden: LadenErgebnis,
    pub inhalte: crate::InhaltErgebnis,
    pub vorschauen: crate::VorschauErgebnis,
    pub ocr: crate::OcrErgebnis,
    pub textebene: crate::TextebeneErgebnis,
    pub auslagern: AuslagernErgebnis,
    pub abgebrochen: bool,
}

/// Datum einer Datei als Sekunden, für Tests.
#[cfg(test)]
fn sekunden(m: &std::fs::Metadata) -> i64 {
    m.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dateibaum::{DateiEintrag, Durchlauf};
    use crate::regeln::Stufe;

    fn eintrag(pfad: &str, groesse: u64, geaendert: i64) -> DateiEintrag {
        DateiEintrag {
            pfad: pfad.into(),
            name: pfad.rsplit('/').next().unwrap().into(),
            endung: pfad.rsplit('.').next().unwrap().into(),
            groesse,
            geaendert,
            stufe: Stufe::Voll,
            nur_online: groesse == 0,
        }
    }

    fn durchlauf(dateien: Vec<DateiEintrag>) -> Durchlauf {
        Durchlauf { dateien, gelesene_verzeichnisse: vec!["A".into()], ..Default::default() }
    }

    fn zahl(r: &Register, sql: &str) -> i64 {
        r.db.query_row(sql, [], |z| z.get(0)).unwrap()
    }

    #[test]
    fn delta_zeigt_neue_geaenderte_und_offene_dateien_ohne_zu_aendern() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![eintrag("A/alt.pdf", 10, 1), eintrag("A/offen.pdf", 10, 1)])).unwrap();
        // alt.pdf ist fertig gelesen und hat Vorschauen
        let alt: i64 = zahl(&r, "SELECT id FROM datei WHERE pfad = 'A/alt.pdf'");
        r.db.execute(
            "UPDATE datei SET inhalt_geaendert = 1, inhalt_groesse = 10, inhalt_verfahren = 99,
                              vorschau_geaendert = 1, vorschau_groesse = 10, vorschau_verfahren = 99 WHERE id = ?1",
            [alt],
        )
        .unwrap();
        let vorher = zahl(&r, "SELECT count(*) FROM datei");

        let d = r
            .delta_pruefen(&durchlauf(vec![
                eintrag("A/alt.pdf", 12, 2),
                eintrag("A/offen.pdf", 10, 1),
                eintrag("A/B/neu.pdf", 0, 3),
                eintrag("A/B/musik.mp3", 5, 3),
            ]))
            .unwrap();
        let liste: Vec<_> = d.dateien.iter().map(|x| (x.pfad.as_str(), x.art, x.nur_online, x.schritte)).collect();
        assert_eq!(
            liste,
            vec![
                ("A/B/neu.pdf", Art::Neu, true, INHALT | VORSCHAU),
                ("A/alt.pdf", Art::Geaendert, false, INHALT | VORSCHAU),
                ("A/offen.pdf", Art::Offen, false, INHALT | VORSCHAU),
            ]
        );
        assert_eq!(d.nur_namen, 1);
        assert!(d.lauf.probe);
        // nichts geändert
        assert_eq!(zahl(&r, "SELECT count(*) FROM datei"), vorher);

        let u = d.uebersicht();
        assert_eq!((u.dateien, u.neu, u.geaendert, u.offen, u.nur_online), (3, 1, 1, 1, 1));
        assert_eq!(u.schritte, [3, 3, 0, 0]);
        let ordner: Vec<_> =
            u.ordner.iter().map(|o| (o.pfad.as_str(), o.dateien, o.neu, o.nur_online, o.nur_namen)).collect();
        assert_eq!(ordner, vec![("A", 3, 1, 1, 1), ("A/B", 1, 1, 1, 1)]);
        assert_eq!(d.dateien_in("A").len(), 2);
        assert_eq!(d.nur_namen_dateien[0].pfad, "A/B/musik.mp3");
        let pfade = |(l, n): (Vec<DeltaDatei>, u64)| (l.into_iter().map(|x| x.pfad).collect::<Vec<_>>(), n);
        assert_eq!(pfade(d.liste(Filter::Geaendert, None)), (vec!["A/alt.pdf".to_string()], 1));
        assert_eq!(pfade(d.liste(Filter::Online, None)), (vec!["A/B/neu.pdf".to_string()], 1));
        assert_eq!(pfade(d.liste(Filter::Alle, Some("A/B"))), (vec!["A/B/neu.pdf".to_string()], 1));
        assert_eq!(d.liste(Filter::Alle, Some("A/")).1, 0);
        assert_eq!(pfade(d.liste(Filter::Namen, None)), (vec!["A/B/musik.mp3".to_string()], 1));
        // Im Ordner stehen auch die Dateien nur mit Namen, ohne Schritte (v0.1.1)
        let in_b: Vec<_> = d.dateien_in("A/B").into_iter().map(|x| (x.pfad, x.schritte)).collect();
        assert_eq!(in_b, vec![("A/B/musik.mp3".to_string(), 0), ("A/B/neu.pdf".to_string(), INHALT | VORSCHAU)]);
    }

    #[test]
    fn abgewaehltes_bleibt_beim_entfernen() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![
            eintrag("A/x/eins.pdf", 10, 1),
            eintrag("A/x/zwei.pdf", 10, 1),
            eintrag("A/y/drei.pdf", 10, 1),
            eintrag("A/y/bleibt.pdf", 10, 1),
        ]))
        .unwrap();
        // Im Delta steht, was wegfiele, mit Grund und Zahl je Ordner
        let nur_bleibt = || durchlauf(vec![eintrag("A/y/bleibt.pdf", 10, 1), eintrag("A/x/neu.pdf", 10, 2)]);
        let d = r.delta_pruefen(&nur_bleibt()).unwrap();
        let weg: Vec<_> = d.entfernen_dateien.iter().map(|x| (x.pfad.as_str(), x.art)).collect();
        assert_eq!(
            weg,
            vec![("A/x/eins.pdf", Art::Entfernen), ("A/x/zwei.pdf", Art::Entfernen), ("A/y/drei.pdf", Art::Entfernen)]
        );
        // Ohne Regeln im Test ist der Grund „ohne Regel“
        assert!(d.entfernen_dateien.iter().all(|x| x.grund == Some(crate::register::Grund::OhneRegel)));
        let u = d.uebersicht();
        let ordner: Vec<_> = u.ordner.iter().map(|o| (o.pfad.as_str(), o.dateien, o.entfernen)).collect();
        // bleibt.pdf ist noch nicht gelesen und steht deshalb als offen in `dateien`
        assert_eq!(ordner, vec![("A", 2, 3), ("A/x", 1, 2), ("A/y", 1, 1)]);
        assert_eq!(d.liste(Filter::Entfernen, Some("A/x")).1, 2);
        assert!(d.dateien_in("A/y").iter().any(|x| x.art == Art::Entfernen));

        // A/x ist abgewählt: Seine beiden Dateien bleiben, A/y/drei.pdf fällt weg
        let e = r.abgleichen_behalten(&nur_bleibt(), &["A/x".to_string()]).unwrap();
        assert_eq!((e.neu, e.entfernt, e.behalten), (1, 1, 2));
        assert_eq!(zahl(&r, "SELECT count(*) FROM datei WHERE pfad LIKE 'A/x/%'"), 3);
        assert_eq!(zahl(&r, "SELECT count(*) FROM datei WHERE pfad = 'A/y/drei.pdf'"), 0);
        // Bei der nächsten Prüfung stehen die behaltenen wieder an
        let d = r.delta_pruefen(&nur_bleibt()).unwrap();
        assert_eq!(d.entfernen_dateien.len(), 2);
        // Ohne Abwahl wird alles entfernt
        let e = r.abgleichen_behalten(&nur_bleibt(), &[]).unwrap();
        assert_eq!((e.entfernt, e.behalten), (2, 0));
    }

    #[test]
    fn aufnehmen_entfernt_nichts() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![eintrag("A/weg.pdf", 10, 1)])).unwrap();
        let e = r.aufnehmen(&durchlauf(vec![eintrag("A/neu.pdf", 10, 1)])).unwrap();
        assert_eq!((e.neu, e.entfernt), (1, 1));
        assert_eq!(zahl(&r, "SELECT count(*) FROM datei"), 2);
        // Erst der volle Abgleich entfernt
        let e = r.abgleichen(&durchlauf(vec![eintrag("A/neu.pdf", 10, 1)])).unwrap();
        assert_eq!(e.entfernt, 1);
        assert_eq!(zahl(&r, "SELECT count(*) FROM datei"), 1);
    }

    #[test]
    fn auswahl_begrenzt_die_schritte() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![
            eintrag("A/x/eins.pdf", 10, 1),
            eintrag("A/x/zwei.pdf", 0, 1),
            eintrag("A/y/drei.pdf", 10, 1),
        ]))
        .unwrap();
        let (ohne, _) = r.auswahl_bilden(&["A/y".to_string()], true, None).unwrap();
        assert_eq!(ohne.len(), 1);
        let (auswahl, online) = r.auswahl_bilden(&["A/y".to_string()], false, None).unwrap();
        assert_eq!(auswahl.len(), 2);
        assert_eq!(online.iter().map(|o| o.1.as_str()).collect::<Vec<_>>(), vec!["A/x/zwei.pdf"]);
        r.auswahl_setzen(Some(auswahl));
        let pfade: Vec<_> = r.ausstehend().unwrap().into_iter().map(|a| a.pfad).collect();
        assert_eq!(pfade, vec!["A/x/eins.pdf", "A/x/zwei.pdf"]);
        assert_eq!(r.vorschau_ausstehend().unwrap().len(), 2);
        r.auswahl_setzen(None);
        assert_eq!(r.ausstehend().unwrap().len(), 3);
    }

    #[test]
    fn liest_die_ausgabe_von_onlineonly() {
        let a = "online-only  /Users/test/Dropbox/A/x y.pdf\nlocal        /Users/test/Dropbox/A/z.pdf\n\nfehler\n";
        let p = ausgelagerte_pfade(a);
        assert_eq!(p.len(), 1);
        assert!(p.contains("/Users/test/Dropbox/A/x y.pdf"));
    }

    #[test]
    fn auswahl_nach_pfad() {
        let aus = vec!["A/B".to_string(), "A/c.pdf".to_string()];
        assert!(!ausgewaehlt("A/B/x.pdf", &aus));
        assert!(!ausgewaehlt("A/B", &aus));
        assert!(ausgewaehlt("A/BC/x.pdf", &aus));
        assert!(!ausgewaehlt("A/c.pdf", &aus));
        assert!(ausgewaehlt("A/c.pdfx", &aus));
        assert!(ausgewaehlt("A/d.pdf", &[]));
    }

    #[test]
    fn laden_traegt_lokale_dateien_ein_und_meldet_leere() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("A")).unwrap();
        std::fs::write(tmp.path().join("A/da.pdf"), b"%PDF-1.4 inhalt").unwrap();
        crate::dateibaum::platzhalter_anlegen(&tmp.path().join("A/leer.pdf"));
        let mut konfig = Konfiguration::default();
        konfig.aeste.insert("A".into(), tmp.path().join("A"));
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![eintrag("A/da.pdf", 0, 1), eintrag("A/leer.pdf", 0, 1)])).unwrap();
        let (_, online) = r.auswahl_bilden(&[], false, None).unwrap();
        assert_eq!(online.len(), 2);

        let nein = AtomicBool::new(false);
        let mut zuletzt = None;
        let e = laden(&konfig, &mut r, &online, Duration::from_millis(50), |f| zuletzt = Some(f.clone()), &nein, &nein)
            .unwrap();
        assert_eq!((e.geladen, e.nicht_geladen.clone(), e.uebersprungen), (1, vec!["A/leer.pdf".to_string()], false));
        // Gemeldet wird die Datei, auf die noch gewartet wird
        let f = zuletzt.unwrap();
        assert_eq!((f.erledigt, f.gesamt, f.datei.as_str()), (1, 2, "A/leer.pdf"));
        let m = std::fs::metadata(tmp.path().join("A/da.pdf")).unwrap();
        let (groesse, geaendert, nur_online): (i64, i64, bool) = r
            .db
            .query_row("SELECT groesse, geaendert, nur_online FROM datei WHERE pfad = 'A/da.pdf'", [], |z| {
                Ok((z.get(0)?, z.get(1)?, z.get(2)?))
            })
            .unwrap();
        assert_eq!((groesse, geaendert, nur_online), (15, sekunden(&m), false));
        // Die geladene Datei steht jetzt lokal zum Lesen an, die leere weiter nur online
        let offen: Vec<_> = r.ausstehend().unwrap().into_iter().map(|a| (a.pfad, a.nur_online)).collect();
        assert_eq!(offen, vec![("A/da.pdf".to_string(), false), ("A/leer.pdf".to_string(), true)]);
    }

    /// Markiert eine Datei als fertig verarbeitet (Inhalt und Vorschau zum aktuellen Stand).
    fn fertig_setzen(r: &Register, pfad: &str) {
        r.db.execute(
            "UPDATE datei SET inhalt_geaendert = geaendert, inhalt_groesse = groesse, inhalt_verfahren = 99,
                              vorschau_geaendert = geaendert, vorschau_groesse = groesse, vorschau_verfahren = 99
             WHERE pfad = ?1",
            [pfad],
        )
        .unwrap();
    }

    fn vermerke(r: &Register) -> Vec<(String, String)> {
        let mut a = r
            .db
            .prepare("SELECT d.pfad, a.geraet FROM angefordert a JOIN datei d ON d.id = a.datei ORDER BY d.pfad, a.geraet")
            .unwrap();
        a.query_map([], |z| Ok((z.get(0)?, z.get(1)?))).unwrap().map(Result::unwrap).collect()
    }

    fn pfade(liste: Vec<(i64, String)>) -> Vec<String> {
        liste.into_iter().map(|(_, p)| p).collect()
    }

    /// Fehler bis v0.1.1: Welche Dateien geladen wurden, stand nur im Lauf.
    /// Kam eine Datei erst nach dem Warten an oder endete der Lauf vor dem
    /// Auslagern, blieb sie lokal (v0.1.2).
    #[test]
    fn angeforderte_dateien_werden_auch_spaeter_ausgelagert() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("A")).unwrap();
        std::fs::write(tmp.path().join("A/da.pdf"), b"%PDF-1.4 inhalt").unwrap();
        crate::dateibaum::platzhalter_anlegen(&tmp.path().join("A/spaet.pdf"));
        std::fs::write(tmp.path().join("A/lokal.pdf"), b"%PDF-1.4 lokal").unwrap();
        let mut konfig = Konfiguration::default();
        konfig.geraet = "Notebook".into();
        konfig.aeste.insert("A".into(), tmp.path().join("A"));
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![
            eintrag("A/da.pdf", 0, 1),
            eintrag("A/spaet.pdf", 0, 1),
            eintrag("A/lokal.pdf", 14, 1),
        ]))
        .unwrap();
        let (_, online) = r.auswahl_bilden(&[], false, None).unwrap();
        assert_eq!(online.len(), 2);

        // Laden: da.pdf kommt an, spaet.pdf nicht. Beide sind vermerkt, lokal.pdf nicht.
        let nein = AtomicBool::new(false);
        let e = laden(&konfig, &mut r, &online, Duration::from_millis(50), |_| {}, &nein, &nein).unwrap();
        assert_eq!((e.geladen, e.nicht_geladen_anzahl), (1, 1));
        let notebook = |p: &str| (p.to_string(), "Notebook".to_string());
        assert_eq!(vermerke(&r), vec![notebook("A/da.pdf"), notebook("A/spaet.pdf")]);

        // Solange ein Schritt offen ist, bleibt die Datei lokal
        assert!(auszulagern(&konfig, &mut r).unwrap().is_empty());
        fertig_setzen(&r, "A/da.pdf");
        fertig_setzen(&r, "A/lokal.pdf");
        assert_eq!(pfade(auszulagern(&konfig, &mut r).unwrap()), vec!["A/da.pdf"]);

        // Der Lauf endet hier, ohne auszulagern. spaet.pdf kommt danach an.
        // Der nächste Lauf nimmt sie auf, verarbeitet sie und stellt beide zurück.
        std::fs::write(tmp.path().join("A/spaet.pdf"), b"%PDF-1.4 spaet").unwrap();
        let m = std::fs::metadata(tmp.path().join("A/spaet.pdf")).unwrap();
        // da.pdf mit Größe und Datum, die das Laden eingetragen hat: unverändert
        let da = std::fs::metadata(tmp.path().join("A/da.pdf")).unwrap();
        r.abgleichen(&durchlauf(vec![
            eintrag("A/da.pdf", da.len(), sekunden(&da)),
            eintrag("A/spaet.pdf", m.len(), 1),
            eintrag("A/lokal.pdf", 14, 1),
        ]))
        .unwrap();
        assert_eq!(pfade(auszulagern(&konfig, &mut r).unwrap()), vec!["A/da.pdf"]);
        fertig_setzen(&r, "A/spaet.pdf");
        assert_eq!(pfade(auszulagern(&konfig, &mut r).unwrap()), vec!["A/da.pdf", "A/spaet.pdf"]);

        // Ein anderes Gerät stellt nichts zurück: "lokal" gilt je Gerät
        let mut mini = konfig.clone();
        mini.geraet = "Mac mini".into();
        assert!(auszulagern(&mini, &mut r).unwrap().is_empty());

        // Steht eine Datei schon wieder auf "nur online" (von Hand), fällt der Vermerk weg
        crate::dateibaum::platzhalter_anlegen(&tmp.path().join("A/da.pdf"));
        assert_eq!(pfade(auszulagern(&konfig, &mut r).unwrap()), vec!["A/spaet.pdf"]);
        assert_eq!(vermerke(&r), vec![notebook("A/spaet.pdf")]);

        // Nach dem Auslagern ist der Vermerk weg
        let id: i64 = zahl(&r, "SELECT id FROM datei WHERE pfad = 'A/spaet.pdf'");
        r.angefordert_loeschen(id, "Notebook").unwrap();
        assert!(vermerke(&r).is_empty());
    }

    #[test]
    fn vermerk_faellt_mit_dem_eintrag_weg() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![eintrag("A/weg.pdf", 0, 1), eintrag("A/bleibt.pdf", 0, 1)])).unwrap();
        let ids: Vec<i64> = vec![
            zahl(&r, "SELECT id FROM datei WHERE pfad = 'A/weg.pdf'"),
            zahl(&r, "SELECT id FROM datei WHERE pfad = 'A/bleibt.pdf'"),
        ];
        r.angefordert_eintragen(&ids, "Notebook").unwrap();
        // doppelt eintragen schadet nicht
        r.angefordert_eintragen(&ids, "Notebook").unwrap();
        r.angefordert_eintragen(&[], "Notebook").unwrap();
        assert_eq!(zahl(&r, "SELECT count(*) FROM angefordert"), 2);
        // Die Probe ändert nichts
        r.delta_pruefen(&durchlauf(vec![eintrag("A/bleibt.pdf", 0, 1)])).unwrap();
        assert_eq!(zahl(&r, "SELECT count(*) FROM angefordert"), 2);
        r.abgleichen(&durchlauf(vec![eintrag("A/bleibt.pdf", 0, 1)])).unwrap();
        assert_eq!(vermerke(&r), vec![("A/bleibt.pdf".to_string(), "Notebook".to_string())]);
    }

    /// Ein Ersatz für `onlineonly`: meldet jeden Pfad als "nur online", oder
    /// endet mit `code` ohne Ausgabe.
    #[cfg(unix)]
    fn ersatzwerkzeug(ordner: &std::path::Path, code: Option<i32>) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let pfad = ordner.join(format!("onlineonly-{}", code.unwrap_or(0)));
        let rumpf = match code {
            Some(c) => format!("exit {c}"),
            // online -w -t 300, dann die Pfade
            None => "shift 4\nfor p in \"$@\"; do echo \"online-only  $p\"; done".to_string(),
        };
        std::fs::write(&pfad, format!("#!/bin/sh\n{rumpf}\n")).unwrap();
        std::fs::set_permissions(&pfad, std::fs::Permissions::from_mode(0o755)).unwrap();
        pfad
    }

    /// Zurück auf "nur online" mit einem Ersatz für `onlineonly` (v0.1.2).
    #[cfg(unix)]
    #[test]
    fn auslagern_traegt_ein_und_meldet_fehler() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("A")).unwrap();
        for n in ["a.pdf", "b.pdf"] {
            std::fs::write(tmp.path().join("A").join(n), b"%PDF").unwrap();
        }
        let mut konfig = Konfiguration::default();
        konfig.geraet = "Notebook".into();
        konfig.aeste.insert("A".into(), tmp.path().join("A"));
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![eintrag("A/a.pdf", 4, 1), eintrag("A/b.pdf", 4, 1)])).unwrap();
        let liste: Vec<(i64, String)> = ["A/a.pdf", "A/b.pdf"]
            .iter()
            .map(|p| (zahl(&r, &format!("SELECT id FROM datei WHERE pfad = '{p}'")), p.to_string()))
            .collect();
        let ids: Vec<i64> = liste.iter().map(|l| l.0).collect();
        r.angefordert_eintragen(&ids, "Notebook").unwrap();
        let nein = AtomicBool::new(false);

        // Ohne Werkzeug bleibt alles lokal
        let e = auslagern_mit(None, &konfig, &mut r, &liste, |_| {}, &nein).unwrap();
        assert_eq!((e.werkzeug_fehlt, e.ausgelagert, e.nicht_anzahl), (true, 0, 2));
        assert!(auslagern_mit(None, &konfig, &mut r, &[], |_| {}, &nein).unwrap() == AuslagernErgebnis::default());

        // Dropbox nicht erreichbar (Exit 69)
        let e = auslagern_mit(Some(ersatzwerkzeug(tmp.path(), Some(69))), &konfig, &mut r, &liste, |_| {}, &nein).unwrap();
        assert_eq!((e.ausgelagert, e.nicht_anzahl), (0, 2));
        assert!(e.meldung.unwrap().contains("nicht erreichbar"));
        let e = auslagern_mit(Some(tmp.path().join("gibt es nicht")), &konfig, &mut r, &liste, |_| {}, &nein).unwrap();
        assert!(e.meldung.unwrap().contains("ließ sich nicht starten"));

        // Angehalten vor dem ersten Aufruf
        let e = auslagern_mit(Some(ersatzwerkzeug(tmp.path(), None)), &konfig, &mut r, &liste, |_| {}, &AtomicBool::new(true)).unwrap();
        assert_eq!((e.ausgelagert, e.nicht_anzahl), (0, 2));

        // Erfolg: beide nur online, Vermerk weg
        let mut meldungen = 0;
        let e = auslagern_mit(Some(ersatzwerkzeug(tmp.path(), None)), &konfig, &mut r, &liste, |_| meldungen += 1, &nein).unwrap();
        assert_eq!((e.ausgelagert, e.nicht_anzahl, e.meldung), (2, 0, None));
        assert_eq!(meldungen, 2);
        assert_eq!(zahl(&r, "SELECT count(*) FROM datei WHERE nur_online"), 2);
        assert!(vermerke(&r).is_empty());
    }

    #[test]
    fn listen_nach_art_und_grenze() {
        let datei = |pfad: String, art: Art| DeltaDatei { pfad, art, nur_online: false, schritte: INHALT, grund: None };
        let mut dateien: Vec<DeltaDatei> = (0..LISTE_MAX + 5).map(|i| datei(format!("A/neu{i:05}.pdf"), Art::Neu)).collect();
        dateien.push(datei("A/offen.pdf".into(), Art::Offen));
        let d = Delta { dateien, ..Default::default() };
        let (liste, gesamt) = d.liste(Filter::Neu, None);
        assert_eq!((liste.len(), gesamt), (LISTE_MAX, LISTE_MAX as u64 + 5));
        let (liste, gesamt) = d.liste(Filter::Offen, Some("A"));
        assert_eq!((liste[0].pfad.as_str(), gesamt), ("A/offen.pdf", 1));
        assert_eq!(d.liste(Filter::Offen, Some("B")).1, 0);
        assert_eq!(d.uebersicht().offen, 1);
    }

    #[test]
    fn laden_wartet_nicht_weiter_auf_wunsch() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("A")).unwrap();
        crate::dateibaum::platzhalter_anlegen(&tmp.path().join("A/leer.pdf"));
        let mut konfig = Konfiguration::default();
        konfig.aeste.insert("A".into(), tmp.path().join("A"));
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![eintrag("A/leer.pdf", 0, 1)])).unwrap();
        let (_, online) = r.auswahl_bilden(&[], false, None).unwrap();
        let start = Instant::now();
        let e = laden(
            &konfig,
            &mut r,
            &online,
            Duration::from_secs(60),
            |_| {},
            &AtomicBool::new(true),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!((e.geladen, e.nicht_geladen_anzahl, e.uebersprungen), (0, 1, true));
    }
}
