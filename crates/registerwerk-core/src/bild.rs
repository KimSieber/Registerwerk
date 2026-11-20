//! Vorschauen für einzelne Bilder im Archiv (v0.0.6), etwa versehentlich als
//! Bild gespeicherte Scans. Bildordner deckt Bilderwerk ab, sie bleiben
//! ausgeschlossen.
//!
//! Das Bild wird nach der Ausrichtung aus den EXIF-Angaben gedreht und wie
//! eine PDF-Seite eingepasst (`vorschau::massstab`): kurze Kante 1024 px, lange
//! Kante höchstens 3072 px. Vergrößert wird nie.
//! Kodiert wird wie bei PDF-Seiten (`vorschau.rs`).

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader};

use crate::ocr::Graubild;
use crate::vorschau::Bild;

/// Lädt ein Bild als RGB, gedreht und verkleinert für die Vorschau.
pub fn laden(pfad: &Path) -> Result<Bild, String> {
    Ok(einpassen(oeffnen(pfad)?))
}

/// Inhalt eines Bilds: eine Seite ohne Text, ganz Bild. Die OCR erkennt sie
/// wie eine Scan-Seite ohne Text.
pub fn inhalt() -> crate::pdf::Inhalt {
    crate::pdf::Inhalt { seiten: vec![crate::pdf::Seite { nr: 1, text: String::new(), bildanteil: 1.0 }], erstellt: None }
}

/// Lädt ein Bild für die OCR in Graustufen in voller Auflösung, die längste
/// Kante höchstens [`crate::ocr::MAX_KANTE`].
pub fn fuer_ocr(pfad: &Path) -> Result<Graubild, String> {
    let mut bild = oeffnen(pfad)?;
    let grenze = crate::ocr::MAX_KANTE;
    if bild.width().max(bild.height()) > grenze {
        bild = bild.resize(grenze, grenze, image::imageops::FilterType::Triangle);
    }
    let grau = bild.to_luma8();
    Ok(Graubild { breite: grau.width(), hoehe: grau.height(), grau: grau.into_raw() })
}

/// Öffnet ein Bild und dreht es nach der Ausrichtung aus den EXIF-Angaben.
fn oeffnen(pfad: &Path) -> Result<DynamicImage, String> {
    let leser = ImageReader::open(pfad)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut dekoder = leser.into_decoder().map_err(|e| format!("Bild: {e}"))?;
    let ausrichtung = dekoder.orientation().map_err(|e| format!("Bild: {e}"))?;
    let mut bild = DynamicImage::from_decoder(dekoder).map_err(|e| format!("Bild: {e}"))?;
    bild.apply_orientation(ausrichtung);
    Ok(bild)
}

/// Verkleinert in den Rahmen, ohne zu vergrößern.
fn einpassen(bild: DynamicImage) -> Bild {
    let (b, h) = (bild.width().max(1), bild.height().max(1));
    let faktor = crate::vorschau::massstab(b as f32, h as f32).min(1.0);
    let bild = if faktor < 1.0 {
        let nb = ((b as f32 * faktor).round() as u32).max(1);
        let nh = ((h as f32 * faktor).round() as u32).max(1);
        bild.resize_exact(nb, nh, image::imageops::FilterType::Triangle)
    } else {
        bild
    };
    let rgb = bild.to_rgb8();
    Bild { breite: rgb.width(), hoehe: rgb.height(), rgb: rgb.into_raw() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verkleinert_in_den_rahmen_ohne_zu_vergroessern() {
        let tmp = tempfile::tempdir().unwrap();
        let gross = tmp.path().join("beleg.png");
        image::RgbImage::from_pixel(600, 4000, image::Rgb([250, 250, 250])).save(&gross).unwrap();
        let b = laden(&gross).unwrap();
        assert_eq!((b.breite, b.hoehe), (461, 3072));
        assert_eq!(b.rgb.len(), (b.breite * b.hoehe * 3) as usize);

        let quer = tmp.path().join("quer.jpg");
        image::RgbImage::from_pixel(3000, 2000, image::Rgb([10, 200, 10])).save(&quer).unwrap();
        let b = laden(&quer).unwrap();
        assert_eq!((b.breite, b.hoehe), (1536, 1024));

        let klein = tmp.path().join("klein.bmp");
        image::RgbImage::from_pixel(200, 100, image::Rgb([0, 0, 0])).save(&klein).unwrap();
        assert_eq!((laden(&klein).unwrap().breite, laden(&klein).unwrap().hoehe), (200, 100));

        let g = fuer_ocr(&gross).unwrap();
        assert_eq!((g.breite, g.hoehe, g.grau.len()), (600, 4000, 600 * 4000));

        let kaputt = tmp.path().join("kaputt.jpg");
        std::fs::write(&kaputt, b"kein bild").unwrap();
        assert!(laden(&kaputt).is_err());
    }
}
