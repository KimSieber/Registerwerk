//! Vorschauen für einzelne Bilder im Archiv (v0.0.6), etwa versehentlich als
//! Bild gespeicherte Scans. Bildordner deckt Bilderwerk ab, sie bleiben
//! ausgeschlossen.
//!
//! Das Bild wird nach der Ausrichtung aus den EXIF-Angaben gedreht und wie
//! eine PDF-Seite eingepasst (`vorschau::massstab`): kurze Kante 1024 px, lange
//! Kante höchstens 3072 px. Vergrößert wird nie.
//! Kodiert wird wie bei PDF-Seiten (`vorschau.rs`).
//!
//! HEIC und HEIF (iPhone) liest seit v0.0.7 `zune-heic`. TIFF aus
//! Faxgeräten (CCITT Gruppe 3) liest ein eigener Leser mit `fax`.

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
    let endung = pfad.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if endung == "heic" || endung == "heif" {
        // Manche Datei mit dieser Endung ist in Wahrheit ein JPEG
        return heic(pfad).or_else(|fehler| allgemein(pfad).map_err(|_| fehler));
    }
    if endung == "tif" || endung == "tiff" {
        // Faxe sind mit CCITT Gruppe 3 gepackt, das liest `image` nicht
        return allgemein(pfad).or_else(|fehler| fax(pfad).ok_or(fehler));
    }
    allgemein(pfad)
}

/// TIFF aus Faxgeräten (v0.0.7): schwarzweiß, gepackt mit CCITT Gruppe 3 in
/// eindimensionaler Kodierung. Gelesen wird die erste Seite. Faxe in normaler
/// Auflösung haben halb so viele Zeilen wie Spalten je Zoll: Die Zeilen
/// werden dann verdoppelt, damit das Bild nicht gestaucht aussieht.
/// `None`, wenn die Datei kein solches Fax ist.
fn fax(pfad: &Path) -> Option<DynamicImage> {
    use tiff::decoder::Decoder;
    use tiff::tags::Tag;
    const T4_OPTIONS: Tag = Tag::Unknown(292);
    let daten = std::fs::read(pfad).ok()?;
    let mut d = Decoder::new(std::io::Cursor::new(&daten)).ok()?;
    let zahl = |d: &mut Decoder<_>, tag: Tag| d.find_tag_unsigned::<u32>(tag).ok().flatten();
    if zahl(&mut d, Tag::Compression)? != 3 || zahl(&mut d, T4_OPTIONS).unwrap_or(0) & 1 != 0 {
        return None;
    }
    let breite = zahl(&mut d, Tag::ImageWidth)?;
    let hoehe = zahl(&mut d, Tag::ImageLength)?;
    if breite == 0 || hoehe == 0 || breite > 20_000 || hoehe > 100_000 {
        return None;
    }
    // 0 heißt weiß (üblich) oder schwarz
    let null_ist_weiss = zahl(&mut d, Tag::PhotometricInterpretation).unwrap_or(0) == 0;
    let gedreht = zahl(&mut d, Tag::FillOrder).unwrap_or(1) == 2;
    let orte = d.get_tag_u64_vec(Tag::StripOffsets).ok()?;
    let laengen = d.get_tag_u64_vec(Tag::StripByteCounts).ok()?;
    // Die Auflösung steht als Bruch in der Datei
    let aufloesung = |d: &mut Decoder<_>, tag: Tag| match d.find_tag(tag).ok().flatten() {
        Some(tiff::decoder::ifd::Value::Rational(z, n)) if n > 0 => z as f64 / n as f64,
        Some(wert) => wert.into_f64().unwrap_or(0.0),
        None => 0.0,
    };
    let x = aufloesung(&mut d, Tag::XResolution);
    let y = aufloesung(&mut d, Tag::YResolution);
    let je_zeile: u32 = if y > 0.0 && x / y >= 1.5 { 2 } else { 1 };

    let (weiss, schwarz) = if null_ist_weiss { (255u8, 0u8) } else { (0u8, 255u8) };
    let mut pixel: Vec<u8> = Vec::with_capacity((breite * hoehe * je_zeile) as usize);
    let mut zeilen = 0u32;
    for (ort, laenge) in orte.iter().zip(&laengen) {
        let streifen = daten.get(*ort as usize..(*ort as usize).checked_add(*laenge as usize)?)?;
        let bytes = streifen.iter().map(|b| if gedreht { b.reverse_bits() } else { *b });
        // Ein Streifen ohne Endmarke gilt trotzdem, soweit er gelesen wurde
        let _ = fax::decoder::decode_g3(bytes, |wechsel| {
            if zeilen < hoehe {
                let zeile: Vec<u8> = fax::decoder::pels(wechsel, breite)
                    .map(|farbe| if farbe == fax::Color::White { weiss } else { schwarz })
                    .collect();
                for _ in 0..je_zeile {
                    pixel.extend_from_slice(&zeile);
                }
                zeilen += 1;
            }
        });
    }
    if zeilen == 0 {
        return None;
    }
    image::GrayImage::from_raw(breite, zeilen * je_zeile, pixel).map(DynamicImage::ImageLuma8)
}

/// HEIC und HEIF (v0.0.7) mit `zune-heic`, einem Decoder in reinem Rust.
/// Drehung und Spiegelung aus der Datei sind schon angewendet. Die
/// Videotoolbox von macOS bleibt aus: Das Ergebnis soll auf jedem System
/// dasselbe sein (Grundsatz 4).
fn heic(pfad: &Path) -> Result<DynamicImage, String> {
    use zune_heic::zune_core::bytestream::ZCursor;
    use zune_heic::zune_core::options::DecoderOptions;
    let daten = std::fs::read(pfad).map_err(|e| e.to_string())?;
    let optionen = DecoderOptions::default().hvec_set_use_videotoolbox(false);
    let mut dekoder = zune_heic::HeifDecoder::new_with_options(ZCursor::new(&daten), optionen);
    let pixel = dekoder.decode().map_err(|e| format!("HEIC: {e:?}"))?;
    let (Some(b), Some(h)) = (dekoder.width(), dekoder.height()) else {
        return Err("HEIC: Größe fehlt".into());
    };
    let (b, h) = (b as u32, h as u32);
    let kanaele = pixel.len() / (b as usize * h as usize).max(1);
    let bild = match kanaele {
        1 => image::GrayImage::from_raw(b, h, pixel).map(DynamicImage::ImageLuma8),
        2 => image::GrayAlphaImage::from_raw(b, h, pixel).map(DynamicImage::ImageLumaA8),
        3 => image::RgbImage::from_raw(b, h, pixel).map(DynamicImage::ImageRgb8),
        4 => image::RgbaImage::from_raw(b, h, pixel).map(DynamicImage::ImageRgba8),
        _ => None,
    };
    bild.ok_or_else(|| "HEIC: unerwartetes Pixelformat".to_string())
}

fn allgemein(pfad: &Path) -> Result<DynamicImage, String> {
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

        // Fax (CCITT Gruppe 3): 400 x 120 Pixel bei 204 x 98 dpi, die Zeilen
        // werden verdoppelt. Unter 1024 px wird nichts vergrößert.
        let fax = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/daten/fax_g3.tif");
        let b = laden(&fax).unwrap();
        assert_eq!((b.breite, b.hoehe), (400, 240));
        let schwarz = b.rgb.chunks_exact(3).filter(|p| p[0] < 128).count();
        // die Vorlage hat 4.743 schwarze Pixel, jede Zeile steht zweimal
        assert_eq!(schwarz, 2 * 4743);
        // das Rechteck oben links ist schwarz, die Ecke daneben weiß
        let bei = |x: usize, y: usize| b.rgb[(y * 400 + x) * 3];
        assert_eq!((bei(60, 2 * 30), bei(5, 2 * 5), bei(130, 2 * 30)), (0, 255, 255));
        assert!(fuer_ocr(&fax).is_ok());

        // HEIC: kaputte Datei meldet den Fehler des HEIC-Decoders,
        // ein JPEG mit falscher Endung wird trotzdem gelesen
        let kein_heic = tmp.path().join("kaputt.heic");
        std::fs::write(&kein_heic, b"kein bild").unwrap();
        assert!(laden(&kein_heic).unwrap_err().starts_with("HEIC"));
        let falsch = tmp.path().join("falsch.HEIC");
        std::fs::copy(&quer, &falsch).unwrap();
        assert_eq!(laden(&falsch).unwrap().breite, 1536);
    }
}
