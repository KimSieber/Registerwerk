//! Suche (Konzept, Abschnitt 11; Stufe 2, v0.0.6).
//!
//! Der Suchindex liegt in der Index-Datenbank und ist wie alles im Register
//! nur ein Auszug: [`Register::suche_neu_aufbauen`] erzeugt ihn jederzeit neu.
//!
//! Tabellen (Schema 6), alle FTS5 ohne eigene Kopie des Textes:
//! - `suche_datei`: Name und Ordner jeder Datei, Trigramme. Schlüssel ist die
//!   Datei, so sind auch Dateien ohne Inhalt (andere Typen, "nur online")
//!   über ihren Namen auffindbar.
//! - `suche_kompakt`: gültiger Text je Seite (`seite_text`) ohne Leerraum und
//!   Bindestriche, Trigramme. Findet jeden Teil eines Wortes, also auch Teile
//!   zusammengesetzter Wörter, und Wörter, die die OCR auseinandergerissen hat
//!   ("Rech nung"). Ein eigener Index über den Text mit Leerraum entfiel nach
//!   der Messung im Pilot (v0.0.6): Er fand nichts, was dieser nicht findet,
//!   und hätte den Index verdoppelt. Ob ein Wort zusammenhängend dasteht,
//!   prüft die Suche stattdessen im gespeicherten Text der Trefferseiten.
//! - `suche_wort`: Wörter je Seite und je Datei (Name, Ordner), ohne
//!   Positionen. Grundlage der Wortliste `suche_wortliste` für die unscharfe
//!   Suche bei Tipp- und OCR-Fehlern.
//!
//! Schlüssel einer Seite ist `datei * 2^20 + nr`. Name und Ordner stehen in
//! `suche_wort` unter Seite 0.
//!
//! Aktuell gehalten wird der Index über `datei.suche_stand`: Auslöser im
//! Register setzen ihn zurück, sobald sich Text einer Seite ändert, und
//! entfernen den Index einer Datei, wenn sie aus dem Register verschwindet.
//! [`Register::suche_aktualisieren`] indiziert danach alle Dateien neu, deren
//! Stand nicht [`SUCHE_VERFAHREN`] ist.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rusqlite::types::Value;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::register::Register;
use crate::Ergebnis;

/// Version des Verfahrens, mit dem der Suchindex aufgebaut wird. Erhöhen,
/// wenn sich Tabellen oder Aufbereitung ändern. Dann wird neu indiziert.
pub const SUCHE_VERFAHREN: i64 = 2;
// 1: Trigramme für Name, Ordner, Text und kompakten Text, Wortliste (v0.0.6)
// 2: ohne Index über den Text mit Leerraum (v0.0.6)

/// Schlüssel einer Seite: `datei * SEITEN_FAKTOR + nr`.
pub const SEITEN_FAKTOR: i64 = 1 << 20;
/// Kürzere Suchwörter werden übergangen. Trigramme brauchen drei Zeichen.
pub const MIN_ZEICHEN: usize = 3;
/// Findet ein Suchwort weniger Dokumente, kommen ähnliche Wörter aus der
/// Wortliste dazu. **Annahme**
pub const UNSCHARF_UNTER: usize = 3;
/// Höchstens so viele ähnliche Wörter je Suchwort, die häufigsten zuerst.
pub const UNSCHARF_MAX: usize = 20;
/// Unscharf erst ab dieser Länge. Abstand 1 bis 7 Zeichen, ab 8 Abstand 2.
pub const UNSCHARF_AB: usize = 4;
/// Bis zu so vielen Trefferseiten je Suchwort wird im Text geprüft, ob das
/// Wort zusammenhängend dasteht. Darüber zählen alle Seiten voll. **Annahme**
pub const PRUEFEN_BIS: usize = 500;
/// Höchstzahl der gelieferten Treffer, wenn die Anfrage keine nennt.
pub const MAX_TREFFER: usize = 200;

/// Gewichte je Suchwort. Dateiname zählt am meisten (Konzept, Abschnitt 11).
const G_NAME: f64 = 8.0;
const G_ORDNER: f64 = 3.0;
const G_TEXT: f64 = 1.0;
/// Steht nicht zusammenhängend im Text, nur ohne Leerraum. Auseinandergerissen
/// oder ein Zufallstreffer über Wortgrenzen.
const G_KOMPAKT: f64 = 0.7;
/// Nur über ein ähnliches Wort gefunden.
const G_UNSCHARF: f64 = 0.5;

/// Eine Suchanfrage.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Anfrage {
    pub text: String,
    /// Dokumentdatum von, als `JJJJ`, `JJJJ-MM` oder `JJJJ-MM-TT`.
    pub von: Option<String>,
    /// Dokumentdatum bis, wie `von`.
    pub bis: Option<String>,
    /// Dateiendungen, klein geschrieben. Leer: alle.
    #[serde(default)]
    pub endungen: Vec<String>,
    /// Ordner relativ zum Cloud-Ordner. Nur Dateien darunter.
    pub ordner: Option<String>,
    /// Neueste zuerst statt nach Gewicht.
    #[serde(default)]
    pub nach_datum: bool,
    /// Höchstzahl der Treffer, 0: [`MAX_TREFFER`].
    #[serde(default)]
    pub max: usize,
}

/// Ein Dokument in der Trefferliste.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Treffer {
    pub datei: i64,
    pub pfad: String,
    pub name: String,
    pub endung: String,
    pub datum: Option<String>,
    pub seiten: Option<i64>,
    /// Seiten, auf denen mindestens ein Suchwort steht, aufsteigend.
    pub trefferseiten: Vec<u32>,
    /// Seite mit den meisten Suchwörtern.
    pub beste_seite: Option<u32>,
    pub punkte: f64,
    /// Mindestens ein Suchwort steht im Dateinamen.
    pub im_namen: bool,
    /// Mindestens ein Suchwort wurde nur über ein ähnliches Wort gefunden.
    pub unscharf: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SuchErgebnis {
    pub treffer: Vec<Treffer>,
    /// Alle passenden Dokumente, auch jenseits von `max`.
    pub gesamt: usize,
    /// Verwendete Suchwörter.
    pub woerter: Vec<String>,
    /// Übergangene Suchwörter (kürzer als [`MIN_ZEICHEN`]).
    pub uebergangen: Vec<String>,
    /// Ähnliche Wörter, die je Suchwort dazukamen.
    pub aehnliche: Vec<(String, Vec<String>)>,
    pub dauer_ms: f64,
}

/// Ergebnis eines Aufbaus oder einer Aktualisierung des Index.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SucheAufbau {
    pub dateien: u64,
    pub seiten: u64,
    pub dauer_ms: u64,
    pub abgebrochen: bool,
}

/// Kleinbuchstaben ohne Akzente, wie die Tokenizer von FTS5 (`ä` → `a`).
pub fn falten(s: &str) -> String {
    s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).flat_map(char::to_lowercase).collect()
}

/// Text ohne Leerraum, Bindestriche und weiche Trennzeichen.
pub fn kompakt(s: &str) -> String {
    s.chars().filter(|c| !(c.is_whitespace() || *c == '-' || *c == '\u{ad}')).collect()
}

/// Ein Wort als Zeichenkette für FTS5 (in Anführungszeichen).
fn fts(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn ordner_von(pfad: &str) -> &str {
    pfad.rsplit_once('/').map(|(o, _)| o).unwrap_or("")
}

/// Fügt einen Datumswert mit einer Vorlage zu einem vollständigen Tag auf:
/// `2019` → `2019-01-01` (Anfang) oder `2019-12-31` (Ende). Für Vergleiche als
/// Text genügt `-31` auch in kurzen Monaten.
fn auffuellen(d: &str, vorlage: &str) -> String {
    let d = d.trim();
    if d.len() >= vorlage.len() {
        return d.to_string();
    }
    format!("{d}{}", &vorlage[d.len()..])
}

/// Schema 6: Suchindex. Aufgerufen in `Register::einrichten`.
pub(crate) fn migration_6(db: &Connection) -> Ergebnis<()> {
    let f = SEITEN_FAKTOR;
    db.execute_batch(&format!(
        "-- Verfahren, mit dem die Datei zuletzt indiziert wurde. NULL: ausstehend
         ALTER TABLE datei ADD COLUMN suche_stand INTEGER;
         CREATE VIRTUAL TABLE suche_datei USING fts5(name, ordner,
             content='', contentless_delete=1, tokenize='trigram remove_diacritics 1');
         CREATE VIRTUAL TABLE suche_kompakt USING fts5(text,
             content='', contentless_delete=1, tokenize='trigram remove_diacritics 1');
         CREATE VIRTUAL TABLE suche_wort USING fts5(text,
             content='', contentless_delete=1, detail=none,
             tokenize='unicode61 remove_diacritics 2');
         CREATE VIRTUAL TABLE suche_wortliste USING fts5vocab(suche_wort, row);
         -- Text einer Seite geändert: Datei neu indizieren
         CREATE TRIGGER suche_seite_neu AFTER INSERT ON seite BEGIN
             UPDATE datei SET suche_stand = NULL WHERE id = new.datei AND suche_stand IS NOT NULL;
         END;
         CREATE TRIGGER suche_seite_weg AFTER DELETE ON seite BEGIN
             UPDATE datei SET suche_stand = NULL WHERE id = old.datei AND suche_stand IS NOT NULL;
         END;
         CREATE TRIGGER suche_ocr_neu AFTER INSERT ON ocr BEGIN
             UPDATE datei SET suche_stand = NULL WHERE id = new.datei AND suche_stand IS NOT NULL;
         END;
         CREATE TRIGGER suche_ocr_weg AFTER DELETE ON ocr BEGIN
             UPDATE datei SET suche_stand = NULL WHERE id = old.datei AND suche_stand IS NOT NULL;
         END;
         CREATE TRIGGER suche_ocr_text AFTER UPDATE OF text, uebernommen ON ocr BEGIN
             UPDATE datei SET suche_stand = NULL WHERE id = new.datei AND suche_stand IS NOT NULL;
         END;
         -- Datei entfernt: ihr Index geht mit
         CREATE TRIGGER suche_datei_weg AFTER DELETE ON datei BEGIN
             DELETE FROM suche_datei   WHERE rowid = old.id;
             DELETE FROM suche_kompakt WHERE rowid BETWEEN old.id * {f} AND old.id * {f} + {f} - 1;
             DELETE FROM suche_wort    WHERE rowid BETWEEN old.id * {f} AND old.id * {f} + {f} - 1;
         END;
         PRAGMA user_version = 6;"
    ))?;
    Ok(())
}

/// Schema 7: Index über den Text mit Leerraum entfernt (v0.0.6). Registern
/// aus der ersten Fassung von Schema 6 fehlt dann nur der Index, sie werden
/// mit dem neuen Verfahren neu indiziert.
pub(crate) fn migration_7(db: &Connection) -> Ergebnis<()> {
    let f = SEITEN_FAKTOR;
    db.execute_batch(&format!(
        "DROP TABLE IF EXISTS suche_text;
         DROP TRIGGER IF EXISTS suche_datei_weg;
         CREATE TRIGGER suche_datei_weg AFTER DELETE ON datei BEGIN
             DELETE FROM suche_datei   WHERE rowid = old.id;
             DELETE FROM suche_kompakt WHERE rowid BETWEEN old.id * {f} AND old.id * {f} + {f} - 1;
             DELETE FROM suche_wort    WHERE rowid BETWEEN old.id * {f} AND old.id * {f} + {f} - 1;
         END;
         UPDATE datei SET suche_stand = NULL;
         PRAGMA user_version = 7;"
    ))?;
    Ok(())
}

/// Ein Suchwort in einem Dokument.
#[derive(Debug, Clone, Default)]
struct Fund {
    gewicht: f64,
    seiten: Vec<u32>,
    im_namen: bool,
    unscharf: bool,
}

/// Alle Suchwörter in einem Dokument.
#[derive(Debug, Clone, Default)]
struct Gesamt {
    punkte: f64,
    /// Seite → Zahl der Suchwörter darauf
    seiten: BTreeMap<u32, u32>,
    im_namen: bool,
    unscharf: bool,
}

impl Register {
    /// Indiziert alle Dateien, deren Index fehlt oder veraltet ist. Arbeitet in
    /// Abschnitten von je 500 Dateien, ein Abbruch verliert nichts. Meldet den
    /// Fortschritt nach jedem Abschnitt.
    pub fn suche_aktualisieren(
        &mut self,
        mut fortschritt: impl FnMut(&crate::Fortschritt),
        abbrechen: &AtomicBool,
    ) -> Ergebnis<SucheAufbau> {
        let start = Instant::now();
        let mut e = SucheAufbau::default();
        let offen: Vec<(i64, String, String)> = self
            .db
            .prepare("SELECT id, pfad, name FROM datei WHERE suche_stand IS NOT ?1 ORDER BY id")?
            .query_map([SUCHE_VERFAHREN], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?)))?
            .collect::<Result<_, _>>()?;
        self.wortliste.borrow_mut().take();
        let f = SEITEN_FAKTOR;
        for abschnitt in offen.chunks(500) {
            if abbrechen.load(Ordering::Relaxed) {
                e.abgebrochen = true;
                break;
            }
            let tx = self.db.transaction()?;
            {
                let mut weg_datei = tx.prepare("DELETE FROM suche_datei WHERE rowid = ?1")?;
                let mut weg_kompakt = tx.prepare("DELETE FROM suche_kompakt WHERE rowid BETWEEN ?1 AND ?2")?;
                let mut weg_wort = tx.prepare("DELETE FROM suche_wort WHERE rowid BETWEEN ?1 AND ?2")?;
                let mut neu_datei = tx.prepare("INSERT INTO suche_datei (rowid, name, ordner) VALUES (?1, ?2, ?3)")?;
                let mut neu_kompakt = tx.prepare("INSERT INTO suche_kompakt (rowid, text) VALUES (?1, ?2)")?;
                let mut neu_wort = tx.prepare("INSERT INTO suche_wort (rowid, text) VALUES (?1, ?2)")?;
                let mut seiten = tx.prepare("SELECT nr, text FROM seite_text WHERE datei = ?1 AND text <> ''")?;
                let mut fertig = tx.prepare("UPDATE datei SET suche_stand = ?2 WHERE id = ?1")?;
                for (id, pfad, name) in abschnitt {
                    let (a, b) = (id * f, id * f + f - 1);
                    weg_datei.execute([id])?;
                    weg_kompakt.execute([a, b])?;
                    weg_wort.execute([a, b])?;
                    let ordner = ordner_von(pfad);
                    neu_datei.execute(params![id, name, ordner])?;
                    neu_wort.execute(params![a, format!("{name} {ordner}")])?;
                    let liste: Vec<(i64, String)> =
                        seiten.query_map([id], |z| Ok((z.get(0)?, z.get(1)?)))?.collect::<Result<_, _>>()?;
                    for (nr, text) in liste {
                        let schluessel = a + nr;
                        neu_kompakt.execute(params![schluessel, kompakt(&text)])?;
                        neu_wort.execute(params![schluessel, text])?;
                        e.seiten += 1;
                    }
                    fertig.execute(params![id, SUCHE_VERFAHREN])?;
                    e.dateien += 1;
                }
            }
            tx.commit()?;
            fortschritt(&crate::Fortschritt {
                erledigt: e.dateien,
                gesamt: offen.len() as u64,
                datei: abschnitt.last().map(|d| d.1.clone()).unwrap_or_default(),
            });
        }
        e.dauer_ms = start.elapsed().as_millis() as u64;
        Ok(e)
    }

    /// Leert den Suchindex und baut ihn vollständig neu auf.
    pub fn suche_neu_aufbauen(
        &mut self,
        fortschritt: impl FnMut(&crate::Fortschritt),
        abbrechen: &AtomicBool,
    ) -> Ergebnis<SucheAufbau> {
        self.db.execute_batch(
            "INSERT INTO suche_datei (suche_datei) VALUES ('delete-all');
             INSERT INTO suche_kompakt (suche_kompakt) VALUES ('delete-all');
             INSERT INTO suche_wort (suche_wort) VALUES ('delete-all');
             UPDATE datei SET suche_stand = NULL;",
        )?;
        self.suche_aktualisieren(fortschritt, abbrechen)
    }

    /// Dateien, deren Index fehlt oder veraltet ist.
    pub fn suche_ausstehend(&self) -> Ergebnis<u64> {
        Ok(self
            .db
            .query_row("SELECT count(*) FROM datei WHERE suche_stand IS NOT ?1", [SUCHE_VERFAHREN], |z| {
                z.get::<_, i64>(0)
            })? as u64)
    }

    /// Wortliste für die unscharfe Suche: alle Wörter ab drei Zeichen mit der
    /// Zahl der Seiten, auf denen sie stehen. Wird beim ersten Gebrauch
    /// aufgebaut und bis zur nächsten Aktualisierung behalten.
    pub fn wortliste(&self) -> Ergebnis<Arc<fst::Map<Vec<u8>>>> {
        if let Some(w) = self.wortliste.borrow().as_ref() {
            return Ok(w.clone());
        }
        let mut woerter: Vec<(String, u64)> = self
            .db
            .prepare("SELECT term, doc FROM suche_wortliste WHERE length(term) >= ?1")?
            .query_map([MIN_ZEICHEN as i64], |z| Ok((z.get(0)?, z.get::<_, i64>(1)? as u64)))?
            .collect::<Result<_, _>>()?;
        woerter.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        woerter.dedup_by(|a, b| a.0 == b.0);
        let map = fst::Map::from_iter(woerter.into_iter().map(|(w, n)| (w.into_bytes(), n)))
            .map_err(|e| crate::Fehler::Konfiguration(format!("Wortliste: {e}")))?;
        let map = Arc::new(map);
        *self.wortliste.borrow_mut() = Some(map.clone());
        Ok(map)
    }

    /// Ähnliche Wörter aus der Wortliste, die häufigsten zuerst, ohne das Wort
    /// selbst. `wort` ist gefaltet.
    pub fn aehnliche_woerter(&self, wort: &str) -> Ergebnis<Vec<String>> {
        use fst::{IntoStreamer, Streamer};
        let laenge = wort.chars().count();
        if laenge < UNSCHARF_AB {
            return Ok(Vec::new());
        }
        let liste = self.wortliste()?;
        let abstand = if laenge >= 8 { 2 } else { 1 };
        let automat = match fst::automaton::Levenshtein::new(wort, abstand) {
            Ok(a) => a,
            Err(_) => match fst::automaton::Levenshtein::new(wort, 1) {
                Ok(a) => a,
                Err(_) => return Ok(Vec::new()),
            },
        };
        let mut gefunden: Vec<(String, u64)> = Vec::new();
        let mut strom = liste.search(automat).into_stream();
        while let Some((w, n)) = strom.next() {
            if let Ok(w) = std::str::from_utf8(w) {
                if w != wort {
                    gefunden.push((w.to_string(), n));
                }
            }
        }
        gefunden.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(gefunden.into_iter().take(UNSCHARF_MAX).map(|(w, _)| w).collect())
    }

    /// Ein Suchwort in Name, Ordner, Text und kompaktem Text. Liefert je
    /// Dokument Gewicht und Seiten.
    fn wort_finden(&self, wort: &str) -> Ergebnis<HashMap<i64, Fund>> {
        let mut funde: HashMap<i64, Fund> = HashMap::new();
        let q = fts(wort);
        for (spalte, gewicht) in [("name", G_NAME), ("ordner", G_ORDNER)] {
            let mut abfrage = self.db.prepare_cached("SELECT rowid FROM suche_datei WHERE suche_datei MATCH ?1")?;
            let ids = abfrage.query_map([format!("{spalte} : {q}")], |z| z.get::<_, i64>(0))?;
            for id in ids {
                let fund = funde.entry(id?).or_default();
                if gewicht > fund.gewicht {
                    fund.gewicht = gewicht;
                }
                fund.im_namen |= spalte == "name";
            }
        }
        // Seiten: zusammenhängend im Text 1, sonst 0,7
        let mut seiten: HashMap<i64, f64> = HashMap::new();
        let k = kompakt(wort);
        if k.chars().count() >= MIN_ZEICHEN {
            let mut abfrage = self.db.prepare_cached("SELECT rowid FROM suche_kompakt WHERE suche_kompakt MATCH ?1")?;
            for s in abfrage.query_map([fts(&k)], |z| z.get::<_, i64>(0))? {
                seiten.insert(s?, G_TEXT);
            }
        }
        if seiten.len() <= PRUEFEN_BIS {
            let mut text = self.db.prepare_cached("SELECT text FROM seite_text WHERE datei = ?1 AND nr = ?2")?;
            for (s, g) in seiten.iter_mut() {
                let t: String = text.query_row([s / SEITEN_FAKTOR, s % SEITEN_FAKTOR], |z| z.get(0))?;
                if !falten(&t).contains(wort) {
                    *g = G_KOMPAKT;
                }
            }
        }
        seiten_eintragen(&mut funde, seiten, false);
        Ok(funde)
    }

    /// Dokumente zu ähnlichen Wörtern, nur über ganze Wörter (`suche_wort`).
    fn aehnliche_finden(&self, varianten: &[String]) -> Ergebnis<HashMap<i64, Fund>> {
        let mut funde: HashMap<i64, Fund> = HashMap::new();
        if varianten.is_empty() {
            return Ok(funde);
        }
        let q = varianten.iter().map(|v| fts(v)).collect::<Vec<_>>().join(" OR ");
        let mut seiten: HashMap<i64, f64> = HashMap::new();
        let mut abfrage = self.db.prepare_cached("SELECT rowid FROM suche_wort WHERE suche_wort MATCH ?1")?;
        for s in abfrage.query_map([q], |z| z.get::<_, i64>(0))? {
            let s = s?;
            if s % SEITEN_FAKTOR == 0 {
                let fund = funde.entry(s / SEITEN_FAKTOR).or_default();
                fund.gewicht = fund.gewicht.max(G_ORDNER * G_UNSCHARF);
                fund.unscharf = true;
            } else {
                seiten.insert(s, G_TEXT * G_UNSCHARF);
            }
        }
        seiten_eintragen(&mut funde, seiten, true);
        Ok(funde)
    }

    /// Sucht Dokumente. Jedes Suchwort muss vorkommen, im Namen, im Ordner
    /// oder auf einer Seite. Groß- und Kleinschreibung und Akzente zählen
    /// nicht, Teile von Wörtern werden gefunden.
    pub fn suchen(&self, anfrage: &Anfrage) -> Ergebnis<SuchErgebnis> {
        let start = Instant::now();
        let mut e = SuchErgebnis::default();
        let mut gesamt: Option<HashMap<i64, Gesamt>> = None;
        for roh in anfrage.text.split_whitespace() {
            let wort = falten(roh.trim_matches(|c: char| !c.is_alphanumeric()));
            if wort.chars().count() < MIN_ZEICHEN {
                if !roh.is_empty() {
                    e.uebergangen.push(roh.to_string());
                }
                continue;
            }
            e.woerter.push(wort.clone());
            let mut funde = self.wort_finden(&wort)?;
            if funde.len() < UNSCHARF_UNTER {
                let varianten = self.aehnliche_woerter(&wort)?;
                if !varianten.is_empty() {
                    for (id, f) in self.aehnliche_finden(&varianten)? {
                        funde.entry(id).or_insert(f);
                    }
                    e.aehnliche.push((wort.clone(), varianten));
                }
            }
            gesamt = Some(match gesamt {
                None => funde.into_iter().map(|(id, f)| (id, Gesamt::default().mit(f))).collect(),
                Some(mut bisher) => {
                    bisher.retain(|id, _| funde.contains_key(id));
                    for (id, g) in bisher.iter_mut() {
                        if let Some(f) = funde.remove(id) {
                            *g = std::mem::take(g).mit(f);
                        }
                    }
                    bisher
                }
            });
            if gesamt.as_ref().is_some_and(HashMap::is_empty) {
                break;
            }
        }
        // Filter und Angaben zu den Dateien in einer Abfrage
        let endungen: Vec<Value> = anfrage.endungen.iter().map(|x| Value::Text(x.to_lowercase())).collect();
        let von = anfrage.von.as_deref().filter(|d| !d.trim().is_empty()).map(|d| auffuellen(d, "0000-01-01"));
        let bis = anfrage.bis.as_deref().filter(|d| !d.trim().is_empty()).map(|d| auffuellen(d, "9999-12-31"));
        let ordner = anfrage
            .ordner
            .as_deref()
            .map(|o| o.trim_matches('/'))
            .filter(|o| !o.is_empty())
            .map(|o| format!("{o}/"));
        // Ohne Suchwort, aber mit Filter: alle passenden Dokumente, neueste zuerst
        let filter_gesetzt = von.is_some() || bis.is_some() || !endungen.is_empty() || ordner.is_some();
        let (gesamt, alle) = match gesamt {
            Some(g) => (g, false),
            None if filter_gesetzt && e.woerter.is_empty() => (HashMap::new(), true),
            None => {
                e.dauer_ms = start.elapsed().as_secs_f64() * 1000.0;
                return Ok(e);
            }
        };
        let ids: Vec<Value> = gesamt.keys().map(|&id| Value::Integer(id)).collect();
        let mut abfrage = self.db.prepare_cached(
            "SELECT id, pfad, name, endung, datum_dokument, seiten FROM datei
             WHERE (?7 = 1 OR id IN rarray(?1))
               AND (?2 IS NULL OR datum_dokument || substr('9999-12-31', length(datum_dokument) + 1) >= ?2)
               AND (?3 IS NULL OR datum_dokument || substr('0000-01-01', length(datum_dokument) + 1) <= ?3)
               AND (?4 = 0 OR endung IN rarray(?5))
               AND (?6 IS NULL OR substr(pfad, 1, length(?6)) = ?6)",
        )?;
        let zeilen = abfrage.query_map(
            params![Rc::new(ids), von, bis, !endungen.is_empty(), Rc::new(endungen), ordner, alle],
            |z| {
                Ok(Treffer {
                    datei: z.get(0)?,
                    pfad: z.get(1)?,
                    name: z.get(2)?,
                    endung: z.get(3)?,
                    datum: z.get(4)?,
                    seiten: z.get(5)?,
                    trefferseiten: Vec::new(),
                    beste_seite: None,
                    punkte: 0.0,
                    im_namen: false,
                    unscharf: false,
                })
            },
        )?;
        let mut treffer = Vec::new();
        for t in zeilen {
            let mut t = t?;
            if let Some(g) = gesamt.get(&t.datei) {
                t.punkte = g.punkte;
                t.im_namen = g.im_namen;
                t.unscharf = g.unscharf;
                t.trefferseiten = g.seiten.keys().copied().collect();
                t.beste_seite = g.seiten.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(s, _)| *s);
            }
            treffer.push(t);
        }
        let neuer = |a: &Treffer, b: &Treffer| b.datum.cmp(&a.datum);
        if anfrage.nach_datum || alle {
            treffer.sort_by(|a, b| neuer(a, b).then(b.punkte.total_cmp(&a.punkte)).then(a.pfad.cmp(&b.pfad)));
        } else {
            treffer.sort_by(|a, b| b.punkte.total_cmp(&a.punkte).then(neuer(a, b)).then(a.pfad.cmp(&b.pfad)));
        }
        e.gesamt = treffer.len();
        treffer.truncate(if anfrage.max == 0 { MAX_TREFFER } else { anfrage.max });
        e.treffer = treffer;
        e.dauer_ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok(e)
    }
}

/// Ein Wert mit seiner Anzahl, etwa ein Ordner und die Zahl seiner Dateien.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Zaehler {
    pub wert: String,
    pub anzahl: u64,
}

/// Angaben für die Filter der Suche.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SucheAngaben {
    pub dateien: u64,
    /// Dateien, deren Suchindex fehlt oder veraltet ist.
    pub ausstehend: u64,
    /// Alle Ordner unterhalb des gemeinsamen Anfangs aller Pfade, nach Pfad
    /// sortiert.
    pub ordner: Vec<Zaehler>,
    /// Dateiendungen, die häufigsten zuerst.
    pub endungen: Vec<Zaehler>,
    /// Jahre des Dokumentdatums, die neuesten zuerst.
    pub jahre: Vec<Zaehler>,
}

/// Angaben zu einer Datei für den Detailbereich.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DateiDetails {
    pub id: i64,
    pub pfad: String,
    pub name: String,
    pub endung: String,
    /// Letzte bekannte Größe, `None` bei Dateien, die nur als Platzhalter
    /// gesehen wurden.
    pub groesse: Option<i64>,
    /// Änderungsdatum der Datei, Sekunden seit 1970.
    pub geaendert: i64,
    pub nur_online: bool,
    pub datum: Option<String>,
    /// dateiname, pdf oder geaendert
    pub datum_quelle: Option<String>,
    pub seiten: Option<i64>,
    /// Seiten mit Vorschau samt Maßen.
    pub vorschau: Vec<crate::scheiben::VorschauSeite>,
}

/// Alle Ordner unterhalb des gemeinsamen Anfangs aller Pfade, mit der Zahl
/// der Dateien darunter (einschließlich Unterordnern). Die Oberfläche zeigt sie
/// als aufklappbaren Baum.
fn ordner_zaehlen(pfade: impl Iterator<Item = String>) -> Vec<Zaehler> {
    let ordner: Vec<Vec<String>> = pfade
        .map(|p| ordner_von(&p).split('/').filter(|t| !t.is_empty()).map(str::to_string).collect())
        .collect();
    if ordner.is_empty() {
        return Vec::new();
    }
    // gemeinsamer Anfang: Ebenen, die alle Dateien teilen
    let mut tiefe = 0;
    while let Some(erster) = ordner[0].get(tiefe) {
        if ordner.iter().any(|o| o.get(tiefe) != Some(erster)) {
            break;
        }
        tiefe += 1;
    }
    let mut zaehler: BTreeMap<String, u64> = BTreeMap::new();
    for o in &ordner {
        for ebene in tiefe + 1..=o.len() {
            *zaehler.entry(o[..ebene].join("/")).or_default() += 1;
        }
    }
    zaehler.into_iter().map(|(wert, anzahl)| Zaehler { wert, anzahl }).collect()
}

impl Register {
    /// Angaben für die Filter: Ordner, Dateitypen, Jahre.
    pub fn suche_angaben(&self) -> Ergebnis<SucheAngaben> {
        let pfade: Vec<String> =
            self.db.prepare("SELECT pfad FROM datei")?.query_map([], |z| z.get(0))?.collect::<Result<_, _>>()?;
        let liste = |sql: &str| -> Ergebnis<Vec<Zaehler>> {
            Ok(self
                .db
                .prepare(sql)?
                .query_map([], |z| Ok(Zaehler { wert: z.get(0)?, anzahl: z.get::<_, i64>(1)? as u64 }))?
                .collect::<Result<_, _>>()?)
        };
        Ok(SucheAngaben {
            dateien: pfade.len() as u64,
            ausstehend: self.suche_ausstehend()?,
            ordner: ordner_zaehlen(pfade.into_iter()),
            endungen: liste("SELECT endung, count(*) FROM datei WHERE endung <> '' GROUP BY endung ORDER BY count(*) DESC, endung")?,
            jahre: liste(
                "SELECT substr(datum_dokument, 1, 4), count(*) FROM datei WHERE datum_dokument IS NOT NULL
                 GROUP BY 1 ORDER BY 1 DESC",
            )?,
        })
    }

    /// Angaben zu einer Datei. `None`, wenn es sie nicht mehr gibt.
    pub fn datei_details(&self, id: i64) -> Ergebnis<Option<DateiDetails>> {
        use rusqlite::OptionalExtension;
        let details = self
            .db
            .query_row(
                "SELECT pfad, name, endung, groesse, geaendert, nur_online, datum_dokument, datum_quelle, seiten
                 FROM datei WHERE id = ?1",
                [id],
                |z| {
                    Ok(DateiDetails {
                        id,
                        pfad: z.get(0)?,
                        name: z.get(1)?,
                        endung: z.get(2)?,
                        groesse: z.get(3)?,
                        geaendert: z.get(4)?,
                        nur_online: z.get(5)?,
                        datum: z.get(6)?,
                        datum_quelle: z.get(7)?,
                        seiten: z.get(8)?,
                        vorschau: Vec::new(),
                    })
                },
            )
            .optional()?;
        let Some(mut d) = details else { return Ok(None) };
        d.vorschau = self.datei_vorschau(id)?.map(|v| v.seiten).unwrap_or_default();
        Ok(Some(d))
    }
}

/// Trägt Seiten (Schlüssel → Gewicht) als Funde je Dokument ein. Das Gewicht
/// des Textes ist das beste einer Seite, dazu wenig für jede weitere Seite.
fn seiten_eintragen(funde: &mut HashMap<i64, Fund>, seiten: HashMap<i64, f64>, unscharf: bool) {
    let mut je_datei: HashMap<i64, (f64, Vec<u32>)> = HashMap::new();
    for (s, g) in seiten {
        let eintrag = je_datei.entry(s / SEITEN_FAKTOR).or_default();
        eintrag.0 = eintrag.0.max(g);
        eintrag.1.push((s % SEITEN_FAKTOR) as u32);
    }
    for (id, (g, mut nr)) in je_datei {
        nr.sort_unstable();
        let fund = funde.entry(id).or_default();
        fund.gewicht += g + 0.1 * (nr.len() as f64).ln_1p();
        fund.seiten = nr;
        fund.unscharf |= unscharf;
    }
}

impl Gesamt {
    fn mit(mut self, f: Fund) -> Self {
        self.punkte += f.gewicht;
        self.im_namen |= f.im_namen;
        self.unscharf |= f.unscharf;
        for s in f.seiten {
            *self.seiten.entry(s).or_default() += 1;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dateibaum::{DateiEintrag, Durchlauf};
    use crate::pdf::{Inhalt, Seite};

    fn register(dateien: &[(&str, &[&str])]) -> Register {
        let mut r = Register::im_speicher().unwrap();
        let eintraege = dateien
            .iter()
            .map(|(pfad, _)| {
                let name = pfad.rsplit('/').next().unwrap().to_string();
                let endung = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
                DateiEintrag { pfad: pfad.to_string(), name, endung, groesse: 10, geaendert: 100, stufe: crate::regeln::Stufe::Voll, nur_online: false }
            })
            .collect();
        r.abgleichen(&Durchlauf { dateien: eintraege, gelesene_verzeichnisse: vec!["A".into()], ..Default::default() })
            .unwrap();
        for (pfad, seiten) in dateien {
            if seiten.is_empty() {
                continue;
            }
            let id: i64 = r.db.query_row("SELECT id FROM datei WHERE pfad = ?1", [pfad], |z| z.get(0)).unwrap();
            let inhalt = Inhalt {
                seiten: seiten
                    .iter()
                    .enumerate()
                    .map(|(i, t)| Seite { nr: i as u32 + 1, text: t.to_string(), bildanteil: 1.0 })
                    .collect(),
                erstellt: None,
                grob: false,
            };
            r.inhalt_speichern(id, 100, 10, &Ok(inhalt)).unwrap();
        }
        r.suche_aktualisieren(|_| {}, &AtomicBool::new(false)).unwrap();
        r
    }

    fn suche(r: &Register, text: &str) -> SuchErgebnis {
        r.suchen(&Anfrage { text: text.into(), ..Default::default() }).unwrap()
    }

    fn namen(e: &SuchErgebnis) -> Vec<&str> {
        e.treffer.iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn findet_teilwoerter_ohne_gross_und_akzente() {
        let r = register(&[
            ("A/2025-04-04 Hagebaumarkt Quittung.pdf", &["Bewässerungssystem Gemeinschaftshecke 49,99 EUR"]),
            ("A/2020 Brief.pdf", &["Sehr geehrter Herr", "Die ABRECHNUNG folgt"]),
        ]);
        assert_eq!(namen(&suche(&r, "bewässerung")), ["2025-04-04 Hagebaumarkt Quittung.pdf"]);
        assert_eq!(namen(&suche(&r, "Bewasserung")), ["2025-04-04 Hagebaumarkt Quittung.pdf"]);
        let e = suche(&r, "rechnung");
        assert_eq!(namen(&e), ["2020 Brief.pdf"]);
        assert_eq!((e.treffer[0].trefferseiten.clone(), e.treffer[0].beste_seite), (vec![2], Some(2)));
    }

    #[test]
    fn name_zaehlt_mehr_als_text() {
        let r = register(&[
            ("A/2019 Brief Stadtwerke.pdf", &["Ihre Quittung liegt bei"]),
            ("A/2018 Edeka Quittung.pdf", &["Summe 12,00"]),
        ]);
        let e = suche(&r, "quittung");
        assert_eq!(namen(&e), ["2018 Edeka Quittung.pdf", "2019 Brief Stadtwerke.pdf"]);
        assert!(e.treffer[0].im_namen && !e.treffer[1].im_namen);
        // nach Datum: neueste zuerst
        let e = r.suchen(&Anfrage { text: "quittung".into(), nach_datum: true, ..Default::default() }).unwrap();
        assert_eq!(namen(&e), ["2019 Brief Stadtwerke.pdf", "2018 Edeka Quittung.pdf"]);
    }

    #[test]
    fn alle_woerter_muessen_vorkommen_auch_verteilt() {
        let r = register(&[
            ("A/2019 Edeka.pdf", &["Getränke 5,00", "Summe"]),
            ("A/2019 Aral.pdf", &["Getränke 2,00"]),
        ]);
        // "edeka" im Namen, "getränke" im Text
        assert_eq!(namen(&suche(&r, "Edeka Getränke")), ["2019 Edeka.pdf"]);
        assert!(suche(&r, "Edeka Diesel").treffer.is_empty());
    }

    #[test]
    fn findet_auseinandergerissene_woerter() {
        let r = register(&[
            ("A/a.pdf", &["Ihre Rech nung vom 3. Mai"]),
            ("A/b.pdf", &["Kranken-\nversicherung"]),
            ("A/c.pdf", &["Rechnung"]),
        ]);
        let e = suche(&r, "Rechnung");
        assert_eq!(namen(&e), ["c.pdf", "a.pdf"], "zusammenhängend vor auseinandergerissen");
        assert!(e.treffer[0].punkte > e.treffer[1].punkte);
        assert_eq!(namen(&suche(&r, "Krankenversicherung")), ["b.pdf"]);
    }

    #[test]
    fn unscharf_bei_tippfehlern() {
        let r = register(&[
            ("A/a.pdf", &["Quittung über 12 Euro"]),
            ("A/b.pdf", &["Bescheinigung über die Zahlung"]),
        ]);
        let e = suche(&r, "Quitung");
        assert_eq!(namen(&e), ["a.pdf"]);
        assert!(e.treffer[0].unscharf);
        assert_eq!(e.aehnliche, vec![("quitung".to_string(), vec!["quittung".to_string()])]);
        // Abstand 2 erst ab 8 Zeichen
        assert_eq!(namen(&suche(&r, "Bescheinigunk")), ["b.pdf"]);
        assert!(suche(&r, "Qitng").treffer.is_empty());
    }

    #[test]
    fn kurze_woerter_werden_uebergangen() {
        let r = register(&[("A/a.pdf", &["ab 12 Uhr geöffnet"])]);
        let e = suche(&r, "ab geöffnet");
        assert_eq!(namen(&e), ["a.pdf"]);
        assert_eq!(e.uebergangen, ["ab"]);
        assert!(suche(&r, "ab").treffer.is_empty());
    }

    #[test]
    fn filter_datum_typ_ordner() {
        let r = register(&[
            ("A/2019 Beleg.pdf", &["Beleg"]),
            ("A/2019-04 Beleg.pdf", &["Beleg"]),
            ("A/2020-01-15 Beleg.pdf", &["Beleg"]),
            ("A/X/2021-06-30 Beleg.txt", &[]),
        ]);
        let mit = |von: Option<&str>, bis: Option<&str>, endungen: &[&str], ordner: Option<&str>| {
            let a = Anfrage {
                text: "beleg".into(),
                von: von.map(Into::into),
                bis: bis.map(Into::into),
                endungen: endungen.iter().map(|s| s.to_string()).collect(),
                ordner: ordner.map(Into::into),
                nach_datum: true,
                ..Default::default()
            };
            let e = r.suchen(&a).unwrap();
            e.treffer.iter().map(|t| t.name.clone()).collect::<Vec<_>>()
        };
        assert_eq!(mit(None, None, &[], None).len(), 4);
        // "2019" überschneidet sich mit April 2019, "2019-04" auch
        assert_eq!(mit(Some("2019-04-10"), Some("2019-04-20"), &[], None), ["2019-04 Beleg.pdf", "2019 Beleg.pdf"]);
        assert_eq!(mit(Some("2020"), None, &[], None), ["2021-06-30 Beleg.txt", "2020-01-15 Beleg.pdf"]);
        assert_eq!(mit(None, Some("2019-12"), &[], None), ["2019-04 Beleg.pdf", "2019 Beleg.pdf"]);
        assert_eq!(mit(None, None, &["txt"], None), ["2021-06-30 Beleg.txt"]);
        assert_eq!(mit(None, None, &[], Some("A/X")), ["2021-06-30 Beleg.txt"]);
        assert_eq!(mit(None, None, &["pdf"], Some("A/X")).len(), 0);
    }

    #[test]
    fn index_folgt_aenderungen_und_entfernen() {
        let mut r = register(&[("A/a.pdf", &["Alter Text"]), ("A/b.pdf", &["Bleibt"])]);
        assert_eq!(r.suche_ausstehend().unwrap(), 0);
        let id: i64 = r.db.query_row("SELECT id FROM datei WHERE pfad = 'A/a.pdf'", [], |z| z.get(0)).unwrap();
        let inhalt = Inhalt { seiten: vec![Seite { nr: 1, text: "Neuer Inhalt".into(), bildanteil: 1.0 }], erstellt: None, grob: false };
        r.inhalt_speichern(id, 100, 10, &Ok(inhalt)).unwrap();
        assert_eq!(r.suche_ausstehend().unwrap(), 1);
        r.suche_aktualisieren(|_| {}, &AtomicBool::new(false)).unwrap();
        assert!(suche(&r, "alter").treffer.is_empty());
        assert_eq!(namen(&suche(&r, "neuer")), ["a.pdf"]);
        // die Wortliste kennt das alte Wort nicht mehr
        assert!(!r.aehnliche_woerter("altr").unwrap().contains(&"alter".to_string()));

        // a.pdf verschwindet aus dem Dateibaum
        let b = DateiEintrag { pfad: "A/b.pdf".into(), name: "b.pdf".into(), endung: "pdf".into(), groesse: 10, geaendert: 100, stufe: crate::regeln::Stufe::Voll, nur_online: false };
        r.abgleichen(&Durchlauf { dateien: vec![b], gelesene_verzeichnisse: vec!["A".into()], ..Default::default() })
            .unwrap();
        assert!(suche(&r, "neuer").treffer.is_empty());
        assert!(suche(&r, "a.pdf").treffer.is_empty());
        let reste: i64 = r.db.query_row("SELECT count(*) FROM suche_wortliste WHERE term = 'neuer'", [], |z| z.get(0)).unwrap();
        assert_eq!(reste, 0);
        assert_eq!(namen(&suche(&r, "bleibt")), ["b.pdf"]);
    }

    #[test]
    fn neuaufbau_ergibt_denselben_stand() {
        let mut r = register(&[("A/a.pdf", &["Eins zwei drei"]), ("A/b.txt", &[])]);
        let vorher = suche(&r, "zwei");
        let e = r.suche_neu_aufbauen(|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!((e.dateien, e.seiten), (2, 1));
        assert_eq!(suche(&r, "zwei").treffer, vorher.treffer);
        // Dateien ohne Inhalt sind über den Namen auffindbar
        assert_eq!(namen(&suche(&r, "b.txt")), ["b.txt"]);
    }

    #[test]
    fn stellt_erste_fassung_von_schema_6_um() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let r = Register::oeffnen(tmp.path()).unwrap();
            r.db.execute_batch(
                "CREATE VIRTUAL TABLE suche_text USING fts5(text, content='', contentless_delete=1,
                     tokenize='trigram remove_diacritics 1');
                 UPDATE datei SET suche_stand = 1;
                 ALTER TABLE datei DROP COLUMN stufe;
                 ALTER TABLE datei DROP COLUMN inhalt_hinweis;
                 ALTER TABLE lauf DROP COLUMN geraet;
                 DROP TABLE ordner_gesehen;
                 DROP TABLE geraet;
                 DROP TABLE merkmal;
                 PRAGMA user_version = 6;",
            )
            .unwrap();
        }
        let r = Register::oeffnen(tmp.path()).unwrap();
        let version: i64 = r.db.query_row("PRAGMA user_version", [], |z| z.get(0)).unwrap();
        let tabellen: i64 = r
            .db
            .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'suche_text'", [], |z| z.get(0))
            .unwrap();
        assert_eq!((version, tabellen), (10, 0));
    }

    #[test]
    fn auseinandergerissen_zaehlt_weniger_auch_ueber_wortgrenzen() {
        let r = register(&[("A/a.pdf", &["unsere Stelle"]), ("A/b.pdf", &["Reste vom Essen"])]);
        let e = suche(&r, "reste");
        assert_eq!(namen(&e), ["b.pdf", "a.pdf"]);
        assert!(e.treffer[0].punkte > e.treffer[1].punkte);
    }

    #[test]
    fn nur_filter_ohne_suchwort_zeigt_alle_passenden() {
        let r = register(&[
            ("A/Belege/2019 Edeka.pdf", &["Summe"]),
            ("A/Belege/2021 Aral.pdf", &["Diesel"]),
            ("A/Briefe/2020 Stadt.pdf", &["Bescheid"]),
        ]);
        let a = Anfrage { ordner: Some("A/Belege".into()), ..Default::default() };
        assert_eq!(namen(&r.suchen(&a).unwrap()), ["2021 Aral.pdf", "2019 Edeka.pdf"]);
        // ohne Filter und ohne Suchwort: nichts
        assert!(suche(&r, "").treffer.is_empty());
        // Suchwort ohne Treffer bleibt ohne Treffer, auch mit Filter
        let a = Anfrage { text: "nirgends".into(), ordner: Some("A".into()), ..Default::default() };
        assert!(r.suchen(&a).unwrap().treffer.is_empty());
    }

    #[test]
    fn angaben_und_details() {
        let r = register(&[
            ("Archiv/Alles/2019 Edeka.pdf", &["Summe"]),
            ("Archiv/Alles/Vater/2025/x.pdf", &["Text"]),
            ("Archiv/Belege/2021 Aral.pdf", &["Diesel"]),
            ("Archiv/Belege/y.txt", &[]),
        ]);
        let a = r.suche_angaben().unwrap();
        assert_eq!(a.dateien, 4);
        assert_eq!(a.ausstehend, 0);
        let ordner: Vec<(&str, u64)> = a.ordner.iter().map(|z| (z.wert.as_str(), z.anzahl)).collect();
        assert_eq!(
            ordner,
            [("Archiv/Alles", 2), ("Archiv/Alles/Vater", 1), ("Archiv/Alles/Vater/2025", 1), ("Archiv/Belege", 2)]
        );
        assert_eq!(a.endungen[0], Zaehler { wert: "pdf".into(), anzahl: 3 });
        assert_eq!(a.jahre[0].wert, "2021");

        let id: i64 = r.db.query_row("SELECT id FROM datei WHERE name = '2019 Edeka.pdf'", [], |z| z.get(0)).unwrap();
        let d = r.datei_details(id).unwrap().unwrap();
        assert_eq!((d.pfad.as_str(), d.datum.as_deref(), d.seiten), ("Archiv/Alles/2019 Edeka.pdf", Some("2019"), Some(1)));
        assert!(r.datei_details(9999).unwrap().is_none());
    }

    #[test]
    fn ordner_zaehlen_ueber_dem_gemeinsamen_anfang() {
        let z = ordner_zaehlen(["A/B/C/x", "A/B/D/E/F/y", "A/B/z"].iter().map(|s| s.to_string()));
        let z: Vec<&str> = z.iter().map(|z| z.wert.as_str()).collect();
        assert_eq!(z, ["A/B/C", "A/B/D", "A/B/D/E", "A/B/D/E/F"]);
    }

    #[test]
    fn hilfsfunktionen() {
        assert_eq!(falten("Größe ÄRGER Café"), "große arger cafe");
        assert_eq!(kompakt("Rech nung\nKranken-\nkasse\u{ad}x"), "RechnungKrankenkassex");
        assert_eq!(fts("a\"b"), "\"a\"\"b\"");
        assert_eq!(auffuellen("2019", "0000-01-01"), "2019-01-01");
        assert_eq!(auffuellen("2019-04", "9999-12-31"), "2019-04-31");
        assert_eq!(ordner_von("A/B/c.pdf"), "A/B");
    }
}
