//! Das Register: eine SQLite-Datenbank mit Kopien und Auszügen.
//!
//! Es enthält keine eigene Information und lässt sich jederzeit aus den Dateien
//! neu aufbauen (Grundsatz 2). In dieser Version nimmt es nur den Dateibaum
//! auf. Volltext, Vorschauen und Verschlüsselung folgen.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::dateibaum::Durchlauf;
use crate::{datum, Ergebnis};

pub const DATEINAME: &str = "register.db";
const SCHEMA_VERSION: i64 = 1;

pub struct Register {
    db: Connection,
    pfad: Option<PathBuf>,
}

/// Ergebnis eines Abgleichs zwischen Dateibaum und Register.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LaufErgebnis {
    pub lauf: i64,
    pub dauer_ms: u64,
    /// Dateien, die der Durchlauf gesehen hat.
    pub gesehen: u64,
    pub neu: u64,
    pub geaendert: u64,
    pub unveraendert: u64,
    pub entfernt: u64,
    /// Gesehene Dateien mit 0 Byte ("nur online oder leer").
    pub nur_online: u64,
    pub nicht_lesbar: u64,
    pub fehlende_verzeichnisse: Vec<String>,
    /// Verzeichnisse mit Lesefehlern. Für sie wurde nichts entfernt.
    pub unvollstaendige_verzeichnisse: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TypZeile {
    pub endung: String,
    pub anzahl: u64,
    pub bytes: u64,
    pub nur_online: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JahrZeile {
    pub jahr: String,
    pub anzahl: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LetzterLauf {
    pub lauf: i64,
    /// Sekunden seit 1970 (UTC).
    pub beendet: i64,
    pub neu: u64,
    pub geaendert: u64,
    pub entfernt: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RegisterStand {
    pub dateien: u64,
    /// Summe der bekannten Größen. Für Dateien, die nie lokal gesehen wurden,
    /// ist die Größe unbekannt und zählt als 0.
    pub bytes: u64,
    pub nur_online: u64,
    pub groesse_unbekannt: u64,
    pub mit_datum: u64,
    pub typen: Vec<TypZeile>,
    pub jahre: Vec<JahrZeile>,
    pub letzter_lauf: Option<LetzterLauf>,
}

fn jetzt() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Register {
    /// Öffnet das Register in `ordner` und legt es bei Bedarf an.
    pub fn oeffnen(ordner: &Path) -> Ergebnis<Self> {
        std::fs::create_dir_all(ordner)?;
        let pfad = ordner.join(DATEINAME);
        let db = Connection::open(&pfad)?;
        Self::einrichten(db, Some(pfad))
    }

    /// Register nur im Arbeitsspeicher, für Tests.
    pub fn im_speicher() -> Ergebnis<Self> {
        Self::einrichten(Connection::open_in_memory()?, None)
    }

    pub fn pfad(&self) -> Option<&Path> {
        self.pfad.as_deref()
    }

    fn einrichten(db: Connection, pfad: Option<PathBuf>) -> Ergebnis<Self> {
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |z| z.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(crate::Fehler::Konfiguration(format!(
                "Das Register stammt von einer neueren Programmversion (Schema {version})"
            )));
        }
        if version < 1 {
            db.execute_batch(
                "BEGIN;
                 CREATE TABLE lauf (
                     id            INTEGER PRIMARY KEY,
                     begonnen      INTEGER NOT NULL,
                     beendet       INTEGER,
                     gesehen       INTEGER NOT NULL DEFAULT 0,
                     neu           INTEGER NOT NULL DEFAULT 0,
                     geaendert     INTEGER NOT NULL DEFAULT 0,
                     unveraendert  INTEGER NOT NULL DEFAULT 0,
                     entfernt      INTEGER NOT NULL DEFAULT 0,
                     nicht_lesbar  INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE TABLE datei (
                     id              INTEGER PRIMARY KEY,
                     pfad            TEXT NOT NULL UNIQUE,
                     name            TEXT NOT NULL,
                     endung          TEXT NOT NULL,
                     -- letzte bekannte Größe; NULL, solange die Datei nur als
                     -- Platzhalter gesehen wurde
                     groesse         INTEGER,
                     geaendert       INTEGER NOT NULL,
                     nur_online      INTEGER NOT NULL,
                     datum_dokument  TEXT,
                     datum_quelle    TEXT,
                     erfasst_lauf    INTEGER NOT NULL REFERENCES lauf(id),
                     gesehen_lauf    INTEGER NOT NULL REFERENCES lauf(id)
                 );
                 CREATE INDEX datei_endung ON datei(endung);
                 CREATE INDEX datei_datum ON datei(datum_dokument);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        Ok(Self { db, pfad })
    }

    /// Gleicht das Register mit einem Durchlauf des Dateibaums ab.
    ///
    /// - Neu ist jeder Pfad, den das Register nicht kennt.
    /// - Geändert ist eine Datei, deren Größe oder Änderungsdatum abweicht.
    /// - Entfernt ist ein Pfad, der im Register steht und im Dateibaum fehlt.
    ///   Das gilt nur für Verzeichnisse, die vollständig gelesen wurden.
    ///
    /// Wechselt eine Datei bei gleichem Änderungsdatum zwischen lokal und
    /// "nur online", ist das keine Änderung des Inhalts. Die letzte bekannte
    /// Größe bleibt dann erhalten.
    pub fn abgleichen(&mut self, durchlauf: &Durchlauf) -> Ergebnis<LaufErgebnis> {
        let start = Instant::now();
        let tx = self.db.transaction()?;
        tx.execute("INSERT INTO lauf (begonnen) VALUES (?1)", [jetzt()])?;
        let lauf = tx.last_insert_rowid();

        let mut e = LaufErgebnis {
            lauf,
            gesehen: durchlauf.dateien.len() as u64,
            nicht_lesbar: durchlauf.nicht_lesbar.len() as u64,
            fehlende_verzeichnisse: durchlauf.fehlende_verzeichnisse.clone(),
            ..Default::default()
        };

        {
            let mut suchen = tx.prepare("SELECT id, groesse, geaendert FROM datei WHERE pfad = ?1")?;
            let mut einfuegen = tx.prepare(
                "INSERT INTO datei (pfad, name, endung, groesse, geaendert, nur_online,
                                    datum_dokument, datum_quelle, erfasst_lauf, gesehen_lauf)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            )?;
            let mut aktualisieren = tx.prepare(
                "UPDATE datei SET groesse = ?2, geaendert = ?3, nur_online = ?4, gesehen_lauf = ?5
                 WHERE id = ?1",
            )?;

            for d in &durchlauf.dateien {
                let leer = d.nur_online_oder_leer();
                if leer {
                    e.nur_online += 1;
                }
                let groesse_neu = (!leer).then_some(d.groesse as i64);
                let vorhanden: Option<(i64, Option<i64>, i64)> = suchen
                    .query_row([&d.pfad], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?)))
                    .optional()?;
                match vorhanden {
                    None => {
                        let datum = datum::aus_dateiname(&d.name);
                        let quelle = datum.as_ref().map(|_| "dateiname");
                        einfuegen.execute(params![
                            d.pfad, d.name, d.endung, groesse_neu, d.geaendert, leer, datum, quelle, lauf
                        ])?;
                        e.neu += 1;
                    }
                    Some((id, groesse_alt, geaendert_alt)) => {
                        let gleiches_datum = geaendert_alt == d.geaendert;
                        let gleiche_groesse = groesse_alt == groesse_neu;
                        // Bei gleichem Datum ist ein Wechsel von oder zu 0 Byte
                        // nur ein Wechsel zwischen lokal und "nur online".
                        let nur_statuswechsel = gleiches_datum && (leer || groesse_alt.is_none());
                        if gleiches_datum && (gleiche_groesse || nur_statuswechsel) {
                            e.unveraendert += 1;
                        } else {
                            e.geaendert += 1;
                        }
                        let groesse = if gleiches_datum { groesse_neu.or(groesse_alt) } else { groesse_neu };
                        aktualisieren.execute(params![id, groesse, d.geaendert, leer, lauf])?;
                    }
                }
            }

            let mut entfernen = tx.prepare(
                "DELETE FROM datei
                 WHERE gesehen_lauf <> ?1 AND substr(pfad, 1, length(?2)) = ?2",
            )?;
            for verzeichnis in &durchlauf.gelesene_verzeichnisse {
                e.entfernt += entfernen.execute(params![lauf, format!("{verzeichnis}/")])? as u64;
            }
        }

        e.unvollstaendige_verzeichnisse = durchlauf.unvollstaendige_verzeichnisse.clone();
        tx.execute(
            "UPDATE lauf SET beendet = ?2, gesehen = ?3, neu = ?4, geaendert = ?5,
                             unveraendert = ?6, entfernt = ?7, nicht_lesbar = ?8
             WHERE id = ?1",
            params![
                lauf, jetzt(), e.gesehen as i64, e.neu as i64, e.geaendert as i64,
                e.unveraendert as i64, e.entfernt as i64, e.nicht_lesbar as i64
            ],
        )?;
        tx.commit()?;
        e.dauer_ms = start.elapsed().as_millis() as u64;
        Ok(e)
    }

    pub fn stand(&self) -> Ergebnis<RegisterStand> {
        let (dateien, bytes, nur_online, groesse_unbekannt, mit_datum) = self.db.query_row(
            "SELECT count(*), coalesce(sum(groesse), 0), coalesce(sum(nur_online), 0),
                    coalesce(sum(groesse IS NULL), 0), count(datum_dokument)
             FROM datei",
            [],
            |z| {
                Ok((
                    z.get::<_, i64>(0)? as u64,
                    z.get::<_, i64>(1)? as u64,
                    z.get::<_, i64>(2)? as u64,
                    z.get::<_, i64>(3)? as u64,
                    z.get::<_, i64>(4)? as u64,
                ))
            },
        )?;

        let typen = self
            .db
            .prepare(
                "SELECT endung, count(*), coalesce(sum(groesse), 0), coalesce(sum(nur_online), 0)
                 FROM datei GROUP BY endung ORDER BY count(*) DESC, endung",
            )?
            .query_map([], |z| {
                Ok(TypZeile {
                    endung: z.get(0)?,
                    anzahl: z.get::<_, i64>(1)? as u64,
                    bytes: z.get::<_, i64>(2)? as u64,
                    nur_online: z.get::<_, i64>(3)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let jahre = self
            .db
            .prepare(
                "SELECT substr(datum_dokument, 1, 4), count(*) FROM datei
                 WHERE datum_dokument IS NOT NULL GROUP BY 1 ORDER BY 1",
            )?
            .query_map([], |z| {
                Ok(JahrZeile { jahr: z.get(0)?, anzahl: z.get::<_, i64>(1)? as u64 })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let letzter_lauf = self
            .db
            .query_row(
                "SELECT id, beendet, neu, geaendert, entfernt FROM lauf
                 WHERE beendet IS NOT NULL ORDER BY id DESC LIMIT 1",
                [],
                |z| {
                    Ok(LetzterLauf {
                        lauf: z.get(0)?,
                        beendet: z.get(1)?,
                        neu: z.get::<_, i64>(2)? as u64,
                        geaendert: z.get::<_, i64>(3)? as u64,
                        entfernt: z.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()?;

        Ok(RegisterStand { dateien, bytes, nur_online, groesse_unbekannt, mit_datum, typen, jahre, letzter_lauf })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dateibaum::DateiEintrag;

    fn datei(pfad: &str, groesse: u64, geaendert: i64) -> DateiEintrag {
        let name = pfad.rsplit('/').next().unwrap().to_string();
        let endung = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        DateiEintrag { pfad: pfad.into(), name, endung, groesse, geaendert }
    }

    fn durchlauf(dateien: Vec<DateiEintrag>) -> Durchlauf {
        Durchlauf { dateien, gelesene_verzeichnisse: vec!["A".into()], ..Default::default() }
    }

    #[test]
    fn erkennt_neu_geaendert_entfernt() {
        let mut r = Register::im_speicher().unwrap();
        let e = r
            .abgleichen(&durchlauf(vec![
                datei("A/2020-01-02 a.pdf", 10, 100),
                datei("A/b.pdf", 20, 100),
                datei("A/c.txt", 30, 100),
            ]))
            .unwrap();
        assert_eq!((e.neu, e.geaendert, e.unveraendert, e.entfernt), (3, 0, 0, 0));

        let e = r
            .abgleichen(&durchlauf(vec![
                datei("A/2020-01-02 a.pdf", 10, 100), // unverändert
                datei("A/b.pdf", 25, 200),            // geändert
                datei("A/d.pdf", 5, 100),             // neu, c.txt fehlt
            ]))
            .unwrap();
        assert_eq!((e.neu, e.geaendert, e.unveraendert, e.entfernt), (1, 1, 1, 1));

        let s = r.stand().unwrap();
        assert_eq!((s.dateien, s.bytes, s.mit_datum), (3, 40, 1));
        assert_eq!(s.jahre, vec![JahrZeile { jahr: "2020".into(), anzahl: 1 }]);
        assert_eq!(s.typen[0].endung, "pdf");
        assert_eq!(s.letzter_lauf.unwrap().lauf, 2);
    }

    #[test]
    fn wechsel_auf_nur_online_ist_keine_aenderung() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![datei("A/a.pdf", 500, 100), datei("A/p.pdf", 0, 100)])).unwrap();
        let s = r.stand().unwrap();
        assert_eq!((s.nur_online, s.groesse_unbekannt, s.bytes), (1, 1, 500));

        // a.pdf wird Platzhalter, p.pdf wird geladen: beides bei gleichem Datum
        let e = r
            .abgleichen(&durchlauf(vec![datei("A/a.pdf", 0, 100), datei("A/p.pdf", 700, 100)]))
            .unwrap();
        assert_eq!((e.geaendert, e.unveraendert, e.nur_online), (0, 2, 1));
        let s = r.stand().unwrap();
        // die bekannte Größe von a.pdf bleibt erhalten
        assert_eq!((s.nur_online, s.groesse_unbekannt, s.bytes), (1, 0, 1200));

        // Platzhalter mit neuem Datum: Inhalt hat sich geändert, Größe unbekannt
        let e = r
            .abgleichen(&durchlauf(vec![datei("A/a.pdf", 0, 300), datei("A/p.pdf", 700, 100)]))
            .unwrap();
        assert_eq!((e.geaendert, e.unveraendert), (1, 1));
        assert_eq!(r.stand().unwrap().groesse_unbekannt, 1);
    }

    #[test]
    fn entfernt_nur_in_vollstaendig_gelesenen_verzeichnissen() {
        let mut r = Register::im_speicher().unwrap();
        let mut d = durchlauf(vec![datei("A/a.pdf", 1, 1), datei("AB/x.pdf", 1, 1), datei("B/b.pdf", 1, 1)]);
        d.gelesene_verzeichnisse = vec!["A".into(), "AB".into(), "B".into()];
        r.abgleichen(&d).unwrap();

        // B fehlt auf diesem Gerät, A ist leer gelesen. "AB" darf nicht als Teil von "A" gelten.
        let mut d = durchlauf(vec![datei("AB/x.pdf", 1, 1)]);
        d.gelesene_verzeichnisse = vec!["A".into(), "AB".into()];
        d.fehlende_verzeichnisse = vec!["B".into()];
        let e = r.abgleichen(&d).unwrap();
        assert_eq!(e.entfernt, 1);
        assert_eq!(r.stand().unwrap().dateien, 2);
    }

    #[test]
    fn register_ueberlebt_schliessen_und_oeffnen() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(vec![datei("A/a.pdf", 1, 1)])).unwrap();
        }
        let r = Register::oeffnen(tmp.path()).unwrap();
        assert_eq!(r.stand().unwrap().dateien, 1);
        assert!(r.pfad().unwrap().ends_with(DATEINAME));
    }
}
