//! PDFs lesen über PDFium.
//!
//! PDFium ist eine C++-Bibliothek. Sie liegt nicht im Repo, sondern wird mit
//! `tools/pdfium_holen.sh` geladen und zur Laufzeit eingebunden. Die
//! Funktionsversion von `pdfium-render` (Feature `pdfium_NNNN` in Cargo.toml)
//! muss zur Version der Bibliothek passen.
//!
//! Dieses Modul liest nur. Es verändert keine Datei. Auch die Prüfung der
//! Textebene ([`textebene_pruefen`]) liest nur.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pdfium_render::prelude::*;

/// Für Werkzeuge, die den Kern nutzen, ohne pdfium-render selbst einzubinden.
pub use pdfium_render::prelude::Pdfium;

use crate::{Ergebnis, Fehler};

/// Ab diesem Anteil der Seitenfläche, den Bilder abdecken, gilt eine Seite als Scan.
pub const SCAN_FLAECHE: f32 = 0.7;

/// Ordner, in dem `tools/pdfium_holen.sh` die Bibliothek ablegt, relativ zum Projekt.
pub const ENTWICKLUNGS_ORDNER: &str = "bibliotheken/pdfium/lib";

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Inhalt einer Seite.
#[derive(Debug, Clone, PartialEq)]
pub struct Seite {
    /// Seitennummer ab 1.
    pub nr: u32,
    pub text: String,
    /// Anteil der Seitenfläche, den Bilder abdecken, zwischen 0 und 1.
    pub bildanteil: f32,
}

impl Seite {
    pub fn ist_scan(&self) -> bool {
        self.bildanteil >= SCAN_FLAECHE
    }
}

/// Was aus einer PDF gelesen wird.
#[derive(Debug, Clone, PartialEq)]
pub struct Inhalt {
    pub seiten: Vec<Seite>,
    /// Erstellungsdatum aus den Metadaten, als Rohwert (`D:JJJJMMTT...`).
    pub erstellt: Option<String>,
    /// Der Text ist nur grob gelesen: lesbare Zeichenfolgen der Datei, weil
    /// kein Leser das Format verstand (v0.0.9).
    pub grob: bool,
}

/// Name der Bibliothek auf dieser Plattform in einem Ordner,
/// z.B. `ordner/libpdfium.dylib`.
pub fn bibliothek_in(ordner: &Path) -> PathBuf {
    Pdfium::pdfium_platform_library_name_at_path(ordner)
}

/// Sucht den Ordner `bibliotheken/pdfium/lib` ab `start` aufwärts.
pub fn suchen_ab(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|ort| ort.join(ENTWICKLUNGS_ORDNER))
        .find(|ordner| bibliothek_in(ordner).is_file())
}

/// Bindet PDFium aus `ordner` ein. Die Bibliothek wird nur einmal je Programmlauf
/// geladen. Spätere Aufrufe liefern dieselbe Instanz, auch mit anderem Ordner.
pub fn einbinden(ordner: &Path) -> Ergebnis<&'static Pdfium> {
    // Ohne eigene kleine Funktionen für die Fehler: Dieser Weg lässt sich in
    // Tests nicht gehen, weil die Bibliothek nur einmal je Lauf geladen wird.
    let pdfium = PDFIUM.get_or_init(|| {
        let bibliothek = bibliothek_in(ordner);
        match Pdfium::bind_to_library(&bibliothek) {
            Ok(b) => Ok(Pdfium::new(b)),
            Err(e) => Err(format!("PDFium nicht geladen aus {}: {e}", bibliothek.display())),
        }
    });
    match pdfium {
        Ok(p) => Ok(p),
        Err(e) => Err(Fehler::Pdf(e.clone())),
    }
}

/// Liest Text, Bildanteil je Seite und das Erstellungsdatum. Fehler beim
/// Öffnen werden als Text gemeldet, damit sie ins Register geschrieben werden
/// können.
pub fn lesen(pdfium: &Pdfium, pfad: &Path) -> Result<Inhalt, String> {
    let dokument = pdfium.load_pdf_from_file(pfad, None).map_err(|e| beschreiben(&e))?;
    let erstellt = dokument
        .metadata()
        .get(PdfDocumentMetadataTagType::CreationDate)
        .map(|t| t.value().to_string())
        .filter(|w| !w.trim().is_empty());
    let mut seiten = Vec::new();
    for (i, seite) in dokument.pages().iter().enumerate() {
        let text = seitentext(&seite);
        seiten.push(Seite { nr: i as u32 + 1, text, bildanteil: bildanteil(&seite) });
    }
    Ok(Inhalt { seiten, erstellt, grob: false })
}

/// Prüft eine neue Fassung mit Textebene gegen das Original und liest ihren
/// Inhalt (v0.0.5):
///
/// - beide lassen sich öffnen und haben dieselbe Seitenzahl,
/// - die Seiten `seiten` hatten im Original keinen Text und haben jetzt
///   welchen,
/// - diese Seiten sehen gerendert genau gleich aus wie im Original.
pub fn textebene_pruefen(pdfium: &Pdfium, alt: &Path, neu: &Path, seiten: &[u32]) -> Result<Inhalt, String> {
    {
        let a = pdfium.load_pdf_from_file(alt, None).map_err(|e| format!("Original: {}", beschreiben(&e)))?;
        let n = pdfium.load_pdf_from_file(neu, None).map_err(|e| format!("neue Fassung: {}", beschreiben(&e)))?;
        if a.pages().len() != n.pages().len() {
            return Err(format!("Seitenzahl {} statt {}", n.pages().len(), a.pages().len()));
        }
        let config = PdfRenderConfig::new().scale_page_by_factor(1.0).set_clear_color(PdfColor::WHITE);
        for &nr in seiten {
            let index = nr.checked_sub(1).and_then(|i| i32::try_from(i).ok()).ok_or("Seitennummer ungültig")?;
            let sa = a.pages().get(index).map_err(|e| format!("Seite {nr}: {e:?}"))?;
            let sn = n.pages().get(index).map_err(|e| format!("Seite {nr}: {e:?}"))?;
            let text = |s: &PdfPage| crate::guete::zaehlen(&seitentext(s));
            if !text(&sa).ohne_text() {
                return Err(format!("Seite {nr} hat im Original schon Text"));
            }
            if text(&sn).ohne_text() {
                return Err(format!("Seite {nr} hat in der neuen Fassung keinen Text"));
            }
            let bild = |s: &PdfPage| {
                s.render_with_config(&config)
                    .map(|b| (b.width(), b.height(), b.as_rgba_bytes()))
                    .map_err(|e| format!("Seite {nr} rendern: {e:?}"))
            };
            if bild(&sa)? != bild(&sn)? {
                return Err(format!("Seite {nr} sieht in der neuen Fassung anders aus"));
            }
        }
    }
    lesen(pdfium, neu)
}

/// Text einer Seite innerhalb ihres sichtbaren Bereichs.
///
/// `PdfPageText::all()` begrenzt auf die Seitengröße nach der Drehung, aber in
/// den Koordinaten vor der Drehung. Bei Seiten mit `/Rotate 270` fiel der
/// Text dadurch heraus (gefunden in v0.0.5). Hier wird auf den sichtbaren
/// Bereich in Seitenkoordinaten begrenzt.
pub fn seitentext(seite: &PdfPage) -> String {
    let Ok(text) = seite.text() else { return String::new() };
    match seite.boundaries().bounding() {
        Ok(b) => text.inside_rect(b.bounds),
        Err(_) => text.all(),
    }
}

fn beschreiben(e: &PdfiumError) -> String {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            "PDF ist mit einem Kennwort geschützt".into()
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            "keine gültige PDF oder beschädigt".into()
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError) => {
            "Datei ließ sich nicht öffnen".into()
        }
        PdfiumError::IoError(io) => format!("Dateizugriff: {io}"),
        andere => format!("{andere:?}"),
    }
}

/// Anteil der Seitenfläche, den Bilder abdecken (höchstens 1). Bilder in
/// Formularobjekten (Form XObjects) werden mitgezählt.
fn bildanteil(seite: &PdfPage) -> f32 {
    let flaeche = seite.width().value * seite.height().value;
    if flaeche <= 0.0 {
        return 0.0;
    }
    let bilder: f32 = seite.objects().iter().map(|o| bildflaeche(&o, 0)).sum();
    (bilder / flaeche).min(1.0)
}

fn bildflaeche(obj: &PdfPageObject, tiefe: u8) -> f32 {
    match obj.object_type() {
        PdfPageObjectType::Image => obj.bounds().map(|b| b.width().value * b.height().value).unwrap_or(0.0),
        PdfPageObjectType::XObjectForm if tiefe < 4 => obj
            .as_x_object_form_object()
            .map(|f| f.iter().map(|o| bildflaeche(&o, tiefe + 1)).sum())
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testumgebung::{pdfium, pdfium_ordner, test_pdf};

    /// Eine PDF mit einer leeren Seite A4.
    fn leere_seite() -> Vec<u8> {
        let objekte = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>",
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut lagen = Vec::new();
        for (i, o) in objekte.iter().enumerate() {
            lagen.push(pdf.len());
            pdf.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = pdf.len();
        pdf.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objekte.len() + 1).bytes());
        for l in lagen {
            pdf.extend(format!("{l:010} 00000 n \n").bytes());
        }
        pdf.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%EOF\n", objekte.len() + 1).bytes());
        pdf
    }

    #[test]
    fn findet_die_bibliothek() {
        assert!(bibliothek_in(&pdfium_ordner()).is_file());
        assert!(suchen_ab(Path::new("/")).is_none());
        // Ein zweites Einbinden liefert dieselbe Instanz, auch mit anderem Ordner.
        // Erst einbinden: Ein falscher Ordner zuerst bliebe für alle Tests stehen.
        let erste = pdfium() as *const Pdfium;
        assert_eq!(einbinden(Path::new("/gibt/es/nicht")).unwrap() as *const Pdfium, erste);
    }

    #[test]
    fn liest_text_bildanteil_und_datum() {
        let i = lesen(pdfium(), &test_pdf()).unwrap();
        assert_eq!(i.seiten.len(), 2);
        assert!(!i.grob);
        assert!(i.erstellt.as_deref().unwrap().starts_with("D:20240315"), "{:?}", i.erstellt);
        let (s1, s2) = (&i.seiten[0], &i.seiten[1]);
        assert_eq!((s1.nr, s2.nr), (1, 2));
        assert!(s1.text.contains("Rechnung Nummer 4711"), "{:?}", s1.text);
        assert!(s1.text.contains("enthält"), "Umlaute: {:?}", s1.text);
        assert!(s1.bildanteil < 0.01 && !s1.ist_scan());
        assert!(s2.text.trim().is_empty(), "{:?}", s2.text);
        assert!(s2.bildanteil > 0.99 && s2.ist_scan());
    }

    /// Ein Scan, dessen Bild in einem Formularobjekt liegt, gilt als Scan (v0.1.2).
    #[test]
    fn bild_im_formularobjekt_zaehlt() {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let pixel = vec![200u8; 20 * 28];
        let bild = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 20, "Height" => 28,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            pixel,
        ));
        let form = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => bild } },
            },
            b"q 595 0 0 842 0 0 cm /Im0 Do Q".to_vec(),
        ));
        let inhalt = doc.add_object(Stream::new(lopdf::Dictionary::new(), b"/Fm0 Do".to_vec()));
        let seiten_id = doc.new_object_id();
        let seite = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => seiten_id, "Contents" => inhalt,
            "Resources" => dictionary! { "XObject" => dictionary! { "Fm0" => form } },
        });
        doc.objects.insert(
            seiten_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![seite.into()], "Count" => 1,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let katalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => seiten_id });
        doc.trailer.set("Root", katalog);
        let tmp = tempfile::tempdir().unwrap();
        let pfad = tmp.path().join("form.pdf");
        doc.save(&pfad).unwrap();
        let i = lesen(pdfium(), &pfad).unwrap();
        assert!(i.seiten[0].ist_scan(), "Bildanteil {}", i.seiten[0].bildanteil);
    }

    #[test]
    fn meldet_unlesbare_dateien() {
        let tmp = tempfile::tempdir().unwrap();
        let kaputt = tmp.path().join("kaputt.pdf");
        std::fs::write(&kaputt, b"%PDF-1.4 kaputt").unwrap();
        let f = lesen(pdfium(), &kaputt).unwrap_err();
        assert!(f.contains("keine gültige PDF"), "{f}");
        let f = lesen(pdfium(), &tmp.path().join("fehlt.pdf")).unwrap_err();
        assert!(!f.is_empty());
    }

    #[test]
    fn pruefung_der_textebene_lehnt_falsches_ab() {
        let tmp = tempfile::tempdir().unwrap();
        let pdf = test_pdf();
        // Seite 1 hatte schon Text
        let f = textebene_pruefen(pdfium(), &pdf, &pdf, &[1]).unwrap_err();
        assert!(f.contains("schon Text"), "{f}");
        // Seite 2 hat auch in der "neuen Fassung" keinen Text
        let f = textebene_pruefen(pdfium(), &pdf, &pdf, &[2]).unwrap_err();
        assert!(f.contains("keinen Text"), "{f}");
        assert!(textebene_pruefen(pdfium(), &pdf, &pdf, &[0]).unwrap_err().contains("ungültig"));
        assert!(textebene_pruefen(pdfium(), &pdf, &pdf, &[3]).unwrap_err().contains("Seite 3"));
        // Ohne Seiten wird nur gelesen
        assert_eq!(textebene_pruefen(pdfium(), &pdf, &pdf, &[]).unwrap().seiten.len(), 2);
        // Andere Seitenzahl, beschädigte Fassung
        let eine = tmp.path().join("eine.pdf");
        std::fs::write(&eine, leere_seite()).unwrap();
        assert!(textebene_pruefen(pdfium(), &pdf, &eine, &[2]).unwrap_err().contains("Seitenzahl"));
        let kaputt = tmp.path().join("kaputt.pdf");
        std::fs::write(&kaputt, b"nichts").unwrap();
        assert!(textebene_pruefen(pdfium(), &pdf, &kaputt, &[2]).unwrap_err().starts_with("neue Fassung"));
        assert!(textebene_pruefen(pdfium(), &kaputt, &pdf, &[2]).unwrap_err().starts_with("Original"));
    }
}
