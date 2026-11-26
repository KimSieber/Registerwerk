//! Texterkennung mit Tesseract.
//!
//! Tesseract wird statisch ins Programm gebaut (`build.rs`), eine eigene
//! Installation ist nicht nötig. Dieses Crate bindet nur die wenigen
//! Funktionen der C-Schnittstelle an, die Registerwerk braucht, und kapselt
//! sie in [`Erkenner`].
//!
//! Ein Erkenner gehört zu genau einem Faden. Für paralleles Erkennen bekommt
//! jeder Faden einen eigenen. Das ist in Tesseract so vorgesehen.

use std::ffi::{c_char, c_int, CStr, CString};
use std::path::Path;
use std::sync::Once;

static STILL: Once = Once::new();

#[repr(C)]
struct TessBaseApi {
    _privat: [u8; 0],
}

#[repr(C)]
struct TessResultIterator {
    _privat: [u8; 0],
}

#[repr(C)]
struct TessPageIterator {
    _privat: [u8; 0],
}

/// `OEM_LSTM_ONLY` aus `capi.h`: nur das LSTM-Verfahren. Das alte Verfahren
/// ist beim Bau abgeschaltet.
const OEM_LSTM_ONLY: c_int = 1;
/// `PSM_AUTO` aus `publictypes.h`: Seite automatisch aufteilen, ohne Erkennung
/// der Ausrichtung.
const PSM_AUTO: c_int = 3;

/// Ebenen der Iteratoren aus `publictypes.h`.
const RIL_TEXTLINE: c_int = 2;
const RIL_WORD: c_int = 3;

/// `L_SEVERITY_NONE` aus Leptonicas `environ.h`: keine Meldungen.
const L_SEVERITY_NONE: c_int = 6;

extern "C" {
    // Leptonica
    fn setMsgSeverity(newsev: c_int) -> c_int;
    // Tesseract
    fn TessVersion() -> *const c_char;
    fn TessDeleteText(text: *const c_char);
    fn TessBaseAPICreate() -> *mut TessBaseApi;
    fn TessBaseAPIDelete(handle: *mut TessBaseApi);
    fn TessBaseAPIEnd(handle: *mut TessBaseApi);
    fn TessBaseAPIInit2(handle: *mut TessBaseApi, datapath: *const c_char, language: *const c_char, oem: c_int)
        -> c_int;
    fn TessBaseAPISetVariable(handle: *mut TessBaseApi, name: *const c_char, value: *const c_char) -> c_int;
    fn TessBaseAPISetPageSegMode(handle: *mut TessBaseApi, mode: c_int);
    fn TessBaseAPISetImage(
        handle: *mut TessBaseApi,
        imagedata: *const u8,
        width: c_int,
        height: c_int,
        bytes_per_pixel: c_int,
        bytes_per_line: c_int,
    );
    fn TessBaseAPISetSourceResolution(handle: *mut TessBaseApi, ppi: c_int);
    fn TessBaseAPIGetUTF8Text(handle: *mut TessBaseApi) -> *mut c_char;
    fn TessBaseAPIMeanTextConf(handle: *mut TessBaseApi) -> c_int;
    fn TessBaseAPIClear(handle: *mut TessBaseApi);
    fn TessBaseAPIGetIterator(handle: *mut TessBaseApi) -> *mut TessResultIterator;
    fn TessResultIteratorDelete(handle: *mut TessResultIterator);
    fn TessResultIteratorNext(handle: *mut TessResultIterator, level: c_int) -> c_int;
    fn TessResultIteratorGetUTF8Text(handle: *const TessResultIterator, level: c_int) -> *mut c_char;
    fn TessResultIteratorGetPageIterator(handle: *mut TessResultIterator) -> *mut TessPageIterator;
    fn TessPageIteratorBoundingBox(
        handle: *const TessPageIterator,
        level: c_int,
        left: *mut c_int,
        top: *mut c_int,
        right: *mut c_int,
        bottom: *mut c_int,
    ) -> c_int;
    fn TessPageIteratorBaseline(
        handle: *const TessPageIterator,
        level: c_int,
        x1: *mut c_int,
        y1: *mut c_int,
        x2: *mut c_int,
        y2: *mut c_int,
    ) -> c_int;
    fn TessPageIteratorIsAtFinalElement(handle: *const TessPageIterator, level: c_int, element: c_int) -> c_int;
}

/// Version der eingebauten Tesseract-Bibliothek, z.B. "5.5.2".
pub fn version() -> String {
    // SAFETY: TessVersion liefert eine statische, nullterminierte Zeichenkette.
    unsafe { CStr::from_ptr(TessVersion()).to_string_lossy().into_owned() }
}

/// Ergebnis der Erkennung einer Seite.
#[derive(Debug, Clone, PartialEq)]
pub struct Erkannt {
    pub text: String,
    /// Mittlere Sicherheit der Wörter laut Tesseract, 0 bis 100.
    pub konfidenz: i32,
    /// Die Wörter mit ihrer Lage im Bild, in Lesereihenfolge.
    pub woerter: Vec<Wort>,
}

/// Ein erkanntes Wort und seine Lage im Bild, in Pixel ab der linken oberen
/// Ecke. Grundlage für die Textebene in der PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct Wort {
    pub text: String,
    /// Umschließendes Rechteck: links, oben, rechts, unten.
    pub links: i32,
    pub oben: i32,
    pub rechts: i32,
    pub unten: i32,
    /// Höhe der Grundlinie am linken und am rechten Rand des Worts. Bei
    /// schräg liegenden Scans unterscheiden sich die beiden Werte.
    pub grund_links: f32,
    pub grund_rechts: f32,
    /// Letztes Wort seiner Zeile.
    pub zeilenende: bool,
}

/// Ein initialisiertes Tesseract mit geladenen Sprachdaten.
pub struct Erkenner {
    handle: *mut TessBaseApi,
}

// SAFETY: Eine TessBaseAPI-Instanz darf zwischen Fäden weitergegeben werden,
// solange sie nur von einem Faden zur Zeit benutzt wird. Das stellt `&mut self`
// sicher. `Sync` ist bewusst nicht implementiert.
unsafe impl Send for Erkenner {}

impl Erkenner {
    /// Lädt die Sprachdaten `sprachen` (z.B. "deu+eng") aus dem Ordner
    /// `tessdata`, in dem die `.traineddata`-Dateien liegen.
    pub fn neu(tessdata: &Path, sprachen: &str) -> Result<Self, String> {
        let ordner = tessdata
            .to_str()
            .ok_or_else(|| format!("Pfad der Sprachdaten nicht darstellbar: {}", tessdata.display()))?;
        let ordner = CString::new(ordner).map_err(|e| e.to_string())?;
        let sprachen_c = CString::new(sprachen).map_err(|e| e.to_string())?;
        for sprache in sprachen.split('+') {
            let datei = tessdata.join(format!("{sprache}.traineddata"));
            if !datei.is_file() {
                return Err(format!(
                    "Sprachdaten fehlen: {}. Bitte im Projektordner ausführen: bash tools/tesseract_holen.sh",
                    datei.display()
                ));
            }
        }
        // Leptonica meldet sonst bei jedem Start fehlende Bildformate auf der
        // Konsole. Sie werden nicht gebraucht, die Seiten kommen als Rohdaten.
        // SAFETY: setzt nur eine globale Einstellung von Leptonica.
        STILL.call_once(|| unsafe {
            setMsgSeverity(L_SEVERITY_NONE);
        });
        // SAFETY: Der Zeiger aus TessBaseAPICreate wird nur über diese Struktur
        // benutzt und in Drop freigegeben. Alle übergebenen Zeichenketten leben
        // länger als der jeweilige Aufruf.
        unsafe {
            let handle = TessBaseAPICreate();
            if handle.is_null() {
                return Err("Tesseract ließ sich nicht anlegen".into());
            }
            let erkenner = Erkenner { handle };
            // Meldungen von Tesseract nicht auf die Konsole schreiben
            if cfg!(unix) {
                let name = CString::new("debug_file").unwrap();
                let wert = CString::new("/dev/null").unwrap();
                TessBaseAPISetVariable(handle, name.as_ptr(), wert.as_ptr());
            }
            if TessBaseAPIInit2(handle, ordner.as_ptr(), sprachen_c.as_ptr(), OEM_LSTM_ONLY) != 0 {
                return Err(format!("Tesseract: Sprachdaten {sprachen} aus {} nicht geladen", tessdata.display()));
            }
            TessBaseAPISetPageSegMode(handle, PSM_AUTO);
            Ok(erkenner)
        }
    }

    /// Erkennt den Text eines Graustufenbildes (ein Byte je Pixel, Zeile für
    /// Zeile ohne Lücken). `dpi` ist die Auflösung, mit der gerendert wurde.
    pub fn erkennen(&mut self, grau: &[u8], breite: u32, hoehe: u32, dpi: u32) -> Result<Erkannt, String> {
        if breite == 0 || hoehe == 0 || grau.len() != breite as usize * hoehe as usize {
            return Err("OCR: Bild ohne gültige Maße".into());
        }
        if breite > i32::MAX as u32 || hoehe > i32::MAX as u32 {
            return Err("OCR: Bild zu groß".into());
        }
        // SAFETY: `grau` hat genau breite × hoehe Bytes und lebt bis nach
        // TessBaseAPIClear. Der Text wird kopiert und mit TessDeleteText
        // freigegeben.
        unsafe {
            TessBaseAPISetImage(self.handle, grau.as_ptr(), breite as c_int, hoehe as c_int, 1, breite as c_int);
            TessBaseAPISetSourceResolution(self.handle, dpi as c_int);
            let zeiger = TessBaseAPIGetUTF8Text(self.handle);
            let ergebnis = if zeiger.is_null() {
                Err("OCR: Tesseract lieferte keinen Text".to_string())
            } else {
                let text = CStr::from_ptr(zeiger).to_string_lossy().into_owned();
                TessDeleteText(zeiger);
                let woerter = self.woerter();
                Ok(Erkannt { text, konfidenz: TessBaseAPIMeanTextConf(self.handle), woerter })
            };
            TessBaseAPIClear(self.handle);
            ergebnis
        }
    }
}

impl Erkenner {
    /// Liest die Wörter der zuletzt erkannten Seite. Nur zwischen der
    /// Erkennung und `TessBaseAPIClear` aufzurufen.
    ///
    /// # Safety
    /// `self.handle` muss eine erkannte Seite halten.
    unsafe fn woerter(&mut self) -> Vec<Wort> {
        let mut woerter = Vec::new();
        let it = TessBaseAPIGetIterator(self.handle);
        if it.is_null() {
            return woerter;
        }
        // Der Seiten-Iterator ist derselbe Zeiger in anderer Sicht und wird
        // nicht eigens freigegeben.
        let seite = TessResultIteratorGetPageIterator(it);
        loop {
            let zeiger = TessResultIteratorGetUTF8Text(it, RIL_WORD);
            let text = if zeiger.is_null() {
                String::new()
            } else {
                let t = CStr::from_ptr(zeiger).to_string_lossy().trim().to_string();
                TessDeleteText(zeiger);
                t
            };
            let (mut l, mut o, mut r, mut u) = (0, 0, 0, 0);
            let (mut x1, mut y1, mut x2, mut y2) = (0, 0, 0, 0);
            let rahmen = TessPageIteratorBoundingBox(seite, RIL_WORD, &mut l, &mut o, &mut r, &mut u) != 0;
            let grund = TessPageIteratorBaseline(seite, RIL_WORD, &mut x1, &mut y1, &mut x2, &mut y2) != 0;
            if rahmen && !text.is_empty() && r > l && u > o {
                let (grund_links, grund_rechts) =
                    if grund { grundlinie(x1, y1, x2, y2, l, r) } else { (u as f32, u as f32) };
                woerter.push(Wort {
                    text,
                    links: l,
                    oben: o,
                    rechts: r,
                    unten: u,
                    grund_links,
                    grund_rechts,
                    zeilenende: TessPageIteratorIsAtFinalElement(seite, RIL_TEXTLINE, RIL_WORD) != 0,
                });
            } else if let Some(letztes) = woerter.last_mut() {
                // Ein übergangenes Wort am Zeilenende beendet trotzdem die Zeile
                if TessPageIteratorIsAtFinalElement(seite, RIL_TEXTLINE, RIL_WORD) != 0 {
                    letztes.zeilenende = true;
                }
            }
            if TessResultIteratorNext(it, RIL_WORD) == 0 {
                break;
            }
        }
        TessResultIteratorDelete(it);
        woerter
    }
}

/// Höhe der Grundlinie (x1,y1)–(x2,y2) an den Stellen `links` und `rechts`.
pub fn grundlinie(x1: i32, y1: i32, x2: i32, y2: i32, links: i32, rechts: i32) -> (f32, f32) {
    if x2 == x1 {
        return (y1 as f32, y1 as f32);
    }
    let steigung = (y2 - y1) as f32 / (x2 - x1) as f32;
    let y = |x: i32| y1 as f32 + steigung * (x - x1) as f32;
    (y(links), y(rechts))
}

impl Drop for Erkenner {
    fn drop(&mut self) {
        // SAFETY: handle stammt aus TessBaseAPICreate und wird nur hier freigegeben.
        unsafe {
            TessBaseAPIEnd(self.handle);
            TessBaseAPIDelete(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ordner mit den Sätzen der Sprachdaten: `REGISTERWERK_TESSDATA` oder
    /// `bibliotheken/tesseract/tessdata` im Projekt. Fehlen sie, ist der Test
    /// rot (v0.1.2, vorher still übersprungen).
    fn tessdata_basis() -> std::path::PathBuf {
        if let Some(o) = std::env::var_os("REGISTERWERK_TESSDATA") {
            return o.into();
        }
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|o| o.join("bibliotheken/tesseract/tessdata"))
            .find(|o| o.join("standard/deu.traineddata").is_file())
            .expect("Sprachdaten fehlen. Bitte im Projektordner ausführen: bash tools/tesseract_holen.sh")
    }

    #[test]
    fn version_ist_5() {
        assert!(version().starts_with('5'), "{}", version());
    }

    #[test]
    fn fehlende_sprachdaten_werden_gemeldet() {
        let tmp = std::env::temp_dir().join("registerwerk-ocr-leer");
        std::fs::create_dir_all(&tmp).unwrap();
        let fehler = Erkenner::neu(&tmp, "deu").err().unwrap();
        assert!(fehler.contains("tesseract_holen.sh"), "{fehler}");
    }

    #[test]
    fn leeres_bild_wird_abgelehnt() {
        // Ordner mit den Sätzen der Sprachdaten, wie bibliotheken/tesseract/tessdata
        let basis = tessdata_basis();
        let mut e = Erkenner::neu(&basis.join("standard"), "deu").unwrap();
        assert!(e.erkennen(&[], 0, 0, 300).is_err());
        let weiss = vec![255u8; 200 * 100];
        let r = e.erkennen(&weiss, 200, 100, 300).unwrap();
        assert!(r.text.trim().is_empty());
        assert!(r.woerter.is_empty());
    }

    /// Zwei Zeilen Text als Graustufenbild (PGM, `tests/daten/zeilen.pgm`,
    /// erzeugt mit Pillow und DejaVu Sans): "Kontoauszug Stuttgart" und
    /// "Betrag 123,45" (v0.1.2).
    fn zeilen() -> (Vec<u8>, u32, u32) {
        let d = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/daten/zeilen.pgm")).unwrap();
        // Kopf: P5, Breite, Höhe, Höchstwert, je durch Leerraum getrennt
        let mut teile = Vec::new();
        let mut i = 0;
        while teile.len() < 4 {
            while d[i].is_ascii_whitespace() {
                i += 1;
            }
            let anfang = i;
            while !d[i].is_ascii_whitespace() {
                i += 1;
            }
            teile.push(std::str::from_utf8(&d[anfang..i]).unwrap().to_string());
        }
        assert_eq!(teile[0], "P5");
        let (b, h): (u32, u32) = (teile[1].parse().unwrap(), teile[2].parse().unwrap());
        (d[i + 1..].to_vec(), b, h)
    }

    #[test]
    fn erkennt_woerter_mit_lage_und_zeilenende() {
        let (grau, b, h) = zeilen();
        let mut e = Erkenner::neu(&tessdata_basis().join("standard"), "deu+eng").unwrap();
        let r = e.erkennen(&grau, b, h, 300).unwrap();
        assert!(r.text.contains("Kontoauszug Stuttgart"), "{:?}", r.text);
        assert!(r.text.contains("123,45"), "{:?}", r.text);
        assert!(r.konfidenz > 50, "{}", r.konfidenz);
        let woerter: Vec<(&str, bool)> = r.woerter.iter().map(|w| (w.text.as_str(), w.zeilenende)).collect();
        assert_eq!(woerter, [("Kontoauszug", false), ("Stuttgart", true), ("Betrag", false), ("123,45", true)]);
        // Lage: erste Zeile über der zweiten, links vor rechts
        let (k, s, bt) = (&r.woerter[0], &r.woerter[1], &r.woerter[2]);
        assert!(k.rechts < s.links && k.unten < bt.oben, "{k:?} {s:?} {bt:?}");
        assert!(k.grund_links > k.oben as f32 && k.grund_links <= k.unten as f32 + 1.0, "{k:?}");
        // Derselbe Erkenner ein zweites Mal
        assert_eq!(e.erkennen(&grau, b, h, 300).unwrap().woerter.len(), 4);
        // Falsche Maße
        assert!(e.erkennen(&grau, b + 1, h, 300).unwrap_err().contains("Maße"));
    }

    #[test]
    fn ungueltige_sprachangabe() {
        assert!(Erkenner::neu(&tessdata_basis().join("standard"), "de\0u").is_err());
        assert!(version().contains('.'));
    }

    #[test]
    fn grundlinie_schraeg_und_waagerecht() {
        assert_eq!(grundlinie(10, 50, 110, 60, 10, 110), (50.0, 60.0));
        assert_eq!(grundlinie(10, 50, 110, 60, 60, 60), (55.0, 55.0));
        assert_eq!(grundlinie(10, 50, 10, 70, 0, 100), (50.0, 50.0));
    }

    /// Erkennt ein gezeichnetes Wort und prüft dessen Lage. Braucht die
    /// Sprachdaten.
    #[test]
    fn woerter_mit_lage() {
        let basis = tessdata_basis();
        let mut e = Erkenner::neu(&basis.join("standard"), "eng").unwrap();
        // Ein schwarzer Balken ergibt kein Wort, nur die Schnittstelle wird geprüft
        let (b, h) = (600u32, 200u32);
        let mut bild = vec![255u8; (b * h) as usize];
        for y in 80..120 {
            for x in 100..500 {
                bild[(y * b + x) as usize] = 0;
            }
        }
        let r = e.erkennen(&bild, b, h, 300).unwrap();
        for w in &r.woerter {
            assert!(w.links < w.rechts && w.oben < w.unten, "{w:?}");
            assert!(w.rechts <= b as i32 && w.unten <= h as i32, "{w:?}");
        }
    }
}
