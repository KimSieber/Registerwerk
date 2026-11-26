//! Hilfen für Tests mit PDFium und Tesseract (v0.1.2).
//!
//! Die Bibliotheken liegen in `bibliotheken/` im Projekt
//! (`tools/pdfium_holen.sh`, `tools/tesseract_holen.sh`). Ein anderer Ort geht
//! über `REGISTERWERK_PDFIUM` und `REGISTERWERK_TESSDATA`. Fehlen sie, sind
//! die Tests rot (Kim, v0.1.2): Bis v0.1.1 kehrten sie still zurück, und
//! niemand merkte, dass PDFium nie getestet wurde.

use std::path::{Path, PathBuf};

fn projekt() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Ordner mit der PDFium-Bibliothek.
pub fn pdfium_ordner() -> PathBuf {
    if let Some(o) = std::env::var_os("REGISTERWERK_PDFIUM") {
        return PathBuf::from(o);
    }
    crate::pdf::suchen_ab(projekt())
        .expect("PDFium fehlt. Bitte im Projektordner ausführen: bash tools/pdfium_holen.sh")
}

/// PDFium, eingebunden aus [`pdfium_ordner`].
pub fn pdfium() -> &'static crate::pdf::Pdfium {
    crate::pdf::einbinden(&pdfium_ordner()).expect("PDFium ließ sich nicht laden")
}

/// Ordner mit den Sprachdaten `standard` (deu, eng).
pub fn tessdata() -> PathBuf {
    let basis = std::env::var_os("REGISTERWERK_TESSDATA").map(PathBuf::from).unwrap_or_else(|| {
        projekt()
            .ancestors()
            .map(|o| o.join("bibliotheken/tesseract/tessdata"))
            .find(|o| o.is_dir())
            .expect("Sprachdaten fehlen. Bitte im Projektordner ausführen: bash tools/tesseract_holen.sh")
    });
    let ordner = basis.join("standard");
    assert!(ordner.join("deu.traineddata").is_file(), "Sprachdaten fehlen in {}", ordner.display());
    ordner
}

/// Test-PDF aus `tests/daten` (`tools/testpdf_erzeugen.py`): Seite 1 mit Text
/// und rotem Balken, Seite 2 ein Scan in Graustufen ohne Text.
pub fn test_pdf() -> PathBuf {
    projekt().join("tests/daten/test.pdf")
}

/// Text auf der Scan-Seite der Test-PDF.
pub const SCAN_TEXT: [&str; 3] = ["Kontoauszug", "Testbank", "Stuttgart"];
