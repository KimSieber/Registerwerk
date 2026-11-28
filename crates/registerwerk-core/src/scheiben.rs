//! Vorschauen im Register: Scheiben anlegen, Vorschauen speichern, lesen und
//! aufräumen.
//!
//! Eine Scheibe ist eine eigene SQLite-Datei mit den AVIF-Bildern. Die
//! Index-Datenbank hält je Seite einen Verweis (Scheibe und Schlüssel).
//! Geschrieben wird erst die Scheibe, dann der Verweis. Bricht das Programm
//! dazwischen ab, bleibt höchstens ein Bild ohne Verweis zurück. Das stört
//! nicht und verschwindet mit dem nächsten Neuaufbau.

use std::cell::RefMut;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::inhalte::Fortschritt;
use crate::register::{Ausstehend, FehlerZeile, Register, VORSCHAU_VERFAHREN};
use crate::vorschau::Vorschau;
use crate::Ergebnis;

/// Dateiname der Scheibe `nr`.
pub fn dateiname(nr: i64) -> String {
    format!("vorschau-{nr:04}.db")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScheibeZeile {
    pub nr: i64,
    pub datei: String,
    pub bytes: u64,
    pub vorschauen: u64,
    pub voll: bool,
}

/// Ergebnis von [`Register::scheiben_aufteilen`].
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AufteilErgebnis {
    pub dauer_ms: u64,
    /// Dateien und Bilder, die in eine andere Scheibe gewandert sind.
    pub dateien: u64,
    pub bilder: u64,
    pub bytes: u64,
    pub neue_scheiben: u64,
    /// Scheiben, deren Datei danach verkleinert wurde.
    pub verdichtet: u64,
    /// Angehalten. Der nächste Lauf macht an dieser Stelle weiter.
    pub abgebrochen: bool,
}

/// Merkmal im Register: Aus dieser Scheibe wurden Bilder verschoben, ihre
/// Datei ist noch nicht verkleinert.
const VERDICHTEN: &str = "verdichten:";

/// So viele Byte wandern beim Aufteilen höchstens in einem Schritt.
const PAKET: u64 = 16_000_000;

/// Kennzahlen zu den Vorschauen.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct VorschauStand {
    pub pdfs: u64,
    /// PDFs, deren Vorschauen zum aktuellen Stand der Datei vorliegen.
    pub erzeugt: u64,
    pub ausstehend: u64,
    /// Davon "nur online".
    pub ausstehend_online: u64,
    pub fehler: u64,
    pub seiten: u64,
    pub farbig: u64,
    pub bytes: u64,
    pub scheiben: Vec<ScheibeZeile>,
    pub fehler_liste: Vec<FehlerZeile>,
}

/// Eine Seite mit Vorschau, für die Anzeige.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VorschauSeite {
    pub nr: u32,
    pub breite: u32,
    pub hoehe: u32,
    pub farbig: bool,
}

/// Eine Datei mit ihren Vorschauen, für die Anzeige.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DateiVorschau {
    pub id: i64,
    pub pfad: String,
    pub datum: Option<String>,
    pub seiten: Vec<VorschauSeite>,
}

/// Bedingung für Dateien mit Vorschau (PDFs und Bilder, `formate.rs`), deren
/// Vorschauen (neu) erzeugt werden müssen.
pub(crate) const AUSSTEHEND: &str = concat!("stufe = 1 AND endung IN ", crate::vorschau_sql!(), " AND (
        vorschau_verfahren IS NULL OR vorschau_verfahren < ?1
        OR vorschau_geaendert IS NOT geaendert
        OR (groesse IS NOT NULL AND vorschau_groesse IS NOT groesse))");

impl Register {
    /// Setzt die Größe, ab der eine Scheibe voll ist. Für Tests.
    pub fn scheibe_grenze_setzen(&mut self, bytes: u64) {
        self.scheibe_grenze = bytes;
    }

    /// Größe, ab der eine Scheibe voll ist.
    pub fn scheibe_grenze(&self) -> u64 {
        self.scheibe_grenze
    }

    /// Scheiben, die über der Grenze liegen und sich teilen lassen. Eine
    /// Scheibe mit einer einzigen, allein zu großen Datei gehört nicht dazu.
    fn zu_grosse_scheiben(&self) -> Ergebnis<Vec<i64>> {
        let liste = self
            .db
            .prepare(
                "SELECT s.nr FROM scheibe s
                 WHERE s.bytes > ?1
                   AND (SELECT count(DISTINCT v.datei) FROM vorschau v WHERE v.scheibe = s.nr) > 1
                 ORDER BY s.nr",
            )?
            .query_map([self.scheibe_grenze as i64], |z| z.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        Ok(liste)
    }

    /// Scheiben, deren Datei nach dem Verschieben noch verkleinert werden muss.
    fn zu_verdichten(&self) -> Ergebnis<Vec<i64>> {
        let namen = self
            .db
            .prepare("SELECT name FROM merkmal WHERE name LIKE ?1 ORDER BY name")?
            .query_map([format!("{VERDICHTEN}%")], |z| z.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(namen.iter().filter_map(|n| n[VERDICHTEN.len()..].parse().ok()).collect())
    }

    /// Gibt es Scheiben über der Grenze oder einen angehaltenen Lauf?
    pub fn aufteilen_noetig(&self) -> Ergebnis<bool> {
        Ok(!self.zu_grosse_scheiben()?.is_empty() || !self.zu_verdichten()?.is_empty())
    }

    /// Löscht aus der jüngsten, noch offenen Scheibe die Bilder ohne Verweis.
    /// Sie entstehen, wenn das Programm zwischen dem Schreiben der Scheibe
    /// und dem Schreiben des Verweises abbricht.
    fn waisen_entfernen(&mut self) -> Ergebnis<u64> {
        let offen: Option<i64> = self
            .db
            .query_row("SELECT nr FROM scheibe WHERE voll = 0 ORDER BY nr DESC LIMIT 1", [], |z| z.get(0))
            .optional()?;
        let Some(nr) = offen else { return Ok(0) };
        let gueltig: HashSet<i64> = self
            .db
            .prepare("SELECT id FROM vorschau WHERE scheibe = ?1")?
            .query_map([nr], |z| z.get(0))?
            .collect::<Result<_, _>>()?;
        let mut scheibe = self.scheibe(nr)?;
        let vorhanden = scheibe
            .prepare("SELECT id FROM vorschau")?
            .query_map([], |z| z.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let tx = scheibe.transaction()?;
        let mut geloescht = 0;
        {
            let mut loeschen = tx.prepare("DELETE FROM vorschau WHERE id = ?1")?;
            for id in vorhanden.iter().filter(|id| !gueltig.contains(id)) {
                geloescht += loeschen.execute([id])? as u64;
            }
        }
        tx.commit()?;
        Ok(geloescht)
    }

    /// Verschiebt die Vorschauen der `dateien` aus der Scheibe `quelle` in die
    /// Scheibe `ziel`. Reihenfolge wie beim Speichern: erst die Bilder in die
    /// neue Scheibe, dann die Verweise, zuletzt raus aus der alten. Ein
    /// Abbruch dazwischen verliert nichts.
    fn paket_verschieben(&mut self, quelle: i64, ziel: i64, dateien: &[i64]) -> Ergebnis<(u64, u64)> {
        // (Datei, Seite, alter Schlüssel)
        let mut seiten: Vec<(i64, i64, i64)> = Vec::new();
        {
            let mut lesen = self.db.prepare("SELECT nr, id FROM vorschau WHERE datei = ?1 AND scheibe = ?2 ORDER BY nr")?;
            for d in dateien {
                for z in lesen.query_map(params![d, quelle], |z| Ok((z.get::<_, i64>(0)?, z.get::<_, i64>(1)?)))? {
                    let (nr, id) = z?;
                    seiten.push((*d, nr, id));
                }
            }
        }
        let mut bilder: Vec<(i64, i64, i64, Vec<u8>)> = Vec::with_capacity(seiten.len());
        {
            let scheibe = self.scheibe(quelle)?;
            let mut lesen = scheibe.prepare("SELECT avif FROM vorschau WHERE id = ?1")?;
            for (d, nr, id) in seiten {
                // Ein Verweis ohne Bild bleibt, wie er ist
                if let Some(avif) = lesen.query_row([id], |z| z.get::<_, Vec<u8>>(0)).optional()? {
                    bilder.push((d, nr, id, avif));
                }
            }
        }
        let mut neu = Vec::with_capacity(bilder.len());
        {
            let mut scheibe = self.scheibe(ziel)?;
            let tx = scheibe.transaction()?;
            {
                let mut einfuegen = tx.prepare("INSERT INTO vorschau (avif) VALUES (?1)")?;
                for (.., avif) in &bilder {
                    einfuegen.execute([avif])?;
                    neu.push(tx.last_insert_rowid());
                }
            }
            tx.commit()?;
        }
        let summe: u64 = bilder.iter().map(|b| b.3.len() as u64).sum();
        let tx = self.db.transaction()?;
        {
            let mut umhaengen = tx.prepare("UPDATE vorschau SET scheibe = ?3, id = ?4 WHERE datei = ?1 AND nr = ?2")?;
            let mut weg = tx.prepare("INSERT INTO vorschau_weg (scheibe, id) VALUES (?1, ?2)")?;
            for ((d, nr, alt, _), id) in bilder.iter().zip(&neu) {
                umhaengen.execute(params![d, nr, ziel, id])?;
                weg.execute(params![quelle, alt])?;
            }
        }
        tx.execute(
            "UPDATE scheibe SET bytes = bytes - ?2, vorschauen = vorschauen - ?3 WHERE nr = ?1",
            params![quelle, summe as i64, bilder.len() as i64],
        )?;
        tx.execute(
            "UPDATE scheibe SET bytes = bytes + ?2, vorschauen = vorschauen + ?3 WHERE nr = ?1",
            params![ziel, summe as i64, bilder.len() as i64],
        )?;
        tx.commit()?;
        self.vorschauen_aufraeumen()?;
        Ok((bilder.len() as u64, summe))
    }

    /// Teilt Scheiben, die über der Grenze liegen (v0.0.8).
    ///
    /// Die ältesten Vorschauen bleiben in der Scheibe, bis die Grenze
    /// erreicht ist. Die übrigen wandern in der Reihenfolge ihrer Erfassung
    /// in neue Scheiben, die Vorschauen einer Datei bleiben zusammen. Danach
    /// wird die Datei der alten Scheibe verkleinert. Es wird keine Datei
    /// gelöscht und keine Kopie angelegt. Der Lauf lässt sich anhalten und
    /// macht beim nächsten Aufruf weiter.
    ///
    /// `fortschritt` zählt Dateien des Archivs, deren Vorschauen wandern.
    pub fn scheiben_aufteilen(
        &mut self,
        mut fortschritt: impl FnMut(&Fortschritt),
        abbrechen: &AtomicBool,
    ) -> Ergebnis<AufteilErgebnis> {
        let start = Instant::now();
        let mut e = AufteilErgebnis::default();
        let hoechste = |r: &Self| r.db.query_row("SELECT coalesce(max(nr), 0) FROM scheibe", [], |z| z.get::<_, i64>(0));
        let vorher = hoechste(self)?;

        // Reste eines abgebrochenen Laufs
        self.vorschauen_aufraeumen()?;
        self.waisen_entfernen()?;

        // Plan: je Scheibe die Dateien, die wandern, mit ihrer Größe
        let grenze = self.scheibe_grenze;
        let mut plan: Vec<(i64, Vec<(i64, u64)>)> = Vec::new();
        for quelle in self.zu_grosse_scheiben()? {
            let dateien = self
                .db
                .prepare("SELECT datei, sum(bytes) FROM vorschau WHERE scheibe = ?1 GROUP BY datei ORDER BY min(id)")?
                .query_map([quelle], |z| Ok((z.get::<_, i64>(0)?, z.get::<_, i64>(1)? as u64)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut bleibt = 0u64;
            let mut ab = dateien.len();
            for (i, (_, b)) in dateien.iter().enumerate() {
                if i > 0 && bleibt + b > grenze {
                    ab = i;
                    break;
                }
                bleibt += b;
            }
            plan.push((quelle, dateien[ab..].to_vec()));
        }
        let gesamt: u64 = plan.iter().map(|(_, d)| d.len() as u64).sum();
        let mut erledigt = 0u64;

        'scheiben: for (quelle, dateien) in plan {
            // Die Scheibe nimmt nichts mehr auf und ist zum Verkleinern vorgemerkt
            self.db.execute("UPDATE scheibe SET voll = 1 WHERE nr = ?1", [quelle])?;
            self.db.execute(
                "INSERT OR REPLACE INTO merkmal (name, wert) VALUES (?1, '1')",
                [format!("{VERDICHTEN}{quelle}")],
            )?;
            let mut i = 0;
            while i < dateien.len() {
                if abbrechen.load(Ordering::SeqCst) {
                    e.abgebrochen = true;
                    break 'scheiben;
                }
                let ziel = self.scheibe_fuer(dateien[i].1)?;
                let mut belegt: u64 =
                    self.db.query_row("SELECT bytes FROM scheibe WHERE nr = ?1", [ziel], |z| z.get::<_, i64>(0))? as u64;
                let mut paket = vec![dateien[i].0];
                let mut paket_bytes = dateien[i].1;
                belegt += dateien[i].1;
                i += 1;
                while i < dateien.len() && belegt + dateien[i].1 <= grenze && paket_bytes + dateien[i].1 <= PAKET {
                    paket.push(dateien[i].0);
                    paket_bytes += dateien[i].1;
                    belegt += dateien[i].1;
                    i += 1;
                }
                let (bilder, bytes) = self.paket_verschieben(quelle, ziel, &paket)?;
                e.dateien += paket.len() as u64;
                e.bilder += bilder;
                e.bytes += bytes;
                erledigt += paket.len() as u64;
                fortschritt(&Fortschritt { erledigt, gesamt, datei: dateiname(ziel) });
            }
        }
        e.neue_scheiben = (hoechste(self)? - vorher) as u64;

        // Verkleinern, auch nach einem früher angehaltenen Lauf. Eine Scheibe,
        // die noch über der Grenze liegt, ist erst beim nächsten Lauf dran.
        if !e.abgebrochen {
            let offen = self.zu_grosse_scheiben()?;
            for nr in self.zu_verdichten()? {
                if offen.contains(&nr) {
                    continue;
                }
                fortschritt(&Fortschritt { erledigt, gesamt, datei: format!("{} wird verkleinert", dateiname(nr)) });
                let bekannt: bool = self.db.query_row("SELECT count(*) FROM scheibe WHERE nr = ?1", [nr], |z| z.get(0))?;
                if bekannt {
                    self.scheibe(nr)?.execute_batch("VACUUM")?;
                    e.verdichtet += 1;
                }
                self.db.execute("DELETE FROM merkmal WHERE name = ?1", [format!("{VERDICHTEN}{nr}")])?;
            }
        }
        e.dauer_ms = start.elapsed().as_millis() as u64;
        Ok(e)
    }

    /// Öffnet die Scheibe `nr` und legt sie bei Bedarf an.
    fn scheibe(&self, nr: i64) -> Ergebnis<RefMut<'_, Connection>> {
        let mut offen = self.scheiben.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(platz) = offen.entry(nr) {
            // Nur lesend (Suche, Anzeige): Die Scheibe bleibt unberührt (v0.0.9)
            if self.nur_lesen {
                let ordner = self.ordner().ok_or_else(|| crate::Fehler::Konfiguration("Register ohne Ordner".into()))?;
                let pfad = ordner.join(dateiname(nr));
                let db = Connection::open_with_flags(
                    &pfad,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                crate::verschluesselung::verbinden(&db, &pfad)?;
                db.busy_timeout(std::time::Duration::from_secs(15))?;
                platz.insert(db);
                return Ok(RefMut::map(offen, |o| o.get_mut(&nr).expect("eben geöffnet")));
            }
            let db = match self.ordner() {
                Some(ordner) => {
                    let pfad = ordner.join(dateiname(nr));
                    let db = Connection::open(&pfad)?;
                    crate::verschluesselung::verbinden(&db, &pfad)?;
                    db
                }
                None => Connection::open_in_memory()?,
            };
            db.busy_timeout(std::time::Duration::from_secs(15))?;
            // Nur eine neue Scheibe wird angelegt. Bis v0.0.8 schrieb jedes
            // Öffnen die Versionsnummer neu und änderte damit die Datei.
            let version: i64 = db.query_row("PRAGMA user_version", [], |z| z.get(0))?;
            if version == 0 {
                db.execute_batch(
                    "PRAGMA journal_mode = DELETE;
                     CREATE TABLE IF NOT EXISTS vorschau (
                         -- AUTOINCREMENT: ein Schlüssel wird nie wieder vergeben
                         id    INTEGER PRIMARY KEY AUTOINCREMENT,
                         avif  BLOB NOT NULL
                     );
                     PRAGMA user_version = 1;",
                )?;
            }
            platz.insert(db);
        }
        Ok(RefMut::map(offen, |o| o.get_mut(&nr).expect("eben geöffnet")))
    }

    /// Pfade aller Scheiben, die das Register kennt.
    pub fn scheiben_dateien(&self) -> Ergebnis<Vec<PathBuf>> {
        let Some(ordner) = self.ordner() else { return Ok(Vec::new()) };
        let namen = self
            .db
            .prepare("SELECT datei FROM scheibe ORDER BY nr")?
            .query_map([], |z| z.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(namen.into_iter().map(|n| ordner.join(n)).collect())
    }

    /// Jüngste Scheibe, die noch `bytes` aufnehmen kann. Ist sie dafür zu
    /// voll und nicht leer, wird sie geschlossen und eine neue angelegt.
    fn scheibe_fuer(&self, bytes: u64) -> Ergebnis<i64> {
        let juengste: Option<(i64, i64, i64)> = self
            .db
            .query_row("SELECT nr, bytes, vorschauen FROM scheibe WHERE voll = 0 ORDER BY nr DESC LIMIT 1", [], |z| {
                Ok((z.get(0)?, z.get(1)?, z.get(2)?))
            })
            .optional()?;
        if let Some((nr, belegt, anzahl)) = juengste {
            if anzahl == 0 || belegt as u64 + bytes <= self.scheibe_grenze {
                return Ok(nr);
            }
            self.db.execute("UPDATE scheibe SET voll = 1 WHERE nr = ?1", [nr])?;
        }
        let nr: i64 = self.db.query_row("SELECT coalesce(max(nr), 0) + 1 FROM scheibe", [], |z| z.get(0))?;
        self.db.execute("INSERT INTO scheibe (nr, datei) VALUES (?1, ?2)", params![nr, dateiname(nr)])?;
        Ok(nr)
    }

    /// Ordner mit Dateien, deren Vorschau aussteht und die nur online liegen,
    /// nach Ordner. Das Gegenstück zu `Register::nur_online_ordner` für die
    /// Vorschauen: Es können andere Dateien sein als beim Inhalt.
    pub fn nur_online_ordner_vorschau(&self) -> Ergebnis<Vec<crate::OnlineOrdner>> {
        let sql = format!(
            "SELECT substr(pfad, 1, max(length(pfad) - length(name) - 1, 0)) AS ordner, count(*)
             FROM datei WHERE nur_online AND {AUSSTEHEND} GROUP BY ordner ORDER BY ordner"
        );
        let liste = self
            .db
            .prepare(&sql)?
            .query_map([VORSCHAU_VERFAHREN], |z| {
                Ok(crate::OnlineOrdner { ordner: z.get(0)?, dateien: z.get::<_, i64>(1)? as u64 })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    /// PDFs, deren Vorschauen fehlen oder nicht zum Stand der Datei passen.
    pub fn vorschau_ausstehend(&self) -> Ergebnis<Vec<Ausstehend>> {
        let sql = format!("SELECT id, pfad, nur_online FROM datei WHERE {AUSSTEHEND} ORDER BY pfad");
        let liste = self
            .db
            .prepare(&sql)?
            .query_map([VORSCHAU_VERFAHREN], |z| {
                Ok(Ausstehend { id: z.get(0)?, pfad: z.get(1)?, nur_online: z.get(2)? })
            })?
            .filter(|a| a.as_ref().map_or(true, |a| self.in_auswahl(a.id)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    /// Speichert die Vorschauen einer Datei und ersetzt einen früheren Stand.
    /// `vorschauen` ist nach Seitennummer geordnet, ab Seite 1. Ein Fehler
    /// wird festgehalten, damit die Datei erst nach einer Änderung erneut
    /// versucht wird.
    pub fn vorschauen_speichern(
        &mut self,
        id: i64,
        geaendert: i64,
        groesse: u64,
        vorschauen: &Result<Vec<Vorschau>, String>,
    ) -> Ergebnis<()> {
        let leer = Vec::new();
        let (liste, fehler) = match vorschauen {
            Ok(v) => (v, None),
            Err(f) => (&leer, Some(f.clone())),
        };
        let summe: u64 = liste.iter().map(|v| v.avif.len() as u64).sum();

        // Zuerst die Bilder in die Scheibe
        let nr = if liste.is_empty() { None } else { Some(self.scheibe_fuer(summe)?) };
        let mut schluessel = Vec::with_capacity(liste.len());
        if let Some(nr) = nr {
            let mut scheibe = self.scheibe(nr)?;
            let tx = scheibe.transaction()?;
            {
                let mut einfuegen = tx.prepare("INSERT INTO vorschau (avif) VALUES (?1)")?;
                for v in liste {
                    einfuegen.execute([&v.avif])?;
                    schluessel.push(tx.last_insert_rowid());
                }
            }
            tx.commit()?;
        }

        // Dann die Verweise. Alte Verweise wandern über den Trigger nach vorschau_weg.
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM vorschau WHERE datei = ?1", [id])?;
        if let Some(nr) = nr {
            let mut einfuegen = tx.prepare(
                "INSERT INTO vorschau (datei, nr, scheibe, id, breite, hoehe, farbig, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (i, (v, s)) in liste.iter().zip(&schluessel).enumerate() {
                einfuegen.execute(params![
                    id,
                    i as i64 + 1,
                    nr,
                    s,
                    v.breite,
                    v.hoehe,
                    v.farbig,
                    v.avif.len() as i64
                ])?;
            }
            tx.execute(
                "UPDATE scheibe SET bytes = bytes + ?2, vorschauen = vorschauen + ?3 WHERE nr = ?1",
                params![nr, summe as i64, liste.len() as i64],
            )?;
        }
        tx.execute(
            "UPDATE datei SET vorschau_geaendert = ?2, vorschau_groesse = ?3, vorschau_verfahren = ?4,
                              vorschau_fehler = ?5
             WHERE id = ?1",
            params![id, geaendert, groesse as i64, VORSCHAU_VERFAHREN, fehler],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Löscht Bilder, deren Verweis entfernt wurde, aus ihren Scheiben.
    /// Liefert die Anzahl der gelöschten Bilder.
    pub fn vorschauen_aufraeumen(&mut self) -> Ergebnis<u64> {
        let weg = self
            .db
            .prepare("SELECT scheibe, id FROM vorschau_weg ORDER BY scheibe, id")?
            .query_map([], |z| Ok((z.get::<_, i64>(0)?, z.get::<_, i64>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        if weg.is_empty() {
            return Ok(0);
        }
        let mut geloescht = 0;
        let mut i = 0;
        while i < weg.len() {
            let nr = weg[i].0;
            let ende = weg[i..].iter().position(|w| w.0 != nr).map_or(weg.len(), |p| i + p);
            // Scheiben, die das Register nicht mehr kennt, werden übergangen.
            let bekannt: bool = self.db.query_row("SELECT count(*) FROM scheibe WHERE nr = ?1", [nr], |z| z.get(0))?;
            if bekannt {
                let mut scheibe = self.scheibe(nr)?;
                let tx = scheibe.transaction()?;
                {
                    let mut loeschen = tx.prepare("DELETE FROM vorschau WHERE id = ?1")?;
                    for (_, id) in &weg[i..ende] {
                        geloescht += loeschen.execute([id])? as u64;
                    }
                }
                tx.commit()?;
            }
            i = ende;
        }
        // Ein Schlüssel wird nie wieder vergeben. Ein zweites Löschen nach
        // einem Abbruch an dieser Stelle trifft also nichts Falsches.
        self.db.execute("DELETE FROM vorschau_weg", [])?;
        Ok(geloescht)
    }

    /// Das AVIF-Bild der Seite `nr` einer Datei, falls vorhanden.
    pub fn vorschau(&self, datei: i64, nr: u32) -> Ergebnis<Option<Vec<u8>>> {
        let verweis: Option<(i64, i64)> = self
            .db
            .query_row("SELECT scheibe, id FROM vorschau WHERE datei = ?1 AND nr = ?2", params![datei, nr], |z| {
                Ok((z.get(0)?, z.get(1)?))
            })
            .optional()?;
        let Some((nr, id)) = verweis else { return Ok(None) };
        let scheibe = self.scheibe(nr)?;
        Ok(scheibe.query_row("SELECT avif FROM vorschau WHERE id = ?1", [id], |z| z.get(0)).optional()?)
    }

    /// Alle Verweise auf Vorschauen als (Datei, Seite). Zum Prüfen.
    pub fn vorschau_verweise(&self) -> Ergebnis<Vec<(i64, u32)>> {
        let liste = self
            .db
            .prepare("SELECT datei, nr FROM vorschau ORDER BY datei, nr")?
            .query_map([], |z| Ok((z.get(0)?, z.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }

    /// Datei mit ihren Vorschauen, für die Anzeige.
    pub fn datei_vorschau(&self, id: i64) -> Ergebnis<Option<DateiVorschau>> {
        let kopf: Option<(String, Option<String>)> = self
            .db
            .query_row("SELECT pfad, datum_dokument FROM datei WHERE id = ?1", [id], |z| Ok((z.get(0)?, z.get(1)?)))
            .optional()?;
        let Some((pfad, datum)) = kopf else { return Ok(None) };
        let seiten = self
            .db
            .prepare("SELECT nr, breite, hoehe, farbig FROM vorschau WHERE datei = ?1 ORDER BY nr")?
            .query_map([id], |z| {
                Ok(VorschauSeite { nr: z.get(0)?, breite: z.get(1)?, hoehe: z.get(2)?, farbig: z.get(3)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(DateiVorschau { id, pfad, datum, seiten }))
    }

    /// Dateien mit Vorschauen, deren Pfad alle Wörter von `muster` enthält.
    /// Nur zum Prüfen der Vorschauen, die eigentliche Suche folgt in Stufe 2.
    ///
    /// `nur_ocr`: nur Dateien mit erkanntem Text (OCR), zum Prüfen der Erkennung.
    pub fn dateien_mit_vorschau(&self, muster: &str, nur_ocr: bool, max: u32) -> Ergebnis<Vec<DateiVorschau>> {
        let woerter: Vec<String> = muster.split_whitespace().map(|w| format!("%{}%", w.replace(['%', '_'], ""))).collect();
        let mut sql = String::from(
            "SELECT d.id FROM datei d WHERE EXISTS (SELECT 1 FROM vorschau v WHERE v.datei = d.id)",
        );
        for i in 0..woerter.len() {
            sql.push_str(&format!(" AND d.pfad LIKE ?{}", i + 1));
        }
        if nur_ocr {
            sql.push_str(" AND EXISTS (SELECT 1 FROM ocr o WHERE o.datei = d.id)");
        }
        sql.push_str(&format!(" ORDER BY d.datum_dokument DESC, d.pfad LIMIT {max}"));
        let ids = self
            .db
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(woerter.iter()), |z| z.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut liste = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(d) = self.datei_vorschau(id)? {
                liste.push(d);
            }
        }
        Ok(liste)
    }

    pub(crate) fn vorschau_stand(&self) -> Ergebnis<VorschauStand> {
        let zahl = |z: &rusqlite::Row, i: usize| -> rusqlite::Result<u64> {
            Ok(z.get::<_, Option<i64>>(i)?.unwrap_or(0) as u64)
        };
        let mut s = VorschauStand::default();
        let sql = format!(
            "SELECT count(*), sum(NOT ({a})), sum({a}), sum(({a}) AND nur_online),
                    sum(NOT ({a}) AND vorschau_fehler IS NOT NULL)
             FROM datei WHERE stufe = 1 AND endung IN {v}",
            a = AUSSTEHEND,
            v = crate::vorschau_sql!()
        );
        (s.pdfs, s.erzeugt, s.ausstehend, s.ausstehend_online, s.fehler) =
            self.db.query_row(&sql, [VORSCHAU_VERFAHREN], |z| {
                Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?, zahl(z, 3)?, zahl(z, 4)?))
            })?;
        (s.seiten, s.farbig, s.bytes) =
            self.db.query_row("SELECT count(*), sum(farbig), sum(bytes) FROM vorschau", [], |z| {
                Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?))
            })?;
        s.scheiben = self
            .db
            .prepare("SELECT nr, datei, bytes, vorschauen, voll FROM scheibe ORDER BY nr")?
            .query_map([], |z| {
                Ok(ScheibeZeile {
                    nr: z.get(0)?,
                    datei: z.get(1)?,
                    bytes: z.get::<_, i64>(2)? as u64,
                    vorschauen: z.get::<_, i64>(3)? as u64,
                    voll: z.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        s.fehler_liste = self
            .db
            .prepare("SELECT pfad, vorschau_fehler FROM datei WHERE vorschau_fehler IS NOT NULL ORDER BY pfad LIMIT 50")?
            .query_map([], |z| Ok(FehlerZeile { pfad: z.get(0)?, meldung: z.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dateibaum::{DateiEintrag, Durchlauf};

    fn v(bytes: usize) -> Vorschau {
        Vorschau { breite: 10, hoehe: 14, farbig: bytes % 2 == 1, avif: vec![7; bytes] }
    }

    fn register_mit(pfade: &[&str]) -> Register {
        let mut r = Register::im_speicher().unwrap();
        r.abgleichen(&durchlauf(pfade)).unwrap();
        r
    }

    fn durchlauf(pfade: &[&str]) -> Durchlauf {
        let dateien = pfade
            .iter()
            .map(|p| DateiEintrag {
                pfad: p.to_string(),
                name: p.rsplit('/').next().unwrap().into(),
                endung: "pdf".into(),
                groesse: 100,
                geaendert: 1,
                stufe: crate::regeln::Stufe::Voll,
                nur_online: false,
            })
            .collect();
        Durchlauf { dateien, gelesene_verzeichnisse: vec!["A".into()], ..Default::default() }
    }

    fn id(r: &Register, pfad: &str) -> i64 {
        r.db.query_row("SELECT id FROM datei WHERE pfad = ?1", [pfad], |z| z.get(0)).unwrap()
    }

    fn bilder_in(r: &Register, nr: i64) -> i64 {
        r.scheibe(nr).unwrap().query_row("SELECT count(*) FROM vorschau", [], |z| z.get(0)).unwrap()
    }

    /// Ganzes Register auf der Platte verschlüsseln (v0.1.0): Index mit
    /// Suchindex und zwei Scheiben, Anhalten und Fortsetzen, danach Lesen,
    /// Suchen und Ergänzen mit dem Schlüssel und Ablehnen ohne ihn.
    #[test]
    fn register_verschluesseln() {
        use crate::verschluesselung::{self as vs, Schluessel, Zustand};
        let tmp = tempfile::tempdir().unwrap();
        let arbeit = tempfile::tempdir().unwrap();
        let ordner = tmp.path();
        let (a, b);
        {
            let mut r = Register::oeffnen(ordner).unwrap();
            r.scheibe_grenze_setzen(15);
            r.abgleichen(&durchlauf(&["A/Kontoauszug.pdf", "A/Rechnung.pdf"])).unwrap();
            a = id(&r, "A/Kontoauszug.pdf");
            b = id(&r, "A/Rechnung.pdf");
            r.vorschauen_speichern(a, 1, 100, &Ok(vec![v(10)])).unwrap();
            r.vorschauen_speichern(b, 1, 100, &Ok(vec![v(11)])).unwrap();
            r.suche_aktualisieren(|_| {}, &AtomicBool::new(false)).unwrap();
            assert_eq!(r.scheiben_dateien().unwrap().len(), 2);
        }
        let vorher = vs::stand(ordner).unwrap();
        assert_eq!((vorher.dateien, vorher.unverschluesselt, vorher.index), (3, 3, Some(Zustand::Unverschluesselt)));

        // Ohne Schlüssel keine Umstellung
        assert!(matches!(vs::umstellen(ordner, None, &AtomicBool::new(false), |_| {}), Err(crate::Fehler::SchluesselFehlt)));
        let schluessel = Schluessel::aus_text(&"0123456789abcdef".repeat(4)).unwrap();
        vs::hinterlegen(ordner, schluessel.clone());
        assert_eq!(vs::schluessel_pruefen(ordner, &schluessel).unwrap(), None);
        // Schlüssel hinterlegt, Register noch unverschlüsselt: öffnet nicht (v0.1.4)
        assert!(vs::stand(ordner).unwrap().gesperrt);
        assert!(matches!(Register::oeffnen_lesend(ordner), Err(crate::Fehler::Unverschluesselt(_))));
        assert!(matches!(Register::oeffnen(ordner), Err(crate::Fehler::Unverschluesselt(_))));

        // Sofort angehalten: nichts geändert
        let e = vs::umstellen(ordner, Some(arbeit.path()), &AtomicBool::new(true), |_| {}).unwrap();
        assert!(e.abgebrochen);
        assert_eq!(vs::stand(ordner).unwrap().unverschluesselt, 3);

        let mut meldungen = Vec::new();
        let e = vs::umstellen(ordner, Some(arbeit.path()), &AtomicBool::new(false), |f| meldungen.push(f.datei.clone())).unwrap();
        assert_eq!((e.dateien, e.abgebrochen), (3, false));
        assert_eq!(meldungen[..3], ["vorschau-0001.db", "vorschau-0002.db", "register.db"]);
        let nachher = vs::stand(ordner).unwrap();
        assert!(nachher.vollstaendig());
        assert!(nachher.arbeitskopien.is_empty());
        assert_eq!(std::fs::read_dir(arbeit.path()).unwrap().count(), 0);
        for p in std::fs::read_dir(ordner).unwrap() {
            let roh = std::fs::read(p.unwrap().path()).unwrap();
            assert!(!roh.windows(11).any(|w| w == b"Kontoauszug"));
        }
        assert_eq!(vs::schluessel_pruefen(ordner, &schluessel).unwrap(), Some(true));
        let falsch = Schluessel::aus_text(&"fedcba9876543210".repeat(4)).unwrap();
        assert_eq!(vs::schluessel_pruefen(ordner, &falsch).unwrap(), Some(false));
        // Ein zweiter Lauf hat nichts mehr zu tun
        assert_eq!(vs::umstellen(ordner, None, &AtomicBool::new(false), |_| {}).unwrap().dateien, 0);

        {
            let r = Register::oeffnen_lesend(ordner).unwrap();
            assert_eq!(r.vorschau(a, 1).unwrap().unwrap().len(), 10);
            assert_eq!(r.vorschau(b, 1).unwrap().unwrap().len(), 11);
            let anfrage = crate::Anfrage { text: "Kontoauszug".into(), ..Default::default() };
            assert_eq!(r.suchen(&anfrage).unwrap().gesamt, 1);
        }
        {
            // Neue Scheibe entsteht verschlüsselt
            let mut r = Register::oeffnen(ordner).unwrap();
            r.scheibe_grenze_setzen(15);
            r.abgleichen(&durchlauf(&["A/Kontoauszug.pdf", "A/Rechnung.pdf", "A/Neu.pdf"])).unwrap();
            let c = id(&r, "A/Neu.pdf");
            r.vorschauen_speichern(c, 1, 100, &Ok(vec![v(12)])).unwrap();
            assert_eq!(r.scheiben_dateien().unwrap().len(), 3);
        }
        assert_eq!(vs::zustand(&ordner.join(dateiname(3))).unwrap(), Zustand::Verschluesselt);
        assert!(vs::stand(ordner).unwrap().vollstaendig());

        vs::vergessen(ordner);
        assert!(matches!(Register::oeffnen_lesend(ordner), Err(crate::Fehler::SchluesselFehlt)));
        assert!(matches!(Register::oeffnen(ordner), Err(crate::Fehler::SchluesselFehlt)));
        vs::hinterlegen(ordner, falsch);
        assert!(matches!(Register::oeffnen_lesend(ordner), Err(crate::Fehler::SchluesselFalsch)));
        vs::vergessen(ordner);
    }

    #[test]
    fn speichert_und_liest_vorschauen() {
        let mut r = register_mit(&["A/a.pdf", "A/b.pdf"]);
        assert_eq!(r.vorschau_ausstehend().unwrap().len(), 2);
        let a = id(&r, "A/a.pdf");
        r.vorschauen_speichern(a, 1, 100, &Ok(vec![v(10), v(11)])).unwrap();
        assert_eq!(r.vorschau(a, 2).unwrap().unwrap().len(), 11);
        assert_eq!(r.vorschau(a, 3).unwrap(), None);
        let b = id(&r, "A/b.pdf");
        r.vorschauen_speichern(b, 1, 100, &Err("kaputt".into())).unwrap();

        let s = r.vorschau_stand().unwrap();
        assert_eq!((s.pdfs, s.erzeugt, s.ausstehend, s.fehler), (2, 2, 0, 1));
        assert_eq!((s.seiten, s.farbig, s.bytes), (2, 1, 21));
        assert_eq!(s.scheiben.len(), 1);
        assert_eq!((s.scheiben[0].bytes, s.scheiben[0].vorschauen), (21, 2));

        let d = r.datei_vorschau(a).unwrap().unwrap();
        assert_eq!(d.seiten.iter().map(|s| s.nr).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(r.dateien_mit_vorschau("a.p", false, 10).unwrap().len(), 1);
        assert_eq!(r.dateien_mit_vorschau("", false, 10).unwrap().len(), 1);
        assert_eq!(r.dateien_mit_vorschau("", true, 10).unwrap().len(), 0);
    }

    #[test]
    fn fehler_bei_tiff_wird_einmal_neu_versucht() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(&["A/a.pdf", "A/fax.pdf"])).unwrap();
            r.db.execute("UPDATE datei SET endung = 'tif' WHERE pfad = 'A/fax.pdf'", []).unwrap();
            for p in ["A/a.pdf", "A/fax.pdf"] {
                let i = id(&r, p);
                r.vorschauen_speichern(i, 1, 100, &Err("Fax3 is unsupported".into())).unwrap();
            }
            assert!(r.vorschau_ausstehend().unwrap().is_empty());
            // Stand vor v0.0.7
            r.db.execute("DELETE FROM merkmal WHERE name = 'fax_tiff'", []).unwrap();
        }
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            let offen: Vec<String> = r.vorschau_ausstehend().unwrap().into_iter().map(|a| a.pfad).collect();
            assert_eq!(offen, vec!["A/fax.pdf"]);
            let i = id(&r, "A/fax.pdf");
            r.vorschauen_speichern(i, 1, 100, &Err("immer noch".into())).unwrap();
        }
        // Nur einmal
        let r = Register::oeffnen(tmp.path()).unwrap();
        assert!(r.vorschau_ausstehend().unwrap().is_empty());
    }

    #[test]
    fn tiff_mit_palette_wird_einmal_neu_versucht() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(&["A/a.pdf", "A/alt.pdf"])).unwrap();
            r.db.execute("UPDATE datei SET endung = 'tif'", []).unwrap();
            let fehler = "Bild: The decoder for Tiff does not support the format features Photometric interpretation RGBPalette with bits per sample [8] is unsupported";
            let a = id(&r, "A/a.pdf");
            r.vorschauen_speichern(a, 1, 100, &Err("anderer Fehler".into())).unwrap();
            let alt = id(&r, "A/alt.pdf");
            r.vorschauen_speichern(alt, 1, 100, &Err(fehler.into())).unwrap();
            r.db.execute("DELETE FROM merkmal WHERE name = 'tiff_palette'", []).unwrap();
        }
        let offen: Vec<String> =
            Register::oeffnen(tmp.path()).unwrap().vorschau_ausstehend().unwrap().into_iter().map(|a| a.pfad).collect();
        assert_eq!(offen, vec!["A/alt.pdf"]);
    }

    #[test]
    fn neue_scheibe_wenn_voll() {
        let mut r = register_mit(&["A/a.pdf", "A/b.pdf", "A/c.pdf", "A/d.pdf"]);
        r.scheibe_grenze_setzen(25);
        for (p, n) in [("A/a.pdf", 20), ("A/b.pdf", 20), ("A/c.pdf", 3), ("A/d.pdf", 40)] {
            let i = id(&r, p);
            r.vorschauen_speichern(i, 1, 100, &Ok(vec![v(n)])).unwrap();
        }
        let s = r.vorschau_stand().unwrap();
        // a: Scheibe 1; b passt nicht mehr: Scheibe 2; c passt zu b; d ist allein zu groß, kommt in eine leere Scheibe 3
        let belegung: Vec<_> = s.scheiben.iter().map(|z| (z.nr, z.vorschauen, z.voll)).collect();
        assert_eq!(belegung, vec![(1, 1, true), (2, 2, true), (3, 1, false)]);
    }

    #[test]
    fn ersetzen_und_entfernen_raeumt_scheiben_auf() {
        let mut r = register_mit(&["A/a.pdf", "A/b.pdf"]);
        let (a, b) = (id(&r, "A/a.pdf"), id(&r, "A/b.pdf"));
        r.vorschauen_speichern(a, 1, 100, &Ok(vec![v(10), v(10)])).unwrap();
        r.vorschauen_speichern(b, 1, 100, &Ok(vec![v(10)])).unwrap();
        // a neu erzeugt: alte Bilder liegen noch in der Scheibe, bis aufgeräumt wird
        r.vorschauen_speichern(a, 2, 100, &Ok(vec![v(12)])).unwrap();
        assert_eq!(bilder_in(&r, 1), 4);
        assert_eq!(r.vorschauen_aufraeumen().unwrap(), 2);
        assert_eq!(bilder_in(&r, 1), 2);
        assert_eq!(r.vorschau(a, 1).unwrap().unwrap().len(), 12);

        // b verschwindet aus dem Dateibaum: Verweise gehen mit der Datei, das Bild beim Aufräumen
        r.abgleichen(&durchlauf(&["A/a.pdf"])).unwrap();
        assert_eq!(r.vorschauen_aufraeumen().unwrap(), 1);
        assert_eq!(bilder_in(&r, 1), 1);
        let s = r.vorschau_stand().unwrap();
        assert_eq!((s.seiten, s.scheiben[0].bytes, s.scheiben[0].vorschauen), (1, 12, 1));
        assert_eq!(r.vorschauen_aufraeumen().unwrap(), 0);
    }

    #[test]
    fn geaenderte_datei_ist_wieder_ausstehend() {
        let mut r = register_mit(&["A/a.pdf"]);
        let a = id(&r, "A/a.pdf");
        r.vorschauen_speichern(a, 1, 100, &Ok(vec![v(5)])).unwrap();
        assert!(r.vorschau_ausstehend().unwrap().is_empty());
        let mut d = durchlauf(&["A/a.pdf"]);
        d.dateien[0].geaendert = 2;
        r.abgleichen(&d).unwrap();
        assert_eq!(r.vorschau_ausstehend().unwrap().len(), 1);
    }

    #[test]
    fn scheiben_liegen_als_dateien_neben_dem_register() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(&["A/a.pdf"])).unwrap();
            let a = id(&r, "A/a.pdf");
            r.vorschauen_speichern(a, 1, 100, &Ok(vec![v(9)])).unwrap();
            assert_eq!(r.scheiben_dateien().unwrap(), vec![tmp.path().join("vorschau-0001.db")]);
        }
        assert!(tmp.path().join("vorschau-0001.db").is_file());
        let r = Register::oeffnen(tmp.path()).unwrap();
        let a = id(&r, "A/a.pdf");
        assert_eq!(r.vorschau(a, 1).unwrap().unwrap().len(), 9);
    }
    fn scheibe_von(r: &Register, datei: i64) -> Vec<i64> {
        r.db.prepare("SELECT DISTINCT scheibe FROM vorschau WHERE datei = ?1")
            .unwrap()
            .query_map([datei], |z| z.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// Fünf Dateien mit zusammen 100 Byte in einer Scheibe, wie vor v0.0.8.
    fn grosse_scheibe(r: &mut Register) -> Vec<i64> {
        r.abgleichen(&durchlauf(&["A/a.pdf", "A/b.pdf", "A/c.pdf", "A/d.pdf", "A/e.pdf"])).unwrap();
        let ids: Vec<i64> = ["A/a.pdf", "A/b.pdf", "A/c.pdf", "A/d.pdf", "A/e.pdf"].iter().map(|p| id(r, p)).collect();
        for (n, i) in ids.iter().enumerate() {
            // zwei Seiten je Datei, erkennbar am Inhalt
            let seiten = vec![
                Vorschau { breite: 1, hoehe: 1, farbig: false, avif: vec![n as u8; 10] },
                Vorschau { breite: 1, hoehe: 1, farbig: false, avif: vec![n as u8 + 100; 10] },
            ];
            r.vorschauen_speichern(*i, 1, 100, &Ok(seiten)).unwrap();
        }
        ids
    }

    fn alles_lesbar(r: &Register, ids: &[i64]) {
        for (n, i) in ids.iter().enumerate() {
            assert_eq!(r.vorschau(*i, 1).unwrap().unwrap(), vec![n as u8; 10]);
            assert_eq!(r.vorschau(*i, 2).unwrap().unwrap(), vec![n as u8 + 100; 10]);
        }
    }

    #[test]
    fn teilt_zu_grosse_scheibe() {
        let mut r = Register::im_speicher().unwrap();
        let ids = grosse_scheibe(&mut r);
        assert!(!r.aufteilen_noetig().unwrap());
        r.scheibe_grenze_setzen(45);
        assert!(r.aufteilen_noetig().unwrap());

        let mut gemeldet = Vec::new();
        let e = r.scheiben_aufteilen(|f| gemeldet.push((f.erledigt, f.gesamt)), &AtomicBool::new(false)).unwrap();
        assert_eq!((e.dateien, e.bilder, e.bytes, e.neue_scheiben, e.verdichtet, e.abgebrochen), (3, 6, 60, 2, 1, false));
        assert_eq!(gemeldet.last(), Some(&(3, 3)));

        // a und b bleiben, c und d in Scheibe 2, e in Scheibe 3. Seiten einer Datei bleiben zusammen
        let lage: Vec<_> = ids.iter().map(|i| scheibe_von(&r, *i)).collect();
        assert_eq!(lage, vec![vec![1], vec![1], vec![2], vec![2], vec![3]]);
        let s = r.vorschau_stand().unwrap();
        let belegung: Vec<_> = s.scheiben.iter().map(|z| (z.nr, z.bytes, z.vorschauen, z.voll)).collect();
        assert_eq!(belegung, vec![(1, 40, 4, true), (2, 40, 4, true), (3, 20, 2, false)]);
        assert_eq!((bilder_in(&r, 1), bilder_in(&r, 2), bilder_in(&r, 3)), (4, 4, 2));
        alles_lesbar(&r, &ids);
        assert!(!r.aufteilen_noetig().unwrap());

        // Ein zweiter Lauf ändert nichts
        let e = r.scheiben_aufteilen(|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!((e.dateien, e.neue_scheiben, e.verdichtet), (0, 0, 0));
        // Neue Vorschauen kommen in die jüngste Scheibe
        r.vorschauen_speichern(ids[0], 2, 100, &Ok(vec![v(5)])).unwrap();
        assert_eq!(scheibe_von(&r, ids[0]), vec![3]);
    }

    #[test]
    fn aufteilen_laesst_sich_anhalten_und_fortsetzen() {
        let mut r = Register::im_speicher().unwrap();
        let ids = grosse_scheibe(&mut r);
        r.scheibe_grenze_setzen(45);
        // Hält nach dem ersten Paket an
        let halt = AtomicBool::new(false);
        let e = r.scheiben_aufteilen(|_| halt.store(true, Ordering::SeqCst), &halt).unwrap();
        assert!(e.abgebrochen);
        assert_eq!((e.dateien, e.verdichtet), (2, 0));
        alles_lesbar(&r, &ids);
        assert!(r.aufteilen_noetig().unwrap());

        let e = r.scheiben_aufteilen(|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!((e.dateien, e.verdichtet, e.abgebrochen), (1, 1, false));
        alles_lesbar(&r, &ids);
        assert!(!r.aufteilen_noetig().unwrap());
        assert_eq!((bilder_in(&r, 1), bilder_in(&r, 2), bilder_in(&r, 3)), (4, 4, 2));
    }

    #[test]
    fn aufteilen_raeumt_bilder_ohne_verweis_weg() {
        let mut r = Register::im_speicher().unwrap();
        let ids = grosse_scheibe(&mut r);
        // Abbruch zwischen Scheibe und Verweis: ein Bild ohne Verweis
        r.scheibe(1).unwrap().execute("INSERT INTO vorschau (avif) VALUES (x'00')", []).unwrap();
        assert_eq!(bilder_in(&r, 1), 11);
        r.scheibe_grenze_setzen(45);
        r.scheiben_aufteilen(|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(bilder_in(&r, 1) + bilder_in(&r, 2) + bilder_in(&r, 3), 10);
        alles_lesbar(&r, &ids);
    }

    #[test]
    fn eine_allein_zu_grosse_datei_wird_nicht_geteilt() {
        let mut r = register_mit(&["A/a.pdf", "A/b.pdf"]);
        r.scheibe_grenze_setzen(25);
        let (a, b) = (id(&r, "A/a.pdf"), id(&r, "A/b.pdf"));
        r.vorschauen_speichern(a, 1, 100, &Ok(vec![v(30), v(30)])).unwrap();
        assert!(!r.aufteilen_noetig().unwrap());
        // Mit einer zweiten Datei daneben wandert nur diese
        r.scheibe_grenze_setzen(1000);
        r.vorschauen_speichern(b, 1, 100, &Ok(vec![v(5)])).unwrap();
        r.scheibe_grenze_setzen(25);
        let e = r.scheiben_aufteilen(|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!((e.dateien, e.bilder), (1, 1));
        assert_eq!((scheibe_von(&r, a), scheibe_von(&r, b)), (vec![1], vec![2]));
        assert!(!r.aufteilen_noetig().unwrap());
    }

    #[test]
    fn aufteilen_verkleinert_die_datei() {
        let tmp = tempfile::tempdir().unwrap();
        let alt = tmp.path().join("vorschau-0001.db");
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.abgleichen(&durchlauf(&["A/a.pdf", "A/b.pdf", "A/c.pdf", "A/d.pdf"])).unwrap();
            for p in ["A/a.pdf", "A/b.pdf", "A/c.pdf", "A/d.pdf"] {
                let i = id(&r, p);
                r.vorschauen_speichern(i, 1, 100, &Ok(vec![v(100_000)])).unwrap();
            }
        }
        let vorher = std::fs::metadata(&alt).unwrap().len();
        {
            let mut r = Register::oeffnen(tmp.path()).unwrap();
            r.scheibe_grenze_setzen(150_000);
            let e = r.scheiben_aufteilen(|_| {}, &AtomicBool::new(false)).unwrap();
            assert_eq!((e.dateien, e.neue_scheiben, e.verdichtet), (3, 3, 1));
        }
        let nachher = std::fs::metadata(&alt).unwrap().len();
        assert!(nachher < vorher / 3, "{vorher} -> {nachher}");
        for nr in 2..=4 {
            assert!(tmp.path().join(dateiname(nr)).is_file());
        }
        let r = Register::oeffnen(tmp.path()).unwrap();
        for p in ["A/a.pdf", "A/b.pdf", "A/c.pdf", "A/d.pdf"] {
            assert_eq!(r.vorschau(id(&r, p), 1).unwrap().unwrap().len(), 100_000);
        }
    }

}
