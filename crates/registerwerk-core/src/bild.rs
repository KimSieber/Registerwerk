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
    crate::pdf::Inhalt { seiten: vec![crate::pdf::Seite { nr: 1, text: String::new(), bildanteil: 1.0 }], erstellt: None, grob: false }
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
        // Faxe (CCITT Gruppe 3) und Bilder mit Farbpalette liest `image` nicht
        return allgemein(pfad).or_else(|fehler| fax(pfad).or_else(|| palette(pfad)).ok_or(fehler));
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

/// TIFF mit Farbpalette (v0.0.8), wie sie ältere Scanner und Programme
/// schreiben: je Pixel eine Nummer in einer Tabelle von Farben. `tiff` liest
/// diese Bilder nicht. Gelesen wird die erste Seite, mit 1, 2, 4 oder 8 Bit
/// je Pixel, ungepackt, PackBits, LZW oder Deflate.
/// `None`, wenn die Datei kein solches Bild ist.
fn palette(pfad: &Path) -> Option<DynamicImage> {
    use tiff::decoder::Decoder;
    use tiff::tags::Tag;
    let daten = std::fs::read(pfad).ok()?;
    let mut d = Decoder::new(std::io::Cursor::new(&daten)).ok()?;
    let zahl = |d: &mut Decoder<_>, tag: Tag| d.find_tag_unsigned::<u32>(tag).ok().flatten();
    if zahl(&mut d, Tag::PhotometricInterpretation)? != 3 || zahl(&mut d, Tag::SamplesPerPixel).unwrap_or(1) != 1 {
        return None;
    }
    let bits = zahl(&mut d, Tag::BitsPerSample).unwrap_or(1);
    if ![1, 2, 4, 8].contains(&bits) {
        return None;
    }
    let breite = zahl(&mut d, Tag::ImageWidth)?;
    let hoehe = zahl(&mut d, Tag::ImageLength)?;
    if breite == 0 || hoehe == 0 || breite > 30_000 || hoehe > 100_000 {
        return None;
    }
    let packung = zahl(&mut d, Tag::Compression).unwrap_or(1);
    let vorhersage = zahl(&mut d, Tag::Predictor).unwrap_or(1);
    // Rot, dann Grün, dann Blau, je 2^bits Werte von 0 bis 65535
    let tabelle = d.get_tag_u16_vec(Tag::ColorMap).ok()?;
    let farben = 1usize << bits;
    if tabelle.len() < 3 * farben {
        return None;
    }
    let orte = d.get_tag_u64_vec(Tag::StripOffsets).ok()?;
    let laengen = d.get_tag_u64_vec(Tag::StripByteCounts).ok()?;

    let je_zeile = (breite as usize * bits as usize).div_ceil(8);
    let mut roh: Vec<u8> = Vec::with_capacity(je_zeile * hoehe as usize);
    for (ort, laenge) in orte.iter().zip(&laengen) {
        let streifen = daten.get(*ort as usize..(*ort as usize).checked_add(*laenge as usize)?)?;
        let mut teil = match packung {
            1 => streifen.to_vec(),
            32773 => packbits(streifen),
            5 => weezl::decode::Decoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8).decode(streifen).ok()?,
            8 | 32946 => {
                let mut aus = Vec::new();
                std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(streifen), &mut aus).ok()?;
                aus
            }
            _ => return None,
        };
        // Vorhersage waagerecht: jedes Byte ist der Abstand zum linken Nachbarn
        if vorhersage == 2 && bits == 8 {
            for zeile in teil.chunks_mut(je_zeile) {
                for i in 1..zeile.len() {
                    zeile[i] = zeile[i].wrapping_add(zeile[i - 1]);
                }
            }
        }
        roh.append(&mut teil);
    }
    let zeilen = (roh.len() / je_zeile).min(hoehe as usize);
    if zeilen == 0 {
        return None;
    }
    let farbe = |i: usize| [(tabelle[i] >> 8) as u8, (tabelle[farben + i] >> 8) as u8, (tabelle[2 * farben + i] >> 8) as u8];
    let maske = (farben - 1) as u8;
    let mut rgb = Vec::with_capacity(breite as usize * zeilen * 3);
    for zeile in roh.chunks_exact(je_zeile).take(zeilen) {
        for x in 0..breite as usize {
            let bit = x * bits as usize;
            let nr = (zeile[bit / 8] >> (8 - bits as usize - bit % 8)) & maske;
            rgb.extend_from_slice(&farbe(nr as usize));
        }
    }
    image::RgbImage::from_raw(breite, zeilen as u32, rgb).map(DynamicImage::ImageRgb8)
}

/// Entpackt PackBits: ein Zählbyte, dann so viele Bytes wörtlich oder ein
/// Byte so oft wiederholt.
fn packbits(daten: &[u8]) -> Vec<u8> {
    let mut aus = Vec::with_capacity(daten.len() * 2);
    let mut i = 0;
    while i < daten.len() {
        let n = daten[i] as i8;
        i += 1;
        if n >= 0 {
            let ende = (i + n as usize + 1).min(daten.len());
            aus.extend_from_slice(&daten[i..ende]);
            i = ende;
        } else if n != -128 {
            if let Some(&b) = daten.get(i) {
                aus.extend(std::iter::repeat_n(b, (1 - n as isize) as usize));
            }
            i += 1;
        }
    }
    aus
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

    /// Baut ein TIFF mit Farbpalette, ein Streifen, Little Endian (v0.1.2).
    /// `farben` je Eintrag Rot, Grün, Blau von 0 bis 65535.
    fn palette_tiff(bits: u16, b: u32, h: u32, packung: u16, vorhersage: u16, streifen: &[u8], farben: &[[u16; 3]]) -> Vec<u8> {
        let anzahl = 1usize << bits;
        let mut tabelle = vec![0u16; 3 * anzahl];
        for (i, f) in farben.iter().enumerate() {
            for k in 0..3 {
                tabelle[k * anzahl + i] = f[k];
            }
        }
        const EINTRAEGE: u32 = 11;
        let daten = 8 + 2 + EINTRAEGE * 12 + 4;
        let streifen_ort = daten + tabelle.len() as u32 * 2;
        let mut t = b"II*\0".to_vec();
        t.extend(8u32.to_le_bytes());
        t.extend((EINTRAEGE as u16).to_le_bytes());
        let mut eintrag = |tag: u16, typ: u16, n: u32, wert: u32| {
            t.extend(tag.to_le_bytes());
            t.extend(typ.to_le_bytes());
            t.extend(n.to_le_bytes());
            t.extend(wert.to_le_bytes());
        };
        let (kurz, lang) = (3u16, 4u16);
        eintrag(256, lang, 1, b);
        eintrag(257, lang, 1, h);
        eintrag(258, kurz, 1, bits as u32);
        eintrag(259, kurz, 1, packung as u32);
        eintrag(262, kurz, 1, 3);
        eintrag(273, lang, 1, streifen_ort);
        eintrag(277, kurz, 1, 1);
        eintrag(278, lang, 1, h);
        eintrag(279, lang, 1, streifen.len() as u32);
        eintrag(317, kurz, 1, vorhersage as u32);
        eintrag(320, kurz, tabelle.len() as u32, daten);
        t.extend(0u32.to_le_bytes());
        for w in tabelle {
            t.extend(w.to_le_bytes());
        }
        t.extend_from_slice(streifen);
        t
    }

    const WEISS: [u16; 3] = [65535, 65535, 65535];
    const ROT: [u16; 3] = [65535, 0, 0];
    const BLAU: [u16; 3] = [0, 0, 65535];

    #[test]
    fn liest_palette_mit_4_bit_und_8_bit_mit_vorhersage() {
        let tmp = tempfile::tempdir().unwrap();
        let farbe = |b: &Bild, x: usize, y: usize| b.rgb[(y * b.breite as usize + x) * 3..][..3].to_vec();

        // 4 Bit, 3 × 2 Pixel, ungepackt: Zeile 1 rot, blau, weiß, Zeile 2 weiß, weiß, rot
        let p4 = tmp.path().join("p4.tif");
        std::fs::write(&p4, palette_tiff(4, 3, 2, 1, 1, &[0x12, 0x00, 0x00, 0x10], &[WEISS, ROT, BLAU])).unwrap();
        let b = laden(&p4).unwrap();
        assert_eq!((b.breite, b.hoehe), (3, 2));
        assert_eq!((farbe(&b, 0, 0), farbe(&b, 1, 0), farbe(&b, 2, 0)), (vec![255, 0, 0], vec![0, 0, 255], vec![255, 255, 255]));
        assert_eq!(farbe(&b, 2, 1), vec![255, 0, 0]);

        // 8 Bit, Deflate mit waagerechter Vorhersage: rot, rot, blau, blau als Abstände 1, 0, 1, 0
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, &[1, 0, 1, 0]).unwrap();
        let p8 = tmp.path().join("p8.tif");
        std::fs::write(&p8, palette_tiff(8, 4, 1, 8, 2, &z.finish().unwrap(), &[WEISS, ROT, BLAU])).unwrap();
        let b = laden(&p8).unwrap();
        assert_eq!((farbe(&b, 0, 0), farbe(&b, 1, 0), farbe(&b, 2, 0), farbe(&b, 3, 0)),
            (vec![255, 0, 0], vec![255, 0, 0], vec![0, 0, 255], vec![0, 0, 255]));

        // Nicht unterstützt: JPEG-Packung, keine Zeile Daten, kaputte Datei
        for (name, daten) in [
            ("jpeg.tif", palette_tiff(8, 4, 1, 7, 1, &[0; 4], &[WEISS])),
            ("leer.tif", palette_tiff(8, 4, 1, 1, 1, &[], &[WEISS])),
            ("kaputt.tif", b"II*\0kaputt".to_vec()),
        ] {
            let datei = tmp.path().join(name);
            std::fs::write(&datei, daten).unwrap();
            assert!(laden(&datei).is_err(), "{name}");
        }
    }

    #[test]
    fn entpackt_packbits() {
        // 2 wörtlich, 3-mal 0xAA, Leerbefehl -128, dann 1 wörtlich
        assert_eq!(packbits(&[1, 5, 6, 0xFE, 0xAA, 0x80, 0, 7]), vec![5, 6, 0xAA, 0xAA, 0xAA, 7]);
        // Abgeschnitten: so viel wie da ist
        assert_eq!(packbits(&[3, 1, 2]), vec![1, 2]);
        assert_eq!(packbits(&[0xFF]), Vec::<u8>::new());
    }

    #[test]
    fn inhalt_und_grosse_bilder_fuer_die_ocr() {
        let i = inhalt();
        assert_eq!(i.seiten.len(), 1);
        assert!(i.seiten[0].ist_scan() && i.seiten[0].text.is_empty());
        // Über der Grenze der OCR wird verkleinert
        let tmp = tempfile::tempdir().unwrap();
        let lang = tmp.path().join("lang.png");
        image::GrayImage::from_pixel(crate::ocr::MAX_KANTE + 1000, 20, image::Luma([255])).save(&lang).unwrap();
        let g = fuer_ocr(&lang).unwrap();
        assert_eq!(g.breite, crate::ocr::MAX_KANTE);
        assert!(fuer_ocr(&tmp.path().join("fehlt.png")).is_err());
        // Fehlende HEIC-Datei: Fehler des Lesens
        assert!(laden(&tmp.path().join("fehlt.heic")).is_err());
    }

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

        // TIFF mit Farbpalette (v0.0.8): weißer Grund, rotes Rechteck oben
        // links, blauer Balken unten rechts, 120 x 80 Pixel
        for name in ["palette_ohne.tif", "palette_packbits.tif", "palette_lzw.tif", "palette_deflate.tif"] {
            let datei = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/daten").join(name);
            let b = match laden(&datei) {
                Ok(b) => b,
                Err(e) => panic!("{name}: {e}"),
            };
            assert_eq!((b.breite, b.hoehe), (120, 80), "{name}");
            let bei = |x: usize, y: usize| &b.rgb[(y * 120 + x) * 3..(y * 120 + x) * 3 + 3];
            assert_eq!(bei(2, 2), [255, 255, 255], "{name}");
            assert_eq!(bei(20, 20), [200, 0, 0], "{name}");
            assert_eq!(bei(80, 60), [0, 0, 180], "{name}");
            assert!(fuer_ocr(&datei).is_ok());
        }

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
