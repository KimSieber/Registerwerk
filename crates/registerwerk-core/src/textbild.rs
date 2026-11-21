//! Vorschau von Textdateien (txt, md) als gezeichnete Seite (v0.0.7).
//!
//! Der Text wird mit der eingebetteten Schrift DejaVu Sans Mono auf eine
//! weiße Seite gezeichnet, ohne fremdes Programm und auf jedem System gleich.
//!
//! - Breite 1024 px, wie die kurze Kante jeder Vorschau.
//! - Höhe mindestens wie A4, bei langem Text bis zur größten Kante einer
//!   Vorschau (3072 px). Was darüber hinausgeht, entfällt mit einem Hinweis.
//!   Der ganze Text steht weiter im Register.
//! - Feste Zeichenbreite, damit Spalten aus Leerzeichen erhalten bleiben.
//!   Lange Zeilen brechen am Wort um, Tabulatoren springen auf 4er-Stellen.

use std::collections::HashMap;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

use crate::vorschau::{Bild, KURZE_KANTE, MAX_KANTE};

/// DejaVu Sans Mono, Lizenz in `schrift/LICENSE-DejaVu.txt`.
const SCHRIFT: &[u8] = include_bytes!("../schrift/DejaVuSansMono.ttf");

const BREITE: u32 = KURZE_KANTE as u32;
/// Höhe einer A4-Seite bei dieser Breite.
const HOEHE_A4: u32 = 1448;
const RAND: u32 = 60;
const SCHRIFTGROESSE: f32 = 17.0;
const ZEILENHOEHE: u32 = 25;
const TABULATOR: usize = 4;
/// Helligkeit von Papier und Schrift.
const PAPIER: u8 = 255;
const TINTE: f32 = 24.0;

/// Ein gerastertes Zeichen: Abstand zur Schreibstelle (x, y), Breite, Deckung.
type Zeichen = (i32, i32, usize, Vec<f32>);

/// Zeichnet den Text als Seite.
pub fn zeichnen(text: &str) -> Bild {
    let schrift = FontRef::try_from_slice(SCHRIFT).expect("eingebettete Schrift ist gültig");
    let skaliert = schrift.as_scaled(PxScale::from(SCHRIFTGROESSE));
    let schritt = skaliert.h_advance(schrift.glyph_id('M'));
    let spalten = (((BREITE - 2 * RAND) as f32) / schritt).floor() as usize;
    let platz = ((MAX_KANTE as u32 - 2 * RAND) / ZEILENHOEHE) as usize;

    let mut zeilen = umbrechen(text, spalten, platz + 1);
    if zeilen.len() > platz {
        zeilen.truncate(platz - 1);
        zeilen.push("[…] Die Vorschau endet hier. Der ganze Text steht im Original.".chars().collect());
    }
    let hoehe = (zeilen.len() as u32 * ZEILENHOEHE + 2 * RAND).clamp(HOEHE_A4, MAX_KANTE as u32);
    let mut grau = vec![PAPIER; (BREITE * hoehe) as usize];

    // Jedes Zeichen einmal rastern: Abstand zur Schreibstelle, Breite, Deckung
    let mut raster: HashMap<char, Option<Zeichen>> = HashMap::new();
    let grundlinie = skaliert.ascent();
    for (nr, zeile) in zeilen.iter().enumerate() {
        let oben = RAND as f32 + nr as f32 * ZEILENHOEHE as f32 + grundlinie;
        for (spalte, &z) in zeile.iter().enumerate() {
            if z == ' ' {
                continue;
            }
            let zeichen = raster.entry(z).or_insert_with(|| {
                let umriss = schrift.outline_glyph(schrift.glyph_id(z).with_scale(PxScale::from(SCHRIFTGROESSE)))?;
                let grenzen = umriss.px_bounds();
                let (b, h) = (grenzen.width().ceil() as usize, grenzen.height().ceil() as usize);
                let mut deckung = vec![0.0f32; b * h];
                umriss.draw(|x, y, d| {
                    if let Some(p) = deckung.get_mut(y as usize * b + x as usize) {
                        *p = d;
                    }
                });
                Some((grenzen.min.x.round() as i32, grenzen.min.y.round() as i32, b, deckung))
            });
            let Some((dx, dy, b, deckung)) = zeichen else { continue };
            let x0 = (RAND as f32 + spalte as f32 * schritt).round() as i32 + *dx;
            let y0 = oben.round() as i32 + *dy;
            for (i, d) in deckung.iter().enumerate() {
                let (x, y) = (x0 + (i % *b) as i32, y0 + (i / *b) as i32);
                if *d > 0.0 && x >= 0 && y >= 0 && (x as u32) < BREITE && (y as u32) < hoehe {
                    let p = &mut grau[(y as u32 * BREITE + x as u32) as usize];
                    let neu = PAPIER as f32 + (TINTE - PAPIER as f32) * d.min(1.0);
                    *p = (*p).min(neu.round() as u8);
                }
            }
        }
    }
    Bild { breite: BREITE, hoehe, rgb: grau.iter().flat_map(|&g| [g, g, g]).collect() }
}

/// Zerlegt den Text in Zeilen von höchstens `spalten` Zeichen. Bricht am
/// letzten Leerzeichen um, sonst hart. Hört nach `hoechstens` Zeilen auf.
fn umbrechen(text: &str, spalten: usize, hoechstens: usize) -> Vec<Vec<char>> {
    let mut zeilen: Vec<Vec<char>> = Vec::new();
    for roh in text.lines() {
        // Tabulatoren auf feste Stellen, Steuerzeichen entfallen
        let mut zeile: Vec<char> = Vec::new();
        for z in roh.chars() {
            match z {
                '\t' => {
                    let bis = (zeile.len() / TABULATOR + 1) * TABULATOR;
                    zeile.resize(bis, ' ');
                }
                '\u{A0}' => zeile.push(' '),
                z if z.is_control() => {}
                z => zeile.push(z),
            }
        }
        while zeile.last() == Some(&' ') {
            zeile.pop();
        }
        if zeile.is_empty() {
            zeilen.push(Vec::new());
        }
        while !zeile.is_empty() {
            if zeile.len() <= spalten {
                zeilen.push(std::mem::take(&mut zeile));
                break;
            }
            // am letzten Leerzeichen der Zeile umbrechen, das Leerzeichen entfällt
            // (nicht in der Einrückung)
            let anfang = zeile.iter().position(|z| *z != ' ').unwrap_or(0);
            let schnitt = zeile[..=spalten].iter().rposition(|z| *z == ' ').filter(|i| *i > anfang);
            let (mut kopf, rest) = match schnitt {
                Some(i) => (zeile[..i].to_vec(), zeile[i + 1..].to_vec()),
                None => (zeile[..spalten].to_vec(), zeile[spalten..].to_vec()),
            };
            while kopf.last() == Some(&' ') {
                kopf.pop();
            }
            zeilen.push(kopf);
            zeile = rest;
            if zeilen.len() >= hoechstens {
                return zeilen;
            }
        }
        if zeilen.len() >= hoechstens {
            return zeilen;
        }
    }
    zeilen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn als_text(zeilen: Vec<Vec<char>>) -> Vec<String> {
        zeilen.into_iter().map(|z| z.into_iter().collect()).collect()
    }

    #[test]
    fn bricht_am_wort_um() {
        let z = als_text(umbrechen("eins zwei drei vier\n\nSpalte\tWert\nüberlangeswortohneleer\n  eingerückt  ", 10, 99));
        assert_eq!(
            z,
            ["eins zwei", "drei vier", "", "Spalte", "Wert", "überlanges", "wortohnele", "er", "  eingerüc", "kt"]
        );
        // genau passend bleibt eine Zeile, Wagenrücklauf entfällt
        assert_eq!(als_text(umbrechen("1234567890\r\nab", 10, 99)), ["1234567890", "ab"]);
        assert_eq!(umbrechen("a\nb\nc\nd", 10, 2).len(), 2);
        assert!(umbrechen("", 10, 9).is_empty());
    }

    fn dunkle(b: &Bild) -> usize {
        b.rgb.chunks_exact(3).filter(|p| p[0] < 128).count()
    }

    #[test]
    fn zeichnet_eine_seite() {
        let leer = zeichnen("");
        assert_eq!((leer.breite, leer.hoehe, dunkle(&leer)), (1024, 1448, 0));

        let b = zeichnen("Notiz zum Telefonat\n\nGröße: 12,50 €\tÄnderung „geprüft“");
        assert_eq!((b.breite, b.hoehe), (1024, 1448));
        assert_eq!(b.rgb.len(), 1024 * 1448 * 3);
        // Schrift ist da, grau, und steht innerhalb der Ränder
        let n = dunkle(&b);
        assert!(n > 800 && n < 20_000, "{n} dunkle Pixel");
        assert!(b.rgb.chunks_exact(3).all(|p| p[0] == p[1] && p[1] == p[2]));
        for (i, p) in b.rgb.chunks_exact(3).enumerate() {
            let (x, y) = (i as u32 % 1024, i as u32 / 1024);
            if p[0] < 255 {
                assert!((55..1024 - 55).contains(&x) && (55..200).contains(&y), "Schrift bei {x},{y}");
            }
        }
        // mehr Text, mehr Schrift
        assert!(dunkle(&zeichnen(&"Zeile mit Text\n".repeat(20))) > 5 * dunkle(&zeichnen("Zeile mit Text")));
    }

    #[test]
    fn langer_text_waechst_und_endet_mit_hinweis() {
        let mittel = zeichnen(&"Zeile\n".repeat(80));
        assert_eq!(mittel.hoehe, 80 * 25 + 120);
        let lang = zeichnen(&"Zeile\n".repeat(500));
        assert_eq!((lang.breite, lang.hoehe), (1024, 118 * 25 + 120));
        // die letzte Zeile ist der Hinweis: deutlich breiter als "Zeile"
        let letzte_zeile = 60 + 117 * 25;
        let breit = (0..1024u32)
            .rev()
            .find(|x| (letzte_zeile..letzte_zeile + 25).any(|y| lang.rgb[((y * 1024 + x) * 3) as usize] < 128))
            .unwrap();
        assert!(breit > 500, "{breit}");
    }
}
