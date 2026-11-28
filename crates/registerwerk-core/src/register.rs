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
//! - 5 (v0.0.5): Lage der Wörter in `ocr`, Stand der Textebene in `ocr` und
//!   `datei`
//! - 6 (v0.0.6): Suchindex (`suche.rs`), Stand der Suche in `datei`
//! - 7 (v0.0.6): ohne Index über den Text mit Leerraum
//! - 8 (v0.0.7): Stufe der Ordnerregel in `datei`
//! - 9 (v0.0.7): Geräte, je Gerät gesehene Ordner, Merkmale des Registers
//! - 10 (v0.0.9): Hinweis zum gelesenen Inhalt in `datei`
//! - 11 (v0.1.2): Tabelle `angefordert`, je Gerät die aus der Cloud
//!   angeforderten Dateien, bis sie wieder "nur online" liegen
//!
//! Die Vorschauen selbst liegen nicht in dieser Datei, sondern in Scheiben
//! (`vorschau-0001.db`, …) im selben Ordner, je etwa [`SCHEIBE_GRENZE`] groß.
//! Neue Vorschauen kommen immer in die jüngste Scheibe. Ältere Scheiben ändern
//! sich nur, wenn Vorschauen entfernt werden (Konzept, Abschnitt 10).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::Serialize;

use std::collections::BTreeMap;

use crate::dateibaum::{self, Durchlauf};
use crate::regeln::{Regeln, Stufe};
use crate::guete::{self, Zaehlung};
use crate::pdf;
use crate::{datum, Ergebnis};

pub const DATEINAME: &str = "register.db";
pub(crate) const SCHEMA_VERSION: i64 = 11;

/// Größe, ab der keine neuen Vorschauen mehr in eine Scheibe kommen.
/// Bis v0.0.7 waren es 1,5 GB. Liegt das Register in einem Cloud-Ordner, wird
/// die wachsende Scheibe nach jedem Lauf neu abgeglichen, deshalb seit v0.0.8
/// rund 200 MB. Größere Scheiben teilt `Register::scheiben_aufteilen`.
pub const SCHEIBE_GRENZE: u64 = 200_000_000;

/// Version des Verfahrens, mit dem Vorschauen erzeugt werden. Erhöhen, wenn
/// sich Auflösung, Format, Qualität oder Farberkennung ändern. Dann werden
/// alle Vorschauen neu erzeugt.
pub const VORSCHAU_VERFAHREN: i64 = 3;
// 1: ravif (rav1e) Stufe 8, Qualität 25, 4:4:4
// 2: libavif mit libaom Stufe 8, Qualität 25, Farbe 4:2:0, Graustufen 4:0:0
// 3: kurze Kante 1024 px statt fester Maßstab bezogen auf A4, lange Kante
//    höchstens 3072 px (v0.0.6)

/// Version des Verfahrens, mit dem Inhalte gelesen werden. Wird sie erhöht,
/// gelten alle bisher gelesenen PDFs als ausstehend und werden neu gelesen.
/// Erhöhen, wenn sich Textauslese, Gütemaß oder Seitenart ändern.
pub const INHALT_VERFAHREN: i64 = 2;
// 1: Text je Seite mit PdfPageText::all()
// 2: Text im sichtbaren Bereich in Seitenkoordinaten, richtig auch bei
//    gedrehten Seiten (v0.0.5)

pub struct Register {
    pub(crate) db: Connection,
    pfad: Option<PathBuf>,
    /// Geöffnete Scheiben, nach Nummer. Ohne Ordner (Tests) im Arbeitsspeicher.
    pub(crate) scheiben: RefCell<HashMap<i64, Connection>>,
    pub(crate) scheibe_grenze: u64,
    /// Wortliste der unscharfen Suche, beim ersten Gebrauch aufgebaut.
    pub(crate) wortliste: RefCell<Option<std::sync::Arc<fst::Map<Vec<u8>>>>>,
    /// Begrenzt die Schritte (Inhalte, Vorschauen, OCR, Textebene) auf diese
    /// Dateien (v0.0.8, Delta-Lauf). `None`: alle ausstehenden.
    pub(crate) auswahl: Option<std::collections::HashSet<i64>>,
    /// Nur lesend geöffnet ([`Register::oeffnen_lesend`]): auch die Scheiben
    /// werden dann nur lesend geöffnet (v0.0.9).
    pub(crate) nur_lesen: bool,
}

/// Art des Abgleichs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modus {
    /// Aufnehmen, ändern und entfernen.
    Voll,
    /// Wie `Voll`, aber zurückgerollt: zeigt, was geschähe.
    Probe,
    /// Nur aufnehmen und ändern, nichts entfernen, keine Inhalte zurücknehmen
    /// (v0.0.8). Entfernen bleibt ein eigener, bestätigter Schritt.
    NurAufnehmen,
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
    /// Von den entfernten Dateien: nicht mehr von den Ordnerregeln erfasst
    /// (Ordner ausgeschlossen oder ohne Regel, Ordner einer Bibliothek). Die übrigen gibt es im Dateibaum nicht mehr.
    pub entfernt_regel: u64,
    /// Seiten mit Text oder Vorschau, die mit `entfernt_regel` verloren gehen.
    pub entfernt_regel_seiten: u64,
    /// Dateien, deren Ordner jetzt auf "nur Metadaten" steht und deren Text
    /// und Vorschauen deshalb entfernt werden.
    pub reduziert: u64,
    pub reduziert_seiten: u64,
    /// Beim Durchlauf übergangene Ordner fremder Bibliotheken.
    pub uebergangene_ordner: u64,
    /// Nur geprüft: Das Register ist unverändert.
    pub probe: bool,
    /// Zu entfernen, aber von Kim abgewählt und deshalb behalten (v0.1.1).
    pub behalten: u64,
    /// Die Dateien hinter `entfernt` und `reduziert`, nach Pfad, damit sich
    /// vor der Bestätigung beurteilen lässt, was wegfällt. Höchstens
    /// [`BETROFFEN_MAX`] Einträge.
    pub betroffen: Vec<Betroffen>,
    /// Ordner, die dieses Gerät früher gesehen hat und jetzt nicht mehr
    /// findet: verschoben, gelöscht oder nicht eingehängt. Ihr Bestand bleibt
    /// im Register, bis er ausdrücklich entfernt wird.
    pub vermisst: Vec<OrdnerBestand>,
    /// Ordner, die es auf diesem Gerät nie gab: Sie gehören zu einem anderen
    /// Gerät und bleiben unberührt.
    pub fremd: Vec<OrdnerBestand>,
}

/// Ein Ordner, der auf diesem Gerät fehlt, mit seinem Bestand im Register.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrdnerBestand {
    /// Pfad im Register (`Ast/…`): der oberste fehlende Ordner.
    pub ordner: String,
    /// Dateien im Register in diesem Ordner samt Unterordnern.
    pub dateien: u64,
    /// Geräte, die den Ordner gesehen haben.
    pub geraete: Vec<String>,
}

/// Ein Ordner mit Dateien, die ausstehen und nur online liegen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OnlineOrdner {
    /// Pfad relativ zum Cloud-Ordner.
    pub ordner: String,
    pub dateien: u64,
}

/// Mehr Dateien nennt ein Lauf nicht einzeln.
pub const BETROFFEN_MAX: usize = 5000;

/// Warum eine Datei nach den Ordnerregeln nicht mehr oder nur noch mit Namen
/// im Register steht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grund {
    /// Für den Ordner gilt keine Regel.
    OhneRegel,
    /// Der Ordner ist ausgeschlossen.
    Ausgeschlossen,
    /// Die Datei liegt in einem Ordner fremder Bibliotheken.
    Bibliothek,
    /// Versteckte Datei mit Punkt am Anfang, seit v0.1.5 übergangen.
    Versteckt,
    /// Der Ordner steht auf "nur Metadaten": Text und Vorschauen entfallen.
    NurMetadaten,
    /// Den Ordner gibt es auf diesem Gerät, die Datei darin nicht mehr:
    /// gelöscht, verschoben oder umbenannt.
    Fehlt,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Betroffen {
    /// Pfad relativ zum Cloud-Ordner.
    pub pfad: String,
    pub grund: Grund,
    /// Seiten mit Text oder Vorschau, die verloren gehen.
    pub seiten: u64,
    /// Wann die Datei zuletzt gesehen wurde (Sekunden seit 1970) und von
    /// welchem Gerät. Eine eben erst auf einem anderen Gerät gesehene Datei
    /// ist oft nicht gelöscht, sondern noch nicht hierher synchronisiert.
    pub zuletzt: Option<i64>,
    pub geraet: Option<String>,
}

impl LaufErgebnis {
    /// Der Abgleich würde Einträge entfernen oder auf den Namen kürzen. Das
    /// soll vorher bestätigt werden: Fehlen allein löscht nie.
    pub fn bestaetigung_noetig(&self) -> bool {
        self.entfernt > 0 || self.reduziert > 0
    }
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
    /// Dateien mit einem Hinweis zum Text, etwa nur grob gelesen (v0.0.9)
    /// oder gekürzt (v0.1.5), und die ersten 50.
    pub hinweise: u64,
    pub hinweis_liste: Vec<FehlerZeile>,
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
    pub textebene: crate::textebene::TextebeneStand,
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

/// Wartezeit, wenn eine andere Verbindung das Register gerade sperrt. Mit dem
/// klassischen Journal (v0.0.9) sperrt ein Schreiber beim Abschluss kurz auch
/// die Leser.
const WARTEN_MS: u64 = 15_000;

/// Tabellen, die ein Abgleich ändert. Die Probe arbeitet auf Kopien davon.
const SCHATTEN: [&str; 4] = ["lauf", "geraet", "datei", "ordner_gesehen"];

/// Enthält `sql` ein Semikolon außerhalb von Zeichenketten, Bezeichnern in
/// Anführungszeichen und Kommentaren, also mehr als eine Anweisung (v0.1.4)?
/// Ein Semikolon am Ende mit nur Leerraum danach zählt nicht.
fn mehrere_anweisungen(sql: &str) -> bool {
    let z: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < z.len() {
        match z[i] {
            // Zeichenkette oder Bezeichner, verdoppeltes Zeichen ist ein Escape
            q @ ('\'' | '"' | '`') => {
                i += 1;
                while i < z.len() {
                    if z[i] == q {
                        if z.get(i + 1) == Some(&q) {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            '[' => {
                while i < z.len() && z[i] != ']' {
                    i += 1;
                }
            }
            '-' if z.get(i + 1) == Some(&'-') => {
                while i < z.len() && z[i] != '\n' {
                    i += 1;
                }
            }
            '/' if z.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < z.len() && !(z[i] == '*' && z.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i += 1;
            }
            ';' => return z[i + 1..].iter().any(|c| !c.is_whitespace()),
            _ => {}
        }
        i += 1;
    }
    false
}

impl Register {
    /// Öffnet das Register in `ordner` zum Schreiben und legt es bei Bedarf an.
    pub fn oeffnen(ordner: &Path) -> Ergebnis<Self> {
        std::fs::create_dir_all(ordner)?;
        let pfad = ordner.join(DATEINAME);
        let db = Connection::open(&pfad)?;
        crate::verschluesselung::verbinden(&db, &pfad)?;
        Self::einrichten(db, Some(pfad))
    }

    /// Legt in `ordner` ein leeres Register an, wenn dort noch keins liegt
    /// (v0.1.3). Ein vorhandenes Register bleibt unberührt, nicht einmal
    /// geöffnet. Liefert, ob es neu angelegt wurde. Ohne diesen Schritt kann
    /// die Prüfung, die nur lesend öffnet, ein neu gewähltes Register nie
    /// abgleichen.
    pub fn anlegen(ordner: &Path) -> Ergebnis<bool> {
        if ordner.join(DATEINAME).is_file() {
            return Ok(false);
        }
        drop(Self::oeffnen(ordner)?);
        Ok(true)
    }

    /// Beim Programmstart, bevor irgendetwas anderes das Register öffnet:
    /// stellt ein Register aus v0.0.8 auf das klassische Journal um (v0.0.9).
    /// Liefert, ob es danach im klassischen Journal läuft. Ohne Register
    /// geschieht nichts.
    pub fn journal_vorbereiten(ordner: &Path) -> Ergebnis<bool> {
        let pfad = ordner.join(DATEINAME);
        if !pfad.is_file() {
            return Ok(true);
        }
        let db = Connection::open(&pfad)?;
        crate::verschluesselung::verbinden(&db, &pfad)?;
        db.busy_timeout(std::time::Duration::ZERO)?;
        journal_umstellen(&db, 40)?;
        let modus: String = db.query_row("PRAGMA journal_mode", [], |z| z.get(0))?;
        Ok(modus.eq_ignore_ascii_case("delete"))
    }

    /// Öffnet das Register nur lesend (v0.0.9): Suche, Anzeige und die Prüfung
    /// beim Start. SQLite verweigert dann jedes Schreiben, die Dateien im
    /// Register-Ordner bleiben unberührt. Muss das Schema noch umgestellt
    /// werden, geschieht das einmal über [`Register::oeffnen`].
    pub fn oeffnen_lesend(ordner: &Path) -> Ergebnis<Self> {
        let pfad = ordner.join(DATEINAME);
        if !pfad.is_file() {
            return Err(crate::Fehler::Konfiguration("Noch kein Register vorhanden.".into()));
        }
        let lesend = || -> Ergebnis<Connection> {
            let db = Connection::open_with_flags(
                &pfad,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
            )?;
            crate::verschluesselung::verbinden(&db, &pfad)?;
            db.busy_timeout(std::time::Duration::from_millis(WARTEN_MS))?;
            // Hilfstabellen der Prüfung nur im Arbeitsspeicher
            db.pragma_update(None, "temp_store", "MEMORY")?;
            Ok(db)
        };
        let mut db = lesend()?;
        if !Self::ist_aktuell(&db) {
            drop(db);
            drop(Self::oeffnen(ordner)?);
            db = lesend()?;
        }
        rusqlite::vtab::array::load_module(&db)?;
        let mut r = Self::fertig(db, Some(pfad));
        r.nur_lesen = true;
        Ok(r)
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
        if pfad.is_some() {
            // Ein Versuch ohne Warten: Sind andere Verbindungen offen, gelingt
            // es beim nächsten Start ([`Register::journal_vorbereiten`]).
            db.busy_timeout(std::time::Duration::ZERO)?;
            journal_umstellen(&db, 1)?;
        }
        db.busy_timeout(std::time::Duration::from_millis(WARTEN_MS))?;
        db.pragma_update(None, "temp_store", "MEMORY")?;
        // Größerer Zwischenspeicher: Ein Schreiber sperrt die Leser erst, wenn
        // er Seiten auslagern muss
        db.pragma_update(None, "cache_size", -65_536)?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        rusqlite::vtab::array::load_module(&db)?;
        if Self::ist_aktuell(&db) {
            // Der Regelfall braucht keine Schreibsperre (v0.0.9). Sonst
            // scheitert jedes Öffnen mit "database is locked", solange eine
            // andere Verbindung schreibt, etwa die Prüfung beim Start.
            return Ok(Self::fertig(db, pfad));
        }
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
        if version < 5 {
            Self::migration_5(&tx)?;
        }
        if version < 6 {
            crate::suche::migration_6(&tx)?;
        }
        if version < 7 {
            crate::suche::migration_7(&tx)?;
        }
        if version < 8 {
            // Stufe der Ordnerregel: 1 voll, 2 nur Metadaten (`regeln.rs`)
            tx.execute_batch(
                "ALTER TABLE datei ADD COLUMN stufe INTEGER NOT NULL DEFAULT 1;
                 PRAGMA user_version = 8;",
            )?;
        }
        if version < 9 {
            // Mehrere Geräte arbeiten auf demselben Register. Es merkt sich,
            // welches Gerät einen Ordner gesehen hat: Fehlt ein Ordner auf
            // einem Gerät, das ihn nie gesehen hat, gehört er zu einem
            // anderen Gerät. `merkmal` hält einmalige Umstellungen fest.
            tx.execute_batch(
                "CREATE TABLE geraet (
                     id    INTEGER PRIMARY KEY,
                     name  TEXT NOT NULL UNIQUE
                 );
                 CREATE TABLE ordner_gesehen (
                     ordner   TEXT NOT NULL,
                     geraet   INTEGER NOT NULL REFERENCES geraet(id) ON DELETE CASCADE,
                     zuletzt  INTEGER NOT NULL,
                     PRIMARY KEY (ordner, geraet)
                 ) WITHOUT ROWID;
                 CREATE TABLE merkmal (
                     name  TEXT PRIMARY KEY,
                     wert  TEXT NOT NULL
                 ) WITHOUT ROWID;
                 ALTER TABLE lauf ADD COLUMN geraet INTEGER REFERENCES geraet(id);
                 PRAGMA user_version = 9;",
            )?;
        }
        if version < 10 {
            // Hinweis zum gelesenen Inhalt, etwa „Text nur grob gelesen“ (v0.0.9)
            tx.execute_batch(
                "ALTER TABLE datei ADD COLUMN inhalt_hinweis TEXT;
                 PRAGMA user_version = 10;",
            )?;
        }
        if version < 11 {
            // Welche Dateien Registerwerk auf welchem Gerät aus der Cloud
            // angefordert hat (v0.1.2). Bis v0.1.1 stand das nur im
            // Arbeitsspeicher eines Laufs. Endete der Lauf vor dem Auslagern
            // oder kam eine Datei erst nach dem Warten, blieb sie lokal.
            // Der Zustand "lokal" gilt je Gerät, deshalb der Gerätename.
            tx.execute_batch(
                "CREATE TABLE angefordert (
                     datei   INTEGER NOT NULL REFERENCES datei(id) ON DELETE CASCADE,
                     geraet  TEXT NOT NULL,
                     zeit    INTEGER NOT NULL,
                     PRIMARY KEY (datei, geraet)
                 ) WITHOUT ROWID;
                 PRAGMA user_version = 11;",
            )?;
        }
        // v0.0.7 liest Fax-TIFFs. Ein festgehaltener Fehler gilt sonst, bis
        // sich die Datei ändert: Diese Dateien werden einmal neu versucht.
        let versucht: bool = tx.query_row("SELECT count(*) FROM merkmal WHERE name = 'fax_tiff'", [], |z| z.get(0))?;
        if !versucht {
            tx.execute_batch(
                "UPDATE datei SET vorschau_verfahren = NULL, vorschau_fehler = NULL
                 WHERE vorschau_fehler IS NOT NULL AND endung IN ('tif','tiff');
                 UPDATE datei SET ocr_verfahren = NULL, ocr_fehler = NULL
                 WHERE ocr_fehler IS NOT NULL AND endung IN ('tif','tiff');
                 INSERT INTO merkmal (name, wert) VALUES ('fax_tiff', '1');",
            )?;
        }
        // v0.0.8 liest TIFF mit Farbpalette: diese Dateien einmal neu versuchen
        let versucht: bool =
            tx.query_row("SELECT count(*) FROM merkmal WHERE name = 'tiff_palette'", [], |z| z.get(0))?;
        if !versucht {
            tx.execute_batch(
                "UPDATE datei SET vorschau_verfahren = NULL, vorschau_fehler = NULL
                 WHERE vorschau_fehler LIKE '%RGBPalette%' AND endung IN ('tif','tiff');
                 UPDATE datei SET ocr_verfahren = NULL, ocr_fehler = NULL
                 WHERE ocr_fehler LIKE '%RGBPalette%' AND endung IN ('tif','tiff');
                 INSERT INTO merkmal (name, wert) VALUES ('tiff_palette', '1');",
            )?;
        }
        // v0.0.9: Lokale leere Dateien galten kurz als Lesefehler. Sie sind
        // gelesen, ohne Seiten, und gehören nicht in die Fehlerliste.
        let erledigt: bool =
            tx.query_row("SELECT count(*) FROM merkmal WHERE name = 'leer_ohne_fehler'", [], |z| z.get(0))?;
        if !erledigt {
            tx.execute("UPDATE datei SET inhalt_fehler = NULL WHERE inhalt_fehler = ?1", [crate::inhalte::LEER])?;
            tx.execute("INSERT INTO merkmal (name, wert) VALUES ('leer_ohne_fehler', '1')", [])?;
        }
        // v0.0.9: Toleranter Leser für alte Office-Formate, RTF, Word 2, Excel 2–5,
        // grober Rückfall. Alte Office-Dateien mit Lesefehler einmal neu versuchen.
        let erledigt: bool =
            tx.query_row("SELECT count(*) FROM merkmal WHERE name = 'office_tolerant'", [], |z| z.get(0))?;
        if !erledigt {
            tx.execute(
                "UPDATE datei SET inhalt_verfahren = NULL
                 WHERE inhalt_fehler IS NOT NULL AND endung IN ('doc','dot','xls','xlt','ppt','pps','pages','key')",
                [],
            )?;
            tx.execute("INSERT INTO merkmal (name, wert) VALUES ('office_tolerant', '1')", [])?;
        }
        tx.commit()?;
        Ok(Self::fertig(db, pfad))
    }

    /// Einmalige Umstellungen, die `einrichten` in `merkmal` festhält.
    const MERKMALE: [&'static str; 4] = ["fax_tiff", "tiff_palette", "leer_ohne_fehler", "office_tolerant"];

    /// Ist das Schema auf dem aktuellen Stand und jede einmalige Umstellung
    /// erledigt? Nur lesend. Im Zweifel `false`, dann prüft `einrichten`
    /// unter der Schreibsperre erneut.
    fn ist_aktuell(db: &Connection) -> bool {
        let version: rusqlite::Result<i64> = db.query_row("PRAGMA user_version", [], |z| z.get(0));
        if version.ok() != Some(SCHEMA_VERSION) {
            return false;
        }
        Self::MERKMALE.iter().all(|name| {
            db.query_row("SELECT count(*) FROM merkmal WHERE name = ?1", [name], |z| z.get::<_, bool>(0))
                .unwrap_or(false)
        })
    }

    fn fertig(db: Connection, pfad: Option<PathBuf>) -> Self {
        Self {
            db,
            pfad,
            scheiben: RefCell::default(),
            scheibe_grenze: SCHEIBE_GRENZE,
            wortliste: RefCell::default(),
            auswahl: None,
            nur_lesen: false,
        }
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

    /// Schema 5: Lage der erkannten Wörter und Stand der Textebene in der PDF.
    fn migration_5(db: &Connection) -> Ergebnis<()> {
        db.execute_batch(
            "-- Lage der Wörter (woerter.rs), nur bei übernommenem Text. Wird
             -- entfernt, sobald die Textebene in der PDF steht.
             ALTER TABLE ocr ADD COLUMN woerter BLOB;
             -- 1: der erkannte Text steht als Textebene in der PDF
             ALTER TABLE ocr ADD COLUMN in_pdf INTEGER NOT NULL DEFAULT 0;
             -- Zeitpunkt, zu dem Registerwerk zuletzt eine Textebene in die
             -- Datei geschrieben hat
             ALTER TABLE datei ADD COLUMN textebene_geschrieben INTEGER;
             -- Fehlermeldung, wenn das Schreiben scheiterte. Die Datei wird
             -- erst nach einer Änderung erneut versucht.
             ALTER TABLE datei ADD COLUMN textebene_fehler TEXT;
             PRAGMA user_version = 5;",
        )?;
        Ok(())
    }

    /// Gleicht das Register mit einem Durchlauf des Dateibaums ab.
    ///
    /// - Neu ist jeder Pfad, den das Register nicht kennt.
    /// - Geändert ist eine Datei, deren Größe oder Änderungsdatum abweicht.
    /// - Entfernt ist ein Pfad, der im Register steht und im Durchlauf fehlt:
    ///   weil es die Datei nicht mehr gibt oder weil die Ordnerregeln sie
    ///   nicht mehr erfassen. Ausgenommen sind Ordner mit Regel, die auf
    ///   diesem Gerät fehlen oder nicht vollständig gelesen wurden.
    /// - Steht der Ordner einer Datei jetzt auf "nur Metadaten", werden ihr
    ///   Text und ihre Vorschauen entfernt.
    ///
    /// Wechselt eine Datei bei gleichem Änderungsdatum zwischen lokal und
    /// "nur online", ist das keine Änderung des Inhalts. Die letzte bekannte
    /// Größe bleibt dann erhalten.
    pub fn abgleichen(&mut self, durchlauf: &Durchlauf) -> Ergebnis<LaufErgebnis> {
        Ok(self.abgleich(durchlauf, Modus::Voll, false, &[])?.0)
    }

    /// Wie [`Register::abgleichen`], entfernt aber nichts unter `behalten`
    /// (Dateien oder Ordner samt Unterordnern). Für den Delta-Lauf (v0.1.1):
    /// Entfernen ist dort ein Schritt von „Verarbeiten“, abgewählte Dateien
    /// bleiben und stehen bei der nächsten Prüfung wieder an.
    pub fn abgleichen_behalten(&mut self, durchlauf: &Durchlauf, behalten: &[String]) -> Ergebnis<LaufErgebnis> {
        Ok(self.abgleich(durchlauf, Modus::Voll, false, behalten)?.0)
    }

    /// Wie [`Register::abgleichen`], lässt das Register aber unverändert.
    /// Zeigt vorab, was ein Abgleich entfernen würde.
    pub fn abgleich_pruefen(&mut self, durchlauf: &Durchlauf) -> Ergebnis<LaufErgebnis> {
        Ok(self.abgleich(durchlauf, Modus::Probe, false, &[])?.0)
    }

    /// Nimmt neue und geänderte Dateien auf, entfernt aber nichts (v0.0.8).
    /// Was zu entfernen wäre, steht wie bei der Probe im Ergebnis.
    pub fn aufnehmen(&mut self, durchlauf: &Durchlauf) -> Ergebnis<LaufErgebnis> {
        Ok(self.abgleich(durchlauf, Modus::NurAufnehmen, false, &[])?.0)
    }

    /// Probe mit Deltaanalyse (v0.0.8): wie [`Register::abgleich_pruefen`],
    /// dazu alle Dateien, bei denen danach ein Schritt ansteht, als neu,
    /// geändert oder schon offen. Das Register bleibt unverändert.
    pub fn delta_pruefen(&mut self, durchlauf: &Durchlauf) -> Ergebnis<crate::ablauf::Delta> {
        let (lauf, delta) = self.abgleich(durchlauf, Modus::Probe, true, &[])?;
        let (dateien, nur_namen_dateien) = delta.unwrap_or_default();
        // Was wegfiele, steht auch im Baum (v0.1.1)
        let entfernen_dateien = lauf
            .betroffen
            .iter()
            .map(|b| crate::ablauf::DeltaDatei {
                pfad: b.pfad.clone(),
                art: crate::ablauf::Art::Entfernen,
                nur_online: false,
                schritte: 0,
                grund: Some(b.grund),
            })
            .collect();
        Ok(crate::ablauf::Delta {
            lauf,
            dateien,
            nur_namen: nur_namen_dateien.len() as u64,
            nur_namen_dateien,
            entfernen_dateien,
        })
    }

    #[allow(clippy::type_complexity)]
    fn abgleich(
        &mut self,
        durchlauf: &Durchlauf,
        modus: Modus,
        mit_delta: bool,
        behalten: &[String],
    ) -> Ergebnis<(LaufErgebnis, Option<(Vec<crate::ablauf::DeltaDatei>, Vec<crate::ablauf::DeltaDatei>)>)> {
        let probe = modus == Modus::Probe;
        let start = Instant::now();
        if !probe {
            return self.abgleich_tx(durchlauf, modus, mit_delta, behalten, start);
        }
        // Die Probe schreibt nicht ins Register (v0.0.9). Sie arbeitet auf
        // Kopien der Tabellen, die sie ändern würde, im Arbeitsspeicher. Die
        // Kopien verdecken die Tabellen des Registers für die Dauer der Probe.
        self.db.execute_batch("PRAGMA foreign_keys = OFF")?;
        let ergebnis = self.schatten_anlegen().and_then(|_| self.abgleich_tx(durchlauf, modus, mit_delta, behalten, start));
        for t in SCHATTEN {
            let _ = self.db.execute_batch(&format!("DROP TABLE IF EXISTS temp.{t}"));
        }
        if !self.nur_lesen {
            let _ = self.db.execute_batch("PRAGMA foreign_keys = ON");
        }
        ergebnis
    }

    /// Kopien der Tabellen, die ein Abgleich ändert, als Hilfstabellen im
    /// Arbeitsspeicher, mit derselben Definition (v0.0.9).
    ///
    /// Die Definition stammt aus der Datei. Sie wird nur ausgeführt, wenn sie
    /// genau eine Anweisung `CREATE TABLE` ist (v0.1.4): Angehängtes SQL in
    /// einer ausgetauschten Datei bricht die Probe ab, statt zu laufen.
    fn schatten_anlegen(&self) -> Ergebnis<()> {
        for t in SCHATTEN {
            let sql: String = self.db.query_row(
                "SELECT sql FROM main.sqlite_schema WHERE type = 'table' AND name = ?1",
                [t],
                |z| z.get(0),
            )?;
            if !sql.starts_with("CREATE TABLE") || mehrere_anweisungen(&sql) {
                return Err(crate::Fehler::Konfiguration(format!(
                    "Die Definition der Tabelle {t} im Register ist ungültig."
                )));
            }
            let sql = sql.replacen("CREATE TABLE", "CREATE TEMP TABLE", 1);
            self.db.execute_batch(&format!(
                "DROP TABLE IF EXISTS temp.{t}; {sql}; INSERT INTO temp.{t} SELECT * FROM main.{t};"
            ))?;
        }
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn abgleich_tx(
        &mut self,
        durchlauf: &Durchlauf,
        modus: Modus,
        mit_delta: bool,
        behalten: &[String],
        start: Instant,
    ) -> Ergebnis<(LaufErgebnis, Option<(Vec<crate::ablauf::DeltaDatei>, Vec<crate::ablauf::DeltaDatei>)>)> {
        let probe = modus == Modus::Probe;
        let nur_aufnehmen = modus == Modus::NurAufnehmen;
        // Von Kim abgewählt: wird nicht entfernt und nicht gekürzt (v0.1.1)
        let bleibt = |pfad: &str| !behalten.is_empty() && !crate::ablauf::ausgewaehlt(pfad, behalten);
        let mut neu_ids: Vec<i64> = Vec::new();
        let mut geaendert_ids: Vec<i64> = Vec::new();
        let mut gesehen: std::collections::HashSet<i64> = Default::default();
        let mut geschrieben = false;
        let tx = self.db.transaction()?;
        if tx.execute("INSERT OR IGNORE INTO geraet (name) VALUES (?1)", [&durchlauf.geraet])? > 0 {
            geschrieben = true;
        }
        let geraet: i64 = tx.query_row("SELECT id FROM geraet WHERE name = ?1", [&durchlauf.geraet], |z| z.get(0))?;
        tx.execute("INSERT INTO lauf (begonnen, geraet) VALUES (?1, ?2)", params![jetzt(), geraet])?;
        let lauf = tx.last_insert_rowid();

        let mut e = LaufErgebnis {
            lauf,
            gesehen: durchlauf.dateien.len() as u64,
            nicht_lesbar: durchlauf.nicht_lesbar.len() as u64,
            fehlende_verzeichnisse: durchlauf.fehlende_verzeichnisse.clone(),
            uebergangene_ordner: durchlauf.uebergangene_ordner,
            probe,
            ..Default::default()
        };

        {
            let betroffen = |pfad: &str, grund: Grund, seiten: Option<i64>, liste: &mut Vec<Betroffen>| {
                if liste.len() < BETROFFEN_MAX {
                    liste.push(Betroffen {
                        pfad: pfad.to_string(),
                        grund,
                        seiten: seiten.unwrap_or(0) as u64,
                        zuletzt: None,
                        geraet: None,
                    });
                }
            };
            let mut suchen =
                tx.prepare("SELECT id, groesse, geaendert, stufe, seiten, nur_online FROM datei WHERE pfad = ?1")?;
            let mut einfuegen = tx.prepare(
                "INSERT INTO datei (pfad, name, endung, groesse, geaendert, nur_online,
                                    datum_dokument, datum_quelle, erfasst_lauf, gesehen_lauf, stufe)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
            )?;
            // Stammt das Dokumentdatum aus dem Änderungsdatum, wandert es mit.
            let mut aktualisieren = tx.prepare(
                "UPDATE datei SET groesse = ?2, geaendert = ?3, nur_online = ?4, gesehen_lauf = ?5,
                    datum_dokument = CASE WHEN datum_quelle = ?6 THEN ?7 ELSE datum_dokument END,
                    stufe = ?8
                 WHERE id = ?1",
            )?;

            for d in &durchlauf.dateien {
                let leer = d.nur_online;
                if leer {
                    e.nur_online += 1;
                }
                let groesse_neu = (!leer).then_some(d.groesse as i64);
                let stufe_neu = d.stufe.als_zahl();
                // id, Größe, Änderungsdatum, Stufe, Seiten, nur online
                type Vorhanden = (i64, Option<i64>, i64, i64, Option<i64>, bool);
                let vorhanden: Option<Vorhanden> = suchen
                    .query_row([&d.pfad], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?, z.get(3)?, z.get(4)?, z.get(5)?)))
                    .optional()?;
                match vorhanden {
                    None => {
                        let (datum, quelle) = dokumentdatum(&d.name, None, d.geaendert);
                        einfuegen.execute(params![
                            d.pfad, d.name, d.endung, groesse_neu, d.geaendert, leer, datum, quelle, lauf, stufe_neu
                        ])?;
                        e.neu += 1;
                        let id = tx.last_insert_rowid();
                        neu_ids.push(id);
                        gesehen.insert(id);
                        geschrieben = true;
                    }
                    Some((id, groesse_alt, geaendert_alt, stufe_alt, seiten, online_alt)) => {
                        gesehen.insert(id);
                        let gleiches_datum = geaendert_alt == d.geaendert;
                        let gleiche_groesse = groesse_alt == groesse_neu;
                        // Bei gleichem Datum ist ein Wechsel von oder zu 0 Byte
                        // nur ein Wechsel zwischen lokal und "nur online".
                        let nur_statuswechsel = gleiches_datum && (leer || groesse_alt.is_none());
                        if gleiches_datum && (gleiche_groesse || nur_statuswechsel) {
                            e.unveraendert += 1;
                        } else {
                            e.geaendert += 1;
                            geaendert_ids.push(id);
                        }
                        // Ohne Entfernen bleibt die alte Stufe, bis der Inhalt
                        // bestätigt zurückgenommen wird
                        let zuruecknehmen = stufe_alt != stufe_neu && d.stufe == Stufe::Metadaten;
                        let halten = zuruecknehmen && !nur_aufnehmen && bleibt(&d.pfad);
                        if halten {
                            e.behalten += 1;
                        }
                        let stufe_neu = if (nur_aufnehmen || halten) && zuruecknehmen { stufe_alt } else { stufe_neu };
                        let groesse = if gleiches_datum { groesse_neu.or(groesse_alt) } else { groesse_neu };
                        // Nur schreiben, was sich ändert (v0.0.9)
                        let anders = groesse != groesse_alt
                            || d.geaendert != geaendert_alt
                            || leer != online_alt
                            || stufe_neu != stufe_alt;
                        if anders {
                            geschrieben = true;
                            aktualisieren.execute(params![
                            id,
                            groesse,
                            d.geaendert,
                            leer,
                            lauf,
                            datum::QUELLE_GEAENDERT,
                            datum::aus_zeitstempel(d.geaendert),
                            stufe_neu
                        ])?;
                        }
                        if zuruecknehmen && !nur_aufnehmen && !halten && Self::inhalt_entfernen(&tx, id, probe)? {
                            geschrieben = true;
                            e.reduziert += 1;
                            e.reduziert_seiten += seiten.unwrap_or(0) as u64;
                            betroffen(&d.pfad, Grund::NurMetadaten, seiten, &mut e.betroffen);
                        }
                    }
                }
            }

            // Nicht mehr gesehen. Entfernt wird nur mit einem Grund:
            // - die Ordnerregeln erfassen die Datei nicht mehr, oder
            // - ihr Ordner ist auf diesem Gerät da und wurde vollständig
            //   gelesen, die Datei fehlt darin.
            // Fehlt der Ordner selbst, bleibt der Bestand: Er liegt auf einem
            // anderen Gerät oder ist gerade nicht erreichbar.
            let unvollstaendig =
                |ordner: &str| durchlauf.unvollstaendige_verzeichnisse.iter().any(|v| v == ordner);
            let fehlend = |ordner: &str| durchlauf.fehlende_verzeichnisse.iter().any(|v| v == ordner);
            // Nicht gesehen ist, was dieser Durchlauf nicht gefunden hat. Seit
            // v0.0.9 ohne Vermerk je Datei: Der hätte bei jedem Durchlauf jede
            // Zeile des Registers geändert. Wann und wo eine Datei zuletzt
            // gesehen wurde, ist damit nicht mehr bekannt.
            type Fehlende = (i64, String, Option<i64>, Option<i64>, Option<String>);
            let mut fehlen: Vec<Fehlende> = Vec::new();
            {
                let mut alle = tx.prepare("SELECT id, pfad, seiten FROM datei")?;
                let mut zeilen = alle.query([])?;
                while let Some(z) = zeilen.next()? {
                    let id: i64 = z.get(0)?;
                    if !gesehen.contains(&id) {
                        fehlen.push((id, z.get(1)?, z.get(2)?, None, None));
                    }
                }
            }
            let mut entfernen = tx.prepare("DELETE FROM datei WHERE id = ?1")?;
            // oberster fehlender Ordner -> Zahl der Dateien darin
            let mut ohne_ordner: BTreeMap<String, u64> = BTreeMap::new();
            for (id, pfad, seiten, zuletzt, zuletzt_geraet) in fehlen {
                let regel = durchlauf.regeln.fuer_datei(&pfad);
                if regel.is_some_and(|(ordner, _)| unvollstaendig(ordner)) {
                    continue;
                }
                let grund = match grund_der_regel(&durchlauf.regeln, &pfad) {
                    Some(grund) => grund,
                    None => {
                        // Die Regel gilt weiter. Gibt es den Ordner hier?
                        let (wurzel, _) = regel.expect("ohne Regel gäbe es einen Grund");
                        let eltern = pfad.rsplit_once('/').map(|(vorn, _)| vorn).unwrap_or("");
                        if fehlend(wurzel) {
                            *ohne_ordner.entry(wurzel.to_string()).or_default() += 1;
                            continue;
                        }
                        if !durchlauf.ordner.contains(eltern) {
                            // der oberste Ordner unter der Regel, den es nicht gibt
                            let mut oberster = eltern;
                            while let Some((vorn, _)) = oberster.rsplit_once('/') {
                                if vorn.len() < wurzel.len() || durchlauf.ordner.contains(vorn) {
                                    break;
                                }
                                oberster = vorn;
                            }
                            *ohne_ordner.entry(oberster.to_string()).or_default() += 1;
                            continue;
                        }
                        Grund::Fehlt
                    }
                };
                if !nur_aufnehmen && bleibt(&pfad) {
                    e.behalten += 1;
                    continue;
                }
                if grund != Grund::Fehlt {
                    e.entfernt_regel += 1;
                    e.entfernt_regel_seiten += seiten.unwrap_or(0) as u64;
                }
                if e.betroffen.len() < BETROFFEN_MAX {
                    e.betroffen.push(Betroffen {
                        pfad,
                        grund,
                        seiten: seiten.unwrap_or(0) as u64,
                        zuletzt,
                        geraet: zuletzt_geraet,
                    });
                }
                if nur_aufnehmen {
                    e.entfernt += 1;
                } else {
                    e.entfernt += entfernen.execute([id])? as u64;
                    geschrieben = true;
                }
            }
            // Fehlende Ordner mit Regel stehen auch ohne Bestand in der Liste
            for ordner in &durchlauf.fehlende_verzeichnisse {
                ohne_ordner.entry(ordner.clone()).or_default();
            }

            // Hat dieses Gerät den Ordner früher gesehen, wird er vermisst.
            // Sonst gehört er zu einem anderen Gerät.
            let mut gesehen_von = tx.prepare(
                "SELECT g.id, g.name FROM ordner_gesehen o JOIN geraet g ON g.id = o.geraet
                 WHERE o.ordner = ?1 ORDER BY g.name",
            )?;
            for (ordner, dateien) in ohne_ordner {
                let geraete: Vec<(i64, String)> =
                    gesehen_von.query_map([&ordner], |z| Ok((z.get(0)?, z.get(1)?)))?.collect::<Result<_, _>>()?;
                let hier = geraete.iter().any(|(id, _)| *id == geraet);
                let bestand = OrdnerBestand { ordner, dateien, geraete: geraete.into_iter().map(|(_, n)| n).collect() };
                if hier {
                    e.vermisst.push(bestand);
                } else if bestand.dateien > 0 || !bestand.geraete.is_empty() {
                    e.fremd.push(bestand);
                } else {
                    // Regel für einen Ordner, den noch kein Gerät gelesen hat
                    e.vermisst.push(bestand);
                }
            }

            // Dieses Gerät hat die betretenen Ordner jetzt gesehen. Nur neue
            // Ordner werden eingetragen (v0.0.9), `zuletzt` ist der Zeitpunkt
            // des ersten Sehens.
            let mut merken =
                tx.prepare("INSERT OR IGNORE INTO ordner_gesehen (ordner, geraet, zuletzt) VALUES (?1, ?2, ?3)")?;
            let zeit = jetzt();
            for ordner in &durchlauf.ordner {
                if merken.execute(params![ordner, geraet, zeit])? > 0 {
                    geschrieben = true;
                }
            }
        }

        e.betroffen.sort_by(|a, b| a.pfad.cmp(&b.pfad));
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
        let delta = if mit_delta {
            Some(crate::ablauf::delta_lesen(&tx, &gesehen, &neu_ids, &geaendert_ids)?)
        } else {
            None
        };
        // Ohne Änderung bleibt das Register unberührt, auch ohne Eintrag des
        // Durchlaufs (v0.0.9). Die Dropbox hat dann nichts abzugleichen.
        if probe || !geschrieben {
            tx.rollback()?;
        } else {
            tx.commit()?;
        }
        e.dauer_ms = start.elapsed().as_millis() as u64;
        Ok((e, delta))
    }

    /// Begrenzt die Schritte auf diese Dateien, `None` hebt das auf.
    pub fn auswahl_setzen(&mut self, auswahl: Option<std::collections::HashSet<i64>>) {
        self.auswahl = auswahl;
    }

    pub(crate) fn in_auswahl(&self, id: i64) -> bool {
        self.auswahl.as_ref().is_none_or(|a| a.contains(&id))
    }

    /// Nach dem Zurückstellen auf "nur online" (v0.0.8). Größe und Datum
    /// bleiben: Die Datei ist inhaltlich unverändert.
    pub fn ausgelagert_eintragen(&mut self, id: i64) -> Ergebnis<()> {
        self.db.execute("UPDATE datei SET nur_online = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Vermerkt, dass Registerwerk diese Dateien auf dem Gerät `geraet` aus
    /// der Cloud angefordert hat (v0.1.2). Der Vermerk bleibt, bis die Datei
    /// wieder "nur online" liegt, auch über das Ende des Laufs hinaus.
    pub fn angefordert_eintragen(&mut self, ids: &[i64], geraet: &str) -> Ergebnis<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        {
            let mut eintragen =
                tx.prepare("INSERT OR IGNORE INTO angefordert (datei, geraet, zeit) VALUES (?1, ?2, ?3)")?;
            let zeit = jetzt();
            for id in ids {
                eintragen.execute(params![id, geraet, zeit])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Löscht den Vermerk "angefordert" einer Datei auf dem Gerät `geraet`.
    pub fn angefordert_loeschen(&mut self, id: i64, geraet: &str) -> Ergebnis<()> {
        self.db.execute("DELETE FROM angefordert WHERE datei = ?1 AND geraet = ?2", params![id, geraet])?;
        Ok(())
    }

    /// Nach dem Laden aus der Cloud (v0.0.8): Größe und Änderungsdatum der
    /// jetzt lokalen Datei, nicht mehr "nur online".
    pub fn geladen_eintragen(&mut self, id: i64, groesse: u64, geaendert: i64) -> Ergebnis<()> {
        self.db.execute(
            "UPDATE datei SET groesse = ?2, geaendert = ?3, nur_online = 0 WHERE id = ?1",
            params![id, groesse as i64, geaendert],
        )?;
        Ok(())
    }

    /// Entfernt Text, erkannten Text und Vorschauen einer Datei, deren Ordner
    /// auf "nur Metadaten" steht. Liefert, ob es etwas zu entfernen gab.
    /// Mit `nur_pruefen` (Probe) wird nichts entfernt.
    fn inhalt_entfernen(tx: &rusqlite::Transaction, id: i64, nur_pruefen: bool) -> Ergebnis<bool> {
        let (name, geaendert, gelesen): (String, i64, bool) = tx.query_row(
            "SELECT name, geaendert,
                    inhalt_verfahren IS NOT NULL OR vorschau_verfahren IS NOT NULL
                    OR EXISTS (SELECT 1 FROM vorschau v WHERE v.datei = datei.id)
             FROM datei WHERE id = ?1",
            [id],
            |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?)),
        )?;
        if !gelesen || nur_pruefen {
            return Ok(gelesen);
        }
        tx.execute("DELETE FROM seite WHERE datei = ?1", [id])?;
        tx.execute("DELETE FROM ocr WHERE datei = ?1", [id])?;
        tx.execute("DELETE FROM vorschau WHERE datei = ?1", [id])?;
        // Das Dokumentdatum darf nicht mehr aus dem Inhalt der PDF stammen
        let (datum, quelle) = dokumentdatum(&name, None, geaendert);
        tx.execute(
            "UPDATE datei SET inhalt_geaendert = NULL, inhalt_groesse = NULL, inhalt_verfahren = NULL,
                              inhalt_fehler = NULL, seiten = NULL, pdf_erstellt = NULL,
                              vorschau_geaendert = NULL, vorschau_groesse = NULL, vorschau_verfahren = NULL,
                              vorschau_fehler = NULL,
                              ocr_geaendert = NULL, ocr_groesse = NULL, ocr_verfahren = NULL, ocr_fehler = NULL,
                              textebene_fehler = NULL, datum_dokument = ?2, datum_quelle = ?3
             WHERE id = ?1",
            params![id, datum, quelle],
        )?;
        Ok(true)
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
        let textebene = self.textebene_stand()?;

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
            textebene,
            typen,
            jahre,
            letzter_lauf,
        })
    }

    /// Bedingung für lesbare Dateien (`formate.rs`), deren Inhalt (neu) gelesen werden muss.
    pub(crate) const AUSSTEHEND: &'static str = concat!("stufe = 1 AND endung IN ", crate::lesbar_sql!(), " AND (
            inhalt_verfahren IS NULL OR inhalt_verfahren < ?1
            OR inhalt_geaendert IS NOT geaendert
            OR (groesse IS NOT NULL AND inhalt_groesse IS NOT groesse))");

    /// Lesbare Dateien (PDF, Office, Text), deren Inhalt noch nicht oder nicht zum aktuellen Stand gelesen ist.
    pub fn ausstehend(&self) -> Ergebnis<Vec<Ausstehend>> {
        let sql = format!("SELECT id, pfad, nur_online FROM datei WHERE {} ORDER BY pfad", Self::AUSSTEHEND);
        let liste = self
            .db
            .prepare(&sql)?
            .query_map([INHALT_VERFAHREN], |z| {
                Ok(Ausstehend { id: z.get(0)?, pfad: z.get(1)?, nur_online: z.get(2)? })
            })?
            .filter(|a| a.as_ref().map_or(true, |a| self.in_auswahl(a.id)))
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
            "UPDATE datei SET ocr_geaendert = NULL, ocr_groesse = NULL, ocr_verfahren = NULL, ocr_fehler = NULL,
                              textebene_fehler = NULL
             WHERE id = ?1",
            [id],
        )?;
        let hinweis = inhalt.as_ref().ok().and_then(|i| i.hinweis);
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
                              inhalt_fehler = ?5, seiten = ?6, pdf_erstellt = ?7, inhalt_hinweis = ?8
             WHERE id = ?1",
            params![id, geaendert, groesse as i64, INHALT_VERFAHREN, fehler, seiten, erstellt, hinweis],
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

    /// Zahl der Dateien im Register je Ordner (samt Unterordnern), in der
    /// Reihenfolge von `ordner`. Der leere Pfad zählt alle. Für den Ordnerbaum
    /// der Regeln.
    pub fn dateien_je_ordner(&self, ordner: &[String]) -> Ergebnis<Vec<u64>> {
        let mut alle = self.db.prepare_cached("SELECT count(*) FROM datei")?;
        // "0" folgt auf "/": der Bereich umfasst genau die Pfade unter dem Ordner
        let mut unter = self.db.prepare_cached("SELECT count(*) FROM datei WHERE pfad >= ?1 AND pfad < ?2")?;
        ordner
            .iter()
            .map(|o| {
                let zahl: i64 = if o.is_empty() {
                    alle.query_row([], |z| z.get(0))?
                } else {
                    unter.query_row([format!("{o}/"), format!("{o}0")], |z| z.get(0))?
                };
                Ok(zahl as u64)
            })
            .collect()
    }

    /// Entfernt den Bestand eines Ordners samt Unterordnern aus dem Register,
    /// auch das Wissen, welches Gerät ihn gesehen hat. Für Ordner, die es
    /// nicht mehr gibt. Liefert die Zahl der entfernten Dateien.
    pub fn ordner_entfernen(&mut self, ordner: &str) -> Ergebnis<u64> {
        if ordner.is_empty() {
            return Ok(0);
        }
        let tx = self.db.transaction()?;
        let (von, bis) = (format!("{ordner}/"), format!("{ordner}0"));
        let dateien = tx.execute("DELETE FROM datei WHERE pfad >= ?1 AND pfad < ?2", params![von, bis])? as u64;
        tx.execute(
            "DELETE FROM ordner_gesehen WHERE ordner = ?1 OR (ordner >= ?2 AND ordner < ?3)",
            params![ordner, von, bis],
        )?;
        tx.commit()?;
        Ok(dateien)
    }

    /// Geräte, die einen Ordner gesehen haben, je Ordner in der Reihenfolge
    /// von `ordner`. Für den Ordnerbaum.
    pub fn geraete_je_ordner(&self, ordner: &[String]) -> Ergebnis<Vec<Vec<String>>> {
        let mut sql = self.db.prepare_cached(
            "SELECT g.name FROM ordner_gesehen o JOIN geraet g ON g.id = o.geraet WHERE o.ordner = ?1 ORDER BY g.name",
        )?;
        ordner
            .iter()
            .map(|o| Ok(sql.query_map([o], |z| z.get(0))?.collect::<Result<Vec<String>, _>>()?))
            .collect()
    }

    /// Ein Merkmal des Registers, etwa eine erledigte einmalige Umstellung.
    pub fn merkmal(&self, name: &str) -> Ergebnis<Option<String>> {
        Ok(self.db.query_row("SELECT wert FROM merkmal WHERE name = ?1", [name], |z| z.get(0)).optional()?)
    }

    /// Die Geräte, die dieses Register kennt, mit dem Zeitpunkt ihres letzten
    /// Durchlaufs (Sekunden seit 1970), das zuletzt tätige zuerst.
    pub fn geraete(&self) -> Ergebnis<Vec<(String, Option<i64>)>> {
        let liste = self
            .db
            .prepare(
                "SELECT g.name, max(l.begonnen) FROM geraet g LEFT JOIN lauf l ON l.geraet = g.id
                 GROUP BY g.id ORDER BY 2 DESC, g.name",
            )?
            .query_map([], |z| Ok((z.get(0)?, z.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    /// Namen der Äste, die irgendein Gerät gesehen hat. Ein Ast, den es auf
    /// diesem Gerät nicht gibt, steht so trotzdem im Ordnerbaum.
    pub fn gesehene_aeste(&self) -> Ergebnis<Vec<String>> {
        let liste = self
            .db
            .prepare("SELECT DISTINCT ordner FROM ordner_gesehen WHERE instr(ordner, '/') = 0 ORDER BY ordner")?
            .query_map([], |z| z.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(liste)
    }

    /// Einmalige Umstellung auf Äste (v0.0.7): Bis dahin standen die Pfade
    /// relativ zum einen Cloud-Ordner im Register. Jeder Pfad bekommt den
    /// Namen des Astes davor, zu dem der Cloud-Ordner geworden ist. Text,
    /// Vorschauen und OCR bleiben, nur der Suchindex über Name und Ordner
    /// entsteht neu. Liefert, ob umgestellt wurde.
    pub fn pfade_voranstellen(&mut self, ast: &str) -> Ergebnis<bool> {
        const MERKMAL: &str = "pfade_mit_ast";
        let tx = self.db.transaction()?;
        let schon: Option<String> =
            tx.query_row("SELECT wert FROM merkmal WHERE name = ?1", [MERKMAL], |z| z.get(0)).optional()?;
        if schon.is_some() {
            return Ok(false);
        }
        tx.execute("UPDATE datei SET pfad = ?1 || '/' || pfad, suche_stand = NULL", [ast])?;
        tx.execute("INSERT INTO merkmal (name, wert) VALUES (?1, ?2)", [MERKMAL, ast])?;
        tx.commit()?;
        Ok(true)
    }

    /// Ordner mit Dateien, deren Inhalt aussteht und die nur online liegen,
    /// nach Ordner. Registerwerk lädt sie nicht selbst: Die Liste zeigt, was
    /// in der Cloud offline verfügbar gemacht werden muss.
    pub fn nur_online_ordner(&self) -> Ergebnis<Vec<OnlineOrdner>> {
        let sql = format!(
            "SELECT substr(pfad, 1, max(length(pfad) - length(name) - 1, 0)) AS ordner, count(*)
             FROM datei WHERE nur_online AND {} GROUP BY ordner ORDER BY ordner",
            Self::AUSSTEHEND
        );
        let liste = self
            .db
            .prepare(&sql)?
            .query_map([INHALT_VERFAHREN], |z| Ok(OnlineOrdner { ordner: z.get(0)?, dateien: z.get::<_, i64>(1)? as u64 }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    /// Wie [`Register::nur_online_ordner`], aber als Zahl je Ordner samt
    /// Unterordnern, in der Reihenfolge von `ordner`. Für den Ordnerbaum.
    pub fn nur_online_je_ordner(&self, ordner: &[String]) -> Ergebnis<Vec<u64>> {
        let sql = format!(
            "SELECT count(*) FROM datei WHERE pfad >= ?2 AND pfad < ?3 AND nur_online AND {}",
            Self::AUSSTEHEND
        );
        let mut unter = self.db.prepare_cached(&sql)?;
        ordner
            .iter()
            .map(|o| {
                let zahl: i64 = unter.query_row(params![INHALT_VERFAHREN, format!("{o}/"), format!("{o}0")], |z| z.get(0))?;
                Ok(zahl as u64)
            })
            .collect()
    }

    /// Nur für Werkzeuge wie `pdf_pruefen`: führt SQL auf dem Register aus.
    pub fn sql_ausfuehren(&self, sql: &str) -> Ergebnis<()> {
        self.db.execute_batch(sql)?;
        Ok(())
    }

    /// Nur für Werkzeuge: eine Zahl aus der ersten Spalte der ersten Zeile.
    pub fn sql_zahl(&self, sql: &str) -> Ergebnis<i64> {
        Ok(self.db.query_row(sql, [], |z| z.get::<_, Option<i64>>(0))?.unwrap_or(0))
    }

    /// Nur für Werkzeuge: Zeilen aus Text und Zahl.
    pub fn sql_liste(&self, sql: &str) -> Ergebnis<Vec<(String, i64)>> {
        let liste = self
            .db
            .prepare(sql)?
            .query_map([], |z| Ok((z.get(0)?, z.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    fn inhalt_stand(&self) -> Ergebnis<InhaltStand> {
        let zahl = |z: &rusqlite::Row, i: usize| -> rusqlite::Result<u64> { Ok(z.get::<_, Option<i64>>(i)?.unwrap_or(0) as u64) };
        let mut s = InhaltStand::default();
        let sql = format!(
            "SELECT count(*), sum(NOT ({a})), sum({a}), sum(({a}) AND nur_online),
                    sum(NOT ({a}) AND inhalt_fehler IS NOT NULL)
             FROM datei WHERE stufe = 1 AND endung IN {l}",
            a = Self::AUSSTEHEND,
            l = crate::lesbar_sql!()
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
        s.hinweise = self.db.query_row("SELECT count(*) FROM datei WHERE inhalt_hinweis IS NOT NULL AND inhalt_fehler IS NULL", [], |z| zahl(z, 0))?;
        s.hinweis_liste = self
            .db
            .prepare(
                "SELECT pfad, inhalt_hinweis FROM datei WHERE inhalt_hinweis IS NOT NULL AND inhalt_fehler IS NULL
                 ORDER BY pfad LIMIT 50",
            )?
            .query_map([], |z| Ok(FehlerZeile { pfad: z.get(0)?, meldung: z.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        s.fehler_liste = self
            .db
            .prepare("SELECT pfad, inhalt_fehler FROM datei WHERE inhalt_fehler IS NOT NULL ORDER BY pfad LIMIT 50")?
            .query_map([], |z| Ok(FehlerZeile { pfad: z.get(0)?, meldung: z.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(s)
    }
}

/// Stellt das Journal auf das klassische Verfahren (`DELETE`) um (v0.0.9).
///
/// Bis v0.0.8 lief das Register im WAL-Journal. Dabei fasst schon jeder
/// lesende Zugriff die Dateien `-shm` und `-wal` an, und die Dropbox gleicht
/// sie ab. Mit dem klassischen Journal ändert Lesen keine Datei, nur beim
/// Schreiben entsteht kurz `register.db-journal`.
///
/// Das Umstellen braucht das Register für sich allein: Solange eine andere
/// Verbindung offen ist, gelingt es nicht. Nach `versuche` vergeblichen
/// Versuchen bleibt es beim alten Journal, ohne Fehler.
fn journal_umstellen(db: &Connection, versuche: u32) -> rusqlite::Result<()> {
    // Ist das Register gerade gesperrt, wird es diesmal nicht umgestellt
    match db.query_row("PRAGMA journal_mode", [], |z| z.get::<_, String>(0)) {
        Ok(modus) if modus.eq_ignore_ascii_case("wal") => {}
        Ok(_) => return Ok(()),
        Err(rusqlite::Error::SqliteFailure(f, _))
            if matches!(f.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) =>
        {
            return Ok(())
        }
        Err(f) => return Err(f),
    }
    for i in 0..versuche {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        match db.query_row("PRAGMA journal_mode = DELETE", [], |z| z.get::<_, String>(0)) {
            Ok(modus) if modus.eq_ignore_ascii_case("delete") => return Ok(()),
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(f, _))
                if matches!(f.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {}
            Err(f) => return Err(f),
        }
    }
    Ok(())
}

/// Fällt ein nicht mehr gesehener Eintrag wegen der Ordnerregeln aus dem
/// Register (und nicht, weil es die Datei nicht mehr gibt)? Ohne Blick ins
/// Dateisystem: Bei Ordnernamen wie "build" gilt die Regel als Grund, auch
/// wenn die Datei in Wahrheit gelöscht wurde. Das führt höchstens zu einer
/// Rückfrage mehr.
fn grund_der_regel(regeln: &Regeln, pfad: &str) -> Option<Grund> {
    let Some((ordner, stufe)) = regeln.fuer_datei(pfad) else { return Some(Grund::OhneRegel) };
    if pfad.rsplit('/').next().is_some_and(|name| name.starts_with('.')) {
        return Some(Grund::Versteckt);
    }
    match stufe {
        Stufe::Ausgeschlossen => return Some(Grund::Ausgeschlossen),
        _ => {}
    }
    // Ordner zwischen dem Ordner der Regel und der Datei
    let rest = pfad[ordner.len()..].trim_start_matches('/');
    let mut teile: Vec<&str> = rest.split('/').collect();
    teile.pop();
    teile.into_iter().any(dateibaum::name_wie_bibliothek).then_some(Grund::Bibliothek)
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
        DateiEintrag { pfad: pfad.into(), name, endung, groesse, geaendert, stufe: Stufe::Voll, nur_online: groesse == 0 }
    }

    fn durchlauf(dateien: Vec<DateiEintrag>) -> Durchlauf {
        // alle Ordner der gesehenen Dateien gibt es
        let mut ordner: std::collections::BTreeSet<String> = ["A".to_string()].into();
        for d in &dateien {
            let mut rest = d.pfad.as_str();
            while let Some((vorn, _)) = rest.rsplit_once('/') {
                ordner.insert(vorn.to_string());
                rest = vorn;
            }
        }
        Durchlauf {
            dateien,
            geraet: "Notebook".into(),
            ordner,
            regeln: Regeln::voll(&["A"]),
            gelesene_verzeichnisse: vec!["A".into()],
            ..Default::default()
        }
    }

    #[test]
    fn oeffnet_auch_waehrend_eine_andere_verbindung_schreibt() {
        let tmp = tempfile::tempdir().unwrap();
        let erste = Register::oeffnen(tmp.path()).unwrap();
        // wie die Prüfung beim Start: Schreibsperre über längere Zeit
        erste.db.execute_batch("BEGIN IMMEDIATE; INSERT INTO geraet (name) VALUES ('x')").unwrap();
        let start = Instant::now();
        let zweite = Register::oeffnen(tmp.path()).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(zweite.stand().unwrap().dateien, 0);
        erste.db.execute_batch("ROLLBACK").unwrap();
    }

    /// Inhalt aller Dateien im Ordner, nach Name
    fn dateien_im_ordner(ordner: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = std::fs::read_dir(ordner)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name().to_string_lossy().to_string(), std::fs::read(e.path()).unwrap())
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn erkennt_mehrere_anweisungen() {
        assert!(!mehrere_anweisungen("CREATE TABLE t (x TEXT)"));
        assert!(!mehrere_anweisungen("CREATE TABLE t (x TEXT);  \n"));
        assert!(!mehrere_anweisungen("CREATE TABLE t (x TEXT DEFAULT 'a;b', \"y;\" INT, [z;] INT, `w;` INT)"));
        assert!(!mehrere_anweisungen("CREATE TABLE t (x TEXT DEFAULT 'it''s; ok')"));
        assert!(!mehrere_anweisungen("CREATE TABLE t (\n -- Kommentar; mit Semikolon\n x TEXT /* auch; hier */)"));
        assert!(mehrere_anweisungen("CREATE TABLE t (x TEXT); CREATE TABLE boese(x)"));
        assert!(mehrere_anweisungen("CREATE TABLE t (x TEXT DEFAULT 'a'); ATTACH 'x.db' AS y"));
        assert!(mehrere_anweisungen("CREATE TABLE t (x TEXT) -- Kommentar\n; DROP TABLE t"));
        // Unvollständige Konstrukte am Ende laufen nicht über das Ende hinaus
        assert!(!mehrere_anweisungen("CREATE TABLE t (x TEXT DEFAULT 'offen"));
        assert!(!mehrere_anweisungen("CREATE TABLE t (x) /* offen"));
    }

    /// Eine ausgetauschte Datei mit angehängtem SQL in der gespeicherten
    /// Definition einer Tabelle (Finding S2-5ef6f419, v0.1.4): Die Probe
    /// bricht ab, das angehängte SQL läuft nicht.
    #[test]
    fn probe_fuehrt_kein_angehaengtes_sql_aus() {
        let tmp = tempfile::tempdir().unwrap();
        let d = durchlauf(vec![datei("A/a.pdf", 10, 1)]);
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&d).unwrap();
            r.db.execute_batch(
                "PRAGMA writable_schema = ON;
                 UPDATE sqlite_schema SET sql = sql || '; CREATE TABLE boese(x)' WHERE type = 'table' AND name = 'geraet';
                 PRAGMA writable_schema = OFF;",
            )
            .unwrap();
        }
        let mut r = Register::oeffnen(tmp.path()).unwrap();
        assert!(matches!(r.abgleich_pruefen(&d), Err(crate::Fehler::Konfiguration(m)) if m.contains("geraet")));
        let boese: i64 = r
            .db
            .query_row(
                "SELECT (SELECT count(*) FROM main.sqlite_schema WHERE name = 'boese')
                      + (SELECT count(*) FROM temp.sqlite_schema WHERE name = 'boese')",
                [],
                |z| z.get(0),
            )
            .unwrap();
        assert_eq!(boese, 0);
        // Ohne Manipulation läuft die Probe wie bisher
        let tmp2 = tempfile::tempdir().unwrap();
        let mut gut = Register::oeffnen(tmp2.path()).unwrap();
        gut.abgleichen(&d).unwrap();
        gut.abgleich_pruefen(&d).unwrap();
    }

    #[test]
    fn lesen_und_probe_aendern_keine_datei() {
        let tmp = tempfile::tempdir().unwrap();
        let dateien: Vec<_> = (0..3_000).map(|i| datei(&format!("A/{i:05} beleg.pdf"), 10, 1)).collect();
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(dateien.clone())).unwrap();
        }
        let vorher = dateien_im_ordner(tmp.path());
        // nur register.db, kein Journal
        assert_eq!(vorher.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), vec![DATEINAME]);

        let mut r = Register::oeffnen_lesend(tmp.path()).unwrap();
        let mut neu = dateien.clone();
        neu.push(datei("A/neu.pdf", 10, 1));
        let d = r.delta_pruefen(&durchlauf(neu)).unwrap();
        assert_eq!(d.lauf.neu, 1);
        assert_eq!(d.dateien.len(), 3_001);
        assert_eq!(r.stand().unwrap().dateien, 3_000);
        // Schreiben verweigert SQLite
        assert!(r.abgleichen(&durchlauf(vec![])).is_err());
        drop(r);
        assert!(dateien_im_ordner(tmp.path()) == vorher);

        // Auch ein echter Abgleich ohne Änderung lässt die Datei unberührt
        let mut r = Register::oeffnen(tmp.path()).unwrap();
        let e = r.abgleichen(&durchlauf(dateien.clone())).unwrap();
        assert_eq!((e.neu, e.geaendert, e.entfernt), (0, 0, 0));
        r.aufnehmen(&durchlauf(dateien.clone())).unwrap();
        drop(r);
        assert!(dateien_im_ordner(tmp.path()) == vorher);

        // Eine neue Datei ändert das Register
        let mut r = Register::oeffnen(tmp.path()).unwrap();
        let mut neu = dateien.clone();
        neu.push(datei("A/neu.pdf", 10, 1));
        assert_eq!(r.abgleichen(&durchlauf(neu)).unwrap().neu, 1);
        drop(r);
        assert!(dateien_im_ordner(tmp.path()) != vorher);
    }

    #[test]
    fn legt_ein_leeres_register_an_und_laesst_ein_vorhandenes_unberuehrt() {
        let tmp = tempfile::tempdir().unwrap();
        // Vorher gelingt das Öffnen zum Lesen nicht, wie bei der Prüfung
        assert!(Register::oeffnen_lesend(tmp.path()).is_err());
        assert!(Register::anlegen(tmp.path()).unwrap());
        let mut r = Register::oeffnen_lesend(tmp.path()).unwrap();
        assert_eq!(r.stand().unwrap().dateien, 0);
        let d = r.delta_pruefen(&durchlauf(vec![datei("A/neu.pdf", 10, 1)])).unwrap();
        assert_eq!(d.lauf.neu, 1);
        drop(r);

        // Ein vorhandenes Register wird nicht angefasst
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(vec![datei("A/beleg.pdf", 10, 1)])).unwrap();
        }
        let vorher = dateien_im_ordner(tmp.path());
        assert!(!Register::anlegen(tmp.path()).unwrap());
        assert!(dateien_im_ordner(tmp.path()) == vorher);
    }

    #[test]
    fn legt_auch_den_ordner_an() {
        let tmp = tempfile::tempdir().unwrap();
        let ordner = tmp.path().join("neu");
        assert!(Register::anlegen(&ordner).unwrap());
        assert!(ordner.join(DATEINAME).is_file());
    }

    #[test]
    fn stellt_ein_wal_register_auf_das_klassische_journal_um() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let r = Register::oeffnen(tmp.path()).unwrap();
            let modus: String = r.db.query_row("PRAGMA journal_mode = WAL", [], |z| z.get(0)).unwrap();
            assert_eq!(modus, "wal");
        }
        // Solange eine andere Verbindung offen ist, bleibt es beim WAL, ohne Wartezeit
        let leser = Register::oeffnen_lesend(tmp.path()).unwrap();
        let start = Instant::now();
        drop(Register::oeffnen(tmp.path()).unwrap());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert!(!Register::journal_vorbereiten(tmp.path()).unwrap());
        drop(leser);
        // Allein gelingt es
        assert!(Register::journal_vorbereiten(tmp.path()).unwrap());
        let r = Register::oeffnen_lesend(tmp.path()).unwrap();
        let modus: String = r.db.query_row("PRAGMA journal_mode", [], |z| z.get(0)).unwrap();
        assert_eq!(modus, "delete");
        drop(r);
        let namen: Vec<_> = dateien_im_ordner(tmp.path()).into_iter().map(|(n, _)| n).collect();
        assert_eq!(namen, vec![DATEINAME.to_string()]);
    }

    #[test]
    fn delta_ohne_dateien_in_vermissten_ordnern() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![datei("A/da.pdf", 10, 1), datei("A/weg/log.pdf", 0, 1)])).unwrap();
        // Der Ordner "A/weg" ist gelöscht: Sein Bestand bleibt, steht aber nicht zur Verarbeitung an
        let d = r.delta_pruefen(&durchlauf(vec![datei("A/da.pdf", 10, 1)])).unwrap();
        let pfade: Vec<_> = d.dateien.iter().map(|x| x.pfad.as_str()).collect();
        assert_eq!(pfade, vec!["A/da.pdf"]);
        assert_eq!(d.lauf.vermisst.len(), 1);
        r.aufnehmen(&durchlauf(vec![datei("A/da.pdf", 10, 1)])).unwrap();
        let gesehen: std::collections::HashSet<&str> = ["A/da.pdf"].into();
        let (auswahl, online) = r.auswahl_bilden(&[], false, Some(&gesehen)).unwrap();
        assert_eq!((auswahl.len(), online.len()), (1, 0));
        // ohne Angabe auch die Datei im vermissten Ordner
        assert_eq!(r.auswahl_bilden(&[], false, None).unwrap().1.len(), 1);
        assert_eq!(r.stand().unwrap().dateien, 2);
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
        d.regeln = Regeln::voll(&["A", "AB", "B"]);
        d.gelesene_verzeichnisse = vec!["A".into(), "AB".into(), "B".into()];
        r.abgleichen(&d).unwrap();

        // B fehlt auf diesem Gerät, A ist leer gelesen. "AB" darf nicht als Teil von "A" gelten.
        let mut d = durchlauf(vec![datei("AB/x.pdf", 1, 1)]);
        d.regeln = Regeln::voll(&["A", "AB", "B"]);
        d.gelesene_verzeichnisse = vec!["A".into(), "AB".into()];
        d.fehlende_verzeichnisse = vec!["B".into()];
        // a.pdf gibt es nicht mehr: Auch das wird erst nach Bestätigung entfernt
        let e = r.abgleich_pruefen(&d).unwrap();
        assert_eq!((e.entfernt, e.entfernt_regel, e.bestaetigung_noetig()), (1, 0, true));
        let b = &e.betroffen[0];
        // Wann und wo zuletzt gesehen, ist seit v0.0.9 nicht mehr bekannt
        assert_eq!((b.pfad.as_str(), b.grund, b.geraet.as_deref(), b.zuletzt), ("A/a.pdf", Grund::Fehlt, None, None));
        assert_eq!(r.stand().unwrap().dateien, 3);
        let e = r.abgleichen(&d).unwrap();
        assert_eq!(e.entfernt, 1);
        assert_eq!(r.stand().unwrap().dateien, 2);
        // B hat dieses Gerät früher gesehen: vermisst, aber nicht entfernt
        assert_eq!(e.vermisst, vec![OrdnerBestand { ordner: "B".into(), dateien: 1, geraete: vec!["Notebook".into()] }]);
        assert!(e.fremd.is_empty());

        // Ein Ordner mit Lesefehlern behält seinen Bestand, auch wenn ein
        // Unterordner mit eigener Regel vollständig gelesen wurde
        let mut d = durchlauf(vec![]);
        d.ordner.insert("AB".into());
        d.regeln = Regeln::voll(&["AB", "B", "B/C"]);
        d.unvollstaendige_verzeichnisse = vec!["B".into()];
        d.gelesene_verzeichnisse = vec!["AB".into(), "B/C".into()];
        let e = r.abgleichen(&d).unwrap();
        assert_eq!(e.entfernt, 1);
        assert_eq!(r.sql_liste("SELECT pfad, 0 FROM datei").unwrap(), vec![("B/b.pdf".to_string(), 0)]);
    }

    #[test]
    fn fehlende_ordner_gehoeren_anderen_geraeten_oder_werden_vermisst() {
        let mut r = Register::im_speicher().unwrap();
        // Der Mac mini liest zwei Äste, einer davon mit Unterordnern
        let mut mini = durchlauf(vec![
            datei("A/a.pdf", 1, 1),
            datei("A/2019/b.pdf", 1, 1),
            datei("A/2019/Tief/c.pdf", 1, 1),
            datei("Stick/s.pdf", 1, 1),
        ]);
        mini.geraet = "Mac mini".into();
        mini.ordner.insert("Stick".into());
        mini.regeln = Regeln::voll(&["A", "Stick"]);
        r.abgleichen(&mini).unwrap();

        // Das Notebook hat den Ast "Stick" nicht, und "A/2019" ist dort nicht
        // synchronisiert. Beides hat es nie gesehen: fremd, nichts zu entfernen.
        let mut d = durchlauf(vec![datei("A/a.pdf", 1, 1)]);
        d.regeln = Regeln::voll(&["A", "Stick"]);
        d.fehlende_verzeichnisse = vec!["Stick".into()];
        let e = r.abgleichen(&d).unwrap();
        assert_eq!((e.entfernt, e.bestaetigung_noetig()), (0, false));
        assert!(e.vermisst.is_empty());
        let fremd: Vec<_> = e.fremd.iter().map(|f| (f.ordner.as_str(), f.dateien, f.geraete.join(","))).collect();
        assert_eq!(fremd, vec![("A/2019", 2, "Mac mini".to_string()), ("Stick", 1, "Mac mini".to_string())]);
        assert_eq!(r.stand().unwrap().dateien, 4);
        assert_eq!(
            r.geraete_je_ordner(&["A".to_string(), "Stick".to_string(), "X".to_string()]).unwrap(),
            vec![vec!["Mac mini".to_string(), "Notebook".to_string()], vec!["Mac mini".to_string()], vec![]]
        );

        // Auf dem Mac mini selbst fehlt "A/2019" später: vermisst, weil er
        // den Ordner kannte. Der Bestand bleibt bis zur Entscheidung.
        let mut mini2 = durchlauf(vec![datei("A/a.pdf", 1, 1), datei("Stick/s.pdf", 1, 1)]);
        mini2.geraet = "Mac mini".into();
        mini2.ordner.insert("Stick".into());
        mini2.regeln = Regeln::voll(&["A", "Stick"]);
        let e = r.abgleichen(&mini2).unwrap();
        assert_eq!(e.entfernt, 0);
        assert_eq!(
            e.vermisst,
            vec![OrdnerBestand { ordner: "A/2019".into(), dateien: 2, geraete: vec!["Mac mini".into()] }]
        );
        assert!(e.fremd.is_empty());

        // Ausdrücklich entfernen: Dateien und das Wissen über den Ordner
        assert_eq!(r.ordner_entfernen("A/2019").unwrap(), 2);
        let e = r.abgleichen(&mini2).unwrap();
        assert!(e.vermisst.is_empty() && e.fremd.is_empty());
        assert_eq!(r.stand().unwrap().dateien, 2);
        assert_eq!(r.ordner_entfernen("").unwrap(), 0);
        assert_eq!(r.gesehene_aeste().unwrap(), vec!["A", "Stick"]);
        let geraete: Vec<String> = r.geraete().unwrap().into_iter().map(|g| g.0).collect();
        assert_eq!(geraete.len(), 2);
        assert!(geraete.contains(&"Mac mini".to_string()) && geraete.contains(&"Notebook".to_string()));
    }

    #[test]
    fn stellt_pfade_auf_aeste_um() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![datei("A/a.pdf", 1, 1), datei("A/x/b.pdf", 1, 1)])).unwrap();
        assert!(r.pfade_voranstellen("Dropbox").unwrap());
        // nur einmal
        assert!(!r.pfade_voranstellen("Dropbox").unwrap());
        let pfade: Vec<String> = r.sql_liste("SELECT pfad, 0 FROM datei ORDER BY pfad").unwrap().into_iter().map(|p| p.0).collect();
        assert_eq!(pfade, vec!["Dropbox/A/a.pdf", "Dropbox/A/x/b.pdf"]);
        assert_eq!(r.merkmal("pfade_mit_ast").unwrap().as_deref(), Some("Dropbox"));
        assert_eq!(r.sql_zahl("SELECT count(*) FROM datei WHERE suche_stand IS NOT NULL").unwrap(), 0);
    }

    fn seite(r: &mut Register, pfad: &str) -> i64 {
        let id: i64 = r.db.query_row("SELECT id FROM datei WHERE pfad = ?1", [pfad], |z| z.get(0)).unwrap();
        let inhalt = pdf::Inhalt {
            seiten: vec![pdf::Seite { nr: 1, text: "Rechnung über zwölf Euro".into(), bildanteil: 0.0 }],
            erstellt: Some("D:20190304120000".into()),
            hinweis: None,
        };
        r.inhalt_speichern(id, 100, 10, &Ok(inhalt)).unwrap();
        id
    }

    #[test]
    fn regeln_entfernen_erst_nach_pruefung() {
        let mut r = Register::im_speicher().unwrap();
        let mut d = durchlauf(vec![datei("A/a.pdf", 10, 100), datei("A/x/b.pdf", 10, 100), datei("B/c.pdf", 10, 100)]);
        d.regeln = Regeln::voll(&["A", "B"]);
        r.abgleichen(&d).unwrap();
        seite(&mut r, "A/x/b.pdf");
        seite(&mut r, "B/c.pdf");

        // A/x wird ausgeschlossen, B hat keine Regel mehr
        let mut regeln = Regeln::voll(&["A"]);
        regeln.setzen("A/x", Some(Stufe::Ausgeschlossen)).unwrap();
        let mut d = durchlauf(vec![datei("A/a.pdf", 10, 100)]);
        d.regeln = regeln;
        let e = r.abgleich_pruefen(&d).unwrap();
        assert!(e.probe && e.bestaetigung_noetig());
        assert_eq!((e.entfernt, e.entfernt_regel, e.entfernt_regel_seiten), (2, 2, 2));
        // die betroffenen Dateien mit Grund, nach Pfad
        let liste: Vec<_> = e.betroffen.iter().map(|b| (b.pfad.as_str(), b.grund, b.seiten)).collect();
        assert_eq!(liste, vec![("A/x/b.pdf", Grund::Ausgeschlossen, 1), ("B/c.pdf", Grund::OhneRegel, 1)]);
        // die Prüfung ändert nichts
        assert_eq!(r.stand().unwrap().dateien, 3);
        assert_eq!(r.stand().unwrap().letzter_lauf.unwrap().lauf, 1);

        let e = r.abgleichen(&d).unwrap();
        assert_eq!((e.probe, e.entfernt_regel), (false, 2));
        assert_eq!(r.stand().unwrap().dateien, 1);
    }

    #[test]
    fn nennt_ordner_mit_nur_online_dateien() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![
            datei("A/2019/a.pdf", 0, 1),
            datei("A/2019/b.pdf", 0, 1),
            datei("A/2020/c.pdf", 0, 1),
            datei("A/2020/lokal.pdf", 10, 1),
            datei("A/2020/alt.zip", 0, 1),
            datei("A/d.pdf", 0, 1),
        ]))
        .unwrap();
        let liste: Vec<_> = r.nur_online_ordner().unwrap().into_iter().map(|o| (o.ordner, o.dateien)).collect();
        // zip wird nicht gelesen und steht deshalb nicht aus
        assert_eq!(liste, vec![("A".to_string(), 1), ("A/2019".to_string(), 2), ("A/2020".to_string(), 1)]);
        let ordner: Vec<String> = ["A", "A/2019", "A/2020", "B"].iter().map(|s| s.to_string()).collect();
        assert_eq!(r.nur_online_je_ordner(&ordner).unwrap(), vec![4, 2, 1, 0]);
        // Vorschauen: dieselben PDFs, zip bekommt keine
        let v: Vec<_> = r.nur_online_ordner_vorschau().unwrap().into_iter().map(|o| (o.ordner, o.dateien)).collect();
        assert_eq!(v, vec![("A".to_string(), 1), ("A/2019".to_string(), 2), ("A/2020".to_string(), 1)]);
    }

    #[test]
    fn zaehlt_dateien_je_ordner() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![datei("A/a.pdf", 1, 1), datei("A/B/b.pdf", 1, 1), datei("AB/c.pdf", 1, 1)])).unwrap();
        let ordner: Vec<String> = ["", "A", "A/B", "AB", "X"].iter().map(|s| s.to_string()).collect();
        assert_eq!(r.dateien_je_ordner(&ordner).unwrap(), vec![3, 2, 1, 1, 0]);
    }

    #[test]
    fn grund_des_entfernens() {
        let mut regeln = Regeln::voll(&["A"]);
        regeln.setzen("A/x", Some(Stufe::Ausgeschlossen)).unwrap();
        regeln.setzen("M", Some(Stufe::Metadaten)).unwrap();
        assert_eq!(grund_der_regel(&regeln, "A/a.pdf"), None);
        assert_eq!(grund_der_regel(&regeln, "A/tief/unten/a.zip"), None);
        assert_eq!(grund_der_regel(&regeln, "A/x/a.pdf"), Some(Grund::Ausgeschlossen));
        assert_eq!(grund_der_regel(&regeln, "B/a.pdf"), Some(Grund::OhneRegel));
        // Quelltext fällt seit v0.1.5 nicht mehr wegen seines Typs heraus,
        // versteckte Dateien schon
        assert_eq!(grund_der_regel(&regeln, "A/main.rs"), None);
        assert_eq!(grund_der_regel(&regeln, "M/.localized"), Some(Grund::Versteckt));
        assert_eq!(grund_der_regel(&regeln, "A/p/node_modules/q/a.md"), Some(Grund::Bibliothek));
        assert_eq!(grund_der_regel(&regeln, "A/p/.git/a.txt"), Some(Grund::Bibliothek));
        assert_eq!(grund_der_regel(&regeln, "M/lied.xyz"), None);
        // eine Datei, die so heißt wie eine Bibliothek, ist keine
        assert_eq!(grund_der_regel(&regeln, "A/build"), None);
    }

    #[test]
    fn nur_metadaten_entfernt_text_und_vorschauen() {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(vec![datei("A/buch.pdf", 10, 100), datei("A/leer.pdf", 10, 100)])).unwrap();
        let id = seite(&mut r, "A/buch.pdf");
        assert_eq!(r.stand().unwrap().datum_pdf, 1);
        assert_eq!(r.ausstehend().unwrap().len(), 1);

        let mut d = durchlauf(vec![datei("A/buch.pdf", 10, 100), datei("A/leer.pdf", 10, 100)]);
        d.regeln.setzen("A", Some(Stufe::Metadaten)).unwrap();
        for e in &mut d.dateien {
            e.stufe = Stufe::Metadaten;
        }
        let e = r.abgleich_pruefen(&d).unwrap();
        assert_eq!((e.reduziert, e.reduziert_seiten, e.entfernt, e.bestaetigung_noetig()), (1, 1, 0, true));
        assert_eq!(e.betroffen, vec![Betroffen { pfad: "A/buch.pdf".into(), grund: Grund::NurMetadaten, seiten: 1, zuletzt: None, geraet: None }]);
        assert_eq!(r.stand().unwrap().inhalt.seiten, 1);
        r.abgleichen(&d).unwrap();
        let s = r.stand().unwrap();
        // Die Dateien bleiben mit Namen, Inhalte werden nicht mehr gelesen
        assert_eq!((s.dateien, s.inhalt.seiten, s.inhalt.pdfs, s.vorschau.pdfs, s.datum_pdf), (2, 0, 0, 0, 0));
        assert!(r.ausstehend().unwrap().is_empty() && r.vorschau_ausstehend().unwrap().is_empty());
        // ein zweiter Lauf hat nichts mehr zu entfernen
        assert!(!r.abgleichen(&d).unwrap().bestaetigung_noetig());

        // zurück auf Voll: Inhalte stehen wieder aus
        let d = durchlauf(vec![datei("A/buch.pdf", 10, 100), datei("A/leer.pdf", 10, 100)]);
        assert!(!r.abgleichen(&d).unwrap().bestaetigung_noetig());
        assert_eq!(r.ausstehend().unwrap().len(), 2);
        let _ = id;
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
        // Schema 8: Der Bestand gilt als voll verarbeitet
        assert_eq!(r.sql_zahl("SELECT count(*) FROM datei WHERE stufe = 1").unwrap(), 2);
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
