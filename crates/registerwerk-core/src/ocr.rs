//! Texterkennung (OCR) für Seiten ohne oder mit schwachem Text
//! (Konzept, Abschnitt 7; Stufe 5, erster Teil in v0.0.4).
//!
//! Erkannt werden nur Seiten **ohne Text** (höchstens 10 Zeichen
//! eingebetteter Text, seit v0.0.5). Der erkannte Text gilt, wenn er mehr als
//! 10 Zeichen hat.
//!
//! Seiten mit schwachem Text (Güte unter 60 %) behalten ihren eingebetteten
//! Text. Im Pilot (v0.0.4) war er nicht schlechter, oft besser als das
//! Ergebnis von Tesseract. Die Fälle `ScanSchwach` und `DigitalSchwach`
//! werden weiter bestimmt und angezeigt, aber nicht erkannt.
//!
//! Der erkannte Text kommt ins Register (Tabelle `ocr`), mit der Lage jedes
//! Worts auf der Seite (Spalte `woerter`). Die PDF verändert dieser Schritt
//! nicht. Das Zurückschreiben der Textebene in Scans übernimmt
//! [`textebene`](crate::textebene). Der eingebettete Text bleibt in `seite`
//! erhalten. Welcher Text gilt, zeigt die Sicht `seite_text`.
//!
//! Ablauf wie bei den Vorschauen: Ein Faden rendert die betroffenen Seiten mit
//! 300 dpi in Graustufen, mehrere Fäden erkennen, je Faden ein eigenes
//! Tesseract. Der aufrufende Faden sammelt je Datei und schreibt.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use pdfium_render::prelude::*;
use rusqlite::params;
use serde::Serialize;

use crate::guete::{self, Zaehlung};
use crate::inhalte::Fortschritt;
use crate::register::{Register, INHALT_VERFAHREN};
use crate::woerter::{self, LageWort};
use crate::{pdf, Ergebnis, Konfiguration};

/// Auflösung, mit der Seiten für die OCR gerendert werden. Tesseract liefert
/// mit 300 dpi deutlich bessere Ergebnisse als mit 200 dpi.
pub const DPI: u32 = 300;
/// Längste Kante beim Rendern für die OCR. Begrenzt Übergrößen wie Pläne
/// (A2 bei 300 dpi). **Annahme**
pub const MAX_KANTE: u32 = 7016;
/// Mindestgewinn an Güte, damit ein schwacher Text ersetzt wird.
/// **Annahme** aus dem Konzept.
pub const VERBESSERUNG: f64 = 0.10;
/// Sprachen für Tesseract.
pub const SPRACHEN: &str = "deu+eng";
/// Satz der Sprachdaten: "standard" (tessdata) oder "best" (tessdata_best).
/// Wird nach der Messung in v0.0.4 festgelegt. Ändern heißt
/// [`OCR_VERFAHREN`] erhöhen.
pub const MODELLE: &str = "standard";

/// Version des OCR-Verfahrens. Erhöhen, wenn sich Auflösung, Sprachdaten,
/// Tesseract-Version oder die Übernahmeregel ändern. Dann wird neu erkannt.
pub const OCR_VERFAHREN: i64 = 3;
// 1: 300 dpi Graustufen, Tesseract 5.5.2 LSTM, deu+eng, Sprachdaten MODELLE,
//    Seiten ohne Text und mit schwachem Text
// 2: wie 1, nur noch Seiten ohne Text (unter 20 Zeichen)
// 3: wie 2, Grenze ohne Text bei höchstens 10 Zeichen, Lage der Wörter

/// Ordner der Sprachdaten relativ zum Projekt, je Satz ein Unterordner.
pub const ENTWICKLUNGS_ORDNER: &str = "bibliotheken/tesseract/tessdata";

/// Seiten, die gerendert auf die Erkennung warten dürfen, je Faden. Eine
/// A4-Seite mit 300 dpi hat rund 9 MB in Graustufen.
const PUFFER_JE_FADEN: usize = 1;

/// Warum eine Seite erkannt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fall {
    OhneText,
    ScanSchwach,
    DigitalSchwach,
}

impl Fall {
    pub fn als_text(self) -> &'static str {
        match self {
            Fall::OhneText => "ohne_text",
            Fall::ScanSchwach => "scan_schwach",
            Fall::DigitalSchwach => "digital_schwach",
        }
    }

    fn aus_text(t: &str) -> Option<Self> {
        match t {
            "ohne_text" => Some(Fall::OhneText),
            "scan_schwach" => Some(Fall::ScanSchwach),
            "digital_schwach" => Some(Fall::DigitalSchwach),
            _ => None,
        }
    }

    /// Fall einer Seite nach eingebettetem Text und Bildanteil, `None` wenn
    /// der Text gut genug ist.
    pub fn bestimmen(z: &Zaehlung, bildanteil: f32) -> Option<Self> {
        if z.ohne_text() {
            Some(Fall::OhneText)
        } else if z.guete().unwrap_or(0.0) < guete::SCHWELLE {
            Some(if bildanteil >= pdf::SCAN_FLAECHE { Fall::ScanSchwach } else { Fall::DigitalSchwach })
        } else {
            None
        }
    }
}

/// Übernahmeregel: Gilt der erkannte Text statt des eingebetteten?
pub fn uebernehmen(fall: Fall, alt: &Zaehlung, neu: &Zaehlung) -> bool {
    if neu.ohne_text() {
        return false;
    }
    match fall {
        Fall::OhneText => true,
        Fall::ScanSchwach | Fall::DigitalSchwach => {
            neu.guete().unwrap_or(0.0) >= alt.guete().unwrap_or(0.0) + VERBESSERUNG - 1e-9
        }
    }
}

/// Eine Seite in Graustufen, ein Byte je Pixel, ohne Lücken zwischen den Zeilen.
#[derive(Debug, Clone)]
pub struct Graubild {
    pub breite: u32,
    pub hoehe: u32,
    pub grau: Vec<u8>,
}

/// Ergebnis der Erkennung einer Seite.
#[derive(Debug, Clone, PartialEq)]
pub struct Erkannt {
    pub text: String,
    pub konfidenz: i32,
    pub woerter: Vec<LageWort>,
}

/// Eine Seite, die erkannt werden soll.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrSeite {
    pub nr: u32,
    pub fall: Fall,
    /// Zählung des eingebetteten Textes.
    pub alt: Zaehlung,
}

/// Eine PDF mit Seiten, die erkannt werden sollen.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrAuftrag {
    pub id: i64,
    pub pfad: String,
    pub nur_online: bool,
    /// Stand der Datei, zu dem der Inhalt gelesen wurde.
    pub inhalt_geaendert: i64,
    pub inhalt_groesse: Option<i64>,
    pub seiten: Vec<OcrSeite>,
}

/// Ergebnis für eine Seite, wie es ins Register kommt.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrErgebnisSeite {
    pub nr: u32,
    pub fall: Fall,
    pub text: String,
    pub zaehlung: Zaehlung,
    pub konfidenz: i32,
    pub dauer_ms: u64,
    pub uebernommen: bool,
    /// Lage der Wörter, gepackt ([`woerter::packen`]). Nur bei übernommenem
    /// Text, sonst wird sie nicht gebraucht.
    pub woerter: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct OcrErgebnis {
    pub dauer_ms: u64,
    /// PDFs mit zu erkennenden Seiten zu Beginn des Laufs.
    pub ausstehend: u64,
    pub erkannt: u64,
    pub seiten: u64,
    /// Seiten, deren erkannter Text übernommen wurde.
    pub uebernommen: u64,
    pub fehler: u64,
    pub nur_online: u64,
    /// Übergangen, weil sich die Datei seit dem Lesen des Inhalts geändert hat.
    /// Sie wird nach dem nächsten Einlesen neu gelesen und dann erkannt.
    pub veraendert: u64,
    pub abgebrochen: bool,
    pub messung: OcrMessung,
}

/// Wo die Zeit bleibt, in Millisekunden über alle Fäden summiert.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct OcrMessung {
    pub faeden: u64,
    pub render_ms: u64,
    pub render_wartet_ms: u64,
    pub erkenn_ms: u64,
    pub erkenn_leer_ms: u64,
    pub schreib_ms: u64,
    pub pixel: u64,
}

/// Kennzahlen zur OCR im Register.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct OcrStand {
    /// PDFs mit Seiten, die erkannt werden müssen.
    pub pdfs: u64,
    pub erledigt: u64,
    pub ausstehend: u64,
    pub ausstehend_online: u64,
    pub fehler: u64,
    /// Seiten ohne Text: werden erkannt.
    pub seiten_ohne_text: u64,
    /// Seiten mit schwachem Text: werden nicht erkannt, nur gezählt.
    pub seiten_scan_schwach: u64,
    pub seiten_digital_schwach: u64,
    pub seiten_erkannt: u64,
    pub seiten_uebernommen: u64,
    /// Summe der Erkennungszeit über alle erkannten Seiten.
    pub erkenn_ms: u64,
    /// Seiten ohne Text und mit schwachem Text nach der OCR (gültiger Text).
    pub nachher_ohne_text: u64,
    pub nachher_schwach: u64,
    pub fehler_liste: Vec<crate::register::FehlerZeile>,
}

/// Beide Texte einer Seite zum Vergleich in der Oberfläche.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeitenText {
    pub nr: u32,
    pub pdf_text: String,
    pub pdf_guete: Option<f64>,
    pub pdf_zeichen: u64,
    pub bildanteil: f64,
    pub fall: Option<Fall>,
    pub ocr_text: Option<String>,
    pub ocr_guete: Option<f64>,
    pub ocr_zeichen: Option<u64>,
    pub ocr_konfidenz: Option<i32>,
    pub ocr_ms: Option<u64>,
    pub uebernommen: bool,
}

/// Bedingung für Seiten, die erkannt werden müssen (Spalten aus `seite`):
/// nur Seiten ohne Text.
fn bedingung_seite(s: &str) -> String {
    format!("({s}.zeichen < {min})", min = guete::MIN_ZEICHEN)
}

/// Bedingung für PDFs und Bilder, deren OCR (neu) laufen muss. Der Inhalt muss aktuell
/// gelesen sein. `?1` ist [`INHALT_VERFAHREN`], `?2` [`OCR_VERFAHREN`].
pub(crate) fn bedingung_datei() -> String {
    format!(
        "d.endung IN {} AND d.inhalt_fehler IS NULL
         AND d.inhalt_verfahren >= ?1 AND d.inhalt_geaendert IS d.geaendert
         AND (d.groesse IS NULL OR d.inhalt_groesse IS d.groesse)
         AND EXISTS (SELECT 1 FROM seite s WHERE s.datei = d.id AND {})",
        crate::ocr_sql!(),
        bedingung_seite("s")
    )
}

pub(crate) const OCR_OFFEN: &str = "(d.ocr_verfahren IS NULL OR d.ocr_verfahren < ?2 OR d.ocr_geaendert IS NOT d.inhalt_geaendert)";

impl Register {
    /// PDFs mit Seiten, die erkannt werden müssen, samt dieser Seiten.
    pub fn ocr_ausstehend(&self) -> Ergebnis<Vec<OcrAuftrag>> {
        let sql = format!(
            "SELECT d.id, d.pfad, d.nur_online, d.inhalt_geaendert, d.inhalt_groesse FROM datei d
             WHERE {} AND {OCR_OFFEN} ORDER BY d.pfad",
            bedingung_datei()
        );
        let mut liste = self
            .db
            .prepare(&sql)?
            .query_map(params![INHALT_VERFAHREN, OCR_VERFAHREN], |z| {
                Ok(OcrAuftrag {
                    id: z.get(0)?,
                    pfad: z.get(1)?,
                    nur_online: z.get(2)?,
                    inhalt_geaendert: z.get(3)?,
                    inhalt_groesse: z.get(4)?,
                    seiten: Vec::new(),
                })
            })?
            .filter(|a| a.as_ref().map_or(true, |a| self.in_auswahl(a.id)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seiten = self.db.prepare(&format!(
            "SELECT s.nr, s.zeichen, s.tokens, s.wortartig, s.bildanteil FROM seite s
             WHERE s.datei = ?1 AND {} ORDER BY s.nr",
            bedingung_seite("s")
        ))?;
        for auftrag in &mut liste {
            auftrag.seiten = seiten
                .query_map([auftrag.id], |z| {
                    let alt = Zaehlung {
                        zeichen: z.get::<_, i64>(1)? as usize,
                        tokens: z.get::<_, i64>(2)? as usize,
                        wortartig: z.get::<_, i64>(3)? as usize,
                    };
                    let bildanteil: f64 = z.get(4)?;
                    Ok((z.get::<_, u32>(0)?, alt, bildanteil))
                })?
                .filter_map(|r| match r {
                    Ok((nr, alt, b)) => Fall::bestimmen(&alt, b as f32)
                        .filter(|f| *f == Fall::OhneText)
                        .map(|fall| Ok(OcrSeite { nr, fall, alt })),
                    Err(e) => Some(Err(e)),
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(liste)
    }

    /// Entfernt Erkennungen eines älteren Verfahrens. Dateien, die nach dem
    /// aktuellen Verfahren nichts mehr zu erkennen haben, behalten sonst
    /// veraltete Texte. Liefert die Zahl der betroffenen Dateien.
    pub fn ocr_veraltete_entfernen(&mut self) -> Ergebnis<u64> {
        let tx = self.db.transaction()?;
        tx.execute(
            "DELETE FROM ocr WHERE datei IN (SELECT id FROM datei WHERE ocr_verfahren < ?1)",
            [OCR_VERFAHREN],
        )?;
        let n = tx.execute(
            "UPDATE datei SET ocr_geaendert = NULL, ocr_groesse = NULL, ocr_verfahren = NULL, ocr_fehler = NULL
             WHERE ocr_verfahren < ?1",
            [OCR_VERFAHREN],
        )?;
        tx.commit()?;
        Ok(n as u64)
    }

    /// Speichert die Erkennung einer Datei und ersetzt einen früheren Stand.
    /// `geaendert` ist der Stand der Datei, zu dem ihr Inhalt gelesen wurde.
    pub fn ocr_speichern(
        &mut self,
        id: i64,
        geaendert: i64,
        groesse: Option<i64>,
        ergebnis: &Result<Vec<OcrErgebnisSeite>, String>,
    ) -> Ergebnis<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM ocr WHERE datei = ?1", [id])?;
        let fehler = match ergebnis {
            Ok(seiten) => {
                let mut einfuegen = tx.prepare(
                    "INSERT INTO ocr (datei, nr, fall, text, zeichen, tokens, wortartig, konfidenz, dauer_ms, uebernommen,
                                      woerter)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )?;
                for s in seiten {
                    einfuegen.execute(params![
                        id,
                        s.nr,
                        s.fall.als_text(),
                        s.text,
                        s.zaehlung.zeichen as i64,
                        s.zaehlung.tokens as i64,
                        s.zaehlung.wortartig as i64,
                        s.konfidenz,
                        s.dauer_ms as i64,
                        s.uebernommen,
                        s.woerter
                    ])?;
                }
                None
            }
            Err(f) => Some(f.clone()),
        };
        tx.execute(
            "UPDATE datei SET ocr_geaendert = ?2, ocr_groesse = ?3, ocr_verfahren = ?4, ocr_fehler = ?5 WHERE id = ?1",
            params![id, geaendert, groesse, OCR_VERFAHREN, fehler],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn ocr_stand(&self) -> Ergebnis<OcrStand> {
        let zahl = |z: &rusqlite::Row, i: usize| -> rusqlite::Result<u64> {
            Ok(z.get::<_, Option<i64>>(i)?.unwrap_or(0) as u64)
        };
        let mut s = OcrStand::default();
        let sql = format!(
            "SELECT count(*), sum(NOT {o}), sum({o}), sum({o} AND d.nur_online),
                    sum(NOT {o} AND d.ocr_fehler IS NOT NULL)
             FROM datei d WHERE {b}",
            o = OCR_OFFEN,
            b = bedingung_datei()
        );
        (s.pdfs, s.erledigt, s.ausstehend, s.ausstehend_online, s.fehler) =
            self.db.query_row(&sql, params![INHALT_VERFAHREN, OCR_VERFAHREN], |z| {
                Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?, zahl(z, 3)?, zahl(z, 4)?))
            })?;
        // Schwacher Text wird nicht erkannt, aber gezählt
        (s.seiten_ohne_text, s.seiten_scan_schwach, s.seiten_digital_schwach) = self.db.query_row(
            "SELECT sum(zeichen < ?1),
                    sum(zeichen >= ?1 AND wortartig < ?3 * tokens AND bildanteil >= ?2),
                    sum(zeichen >= ?1 AND wortartig < ?3 * tokens AND bildanteil < ?2)
             FROM seite",
            params![guete::MIN_ZEICHEN as i64, pdf::SCAN_FLAECHE as f64, guete::SCHWELLE],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?)),
        )?;
        (s.seiten_erkannt, s.seiten_uebernommen, s.erkenn_ms) = self.db.query_row(
            "SELECT count(*), sum(uebernommen), sum(dauer_ms) FROM ocr",
            [],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?, zahl(z, 2)?)),
        )?;
        (s.nachher_ohne_text, s.nachher_schwach) = self.db.query_row(
            "SELECT sum(zeichen < ?1), sum(zeichen >= ?1 AND wortartig < ?2 * tokens) FROM seite_text",
            params![guete::MIN_ZEICHEN as i64, guete::SCHWELLE],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?)),
        )?;
        s.fehler_liste = self
            .db
            .prepare("SELECT pfad, ocr_fehler FROM datei WHERE ocr_fehler IS NOT NULL ORDER BY pfad LIMIT 50")?
            .query_map([], |z| Ok(crate::register::FehlerZeile { pfad: z.get(0)?, meldung: z.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(s)
    }

    /// Eingebetteter und erkannter Text aller Seiten einer Datei.
    pub fn seitentexte(&self, datei: i64) -> Ergebnis<Vec<SeitenText>> {
        let guete_aus = |w: i64, t: i64| (t > 0).then(|| w as f64 / t as f64);
        let liste = self
            .db
            .prepare(
                "SELECT s.nr, s.text, s.zeichen, s.tokens, s.wortartig, s.bildanteil,
                        o.fall, o.text, o.zeichen, o.tokens, o.wortartig, o.konfidenz, o.dauer_ms,
                        coalesce(o.uebernommen, 0)
                 FROM seite s LEFT JOIN ocr o ON o.datei = s.datei AND o.nr = s.nr
                 WHERE s.datei = ?1 ORDER BY s.nr",
            )?
            .query_map([datei], |z| {
                let alt = Zaehlung {
                    zeichen: z.get::<_, i64>(2)? as usize,
                    tokens: z.get::<_, i64>(3)? as usize,
                    wortartig: z.get::<_, i64>(4)? as usize,
                };
                let bildanteil: f64 = z.get(5)?;
                let fall_ocr: Option<String> = z.get(6)?;
                let ocr_tokens: Option<i64> = z.get(9)?;
                let ocr_wortartig: Option<i64> = z.get(10)?;
                Ok(SeitenText {
                    nr: z.get(0)?,
                    pdf_text: z.get(1)?,
                    pdf_guete: alt.guete(),
                    pdf_zeichen: alt.zeichen as u64,
                    bildanteil,
                    fall: fall_ocr
                        .as_deref()
                        .and_then(Fall::aus_text)
                        .or_else(|| Fall::bestimmen(&alt, bildanteil as f32)),
                    ocr_text: z.get(7)?,
                    ocr_guete: ocr_tokens.zip(ocr_wortartig).and_then(|(t, w)| guete_aus(w, t)),
                    ocr_zeichen: z.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                    ocr_konfidenz: z.get(11)?,
                    ocr_ms: z.get::<_, Option<i64>>(12)?.map(|v| v as u64),
                    uebernommen: z.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(liste)
    }
}

/// Sucht den Ordner mit den Sprachdaten `satz` ab `start` aufwärts.
pub fn tessdata_suchen_ab(start: &Path, satz: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|ort| ort.join(ENTWICKLUNGS_ORDNER).join(satz))
        .find(|ordner| tessdata_vollstaendig(ordner))
}

/// Liegen alle Sprachen aus [`SPRACHEN`] in `ordner`?
pub fn tessdata_vollstaendig(ordner: &Path) -> bool {
    SPRACHEN.split('+').all(|s| ordner.join(format!("{s}.traineddata")).is_file())
}

/// Maßstab in Pixel je PDF-Punkt für die OCR, begrenzt durch [`MAX_KANTE`].
pub fn massstab(breite_pt: f32, hoehe_pt: f32) -> f32 {
    let regel = DPI as f32 / 72.0;
    let laengste = breite_pt.max(hoehe_pt).max(1.0);
    regel.min(MAX_KANTE as f32 / laengste)
}

/// Rendert eine Seite in Graustufen für die OCR. Liefert auch die
/// tatsächliche Auflösung, die bei Übergrößen unter [`DPI`] liegt.
///
/// PDFium rendert in Farbe, die Umrechnung in Helligkeit (BT.601) passiert
/// hier. Das Graustufenformat von PDFium zeichnet Bilder nicht mit.
pub fn rendern(seite: &PdfPage) -> Result<(Graubild, u32), String> {
    let faktor = massstab(seite.width().value, seite.height().value);
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(faktor)
        .set_clear_color(PdfColor::WHITE)
        .render_form_data(true);
    let bitmap = seite.render_with_config(&config).map_err(|e| format!("Rendern: {e:?}"))?;
    let (breite, hoehe) = (bitmap.width() as u32, bitmap.height() as u32);
    let rgba = bitmap.as_rgba_bytes();
    if breite == 0 || hoehe == 0 || rgba.len() != breite as usize * hoehe as usize * 4 {
        return Err("Rendern: Bild ohne gültige Maße".into());
    }
    let grau = rgba
        .chunks_exact(4)
        .map(|p| ((299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32 + 500) / 1000) as u8)
        .collect();
    let dpi = (faktor * 72.0).round() as u32;
    Ok((Graubild { breite, hoehe, grau }, dpi))
}

/// Rendert die Seiten `nummern` einer PDF und übergibt sie einzeln an `seite`.
/// Gibt `seite` `false` zurück, endet das Rendern mit
/// [`ABGEBROCHEN`](crate::vorschauen::ABGEBROCHEN).
pub fn pdf_rendern(
    pdfium: &Pdfium,
    pfad: &Path,
    nummern: &[u32],
    seite: &mut dyn FnMut(u32, Graubild, u32) -> bool,
) -> Result<(), String> {
    let dokument = pdfium.load_pdf_from_file(pfad, None).map_err(|e| format!("{e:?}"))?;
    let seiten = dokument.pages();
    for &nr in nummern {
        let index = nr.checked_sub(1).and_then(|i| i32::try_from(i).ok()).ok_or("Seitennummer ungültig")?;
        let s = seiten.get(index).map_err(|e| format!("Seite {nr}: {e:?}"))?;
        let (bild, dpi) = rendern(&s)?;
        if !seite(nr, bild, dpi) {
            return Err(crate::vorschauen::ABGEBROCHEN.into());
        }
    }
    Ok(())
}

/// Ein Erkenner für einen Faden. Wird je Faden einmal angelegt.
pub type ErkennerFn = Box<dyn FnMut(&Graubild, u32) -> Result<Erkannt, String> + Send>;

enum Meldung {
    Datei { nr: usize, ergebnis: Result<(), String> },
    Uebergangen { nr: usize, veraendert: bool },
    Seite { nr: usize, seite: u32, erkannt: Result<Erkannt, String>, dauer: Duration },
}

#[derive(Default)]
struct Sammlung {
    seiten: BTreeMap<u32, (Result<Erkannt, String>, Duration)>,
    kopf: Option<Result<(), String>>,
}

/// Erkennt die ausstehenden Seiten.
///
/// - `rendern` rendert die genannten Seiten einer PDF und übergibt jede mit
///   ihrer Auflösung an den Rückruf.
/// - `erkenner_anlegen` legt für jeden Faden einen eigenen Erkenner an.
/// - `faeden`: Anzahl der Erkennungs-Fäden.
pub fn erkennen<R, A>(
    konfig: &Konfiguration,
    register: &mut Register,
    rendern: R,
    erkenner_anlegen: A,
    faeden: usize,
    mut fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<OcrErgebnis>
where
    R: FnMut(&Path, &[u32], &mut dyn FnMut(u32, Graubild, u32) -> bool) -> Result<(), String> + Send,
    A: Fn() -> Result<ErkennerFn, String> + Sync,
{
    let start = Instant::now();
    register.ocr_veraltete_entfernen()?;
    let liste = register.ocr_ausstehend()?;
    let mut e = OcrErgebnis { ausstehend: liste.len() as u64, ..Default::default() };
    if liste.is_empty() {
        return Ok(e);
    }
    let faeden = faeden.max(1);
    // Erkenner vorab anlegen: Fehlende Sprachdaten sollen den Lauf gleich
    // beenden, nicht jede Datei als fehlerhaft markieren.
    let erkenner: Vec<ErkennerFn> =
        (0..faeden).map(|_| erkenner_anlegen()).collect::<Result<_, _>>().map_err(crate::Fehler::Pdf)?;
    let mut rendern = rendern;
    let mut fehler_register: Option<crate::Fehler> = None;

    std::thread::scope(|s| {
        let (auftrag_tx, auftrag_rx) = mpsc::sync_channel::<(usize, u32, Graubild, u32)>(faeden * PUFFER_JE_FADEN);
        let auftrag_rx = Arc::new(Mutex::new(auftrag_rx));
        let (meldung_tx, meldung_rx) = mpsc::channel::<Meldung>();

        let mut erkenn_faeden = Vec::with_capacity(faeden);
        for mut erkenner in erkenner {
            let rx = Arc::clone(&auftrag_rx);
            let tx = meldung_tx.clone();
            erkenn_faeden.push(s.spawn(move || {
                let (mut arbeit, mut leer, mut pixel) = (Duration::ZERO, Duration::ZERO, 0u64);
                loop {
                    let t = Instant::now();
                    let auftrag = rx.lock().map(|r| r.recv());
                    leer += t.elapsed();
                    let Ok(Ok((nr, seite, bild, dpi))) = auftrag else { break };
                    pixel += bild.breite as u64 * bild.hoehe as u64;
                    let t = Instant::now();
                    let erkannt = erkenner(&bild, dpi);
                    let dauer = t.elapsed();
                    arbeit += dauer;
                    if tx.send(Meldung::Seite { nr, seite, erkannt, dauer }).is_err() {
                        break;
                    }
                }
                (arbeit, leer, pixel)
            }));
        }

        let liste_r = &liste;
        let orte = konfig;
        let tx = meldung_tx;
        let render_faden = s.spawn(move || {
            let mut abgebrochen = false;
            let (mut gesamt, mut wartet) = (Duration::ZERO, Duration::ZERO);
            for (nr, auftrag) in liste_r.iter().enumerate() {
                if abbrechen.load(Ordering::Relaxed) {
                    abgebrochen = true;
                    break;
                }
                let pfad = orte.ort_oder_nichts(&auftrag.pfad);
                let meta = std::fs::metadata(&pfad).ok();
                let platzhalter = meta.as_ref().is_none_or(|m| crate::dateibaum::platzhalter(&pfad, m));
                let groesse = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                if auftrag.nur_online || platzhalter {
                    let _ = tx.send(Meldung::Uebergangen { nr, veraendert: false });
                    continue;
                }
                let geaendert = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                // Nur erkennen, wenn die Datei noch dem gelesenen Inhalt entspricht
                if geaendert != auftrag.inhalt_geaendert || auftrag.inhalt_groesse.is_some_and(|g| g as u64 != groesse) {
                    let _ = tx.send(Meldung::Uebergangen { nr, veraendert: true });
                    continue;
                }
                let nummern: Vec<u32> = auftrag.seiten.iter().map(|s| s.nr).collect();
                let t0 = Instant::now();
                let ergebnis = rendern(&pfad, &nummern, &mut |seite, bild, dpi| {
                    if abbrechen.load(Ordering::Relaxed) {
                        return false;
                    }
                    let t = Instant::now();
                    let ok = auftrag_tx.send((nr, seite, bild, dpi)).is_ok();
                    wartet += t.elapsed();
                    ok
                });
                gesamt += t0.elapsed();
                if matches!(&ergebnis, Err(f) if f == crate::vorschauen::ABGEBROCHEN) {
                    abgebrochen = true;
                    break;
                }
                let _ = tx.send(Meldung::Datei { nr, ergebnis });
            }
            (abgebrochen, gesamt.saturating_sub(wartet), wartet)
        });

        let mut offen: HashMap<usize, Sammlung> = HashMap::new();
        let mut erledigt = 0u64;
        let mut schreiben = Duration::ZERO;
        for meldung in meldung_rx {
            let nr = match meldung {
                Meldung::Uebergangen { nr, veraendert } => {
                    if veraendert {
                        e.veraendert += 1;
                    } else {
                        e.nur_online += 1;
                    }
                    erledigt += 1;
                    fortschritt(&Fortschritt { erledigt, gesamt: e.ausstehend, datei: liste[nr].pfad.clone() });
                    continue;
                }
                Meldung::Datei { nr, ergebnis } => {
                    offen.entry(nr).or_default().kopf = Some(ergebnis);
                    nr
                }
                Meldung::Seite { nr, seite, erkannt, dauer } => {
                    offen.entry(nr).or_default().seiten.insert(seite, (erkannt, dauer));
                    nr
                }
            };
            let auftrag = &liste[nr];
            let fertig = match &offen[&nr].kopf {
                Some(Ok(())) => offen[&nr].seiten.len() == auftrag.seiten.len(),
                Some(Err(_)) => true,
                None => false,
            };
            if !fertig || fehler_register.is_some() {
                continue;
            }
            let sammlung = offen.remove(&nr).expect("vorhanden");
            let ergebnis: Result<Vec<OcrErgebnisSeite>, String> = match sammlung.kopf.expect("fertig") {
                Err(f) => Err(f),
                Ok(()) => auftrag
                    .seiten
                    .iter()
                    .map(|s| {
                        let (erkannt, dauer) = sammlung.seiten.get(&s.nr).ok_or("Seite fehlt")?;
                        let erkannt = erkannt.as_ref().map_err(|f| format!("Seite {}: {f}", s.nr))?;
                        let text = erkannt.text.trim().to_string();
                        let zaehlung = guete::zaehlen(&text);
                        let uebernommen = uebernehmen(s.fall, &s.alt, &zaehlung);
                        Ok(OcrErgebnisSeite {
                            nr: s.nr,
                            fall: s.fall,
                            uebernommen,
                            text,
                            zaehlung,
                            konfidenz: erkannt.konfidenz,
                            dauer_ms: dauer.as_millis() as u64,
                            woerter: uebernommen.then(|| woerter::packen(&erkannt.woerter)),
                        })
                    })
                    .collect(),
            };
            match &ergebnis {
                Ok(seiten) => {
                    e.erkannt += 1;
                    e.seiten += seiten.len() as u64;
                    e.uebernommen += seiten.iter().filter(|s| s.uebernommen).count() as u64;
                }
                Err(_) => e.fehler += 1,
            }
            let t = Instant::now();
            let gespeichert = register.ocr_speichern(auftrag.id, auftrag.inhalt_geaendert, auftrag.inhalt_groesse, &ergebnis);
            schreiben += t.elapsed();
            if let Err(f) = gespeichert {
                fehler_register = Some(f);
                abbrechen.store(true, Ordering::Relaxed);
                continue;
            }
            erledigt += 1;
            fortschritt(&Fortschritt { erledigt, gesamt: e.ausstehend, datei: auftrag.pfad.clone() });
        }
        let (abgebrochen, render, wartet) = render_faden.join().unwrap_or((true, Duration::ZERO, Duration::ZERO));
        e.abgebrochen = abgebrochen;
        let m = &mut e.messung;
        m.faeden = faeden as u64;
        m.render_ms = render.as_millis() as u64;
        m.render_wartet_ms = wartet.as_millis() as u64;
        m.schreib_ms = schreiben.as_millis() as u64;
        for f in erkenn_faeden {
            if let Ok((arbeit, leer, pixel)) = f.join() {
                m.erkenn_ms += arbeit.as_millis() as u64;
                m.erkenn_leer_ms += leer.as_millis() as u64;
                m.pixel += pixel;
            }
        }
    });

    if let Some(f) = fehler_register {
        return Err(f);
    }
    e.dauer_ms = start.elapsed().as_millis() as u64;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{Inhalt, Seite};
    use std::fs;

    #[test]
    fn faelle_und_uebernahme() {
        let leer = guete::zaehlen("");
        let gut = guete::zaehlen("Sehr geehrter Herr Sieber, anbei die Rechnung über 123,45 EUR.");
        let salat = guete::zaehlen("ÿ§ 3a %$ Ü1 ~~ ¶ q ® 7x ±± ab ¤¤ ½x Rechnung");
        assert_eq!(Fall::bestimmen(&leer, 1.0), Some(Fall::OhneText));
        assert_eq!(Fall::bestimmen(&gut, 1.0), None);
        assert_eq!(Fall::bestimmen(&salat, 0.9), Some(Fall::ScanSchwach));
        assert_eq!(Fall::bestimmen(&salat, 0.1), Some(Fall::DigitalSchwach));

        assert!(uebernehmen(Fall::OhneText, &leer, &gut));
        assert!(!uebernehmen(Fall::OhneText, &leer, &guete::zaehlen("ab 12")));
        assert!(uebernehmen(Fall::ScanSchwach, &salat, &gut));
        // gleiche Güte ist keine Verbesserung
        assert!(!uebernehmen(Fall::DigitalSchwach, &salat, &salat));
        // genau 10 Prozentpunkte genügen: 5 von 10 → 6 von 10
        let fuenf = guete::zaehlen("aa bb cc dd ee 1x 2x 3x 4x 5x");
        let sechs = guete::zaehlen("aa bb cc dd ee ff 2x 3x 4x 5x");
        assert!(uebernehmen(Fall::ScanSchwach, &fuenf, &sechs));
    }

    #[test]
    fn massstab_300_dpi_und_grenze() {
        assert!((massstab(595.0, 842.0) - 300.0 / 72.0).abs() < 1e-4);
        // A0 (2384 × 3370 pt) wird auf MAX_KANTE begrenzt
        assert!((massstab(2384.0, 3370.0) * 3370.0 - MAX_KANTE as f32).abs() < 1.0);
    }

    // --- Lauf mit Ersatz für PDFium und Tesseract ---

    fn seite(nr: u32, text: &str, bildanteil: f32) -> Seite {
        Seite { nr, text: text.into(), bildanteil }
    }

    const GUT: &str = "Sehr geehrter Herr Sieber, anbei die Rechnung über 123,45 EUR.";
    const SALAT: &str = "ÿ§ 3a %$ Ü1 ~~ ¶ q ® 7x ±± ab ¤¤ ½x";

    fn testleser(pfad: &Path) -> Result<Inhalt, String> {
        let name = pfad.file_name().unwrap().to_string_lossy().to_string();
        let seiten = match name.as_str() {
            // Seite 2 ohne Text, Seite 3 Scan mit Zeichensalat
            "scan.pdf" => vec![seite(1, GUT, 1.0), seite(2, "", 1.0), seite(3, SALAT, 1.0)],
            "auszug.pdf" => vec![seite(1, SALAT, 0.0)],
            "gut.pdf" => vec![seite(1, GUT, 0.0)],
            "kaputt.pdf" => vec![seite(1, "", 1.0)],
            "leer.pdf" => vec![seite(1, "", 1.0)],
            _ => vec![seite(1, "", 1.0)],
        };
        Ok(Inhalt { seiten, erstellt: None, grob: false })
    }

    /// Rendert jede Seite als 1×1-Bild, dessen Pixel die Seitennummer trägt.
    fn testrendern(pfad: &Path, nummern: &[u32], seite: &mut dyn FnMut(u32, Graubild, u32) -> bool) -> Result<(), String> {
        if pfad.ends_with("kaputt.pdf") {
            return Err("keine gültige PDF oder beschädigt".into());
        }
        for &nr in nummern {
            if !seite(nr, Graubild { breite: 1, hoehe: 1, grau: vec![nr as u8] }, DPI) {
                return Err(crate::vorschauen::ABGEBROCHEN.into());
            }
        }
        Ok(())
    }

    fn testerkenner() -> Result<ErkennerFn, String> {
        Ok(Box::new(|bild: &Graubild, dpi: u32| {
            assert_eq!(dpi, DPI);
            let text = match bild.grau[0] {
                // Seite 2 von scan.pdf: lesbarer Text, sonst fast nichts
                2 => GUT.to_string(),
                _ => "x".to_string(),
            };
            Ok(Erkannt { text, konfidenz: 80, woerter: Vec::new() })
        }))
    }

    fn aufbauen() -> (tempfile::TempDir, Konfiguration, Register) {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("A");
        fs::create_dir_all(&a).unwrap();
        for name in ["scan.pdf", "auszug.pdf", "gut.pdf", "kaputt.pdf", "leer.pdf"] {
            fs::write(a.join(name), b"%PDF").unwrap();
        }
        // Ast "A" liegt im Ordner A: Die Pfade im Register heißen "A/…"
        let konfig = Konfiguration::mit_ast("A", &tmp.path().join("A"));
        let mut register = Register::im_speicher().unwrap();
        crate::dateibaum_einlesen(&konfig, &crate::Regeln::voll(&["A"]), &mut register, true).unwrap();
        crate::inhalte::lesen(&konfig, &mut register, testleser, |_| {}, &AtomicBool::new(false)).unwrap();
        (tmp, konfig, register)
    }

    fn lauf(konfig: &Konfiguration, register: &mut Register) -> OcrErgebnis {
        erkennen(konfig, register, testrendern, testerkenner, 2, still, &AtomicBool::new(false)).unwrap()
    }

    fn id(r: &Register, pfad: &str) -> i64 {
        r.db.query_row("SELECT id FROM datei WHERE pfad = ?1", [pfad], |z| z.get(0)).unwrap()
    }

    #[test]
    fn waehlt_nur_seiten_ohne_text() {
        let (_tmp, _konfig, register) = aufbauen();
        let liste = register.ocr_ausstehend().unwrap();
        let pfade: Vec<_> = liste.iter().map(|a| a.pfad.as_str()).collect();
        // auszug.pdf hat nur schwachen Text und wird nicht erkannt
        assert_eq!(pfade, vec!["A/kaputt.pdf", "A/leer.pdf", "A/scan.pdf"]);
        let faelle: Vec<_> = liste[2].seiten.iter().map(|s| (s.nr, s.fall)).collect();
        assert_eq!(faelle, vec![(2, Fall::OhneText)]);
        let s = register.stand().unwrap().ocr;
        assert_eq!((s.pdfs, s.ausstehend), (3, 3));
        assert_eq!((s.seiten_ohne_text, s.seiten_scan_schwach, s.seiten_digital_schwach), (3, 1, 1));
    }

    #[test]
    fn erkennt_und_uebernimmt_nach_regel() {
        let (_tmp, konfig, mut register) = aufbauen();
        let e = lauf(&konfig, &mut register);
        assert_eq!((e.ausstehend, e.erkannt, e.seiten, e.uebernommen, e.fehler), (3, 2, 2, 1, 1));

        let texte = register.seitentexte(id(&register, "A/scan.pdf")).unwrap();
        assert_eq!(texte.len(), 3);
        assert_eq!((texte[0].ocr_text.as_deref(), texte[0].fall), (None, None));
        assert_eq!((texte[1].fall, texte[1].uebernommen), (Some(Fall::OhneText), true));
        assert!(texte[1].ocr_guete.unwrap() > 0.9);
        // Seite 3 mit schwachem Text: nicht erkannt, Fall wird trotzdem angezeigt
        assert_eq!((texte[2].fall, texte[2].ocr_text.as_deref()), (Some(Fall::ScanSchwach), None));
        // leer.pdf lieferte nur "x": erkannt, aber nicht übernommen
        let leer = register.seitentexte(id(&register, "A/leer.pdf")).unwrap();
        assert_eq!((leer[0].ocr_text.as_deref(), leer[0].uebernommen), (Some("x"), false));

        // gültiger Text: OCR nur, wo übernommen
        let gilt: String = register
            .db
            .query_row("SELECT text FROM seite_text WHERE datei = ?1 AND nr = 2", [id(&register, "A/scan.pdf")], |z| z.get(0))
            .unwrap();
        assert_eq!(gilt, GUT);

        let s = register.stand().unwrap().ocr;
        assert_eq!((s.erledigt, s.ausstehend, s.fehler, s.seiten_erkannt, s.seiten_uebernommen), (3, 0, 1, 2, 1));
        // nachher: leer.pdf und kaputt.pdf ohne Text, schwach bleiben scan S.3 und auszug
        assert_eq!((s.nachher_ohne_text, s.nachher_schwach), (2, 2));
        assert_eq!(s.fehler_liste[0].pfad, "A/kaputt.pdf");
        // Zweiter Lauf: nichts mehr zu tun
        assert_eq!(lauf(&konfig, &mut register).ausstehend, 0);
    }

    #[test]
    fn aelteres_verfahren_wird_entfernt() {
        let (_tmp, konfig, mut register) = aufbauen();
        let auszug = id(&register, "A/auszug.pdf");
        // Stand von Verfahren 1: auch schwacher Text erkannt und übernommen
        let alt = OcrErgebnisSeite {
            nr: 1,
            fall: Fall::DigitalSchwach,
            text: GUT.into(),
            zaehlung: guete::zaehlen(GUT),
            konfidenz: 80,
            dauer_ms: 1,
            uebernommen: true,
            woerter: None,
        };
        let (g, gr): (i64, Option<i64>) = register
            .db
            .query_row("SELECT inhalt_geaendert, inhalt_groesse FROM datei WHERE id = ?1", [auszug], |z| Ok((z.get(0)?, z.get(1)?)))
            .unwrap();
        register.ocr_speichern(auszug, g, gr, &Ok(vec![alt])).unwrap();
        register.db.execute("UPDATE datei SET ocr_verfahren = 1 WHERE id = ?1", [auszug]).unwrap();
        assert_eq!(register.stand().unwrap().ocr.seiten_uebernommen, 1);

        lauf(&konfig, &mut register);
        let texte = register.seitentexte(auszug).unwrap();
        assert_eq!((texte[0].ocr_text.as_deref(), texte[0].uebernommen), (None, false));
        assert_eq!(register.stand().unwrap().ocr.seiten_uebernommen, 1);
    }

    #[test]
    fn neues_lesen_verwirft_die_ocr() {
        let (tmp, konfig, mut register) = aufbauen();
        lauf(&konfig, &mut register);
        let scan = tmp.path().join("A/scan.pdf");
        fs::write(&scan, b"%PDF neu").unwrap();
        let spaeter = std::time::SystemTime::now() + Duration::from_secs(3600);
        fs::File::options().write(true).open(&scan).unwrap().set_modified(spaeter).unwrap();
        crate::dateibaum_einlesen(&konfig, &crate::Regeln::voll(&["A"]), &mut register, true).unwrap();
        // Inhalt noch nicht neu gelesen: keine OCR für diese Datei
        assert!(register.ocr_ausstehend().unwrap().is_empty());
        crate::inhalte::lesen(&konfig, &mut register, testleser, |_| {}, &AtomicBool::new(false)).unwrap();
        let id = id(&register, "A/scan.pdf");
        assert!(register.seitentexte(id).unwrap().iter().all(|t| t.ocr_text.is_none()));
        let e = lauf(&konfig, &mut register);
        assert_eq!((e.ausstehend, e.seiten), (1, 1));
    }

    #[test]
    fn geaenderte_datei_wird_nicht_erkannt() {
        let (tmp, konfig, mut register) = aufbauen();
        let leer = tmp.path().join("A/leer.pdf");
        fs::write(&leer, b"%PDF anders und laenger").unwrap();
        let e = lauf(&konfig, &mut register);
        assert_eq!((e.veraendert, e.erkannt), (1, 1));
        assert_eq!(register.ocr_ausstehend().unwrap().len(), 1);
    }

    #[test]
    fn abbruch_behaelt_fertige_dateien() {
        let (_tmp, konfig, mut register) = aufbauen();
        let abbrechen = AtomicBool::new(false);
        let rendern = |pfad: &Path, n: &[u32], seite: &mut dyn FnMut(u32, Graubild, u32) -> bool| {
            let r = testrendern(pfad, n, seite);
            abbrechen.store(true, Ordering::Relaxed);
            r
        };
        let e = erkennen(&konfig, &mut register, rendern, testerkenner, 2, |_| {}, &abbrechen).unwrap();
        assert!(e.abgebrochen);
        // erste Datei (kaputt.pdf) mit Fehler festgehalten
        assert_eq!(e.erkannt + e.fehler, 1);
        assert_eq!(lauf(&konfig, &mut register).ausstehend, 2);
    }

    /// Sprachdaten suchen (v0.1.2): vollständig nur mit allen Sprachen.
    #[test]
    fn findet_sprachdaten_ab_einem_ordner() {
        let tmp = tempfile::tempdir().unwrap();
        let unten = tmp.path().join("app/bin");
        std::fs::create_dir_all(&unten).unwrap();
        let satz = tmp.path().join(ENTWICKLUNGS_ORDNER).join("standard");
        std::fs::create_dir_all(&satz).unwrap();
        std::fs::write(satz.join("deu.traineddata"), b"x").unwrap();
        assert!(!tessdata_vollstaendig(&satz));
        assert_eq!(tessdata_suchen_ab(&unten, "standard"), None);
        std::fs::write(satz.join("eng.traineddata"), b"x").unwrap();
        assert_eq!(tessdata_suchen_ab(&unten, "standard"), Some(satz));
        assert_eq!(tessdata_suchen_ab(&unten, "best"), None);
    }

    /// Rendern für die OCR an der Test-PDF: kaputte Datei, falsche Seite (v0.1.2).
    #[test]
    fn rendert_die_test_pdf_fuer_die_ocr() {
        use crate::testumgebung::{pdfium, test_pdf};
        let mut seiten = Vec::new();
        pdf_rendern(pdfium(), &test_pdf(), &[2], &mut |nr, bild, dpi| {
            seiten.push((nr, bild.breite, dpi));
            true
        })
        .unwrap();
        // A4 mit 300 dpi: 2479 oder 2480 px breit, je nach Rundung von PDFium
        assert_eq!(seiten.len(), 1);
        assert!(seiten[0].0 == 2 && (2479..=2480).contains(&seiten[0].1) && seiten[0].2 == 300, "{seiten:?}");
        assert!(pdf_rendern(pdfium(), &test_pdf(), &[3], &mut |_, _, _| true).unwrap_err().starts_with("Seite 3"));
        assert!(pdf_rendern(pdfium(), &test_pdf(), &[0], &mut |_, _, _| true).unwrap_err().contains("ungültig"));
        let tmp = tempfile::tempdir().unwrap();
        let kaputt = tmp.path().join("kaputt.pdf");
        std::fs::write(&kaputt, b"nichts").unwrap();
        assert!(pdf_rendern(pdfium(), &kaputt, &[1], &mut |_, _, _| true).is_err());
    }

    /// Fortschritt, der nichts tut. Eine Funktion statt `|_| {}`: Diese wird
    /// in allen Tests geteilt und zählt nur einmal für die Abdeckung.
    fn still(_: &crate::Fortschritt) {}

    #[test]
    fn fehlender_erkenner_beendet_den_lauf() {
        let (_tmp, konfig, mut register) = aufbauen();
        let r = erkennen(
            &konfig,
            &mut register,
            testrendern,
            || Err("Sprachdaten fehlen".into()),
            2,
            still,
            &AtomicBool::new(false),
        );
        assert!(r.is_err());
        assert_eq!(register.ocr_ausstehend().unwrap().len(), 3);
    }
}
