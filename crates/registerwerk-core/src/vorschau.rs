//! Vorschauen der Seiten: rendern, Farbe erkennen, als AVIF kodieren
//! (Konzept, Abschnitt 8).
//!
//! Rendern läuft über PDFium und damit nacheinander. Kodieren ist der teure
//! Teil und kann parallel laufen, je Seite mit einem eigenen Encoder.
//!
//! Kodiert wird mit libavif und dem Encoder libaom, mit denselben Einstellungen
//! wie im Vorschauvergleich der Konzeptphase (Pillow, also ebenfalls libavif):
//! Qualität 25, Farbe in 4:2:0. Seiten ohne Farbe werden als echte Graustufen
//! (4:0:0) gespeichert.

use std::path::Path;

use libavif_sys as avif;
use pdfium_render::prelude::*;

/// Kurze Kante einer Vorschau in Pixeln, unabhängig von der Seitengröße in
/// der PDF (v0.0.6). Eine A4-Seite hochkant wird 1024 px breit wie bisher.
/// Die Seitengröße einer PDF ist nicht verlässlich: Scan-Apps legen etwa
/// einen A4-Scan mit 300 dpi auf eine Seite von 57 × 81 mm. Im festen
/// Maßstab wurde er dann 279 px breit und unlesbar.
pub const KURZE_KANTE: f32 = 1024.0;
/// Längste Kante einer Vorschau in Pixeln. Lange Kassenbons werden dadurch
/// schmaler als 1024 px, bleiben aber lesbar (1:6 ergibt 512 px). **Annahme**
pub const MAX_KANTE: u16 = 3072;

/// AVIF-Qualität nach libavif, 0 bis 100 (Konzept: Q25).
pub const QUALITAET: i32 = 25;
/// Geschwindigkeit von libaom, 0 (langsam, klein) bis 10 (schnell).
/// Messversuch v0.0.3: Stufe 8 bei gleicher Qualität und Größe knapp 3-mal
/// so schnell wie ravif/rav1e Stufe 8. Stufe 9 ist doppelt so schnell, braucht
/// für dieselbe Qualität aber rund 45 % mehr Platz.
pub const GESCHWINDIGKEIT: u8 = 8;

/// Ein Pixel gilt als farbig, wenn seine Kanäle um mehr als diesen Wert
/// auseinanderliegen. **Annahme**
pub const FARB_ABSTAND: u8 = 48;
/// Eine Seite gilt als farbig, wenn mindestens dieser Anteil ihrer Pixel
/// farbig ist. **Annahme**
pub const FARB_ANTEIL: f32 = 0.001;

/// Eine gerenderte Seite, RGB mit 8 Bit je Kanal.
#[derive(Debug, Clone)]
pub struct Bild {
    pub breite: u32,
    pub hoehe: u32,
    pub rgb: Vec<u8>,
}

/// Fertige Vorschau einer Seite.
#[derive(Debug, Clone, PartialEq)]
pub struct Vorschau {
    pub breite: u32,
    pub hoehe: u32,
    pub farbig: bool,
    pub avif: Vec<u8>,
}

/// Maßstab in Pixel je PDF-Punkt: kurze Kante [`KURZE_KANTE`], lange Kante
/// höchstens [`MAX_KANTE`].
pub fn massstab(breite_pt: f32, hoehe_pt: f32) -> f32 {
    let kurz = breite_pt.min(hoehe_pt).max(1.0);
    let lang = breite_pt.max(hoehe_pt).max(1.0);
    (KURZE_KANTE / kurz).min(MAX_KANTE as f32 / lang)
}

/// Rendert eine Seite für die Vorschau. Weiß als Hintergrund, Formularfelder
/// werden mitgezeichnet, die Drehung der Seite wird berücksichtigt.
pub fn rendern(seite: &PdfPage) -> Result<Bild, String> {
    let faktor = massstab(seite.width().value, seite.height().value);
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(faktor)
        .set_clear_color(PdfColor::WHITE)
        .render_form_data(true);
    let bitmap = seite.render_with_config(&config).map_err(|e| format!("Rendern: {e:?}"))?;
    let (breite, hoehe) = (bitmap.width() as u32, bitmap.height() as u32);
    let rgba = bitmap.as_rgba_bytes();
    let mut rgb = Vec::with_capacity((breite * hoehe * 3) as usize);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    Ok(Bild { breite, hoehe, rgb })
}

/// Rendert alle Seiten einer PDF, höchstens `max` Stück.
pub fn pdf_rendern(pdfium: &Pdfium, pfad: &Path, max: usize) -> Result<Vec<Bild>, String> {
    let dokument = pdfium.load_pdf_from_file(pfad, None).map_err(|e| format!("{e:?}"))?;
    dokument.pages().iter().take(max).map(|s| rendern(&s)).collect()
}

/// Rendert alle Seiten einer PDF und übergibt sie einzeln an `seite`.
/// Liefert die Seitenzahl. So liegt immer nur eine Seite im Speicher.
/// Gibt `seite` `false` zurück, endet das Rendern sofort mit
/// [`ABGEBROCHEN`](crate::vorschauen::ABGEBROCHEN).
pub fn pdf_rendern_je_seite(
    pdfium: &Pdfium,
    pfad: &Path,
    seite: &mut dyn FnMut(Bild) -> bool,
) -> Result<u32, String> {
    let dokument = pdfium.load_pdf_from_file(pfad, None).map_err(|e| format!("{e:?}"))?;
    let mut n = 0;
    for s in dokument.pages().iter() {
        if !seite(rendern(&s)?) {
            return Err(crate::vorschauen::ABGEBROCHEN.into());
        }
        n += 1;
    }
    Ok(n)
}

/// Anteil der farbigen Pixel, zwischen 0 und 1.
pub fn farbanteil(bild: &Bild) -> f32 {
    let n = bild.rgb.len() / 3;
    if n == 0 {
        return 0.0;
    }
    let farbig = bild
        .rgb
        .chunks_exact(3)
        .filter(|p| {
            let (min, max) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            max - min > FARB_ABSTAND
        })
        .count();
    farbig as f32 / n as f32
}

pub fn ist_farbig(bild: &Bild) -> bool {
    farbanteil(bild) >= FARB_ANTEIL
}

/// Kodiert ein Bild als AVIF. Ohne Farbe nur die Helligkeit (4:0:0).
/// `threads`: `Some(1)` für parallele Läufe über mehrere Seiten.
pub fn kodieren(bild: &Bild, farbig: bool, geschwindigkeit: u8, threads: Option<usize>) -> Result<Vec<u8>, String> {
    if bild.breite == 0 || bild.hoehe == 0 || bild.rgb.len() != (bild.breite * bild.hoehe * 3) as usize {
        return Err("AVIF: Bild ohne gültige Maße".into());
    }
    let format = if farbig { avif::AVIF_PIXEL_FORMAT_YUV420 } else { avif::AVIF_PIXEL_FORMAT_YUV400 };
    // SAFETY: Alle Zeiger stammen aus libavif oder zeigen auf `bild.rgb`, das
    // länger lebt als der Aufruf. libavif liest die Pixel nur. Jede angelegte
    // Struktur wird auf jedem Weg wieder freigegeben (Aufraeumen).
    unsafe {
        let bild_c = avif::avifImageCreate(bild.breite, bild.hoehe, 8, format);
        let encoder = avif::avifEncoderCreate();
        let mut c = Aufraeumen {
            bild: bild_c,
            encoder,
            ausgabe: avif::avifRWData { data: std::ptr::null_mut(), size: 0 },
        };
        if bild_c.is_null() || encoder.is_null() {
            return Err("AVIF: kein Speicher".into());
        }
        (*bild_c).yuvRange = avif::AVIF_RANGE_FULL;
        (*bild_c).matrixCoefficients = avif::AVIF_MATRIX_COEFFICIENTS_BT601 as _;

        let mut rgb: avif::avifRGBImage = std::mem::zeroed();
        avif::avifRGBImageSetDefaults(&mut rgb, bild_c);
        rgb.format = avif::AVIF_RGB_FORMAT_RGB;
        rgb.depth = 8;
        rgb.pixels = bild.rgb.as_ptr() as *mut u8;
        rgb.rowBytes = bild.breite * 3;
        pruefen(avif::avifImageRGBToYUV(bild_c, &rgb))?;

        (*encoder).codecChoice = avif::AVIF_CODEC_CHOICE_AOM;
        (*encoder).speed = geschwindigkeit as _;
        (*encoder).quality = QUALITAET;
        (*encoder).maxThreads = threads.unwrap_or(0) as _;
        pruefen(avif::avifEncoderWrite(encoder, bild_c, &mut c.ausgabe))?;
        Ok(std::slice::from_raw_parts(c.ausgabe.data, c.ausgabe.size).to_vec())
    }
}

/// Gibt die Strukturen von libavif frei, auch bei einem Fehler.
struct Aufraeumen {
    bild: *mut avif::avifImage,
    encoder: *mut avif::avifEncoder,
    ausgabe: avif::avifRWData,
}

impl Drop for Aufraeumen {
    fn drop(&mut self) {
        // SAFETY: Zeiger aus avifImageCreate/avifEncoderCreate oder auf die
        // lokale Ausgabe, jeweils genau einmal freigegeben.
        unsafe {
            avif::avifRWDataFree(&mut self.ausgabe);
            if !self.encoder.is_null() {
                avif::avifEncoderDestroy(self.encoder);
            }
            if !self.bild.is_null() {
                avif::avifImageDestroy(self.bild);
            }
        }
    }
}

fn pruefen(ergebnis: avif::avifResult) -> Result<(), String> {
    if ergebnis == avif::AVIF_RESULT_OK {
        return Ok(());
    }
    // SAFETY: libavif liefert eine statische, nullterminierte Zeichenkette.
    let text = unsafe { std::ffi::CStr::from_ptr(avif::avifResultToString(ergebnis)) };
    Err(format!("AVIF: {}", text.to_string_lossy()))
}

/// Macht aus einer gerenderten Seite die fertige Vorschau.
pub fn erzeugen(bild: &Bild, threads: Option<usize>) -> Result<Vorschau, String> {
    let farbig = ist_farbig(bild);
    let avif = kodieren(bild, farbig, GESCHWINDIGKEIT, threads)?;
    Ok(Vorschau { breite: bild.breite, hoehe: bild.hoehe, farbig, avif })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flaeche(b: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Bild {
        let mut rgb = Vec::new();
        for y in 0..h {
            for x in 0..b {
                rgb.extend_from_slice(&f(x, y));
            }
        }
        Bild { breite: b, hoehe: h, rgb }
    }

    #[test]
    fn massstab_nach_kurzer_kante() {
        let px = |b: f32, h: f32| {
            let m = massstab(b, h);
            ((b * m).round() as u32, (h * m).round() as u32)
        };
        // A4 hochkant und quer wie bisher
        assert_eq!(px(595.276, 841.89), (1024, 1448));
        assert_eq!(px(841.89, 595.276), (1448, 1024));
        // A4-Scan auf zu kleiner Seite (Pilot: 162 × 229 pt): trotzdem 1024 px breit
        assert_eq!(px(162.0, 229.0), (1024, 1448));
        // Kassenbon 1:3,25 und 1:6: lange Kante begrenzt
        assert_eq!(px(226.0, 735.0), (945, 3072));
        assert_eq!(px(226.0, 1356.0), (512, 3072));
        // Plan in A0: gleich groß wie A4
        assert_eq!(px(2384.0, 3370.0), (1024, 1448));
    }

    #[test]
    fn farbe_erkennen() {
        let grau = flaeche(100, 100, |x, _| { let v = (x * 2) as u8; [v, v.saturating_add(10), v] });
        assert!(!ist_farbig(&grau));
        // roter Stempel auf 1 % der Fläche
        let stempel = flaeche(100, 100, |x, y| if x < 10 && y < 10 { [200, 30, 30] } else { [250, 250, 250] });
        assert!(ist_farbig(&stempel));
    }

    #[test]
    fn leeres_und_falsches_bild() {
        let leer = Bild { breite: 0, hoehe: 0, rgb: vec![] };
        assert_eq!(farbanteil(&leer), 0.0);
        assert!(kodieren(&leer, false, 10, Some(1)).unwrap_err().contains("Maße"));
        let zu_kurz = Bild { breite: 4, hoehe: 4, rgb: vec![0; 10] };
        assert!(erzeugen(&zu_kurz, Some(1)).is_err());
    }

    /// Rendert die Test-PDF: Seite 1 mit rotem Balken ist farbig, der Scan in
    /// Graustufen nicht. A4 wird 1024 px breit (v0.1.2).
    #[test]
    fn rendert_die_test_pdf() {
        use crate::testumgebung::{pdfium, test_pdf};
        let seiten = pdf_rendern(pdfium(), &test_pdf(), 10).unwrap();
        assert_eq!(seiten.len(), 2);
        assert_eq!(pdf_rendern(pdfium(), &test_pdf(), 1).unwrap().len(), 1);
        // PDFium rundet die lange Kante auf: 841,89 pt ergeben 1449 statt 1448 px
        for s in &seiten {
            assert_eq!(s.breite, 1024);
            assert!((1448..=1449).contains(&s.hoehe), "{}", s.hoehe);
            assert_eq!(s.rgb.len(), (s.breite * s.hoehe * 3) as usize);
        }
        let v: Vec<Vorschau> = seiten.iter().map(|s| erzeugen(s, Some(1)).unwrap()).collect();
        assert_eq!(v.iter().map(|v| v.farbig).collect::<Vec<_>>(), [true, false]);
        assert!(v.iter().all(|v| &v.avif[4..12] == b"ftypavif" && v.breite == 1024));

        // Je Seite, mit Abbruch nach der ersten
        let mut n = 0;
        assert_eq!(pdf_rendern_je_seite(pdfium(), &test_pdf(), &mut |_| { n += 1; true }).unwrap(), 2);
        assert_eq!(n, 2);
        let f = pdf_rendern_je_seite(pdfium(), &test_pdf(), &mut |_| false).unwrap_err();
        assert_eq!(f, crate::vorschauen::ABGEBROCHEN);

        // Unlesbare Datei
        let tmp = tempfile::tempdir().unwrap();
        let kaputt = tmp.path().join("kaputt.pdf");
        std::fs::write(&kaputt, b"nichts").unwrap();
        assert!(pdf_rendern(pdfium(), &kaputt, 1).is_err());
        assert!(pdf_rendern_je_seite(pdfium(), &kaputt, &mut |_| true).is_err());
    }

    #[test]
    fn kodiert_avif_in_farbe_und_grau() {
        let bild = flaeche(64, 48, |x, y| [(x * 4) as u8, (y * 5) as u8, 90]);
        for farbig in [true, false] {
            let avif = kodieren(&bild, farbig, 10, Some(1)).unwrap();
            assert_eq!(&avif[4..12], b"ftypavif");
        }
    }
}
