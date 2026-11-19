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

/// `OEM_LSTM_ONLY` aus `capi.h`: nur das LSTM-Verfahren. Das alte Verfahren
/// ist beim Bau abgeschaltet.
const OEM_LSTM_ONLY: c_int = 1;
/// `PSM_AUTO` aus `publictypes.h`: Seite automatisch aufteilen, ohne Erkennung
/// der Ausrichtung.
const PSM_AUTO: c_int = 3;

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
                Ok(Erkannt { text, konfidenz: TessBaseAPIMeanTextConf(self.handle) })
            };
            TessBaseAPIClear(self.handle);
            ergebnis
        }
    }
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
        let Some(basis) = std::env::var_os("REGISTERWERK_TESSDATA") else { return };
        let mut e = Erkenner::neu(&Path::new(&basis).join("standard"), "deu").unwrap();
        assert!(e.erkennen(&[], 0, 0, 300).is_err());
        let weiss = vec![255u8; 200 * 100];
        let r = e.erkennen(&weiss, 200, 100, 300).unwrap();
        assert!(r.text.trim().is_empty());
    }
}
