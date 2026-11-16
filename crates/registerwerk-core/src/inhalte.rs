//! Inhalte der PDFs lesen und ins Register schreiben (Stufe 1, v0.0.2).
//!
//! Gelesen werden nur PDFs, deren Inhalt noch fehlt oder nicht zum aktuellen
//! Stand der Datei passt. Ein abgebrochener Lauf setzt beim nächsten Mal dort
//! fort, wo er aufgehört hat.
//!
//! "Nur online"-Dateien werden übergangen. Das Öffnen würde den Download
//! auslösen. Sie werden gelesen, sobald sie lokal vorliegen.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use serde::Serialize;

use crate::pdf::Inhalt;
use crate::{Ergebnis, Konfiguration, Register};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Fortschritt {
    pub erledigt: u64,
    pub gesamt: u64,
    /// Pfad der Datei, die gerade gelesen wurde, relativ zum Cloud-Ordner.
    pub datei: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct InhaltErgebnis {
    pub dauer_ms: u64,
    /// Ausstehende PDFs zu Beginn des Laufs.
    pub ausstehend: u64,
    pub gelesen: u64,
    pub seiten: u64,
    pub fehler: u64,
    /// Übergangen, weil "nur online".
    pub nur_online: u64,
    pub abgebrochen: bool,
}

/// Liest die ausstehenden PDFs mit `leser` und schreibt das Ergebnis ins
/// Register. `fortschritt` wird nach jeder Datei aufgerufen. Ist `abbrechen`
/// gesetzt, endet der Lauf nach der aktuellen Datei. Bereits Gelesenes bleibt
/// gespeichert.
pub fn lesen(
    konfig: &Konfiguration,
    register: &mut Register,
    mut leser: impl FnMut(&Path) -> Result<Inhalt, String>,
    mut fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<InhaltErgebnis> {
    let start = Instant::now();
    let liste = register.ausstehend()?;
    let mut e = InhaltErgebnis { ausstehend: liste.len() as u64, ..Default::default() };

    for (i, eintrag) in liste.iter().enumerate() {
        if abbrechen.load(Ordering::Relaxed) {
            e.abgebrochen = true;
            break;
        }
        let pfad = konfig.cloud_ordner.join(&eintrag.pfad);
        // Vor dem Öffnen noch einmal prüfen: 0 Byte heißt "nur online oder leer".
        // Öffnen würde den Download auslösen.
        let meta = std::fs::metadata(&pfad).ok();
        let groesse = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        if eintrag.nur_online || groesse == 0 {
            e.nur_online += 1;
        } else {
            let geaendert = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let inhalt = leser(&pfad);
            match &inhalt {
                Ok(i) => {
                    e.gelesen += 1;
                    e.seiten += i.seiten.len() as u64;
                }
                Err(_) => e.fehler += 1,
            }
            register.inhalt_speichern(eintrag.id, geaendert, groesse, &inhalt)?;
        }
        fortschritt(&Fortschritt { erledigt: i as u64 + 1, gesamt: e.ausstehend, datei: eintrag.pfad.clone() });
    }
    e.dauer_ms = start.elapsed().as_millis() as u64;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::Seite;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn seite(nr: u32, text: &str, bildanteil: f32) -> Seite {
        Seite { nr, text: text.into(), bildanteil }
    }

    /// Ersatz für PDFium: liefert je Dateiname einen festen Inhalt.
    fn testleser(pfad: &Path) -> Result<Inhalt, String> {
        let name = pfad.file_name().unwrap().to_string_lossy().to_string();
        match name.as_str() {
            "kaputt.pdf" => Err("keine gültige PDF oder beschädigt".into()),
            "Brief ohne Datum.pdf" => Ok(Inhalt {
                seiten: vec![
                    seite(1, "Sehr geehrter Herr Sieber, anbei die Rechnung über 123,45 EUR.", 1.0),
                    seite(2, "", 1.0),
                ],
                erstellt: Some("D:20190515120000+02'00'".into()),
            }),
            _ => Ok(Inhalt {
                seiten: vec![seite(1, "ÿ§ 3a %$ Ü1 ~~ ¶ q ® 7x ±± ab ¤¤ ½x", 0.0)],
                erstellt: None,
            }),
        }
    }

    fn aufbauen() -> (tempfile::TempDir, Konfiguration, Register) {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("A");
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("Brief ohne Datum.pdf"), b"%PDF brief").unwrap();
        fs::write(a.join("2020-01-02 Auszug.pdf"), b"%PDF auszug").unwrap();
        fs::write(a.join("kaputt.pdf"), b"kaputt").unwrap();
        fs::write(a.join("online.pdf"), b"").unwrap();
        fs::write(a.join("notiz.txt"), b"keine pdf").unwrap();
        let konfig = Konfiguration {
            cloud_ordner: tmp.path().to_path_buf(),
            verzeichnisse: vec!["A".into()],
            register_ordner: None,
        };
        let mut register = Register::im_speicher().unwrap();
        crate::dateibaum_einlesen(&konfig, &mut register).unwrap();
        (tmp, konfig, register)
    }

    fn lauf(konfig: &Konfiguration, register: &mut Register) -> InhaltErgebnis {
        lesen(konfig, register, testleser, |_| {}, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn liest_ausstehende_pdfs_und_ueberspringt_online() {
        let (_tmp, konfig, mut register) = aufbauen();
        assert_eq!(register.stand().unwrap().inhalt.ausstehend, 4);

        let mut meldungen = 0;
        let e = lesen(&konfig, &mut register, testleser, |_| meldungen += 1, &AtomicBool::new(false)).unwrap();
        assert_eq!((e.ausstehend, e.gelesen, e.fehler, e.nur_online, e.seiten), (4, 2, 1, 1, 3));
        assert_eq!(meldungen, 4);

        let s = register.stand().unwrap();
        let i = &s.inhalt;
        assert_eq!((i.pdfs, i.gelesen, i.ausstehend, i.ausstehend_online, i.fehler), (4, 3, 1, 1, 1));
        assert_eq!((i.seiten, i.seiten_ohne_text, i.seiten_schwach, i.seiten_scan), (3, 1, 1, 2));
        assert_eq!((i.pdfs_ohne_text, i.pdfs_schwach), (0, 1));
        assert_eq!(i.fehler_liste[0].pfad, "A/kaputt.pdf");
        // Datum: Dateiname vor PDF vor Änderungsdatum
        assert_eq!((s.mit_datum, s.datum_pdf, s.datum_geaendert), (1, 1, 3));
        assert!(s.jahre.iter().any(|j| j.jahr == "2019"));

        // Zweiter Lauf: nur die Online-Datei steht noch aus
        let e = lauf(&konfig, &mut register);
        assert_eq!((e.ausstehend, e.gelesen, e.nur_online), (1, 0, 1));
    }

    #[test]
    fn geaenderte_datei_wird_neu_gelesen() {
        let (tmp, konfig, mut register) = aufbauen();
        lauf(&konfig, &mut register);

        let brief = tmp.path().join("A/Brief ohne Datum.pdf");
        fs::write(&brief, b"%PDF brief, neue Fassung").unwrap();
        let spaeter = SystemTime::now() + Duration::from_secs(3600);
        fs::File::options().write(true).open(&brief).unwrap().set_modified(spaeter).unwrap();
        crate::dateibaum_einlesen(&konfig, &mut register).unwrap();

        let ausstehend = register.ausstehend().unwrap();
        let pfade: Vec<_> = ausstehend.iter().map(|a| a.pfad.as_str()).collect();
        assert_eq!(pfade, vec!["A/Brief ohne Datum.pdf", "A/online.pdf"]);
        let e = lauf(&konfig, &mut register);
        assert_eq!(e.gelesen, 1);
        // Seiten werden ersetzt, nicht verdoppelt
        assert_eq!(register.stand().unwrap().inhalt.seiten, 3);
    }

    #[test]
    fn abbruch_behaelt_gelesenes() {
        let (_tmp, konfig, mut register) = aufbauen();
        let abbrechen = AtomicBool::new(false);
        let e = lesen(
            &konfig,
            &mut register,
            testleser,
            |f| {
                if f.erledigt == 2 {
                    abbrechen.store(true, Ordering::Relaxed)
                }
            },
            &abbrechen,
        )
        .unwrap();
        assert!(e.abgebrochen);
        assert_eq!(e.gelesen + e.fehler + e.nur_online, 2);
        // Fortsetzen: die beiden übrigen
        assert_eq!(lauf(&konfig, &mut register).ausstehend, 2);
    }

    #[test]
    fn entfernte_datei_nimmt_ihre_seiten_mit() {
        let (tmp, konfig, mut register) = aufbauen();
        lauf(&konfig, &mut register);
        fs::remove_file(tmp.path().join("A/Brief ohne Datum.pdf")).unwrap();
        crate::dateibaum_einlesen(&konfig, &mut register).unwrap();
        assert_eq!(register.stand().unwrap().inhalt.seiten, 1);
    }
}
