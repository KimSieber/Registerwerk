//! Das Register: eine SQLite-Datenbank mit Kopien und Auszügen.
//!
//! Es enthält keine eigene Information und lässt sich jederzeit aus den Dateien
//! neu aufbauen (Grundsatz 2). Es nimmt den Dateibaum und den Text je Seite der
//! PDFs auf. Suchindex, Vorschauen und Verschlüsselung folgen.
//!
//! Schema-Versionen (in `PRAGMA user_version`):
//! - 1 (v0.0.1): Tabellen `lauf` und `datei`
//! - 2 (v0.0.2): Tabelle `seite`, Verarbeitungsstand und PDF-Angaben in `datei`,
//!   Dokumentdatum für jede Datei
//! - 3 (v0.0.3): Tabellen `scheibe`, `vorschau` und `vorschau_weg`, Stand der
//!   Vorschauen in `datei`
//! - 4 (v0.0.4): Tabelle `ocr` mit dem erkannten Text, Stand der OCR in
//!   `datei`, Sicht `seite_text` mit dem gültigen Text je Seite
//!
//! Die Vorschauen selbst liegen nicht in dieser Datei, sondern in Scheiben
//! (`vorschau-0001.db`, …) im selben Ordner, je etwa [`SCHEIBE_GRENZE`] groß.
//! Neue Vorschauen kommen immer in die jüngste Scheibe. Ältere Scheiben ändern
//! sich nur, wenn Vorschauen entfernt werden (Konzept, Abschnitt 10).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;

use crate::dateibaum::Durchlauf;
use crate::guete::{self, Zaehlung};
use crate::pdf;
use crate::{datum, Ergebnis};

pub const DATEINAME: &str = "register.db";
const SCHEMA_VERSION: i64 = 4;

/// Größe, ab der keine neuen Vorschauen mehr in eine Scheibe kommen.
/// Konzept: 1 bis 2 GB. **Annahme**
pub const SCHEIBE_GRENZE: u64 = 1_500_000_000;

/// Version des Verfahrens, mit dem Vorschauen erzeugt werden. Erhöhen, wenn
/// sich Auflösung, Format, Qualität oder Farberkennung ändern. Dann werden
/// alle Vorschauen neu erzeugt.
pub const VORSCHAU_VERFAHREN: i64 = 2;
// 1: ravif (rav1e) Stufe 8, Qualität 25, 4:4:4
// 2: libavif mit libaom Stufe 8, Qualität 25, Farbe 4:2:0, Graustufen 4:0:0

/// Version des Verfahrens, mit dem Inhalte gelesen werden. Wird sie erhöht,
/// gelten alle bisher gelesenen PDFs als ausstehend und werden neu gelesen.
/// Erhöhen, wenn sich Textauslese, Gütemaß oder Seitenart ändern.
pub const INHALT_VERFAHREN: i64 = 1;

pub struct Register {
    pub(crate) db: Connection,
    pfad: Option<PathBuf>,
    /// Geöffnete Scheiben, nach Nummer. Ohne Ordner (Tests) im Arbeitsspeicher.
    pub(crate) scheiben: RefCell<HashMap<i64, Connection>>,
    pub(crate) scheibe_grenze: u64,
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

/// Kennzahlen zu den Inhalten der PDFs.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct InhaltStand {
    pub pdfs: u64,
    /// PDFs, deren Inhalt zum aktuellen Stand der Datei gelesen ist.
    pub gelesen: u64,
    /// PDFs, die noch gelesen werden müssen.
    pub ausstehend: u64,
    /// Davon "nur online". Sie werden erst gelesen, wenn sie lokal vorliegen.
    pub ausstehend_online: u64,
    /// Gelesen, aber mit Fehler (beschädigt, Kennwort).
    pub fehler: u64,
    pub seiten: u64,
    /// Seiten mit weniger als 20 sichtbaren Zeichen Text.
    pub seiten_ohne_text: u64,
    /// Seiten mit Text, aber Güte unter der Schwelle.
    pub seiten_schwach: u64,
    /// Seiten, die überwiegend aus Bildern bestehen.
    pub seiten_scan: u64,
    pub pdfs_ohne_text: u64,
    /// PDFs mit Text, deren Güte insgesamt unter der Schwelle liegt.
    pub pdfs_schwach: u64,
    pub fehler_liste: Vec<FehlerZeile>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FehlerZeile {
    pub pfad: String,
    pub meldung: String,
}

/// Eine PDF, deren Inhalt gelesen oder deren Vorschauen erzeugt werden müssen.
#[derive(Debug, Clone, PartialEq)]
pub struct Ausstehend {
    pub id: i64,
    pub pfad: String,
    pub nur_online: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RegisterStand {
    pub dateien: u64,
    /// Summe der bekannten Größen. Für Dateien, die nie lokal gesehen wurden,
    /// ist die Größe unbekannt und zählt als 0.
    pub bytes: u64,
    pub nur_online: u64,
    pub groesse_unbekannt: u64,
    /// Dokumentdatum nach Quelle: Dateiname, PDF-Erstellungsdatum, Änderungsdatum.
    pub mit_datum: u64,
    pub datum_pdf: u64,
    pub datum_geaendert: u64,
    pub inhalt: InhaltStand,
    pub vorschau: crate::scheiben::VorschauStand,
    pub ocr: crate::ocr::OcrStand,
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

    /// Ordner des Registers. `None` für ein Register im Arbeitsspeicher.
    pub fn ordner(&self) -> Option<&Path> {
        self.pfad.as_deref().and_then(Path::parent)
    }

    /// Legt das Schema an oder hebt es auf den aktuellen Stand.
    ///
    /// Die Version wird erst innerhalb einer sperrenden Transaktion gelesen.
    /// Öffnen zwei Aufrufe das Register gleichzeitig, wartet der zweite und
    /// sieht danach das bereits umgestellte Schema.
    fn einrichten(mut db: Connection, pfad: Option<PathBuf>) -> Ergebnis<Self> {
        wal_einschalten(&db)?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |z| z.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(crate::Fehler::Konfiguration(format!(
                "Das Register stammt von einer neueren Programmversion (Schema {version})"
            )));
        }
        if version < 1 {
            tx.execute_batch(
                "CREATE TABLE lauf (
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
                 PRAGMA user_version = 1;",
            )?;
        }
        if version < 2 {
            Self::migration_2(&tx)?;
        }
        if version < 3 {
            Self::migration_3(&tx)?;
        }
        if version < 4 {
            Self::migration_4(&tx)?;
        }
        tx.commit()?;
        Ok(Self { db, pfad, scheiben: RefCell::default(), scheibe_grenze: SCHEIBE_GRENZE })
    }

    /// Schema 2: Text je Seite, Verarbeitungsstand, Dokumentdatum für jede Datei.
    fn migration_2(db: &Connection) -> Ergebnis<()> {
        db.execute_batch(
            "-- Stand der Datei, zu dem der Inhalt gelesen wurde. Weicht das
             -- Änderungsdatum oder die Größe ab, wird neu gelesen.
             ALTER TABLE datei ADD COLUMN inhalt_geaendert INTEGER;
             ALTER TABLE datei ADD COLUMN inhalt_groesse   INTEGER;
             ALTER TABLE datei ADD COLUMN inhalt_verfahren INTEGER;
             -- Fehlermeldung, wenn sich die Datei nicht lesen ließ
             ALTER TABLE datei ADD COLUMN inhalt_fehler    TEXT;
             ALTER TABLE datei ADD COLUMN seiten           INTEGER;
             -- Erstellungsdatum aus den PDF-Metadaten, Rohwert
             ALTER TABLE datei ADD COLUMN pdf_erstellt     TEXT;
             CREATE TABLE seite (
                 datei       INTEGER NOT NULL REFERENCES datei(id) ON DELETE CASCADE,
                 nr          INTEGER NOT NULL,
                 text        TEXT NOT NULL,
                 -- sichtbare Zeichen, Tokens und wortartige Tokens (Gütemaß)
                 zeichen     INTEGER NOT NULL,
                 tokens      INTEGER NOT NULL,
                 wortartig   INTEGER NOT NULL,
                 -- Anteil der Seitenfläche, den Bilder abdecken
                 bildanteil  REAL NOT NULL,
                 PRIMARY KEY (datei, nr)
             ) WITHOUT ROWID;",
        )?;
        // Dokumentdatum aus dem Änderungsdatum für alle Dateien ohne Datum im Namen
        {
            let mut ohne = db.prepare("SELECT id, geaendert FROM datei WHERE datum_dokument IS NULL")?;
            let zeilen = ohne
                .query_map([], |z| Ok((z.get::<_, i64>(0)?, z.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut setzen = db.prepare("UPDATE datei SET datum_dokument = ?2, datum_quelle = ?3 WHERE id = ?1")?;
            for (id, geaendert) in zeilen {
                setzen.execute(params![id, datum::aus_zeitstempel(geaendert), datum::QUELLE_GEAENDERT])?;
            }
        }
        db.execute_batch("PRAGMA user_version = 2;")?;
        Ok(())
    }

    /// Schema 3: Verweise auf die Vorschauen in den Scheiben.
    fn migration_3(db: &Connection) -> Ergebnis<()> {
        db.execute_batch(
            "-- Stand der Datei, zu dem die Vorschauen erzeugt wurden
             ALTER TABLE datei ADD COLUMN vorschau_geaendert INTEGER;
             ALTER TABLE datei ADD COLUMN vorschau_groesse   INTEGER;
             ALTER TABLE datei ADD COLUMN vorschau_verfahren INTEGER;
             ALTER TABLE datei ADD COLUMN vorschau_fehler    TEXT;
             CREATE TABLE scheibe (
                 nr          INTEGER PRIMARY KEY,
                 datei       TEXT NOT NULL UNIQUE,
                 -- Summe der Vorschauen in Byte und ihre Anzahl
                 bytes       INTEGER NOT NULL DEFAULT 0,
                 vorschauen  INTEGER NOT NULL DEFAULT 0,
                 -- 1: nimmt keine neuen Vorschauen mehr auf
                 voll        INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE vorschau (
                 datei    INTEGER NOT NULL REFERENCES datei(id) ON DELETE CASCADE,
                 nr       INTEGER NOT NULL,
                 scheibe  INTEGER NOT NULL REFERENCES scheibe(nr),
                 -- Schlüssel des Bildes in der Scheibe
                 id       INTEGER NOT NULL,
                 breite   INTEGER NOT NULL,
                 hoehe    INTEGER NOT NULL,
                 farbig   INTEGER NOT NULL,
                 bytes    INTEGER NOT NULL,
                 PRIMARY KEY (datei, nr)
             ) WITHOUT ROWID;
             -- Bilder, deren Verweis entfernt wurde. Sie werden beim nächsten
             -- Aufräumen aus ihrer Scheibe gelöscht.
             CREATE TABLE vorschau_weg (
                 scheibe  INTEGER NOT NULL,
                 id       INTEGER NOT NULL
             );
             CREATE TRIGGER vorschau_entfernt AFTER DELETE ON vorschau BEGIN
                 INSERT INTO vorschau_weg (scheibe, id) VALUES (old.scheibe, old.id);
                 UPDATE scheibe SET bytes = bytes - old.bytes, vorschauen = vorschauen - 1
                 WHERE nr = old.scheibe;
             END;
             PRAGMA user_version = 3;",
        )?;
        Ok(())
    }

    /// Schema 4: erkannter Text je Seite und gültiger Text.
    fn migration_4(db: &Connection) -> Ergebnis<()> {
        db.execute_batch(
            "-- Stand des gelesenen Inhalts, zu dem die OCR lief
             ALTER TABLE datei ADD COLUMN ocr_geaendert INTEGER;
             ALTER TABLE datei ADD COLUMN ocr_groesse   INTEGER;
             ALTER TABLE datei ADD COLUMN ocr_verfahren INTEGER;
             ALTER TABLE datei ADD COLUMN ocr_fehler    TEXT;
             -- Erkannter Text je Seite. Nur für Seiten ohne oder mit schwachem
             -- eingebettetem Text. Der eingebettete Text bleibt in `seite`.
             CREATE TABLE ocr (
                 datei        INTEGER NOT NULL REFERENCES datei(id) ON DELETE CASCADE,
                 nr           INTEGER NOT NULL,
                 -- ohne_text, scan_schwach, digital_schwach
                 fall         TEXT NOT NULL,
                 text         TEXT NOT NULL,
                 zeichen      INTEGER NOT NULL,
                 tokens       INTEGER NOT NULL,
                 wortartig    INTEGER NOT NULL,
                 -- mittlere Sicherheit laut Tesseract, 0 bis 100
                 konfidenz    INTEGER NOT NULL,
                 dauer_ms     INTEGER NOT NULL,
                 -- 1: gilt statt des eingebetteten Textes
                 uebernommen  INTEGER NOT NULL,
                 PRIMARY KEY (datei, nr)
             ) WITHOUT ROWID;
             -- Gültiger Text je Seite: erkannt, wo übernommen, sonst eingebettet.
             -- Grundlage für die Suche.
             CREATE VIEW seite_text AS
                 SELECT s.datei, s.nr,
                        CASE WHEN o.uebernommen THEN o.text      ELSE s.text      END AS text,
                        CASE WHEN o.uebernommen THEN o.zeichen   ELSE s.zeichen   END AS zeichen,
                        CASE WHEN o.uebernommen THEN o.tokens    ELSE s.tokens    END AS tokens,
                        CASE WHEN o.uebernommen THEN o.wortartig ELSE s.wortartig END AS wortartig,
                        CASE WHEN o.uebernommen THEN 'ocr'       ELSE 'pdf'       END AS quelle
                 FROM seite s LEFT JOIN ocr o ON o.datei = s.datei AND o.nr = s.nr;
             PRAGMA user_version = 4;",
        )?;
        Ok(())
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
            // Stammt das Dokumentdatum aus dem Änderungsdatum, wandert es mit.
            let mut aktualisieren = tx.prepare(
                "UPDATE datei SET groesse = ?2, geaendert = ?3, nur_online = ?4, gesehen_lauf = ?5,
                    datum_dokument = CASE WHEN datum_quelle = ?6 THEN ?7 ELSE datum_dokument END
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
                        let (datum, quelle) = dokumentdatum(&d.name, None, d.geaendert);
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
                        aktualisieren.execute(params![
                            id,
                            groesse,
                            d.geaendert,
                            leer,
                            lauf,
                            datum::QUELLE_GEAENDERT,
                            datum::aus_zeitstempel(d.geaendert)
                        ])?;
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
        let zahl = |z: &rusqlite::Row, i: usize| -> rusqlite::Result<u64> { Ok(z.get::<_, i64>(i)? as u64) };
        let (dateien, bytes, nur_online, groesse_unbekannt, mit_datum, datum_pdf, datum_geaendert) =
            self.db.query_row(
                "SELECT count(*), coalesce(sum(groesse), 0), coalesce(sum(nur_online), 0),
                        coalesce(sum(groesse IS NULL), 0),
                        coalesce(sum(datum_quelle = ?1), 0), coalesce(sum(datum_quelle = ?2), 0),
                        coalesce(sum(datum_quelle = ?3), 0)
                 FROM datei",
                params![datum::QUELLE_DATEINAME, datum::QUELLE_PDF, datum::QUELLE_GEAENDERT],
                |z| Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?, zahl(z, 3)?, zahl(z, 4)?, zahl(z, 5)?, zahl(z, 6)?)),
            )?;
        let inhalt = self.inhalt_stand()?;
        let vorschau = self.vorschau_stand()?;
        let ocr = self.ocr_stand()?;

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

        Ok(RegisterStand {
            dateien,
            bytes,
            nur_online,
            groesse_unbekannt,
            mit_datum,
            datum_pdf,
            datum_geaendert,
            inhalt,
            vorschau,
            ocr,
            typen,
            jahre,
            letzter_lauf,
        })
    }

    /// Bedingung für PDFs, deren Inhalt (neu) gelesen werden muss.
    const AUSSTEHEND: &'static str = "endung = 'pdf' AND (
            inhalt_verfahren IS NULL OR inhalt_verfahren < ?1
            OR inhalt_geaendert IS NOT geaendert
            OR (groesse IS NOT NULL AND inhalt_groesse IS NOT groesse))";

    /// PDFs, deren Inhalt noch nicht oder nicht zum aktuellen Stand gelesen ist.
    pub fn ausstehend(&self) -> Ergebnis<Vec<Ausstehend>> {
        let sql = format!("SELECT id, pfad, nur_online FROM datei WHERE {} ORDER BY pfad", Self::AUSSTEHEND);
        let liste = self
            .db
            .prepare(&sql)?
            .query_map([INHALT_VERFAHREN], |z| {
                Ok(Ausstehend { id: z.get(0)?, pfad: z.get(1)?, nur_online: z.get(2)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    /// Schreibt den gelesenen Inhalt einer Datei ins Register und ersetzt einen
    /// früheren Stand. `geaendert` und `groesse` sind die Werte der Datei beim
    /// Lesen. Ein Lesefehler wird festgehalten, damit die Datei erst nach einer
    /// Änderung erneut versucht wird.
    pub fn inhalt_speichern(
        &mut self,
        id: i64,
        geaendert: i64,
        groesse: u64,
        inhalt: &Result<pdf::Inhalt, String>,
    ) -> Ergebnis<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM seite WHERE datei = ?1", [id])?;
        // Die Erkennung gehört zum alten Inhalt
        tx.execute("DELETE FROM ocr WHERE datei = ?1", [id])?;
        tx.execute(
            "UPDATE datei SET ocr_geaendert = NULL, ocr_groesse = NULL, ocr_verfahren = NULL, ocr_fehler = NULL
             WHERE id = ?1",
            [id],
        )?;
        let (seiten, erstellt, fehler) = match inhalt {
            Ok(inhalt) => {
                let mut einfuegen = tx.prepare(
                    "INSERT INTO seite (datei, nr, text, zeichen, tokens, wortartig, bildanteil)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for s in &inhalt.seiten {
                    let z: Zaehlung = guete::zaehlen(&s.text);
                    einfuegen.execute(params![
                        id,
                        s.nr,
                        s.text,
                        z.zeichen as i64,
                        z.tokens as i64,
                        z.wortartig as i64,
                        s.bildanteil as f64
                    ])?;
                }
                (Some(inhalt.seiten.len() as i64), inhalt.erstellt.clone(), None)
            }
            Err(meldung) => (None, None, Some(meldung.clone())),
        };
        tx.execute(
            "UPDATE datei SET inhalt_geaendert = ?2, inhalt_groesse = ?3, inhalt_verfahren = ?4,
                              inhalt_fehler = ?5, seiten = ?6, pdf_erstellt = ?7
             WHERE id = ?1",
            params![id, geaendert, groesse as i64, INHALT_VERFAHREN, fehler, seiten, erstellt],
        )?;
        // Dokumentdatum neu bestimmen, das PDF-Erstellungsdatum ist jetzt bekannt
        let (name, geaendert_reg): (String, i64) =
            tx.query_row("SELECT name, geaendert FROM datei WHERE id = ?1", [id], |z| Ok((z.get(0)?, z.get(1)?)))?;
        let (datum, quelle) = dokumentdatum(&name, erstellt.as_deref(), geaendert_reg);
        tx.execute(
            "UPDATE datei SET datum_dokument = ?2, datum_quelle = ?3 WHERE id = ?1",
            params![id, datum, quelle],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn inhalt_stand(&self) -> Ergebnis<InhaltStand> {
        let zahl = |z: &rusqlite::Row, i: usize| -> rusqlite::Result<u64> { Ok(z.get::<_, Option<i64>>(i)?.unwrap_or(0) as u64) };
        let mut s = InhaltStand::default();
        let sql = format!(
            "SELECT count(*), sum(NOT ({a})), sum({a}), sum(({a}) AND nur_online),
                    sum(NOT ({a}) AND inhalt_fehler IS NOT NULL)
             FROM datei WHERE endung = 'pdf'",
            a = Self::AUSSTEHEND
        );
        (s.pdfs, s.gelesen, s.ausstehend, s.ausstehend_online, s.fehler) =
            self.db.query_row(&sql, [INHALT_VERFAHREN], |z| {
                Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?, zahl(z, 3)?, zahl(z, 4)?))
            })?;
        (s.seiten, s.seiten_ohne_text, s.seiten_schwach, s.seiten_scan) = self.db.query_row(
            "SELECT count(*), sum(zeichen < ?1), sum(zeichen >= ?1 AND wortartig < ?2 * tokens),
                    sum(bildanteil >= ?3)
             FROM seite",
            params![guete::MIN_ZEICHEN as i64, guete::SCHWELLE, pdf::SCAN_FLAECHE as f64],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?, zahl(z, 3)?)),
        )?;
        (s.pdfs_ohne_text, s.pdfs_schwach) = self.db.query_row(
            "SELECT sum(max_zeichen < ?1), sum(max_zeichen >= ?1 AND wortartig < ?2 * tokens)
             FROM (SELECT max(zeichen) AS max_zeichen, sum(tokens) AS tokens, sum(wortartig) AS wortartig
                   FROM seite GROUP BY datei)",
            params![guete::MIN_ZEICHEN as i64, guete::SCHWELLE],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?)),
        )?;
        s.fehler_liste = self
            .db
            .prepare("SELECT pfad, inhalt_fehler FROM datei WHERE inhalt_fehler IS NOT NULL ORDER BY pfad LIMIT 50")?
            .query_map([], |z| Ok(FehlerZeile { pfad: z.get(0)?, meldung: z.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(s)
    }
}

/// Schaltet das WAL-Journal ein. Das Umschalten wartet nicht wie andere
/// Zugriffe auf eine Sperre. Öffnen mehrere Aufrufe ein neues Register
/// gleichzeitig, meldet es "database is locked". Dann kurz warten und erneut
/// versuchen.
fn wal_einschalten(db: &Connection) -> rusqlite::Result<()> {
    let mut versuche = 0;
    loop {
        match db.pragma_update(None, "journal_mode", "WAL") {
            Err(rusqlite::Error::SqliteFailure(f, _))
                if f.code == rusqlite::ErrorCode::DatabaseBusy && versuche < 100 =>
            {
                versuche += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            andere => return andere,
        }
    }
}

/// Dokumentdatum nach der Reihenfolge im Konzept (Abschnitt 9): Dateiname,
/// PDF-Erstellungsdatum, Änderungsdatum. Liefert immer ein Datum.
fn dokumentdatum(name: &str, pdf_erstellt: Option<&str>, geaendert: i64) -> (String, &'static str) {
    if let Some(d) = datum::aus_dateiname(name) {
        return (d, datum::QUELLE_DATEINAME);
    }
    if let Some(d) = pdf_erstellt.and_then(datum::aus_pdf_datum) {
        return (d, datum::QUELLE_PDF);
    }
    (datum::aus_zeitstempel(geaendert), datum::QUELLE_GEAENDERT)
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
        // ohne Datum im Namen gilt das Änderungsdatum (hier 1970)
        assert_eq!(
            s.jahre,
            vec![JahrZeile { jahr: "1970".into(), anzahl: 2 }, JahrZeile { jahr: "2020".into(), anzahl: 1 }]
        );
        assert_eq!(s.datum_geaendert, 2);
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
    fn migriert_register_von_schema_1() {
        let tmp = tempfile::tempdir().unwrap();
        {
            // Register im Stand von v0.0.1 nachbauen
            let db = Connection::open(tmp.path().join(DATEINAME)).unwrap();
            db.execute_batch(
                "CREATE TABLE lauf (id INTEGER PRIMARY KEY, begonnen INTEGER NOT NULL, beendet INTEGER,
                     gesehen INTEGER NOT NULL DEFAULT 0, neu INTEGER NOT NULL DEFAULT 0,
                     geaendert INTEGER NOT NULL DEFAULT 0, unveraendert INTEGER NOT NULL DEFAULT 0,
                     entfernt INTEGER NOT NULL DEFAULT 0, nicht_lesbar INTEGER NOT NULL DEFAULT 0);
                 CREATE TABLE datei (id INTEGER PRIMARY KEY, pfad TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                     endung TEXT NOT NULL, groesse INTEGER, geaendert INTEGER NOT NULL,
                     nur_online INTEGER NOT NULL, datum_dokument TEXT, datum_quelle TEXT,
                     erfasst_lauf INTEGER NOT NULL REFERENCES lauf(id),
                     gesehen_lauf INTEGER NOT NULL REFERENCES lauf(id));
                 CREATE INDEX datei_endung ON datei(endung);
                 CREATE INDEX datei_datum ON datei(datum_dokument);
                 INSERT INTO lauf (id, begonnen, beendet) VALUES (1, 1, 2);
                 INSERT INTO datei VALUES (1, 'A/2020 a.pdf', '2020 a.pdf', 'pdf', 5, 1794857940, 0, '2020', 'dateiname', 1, 1);
                 INSERT INTO datei VALUES (2, 'A/b.pdf', 'b.pdf', 'pdf', 5, 1794857940, 0, NULL, NULL, 1, 1);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let r = Register::oeffnen(tmp.path()).unwrap();
        let version: i64 = r.db.query_row("PRAGMA user_version", [], |z| z.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let s = r.stand().unwrap();
        assert_eq!((s.dateien, s.mit_datum, s.datum_geaendert), (2, 1, 1));
        assert_eq!((s.inhalt.pdfs, s.inhalt.ausstehend), (2, 2));
        assert_eq!((s.vorschau.pdfs, s.vorschau.ausstehend), (2, 2));
        assert_eq!(s.ocr.pdfs, 0);
        let datum: String = r.db.query_row("SELECT datum_dokument FROM datei WHERE id = 2", [], |z| z.get(0)).unwrap();
        assert_eq!(datum, "2026-11-16");
    }

    #[test]
    fn gleichzeitiges_oeffnen_stellt_nur_einmal_um() {
        let tmp = tempfile::tempdir().unwrap();
        let ordner = tmp.path().to_path_buf();
        let faeden: Vec<_> = (0..6)
            .map(|_| {
                let o = ordner.clone();
                std::thread::spawn(move || Register::oeffnen(&o).map(|r| r.stand().unwrap().dateien))
            })
            .collect();
        for f in faeden {
            assert_eq!(f.join().unwrap().unwrap(), 0);
        }
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
