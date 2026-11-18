//! Vorschauen aller ausstehenden PDFs erzeugen (Stufe 1, v0.0.3).
//!
//! Ablauf in drei Teilen, die gleichzeitig laufen:
//!
//! - Ein Faden rendert die Seiten nacheinander. PDFium verträgt nur einen
//!   Aufrufer zur Zeit.
//! - Mehrere Fäden kodieren die Seiten als AVIF, je Seite ein eigener Encoder.
//!   Das ist der teure Teil.
//! - Der aufrufende Faden sammelt die Seiten je Datei und schreibt eine Datei
//!   erst ins Register, wenn alle ihre Seiten fertig sind.
//!
//! Ein abgebrochener Lauf setzt beim nächsten Mal fort. "Nur online"-Dateien
//! werden übergangen, wie beim Lesen der Inhalte.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::Serialize;

use crate::inhalte::Fortschritt;
use crate::vorschau::{Bild, Vorschau};
use crate::{Ergebnis, Konfiguration, Register};

/// Seiten, die gerendert auf das Kodieren warten dürfen, je Kodier-Faden.
/// Begrenzt den Speicher bei großen PDFs.
const PUFFER_JE_FADEN: usize = 2;

/// Meldung des Renderers, wenn mitten in einer Datei abgebrochen wurde. Die
/// Datei wird dann nicht gespeichert und bleibt ausstehend.
pub const ABGEBROCHEN: &str = "abgebrochen";

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct VorschauErgebnis {
    pub dauer_ms: u64,
    /// Ausstehende PDFs zu Beginn des Laufs.
    pub ausstehend: u64,
    /// PDFs, deren Vorschauen erzeugt wurden.
    pub erzeugt: u64,
    pub seiten: u64,
    pub bytes: u64,
    pub fehler: u64,
    pub nur_online: u64,
    /// Bilder, die aus den Scheiben entfernt wurden.
    pub aufgeraeumt: u64,
    pub abgebrochen: bool,
    pub messung: Messung,
}

/// Wo die Zeit eines Laufs bleibt. Zeiten in Millisekunden, über alle Fäden
/// summiert. Zeigt, ob das Rendern (ein Faden) oder das Kodieren bremst.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Messung {
    pub faeden: u64,
    /// Rendern einschließlich Öffnen der PDF, ohne Warten auf freie Kodier-Fäden.
    pub render_ms: u64,
    /// Renderer wartet, weil alle Kodier-Fäden belegt sind.
    pub render_wartet_ms: u64,
    pub kodier_ms: u64,
    /// Kodier-Fäden warten auf gerenderte Seiten.
    pub kodier_leer_ms: u64,
    /// Schreiben ins Register.
    pub schreib_ms: u64,
    /// Summe der Pixel aller gerenderten Seiten.
    pub pixel: u64,
}

/// Nachrichten an den sammelnden Faden.
enum Meldung {
    /// Datei fertig gerendert, mit Seitenzahl oder Fehler.
    Datei { nr: usize, seiten: Result<u32, String>, geaendert: i64, groesse: u64 },
    /// Datei übergangen, weil "nur online".
    Uebergangen { nr: usize },
    Seite { nr: usize, seite: u32, vorschau: Result<Vorschau, String> },
}

#[derive(Default)]
struct Sammlung {
    seiten: BTreeMap<u32, Result<Vorschau, String>>,
    kopf: Option<(Result<u32, String>, i64, u64)>,
}

/// Erzeugt die Vorschauen der ausstehenden PDFs.
///
/// - `rendern` rendert eine PDF und übergibt jede Seite einzeln an den
///   Rückruf. Liefert die Seitenzahl. Gibt der Rückruf `false` zurück, soll
///   `rendern` aufhören (Abbruch).
/// - `kodieren` macht aus einer Seite die Vorschau. Wird von mehreren Fäden
///   gleichzeitig aufgerufen.
/// - `faeden`: Anzahl der Kodier-Fäden.
pub fn erzeugen<R, K>(
    konfig: &Konfiguration,
    register: &mut Register,
    rendern: R,
    kodieren: K,
    faeden: usize,
    mut fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<VorschauErgebnis>
where
    R: FnMut(&Path, &mut dyn FnMut(Bild) -> bool) -> Result<u32, String> + Send,
    K: Fn(&Bild) -> Result<Vorschau, String> + Sync,
{
    let start = Instant::now();
    let mut e = VorschauErgebnis { aufgeraeumt: register.vorschauen_aufraeumen()?, ..Default::default() };
    let liste = register.vorschau_ausstehend()?;
    e.ausstehend = liste.len() as u64;
    let faeden = faeden.max(1);
    let mut rendern = rendern;
    let mut fehler_register: Option<crate::Fehler> = None;

    std::thread::scope(|s| {
        let (auftrag_tx, auftrag_rx) = mpsc::sync_channel::<(usize, u32, Bild)>(faeden * PUFFER_JE_FADEN);
        let auftrag_rx = Arc::new(Mutex::new(auftrag_rx));
        let (meldung_tx, meldung_rx) = mpsc::channel::<Meldung>();

        // Kodieren. Jeder Faden liefert (Kodieren, Leerlauf, Pixel).
        let mut kodier_faeden = Vec::with_capacity(faeden);
        for _ in 0..faeden {
            let rx = Arc::clone(&auftrag_rx);
            let tx = meldung_tx.clone();
            let kodieren = &kodieren;
            kodier_faeden.push(s.spawn(move || {
                let (mut kodier, mut leer, mut pixel) = (Duration::ZERO, Duration::ZERO, 0u64);
                loop {
                    let t = Instant::now();
                    let auftrag = rx.lock().map(|r| r.recv());
                    leer += t.elapsed();
                    let Ok(Ok((nr, seite, bild))) = auftrag else { break };
                    pixel += bild.breite as u64 * bild.hoehe as u64;
                    let t = Instant::now();
                    let vorschau = kodieren(&bild);
                    kodier += t.elapsed();
                    if tx.send(Meldung::Seite { nr, seite, vorschau }).is_err() {
                        break;
                    }
                }
                (kodier, leer, pixel)
            }));
        }

        // Rendern
        let liste_r = &liste;
        let cloud = &konfig.cloud_ordner;
        let tx = meldung_tx;
        let render_faden = s.spawn(move || {
            let mut abgebrochen = false;
            let (mut gesamt, mut wartet) = (Duration::ZERO, Duration::ZERO);
            for (nr, eintrag) in liste_r.iter().enumerate() {
                if abbrechen.load(Ordering::Relaxed) {
                    abgebrochen = true;
                    break;
                }
                let pfad = cloud.join(&eintrag.pfad);
                let meta = std::fs::metadata(&pfad).ok();
                let groesse = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                if eintrag.nur_online || groesse == 0 {
                    let _ = tx.send(Meldung::Uebergangen { nr });
                    continue;
                }
                let geaendert = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let mut seite = 0u32;
                let start = Instant::now();
                let seiten = rendern(&pfad, &mut |bild| {
                    if abbrechen.load(Ordering::Relaxed) {
                        return false;
                    }
                    seite += 1;
                    let t = Instant::now();
                    let ok = auftrag_tx.send((nr, seite, bild)).is_ok();
                    wartet += t.elapsed();
                    ok
                });
                gesamt += start.elapsed();
                // Abbruch mitten in der Datei: nichts melden, sie bleibt ausstehend.
                // Ihre schon übergebenen Seiten verwirft der Sammler.
                if matches!(&seiten, Err(f) if f == ABGEBROCHEN) {
                    abgebrochen = true;
                    break;
                }
                // Gemeldet werden die tatsächlich übergebenen Seiten
                let seiten = seiten.map(|_| seite);
                let _ = tx.send(Meldung::Datei { nr, seiten, geaendert, groesse });
            }
            (abgebrochen, gesamt.saturating_sub(wartet), wartet)
            // auftrag_tx und tx fallen hier weg: Kodier-Fäden und Sammler enden
        });

        // Sammeln und Schreiben
        let mut offen: HashMap<usize, Sammlung> = HashMap::new();
        let mut erledigt = 0u64;
        let mut schreiben = Duration::ZERO;
        for meldung in meldung_rx {
            let nr = match meldung {
                Meldung::Uebergangen { nr } => {
                    e.nur_online += 1;
                    erledigt += 1;
                    fortschritt(&Fortschritt { erledigt, gesamt: e.ausstehend, datei: liste[nr].pfad.clone() });
                    continue;
                }
                Meldung::Datei { nr, seiten, geaendert, groesse } => {
                    offen.entry(nr).or_default().kopf = Some((seiten, geaendert, groesse));
                    nr
                }
                Meldung::Seite { nr, seite, vorschau } => {
                    offen.entry(nr).or_default().seiten.insert(seite, vorschau);
                    nr
                }
            };
            let fertig = match &offen[&nr].kopf {
                Some((Ok(n), _, _)) => offen[&nr].seiten.len() as u32 == *n,
                // Bei einem Renderfehler zählen nur die Seiten, die schon übergeben waren
                Some((Err(_), _, _)) => true,
                None => false,
            };
            if !fertig || fehler_register.is_some() {
                continue;
            }
            let sammlung = offen.remove(&nr).expect("vorhanden");
            let (seiten, geaendert, groesse) = sammlung.kopf.expect("fertig");
            let ergebnis: Result<Vec<Vorschau>, String> = match seiten {
                Err(f) => Err(f),
                Ok(_) => sammlung.seiten.into_values().collect(),
            };
            match &ergebnis {
                Ok(v) => {
                    e.erzeugt += 1;
                    e.seiten += v.len() as u64;
                    e.bytes += v.iter().map(|v| v.avif.len() as u64).sum::<u64>();
                }
                Err(_) => e.fehler += 1,
            }
            let t = Instant::now();
            let gespeichert = register.vorschauen_speichern(liste[nr].id, geaendert, groesse, &ergebnis);
            schreiben += t.elapsed();
            if let Err(f) = gespeichert {
                // Weiterschreiben hat keinen Sinn. Rendern anhalten, Rest abwarten.
                fehler_register = Some(f);
                abbrechen.store(true, Ordering::Relaxed);
                continue;
            }
            erledigt += 1;
            fortschritt(&Fortschritt { erledigt, gesamt: e.ausstehend, datei: liste[nr].pfad.clone() });
        }
        let (abgebrochen, render, wartet) = render_faden.join().unwrap_or((true, Duration::ZERO, Duration::ZERO));
        e.abgebrochen = abgebrochen;
        let m = &mut e.messung;
        m.faeden = faeden as u64;
        m.render_ms = render.as_millis() as u64;
        m.render_wartet_ms = wartet.as_millis() as u64;
        m.schreib_ms = schreiben.as_millis() as u64;
        for f in kodier_faeden {
            if let Ok((kodier, leer, pixel)) = f.join() {
                m.kodier_ms += kodier.as_millis() as u64;
                m.kodier_leer_ms += leer.as_millis() as u64;
                m.pixel += pixel;
            }
        }
    });

    if let Some(f) = fehler_register {
        return Err(f);
    }
    // Ersetzte Vorschauen gleich aus den Scheiben nehmen
    e.aufgeraeumt += register.vorschauen_aufraeumen()?;
    e.dauer_ms = start.elapsed().as_millis() as u64;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Ersatz für PDFium: Seitenzahl aus dem Dateiinhalt, "kaputt" liefert einen Fehler.
    fn testrendern(pfad: &Path, seite: &mut dyn FnMut(Bild) -> bool) -> Result<u32, String> {
        let inhalt = fs::read_to_string(pfad).unwrap();
        if inhalt.starts_with("kaputt") {
            return Err("keine gültige PDF oder beschädigt".into());
        }
        let n: u32 = inhalt.trim().parse().unwrap();
        for i in 0..n {
            if !seite(Bild { breite: 4, hoehe: 4, rgb: vec![i as u8; 48] }) {
                return Err(ABGEBROCHEN.into());
            }
        }
        Ok(n)
    }

    /// Ersatz für den Encoder: das Bild selbst, mit kleiner Verzögerung in wechselnder Länge.
    fn testkodieren(bild: &Bild) -> Result<Vorschau, String> {
        std::thread::sleep(std::time::Duration::from_millis((bild.rgb[0] % 3) as u64));
        Ok(Vorschau { breite: bild.breite, hoehe: bild.hoehe, farbig: false, avif: bild.rgb[..1 + bild.rgb[0] as usize].to_vec() })
    }

    fn aufbauen() -> (tempfile::TempDir, Konfiguration, Register) {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("A");
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("a.pdf"), "3").unwrap();
        fs::write(a.join("b.pdf"), "12").unwrap();
        fs::write(a.join("kaputt.pdf"), "kaputt").unwrap();
        fs::write(a.join("online.pdf"), "").unwrap();
        let konfig = Konfiguration {
            cloud_ordner: tmp.path().to_path_buf(),
            verzeichnisse: vec!["A".into()],
            register_ordner: None,
        };
        let mut register = Register::oeffnen(&tmp.path().join("register")).unwrap();
        crate::dateibaum_einlesen(&konfig, &mut register).unwrap();
        (tmp, konfig, register)
    }

    fn lauf(konfig: &Konfiguration, register: &mut Register) -> VorschauErgebnis {
        erzeugen(konfig, register, testrendern, testkodieren, 3, |_| {}, &AtomicBool::new(false)).unwrap()
    }

    fn id(r: &Register, pfad: &str) -> i64 {
        r.db.query_row("SELECT id FROM datei WHERE pfad = ?1", [pfad], |z| z.get(0)).unwrap()
    }

    #[test]
    fn erzeugt_alle_seiten_in_der_richtigen_reihenfolge() {
        let (_tmp, konfig, mut register) = aufbauen();
        let mut meldungen = 0;
        let e = erzeugen(&konfig, &mut register, testrendern, testkodieren, 3, |_| meldungen += 1, &AtomicBool::new(false))
            .unwrap();
        assert_eq!((e.ausstehend, e.erzeugt, e.seiten, e.fehler, e.nur_online), (4, 2, 15, 1, 1));
        assert_eq!(meldungen, 4);
        assert_eq!((e.messung.faeden, e.messung.pixel), (3, 15 * 16));
        let b = id(&register, "A/b.pdf");
        for nr in 1..=12u32 {
            // Seite nr trägt den Wert nr-1, Länge nr
            assert_eq!(register.vorschau(b, nr).unwrap().unwrap().len(), nr as usize);
        }
        let s = register.stand().unwrap().vorschau;
        assert_eq!((s.erzeugt, s.ausstehend, s.ausstehend_online, s.fehler, s.seiten), (3, 1, 1, 1, 15));
        // Zweiter Lauf: nur die Online-Datei
        assert_eq!(lauf(&konfig, &mut register).ausstehend, 1);
    }

    #[test]
    fn geaenderte_datei_ersetzt_ihre_vorschauen() {
        let (tmp, konfig, mut register) = aufbauen();
        lauf(&konfig, &mut register);
        let a = tmp.path().join("A/a.pdf");
        fs::write(&a, "1").unwrap();
        let spaeter = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        fs::File::options().write(true).open(&a).unwrap().set_modified(spaeter).unwrap();
        crate::dateibaum_einlesen(&konfig, &mut register).unwrap();
        let e = lauf(&konfig, &mut register);
        assert_eq!((e.erzeugt, e.seiten, e.aufgeraeumt), (1, 1, 3));
        let s = register.stand().unwrap().vorschau;
        assert_eq!(s.seiten, 13);
        assert_eq!(s.scheiben[0].vorschauen, 13);
    }

    #[test]
    fn abbruch_behaelt_fertige_dateien() {
        let (_tmp, konfig, mut register) = aufbauen();
        let abbrechen = AtomicBool::new(false);
        // Abbruch, sobald die erste Datei gerendert ist
        let rendern = |pfad: &Path, seite: &mut dyn FnMut(Bild) -> bool| {
            let n = testrendern(pfad, seite);
            abbrechen.store(true, Ordering::Relaxed);
            n
        };
        let e = erzeugen(&konfig, &mut register, rendern, testkodieren, 2, |_| {}, &abbrechen).unwrap();
        assert!(e.abgebrochen);
        assert_eq!((e.erzeugt, e.seiten), (1, 3));
        // Fortsetzen: die übrigen drei
        assert_eq!(lauf(&konfig, &mut register).ausstehend, 3);
    }

    #[test]
    fn abbruch_mitten_in_der_datei_speichert_sie_nicht() {
        let (_tmp, konfig, mut register) = aufbauen();
        let abbrechen = AtomicBool::new(false);
        // a.pdf (3 Seiten) fertig, Abbruch nach der 5. Seite von b.pdf (12 Seiten)
        let mut seiten = 0;
        let rendern = |pfad: &Path, seite: &mut dyn FnMut(Bild) -> bool| {
            testrendern(pfad, &mut |bild| {
                seiten += 1;
                if seiten == 3 + 5 {
                    abbrechen.store(true, Ordering::Relaxed);
                }
                seite(bild)
            })
        };
        let e = erzeugen(&konfig, &mut register, rendern, testkodieren, 2, |_| {}, &abbrechen).unwrap();
        assert!(e.abgebrochen);
        assert_eq!((e.erzeugt, e.seiten, e.fehler), (1, 3, 0));
        let s = register.stand().unwrap().vorschau;
        assert_eq!((s.erzeugt, s.fehler, s.seiten), (1, 0, 3));
        assert_eq!(lauf(&konfig, &mut register).ausstehend, 3);
    }
}
