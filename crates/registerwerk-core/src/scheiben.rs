//! Vorschauen im Register: Scheiben anlegen, Vorschauen speichern, lesen und
//! aufräumen.
//!
//! Eine Scheibe ist eine eigene SQLite-Datei mit den AVIF-Bildern. Die
//! Index-Datenbank hält je Seite einen Verweis (Scheibe und Schlüssel).
//! Geschrieben wird erst die Scheibe, dann der Verweis. Bricht das Programm
//! dazwischen ab, bleibt höchstens ein Bild ohne Verweis zurück. Das stört
//! nicht und verschwindet mit dem nächsten Neuaufbau.

use std::cell::RefMut;
use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

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
const AUSSTEHEND: &str = concat!("stufe = 1 AND endung IN ", crate::vorschau_sql!(), " AND (
        vorschau_verfahren IS NULL OR vorschau_verfahren < ?1
        OR vorschau_geaendert IS NOT geaendert
        OR (groesse IS NOT NULL AND vorschau_groesse IS NOT groesse))");

impl Register {
    /// Setzt die Größe, ab der eine Scheibe voll ist. Für Tests.
    pub fn scheibe_grenze_setzen(&mut self, bytes: u64) {
        self.scheibe_grenze = bytes;
    }

    /// Öffnet die Scheibe `nr` und legt sie bei Bedarf an.
    fn scheibe(&self, nr: i64) -> Ergebnis<RefMut<'_, Connection>> {
        let mut offen = self.scheiben.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(platz) = offen.entry(nr) {
            let db = match self.ordner() {
                Some(ordner) => Connection::open(ordner.join(dateiname(nr)))?,
                None => Connection::open_in_memory()?,
            };
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS vorschau (
                     -- AUTOINCREMENT: ein Schlüssel wird nie wieder vergeben
                     id    INTEGER PRIMARY KEY AUTOINCREMENT,
                     avif  BLOB NOT NULL
                 );
                 PRAGMA user_version = 1;",
            )?;
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
}
