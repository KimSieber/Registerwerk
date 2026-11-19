//! Lage der erkannten Wörter auf einer Seite (v0.0.5).
//!
//! Die OCR liefert zu jedem Wort sein Rechteck und seine Grundlinie in Pixeln
//! des gerenderten Bildes. Gespeichert werden die Werte bezogen auf die
//! Bildgröße (0 bis 1, Ursprung links oben). Sie hängen damit nicht von der
//! Auflösung ab, mit der gerendert wurde, und lassen sich beim Schreiben der
//! Textebene auf die Seite der PDF umrechnen.
//!
//! Im Register liegen die Wörter einer Seite als kompakter Block in
//! `ocr.woerter`. Sie werden nur gebraucht, bis die Textebene in die PDF
//! geschrieben ist, und danach entfernt.
//!
//! Aufbau des Blocks: Kennung `W1`, dann je Wort sechs Werte als u16
//! (links, oben, rechts, unten, Grundlinie links, Grundlinie rechts, je
//! Bruchteil × 65535), ein Byte Merkmale (Bit 0: Zeilenende), die Länge des
//! Textes in Byte als u16 und der Text in UTF-8. Alle Zahlen little endian.

/// Ein Wort mit seiner Lage, bezogen auf die Bildgröße (0 bis 1).
#[derive(Debug, Clone, PartialEq)]
pub struct LageWort {
    pub text: String,
    pub links: f32,
    pub oben: f32,
    pub rechts: f32,
    pub unten: f32,
    /// Höhe der Grundlinie am linken und am rechten Rand.
    pub grund_links: f32,
    pub grund_rechts: f32,
    pub zeilenende: bool,
}

const KENNUNG: &[u8; 2] = b"W1";

/// Rechnet die Wörter aus Tesseract auf die Bildgröße um.
pub fn aus_tesseract(woerter: &[registerwerk_ocr::Wort], breite: u32, hoehe: u32) -> Vec<LageWort> {
    let (b, h) = (breite.max(1) as f32, hoehe.max(1) as f32);
    woerter
        .iter()
        .map(|w| LageWort {
            text: w.text.clone(),
            links: w.links as f32 / b,
            oben: w.oben as f32 / h,
            rechts: w.rechts as f32 / b,
            unten: w.unten as f32 / h,
            grund_links: w.grund_links / h,
            grund_rechts: w.grund_rechts / h,
            zeilenende: w.zeilenende,
        })
        .collect()
}

fn wert(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

pub fn packen(woerter: &[LageWort]) -> Vec<u8> {
    let mut b = Vec::with_capacity(2 + woerter.len() * 24);
    b.extend_from_slice(KENNUNG);
    for w in woerter {
        for v in [w.links, w.oben, w.rechts, w.unten, w.grund_links, w.grund_rechts] {
            b.extend_from_slice(&wert(v).to_le_bytes());
        }
        b.push(w.zeilenende as u8);
        let text = w.text.as_bytes();
        let laenge = text.len().min(u16::MAX as usize);
        // Nicht mitten in einem Zeichen abschneiden
        let laenge = (0..=laenge).rev().find(|&i| w.text.is_char_boundary(i)).unwrap_or(0);
        b.extend_from_slice(&(laenge as u16).to_le_bytes());
        b.extend_from_slice(&text[..laenge]);
    }
    b
}

pub fn entpacken(b: &[u8]) -> Result<Vec<LageWort>, String> {
    if b.len() < 2 || &b[..2] != KENNUNG {
        return Err("Wortlagen: unbekanntes Format".into());
    }
    let mut woerter = Vec::new();
    let mut i = 2;
    let u16_bei = |i: usize| -> Result<u16, String> {
        b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| "Wortlagen: unvollständig".to_string())
    };
    while i < b.len() {
        let mut v = [0f32; 6];
        for (k, w) in v.iter_mut().enumerate() {
            *w = u16_bei(i + 2 * k)? as f32 / 65535.0;
        }
        i += 12;
        let merkmale = *b.get(i).ok_or("Wortlagen: unvollständig")?;
        let laenge = u16_bei(i + 1)? as usize;
        i += 3;
        let text = b.get(i..i + laenge).ok_or("Wortlagen: unvollständig")?;
        let text = std::str::from_utf8(text).map_err(|_| "Wortlagen: Text kein UTF-8")?.to_string();
        i += laenge;
        woerter.push(LageWort {
            text,
            links: v[0],
            oben: v[1],
            rechts: v[2],
            unten: v[3],
            grund_links: v[4],
            grund_rechts: v[5],
            zeilenende: merkmale & 1 != 0,
        });
    }
    Ok(woerter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packen_und_entpacken() {
        let woerter = vec![
            LageWort {
                text: "Grüße".into(),
                links: 0.1,
                oben: 0.2,
                rechts: 0.3,
                unten: 0.25,
                grund_links: 0.24,
                grund_rechts: 0.245,
                zeilenende: false,
            },
            LageWort {
                text: "123,45€".into(),
                links: 0.0,
                oben: 0.9,
                rechts: 1.0,
                unten: 1.0,
                grund_links: 0.99,
                grund_rechts: 0.99,
                zeilenende: true,
            },
        ];
        let b = packen(&woerter);
        let zurueck = entpacken(&b).unwrap();
        assert_eq!(zurueck.len(), 2);
        for (a, z) in woerter.iter().zip(&zurueck) {
            assert_eq!((&a.text, a.zeilenende), (&z.text, z.zeilenende));
            for (x, y) in [(a.links, z.links), (a.oben, z.oben), (a.rechts, z.rechts), (a.grund_rechts, z.grund_rechts)] {
                assert!((x - y).abs() < 1e-4, "{x} {y}");
            }
        }
        assert!(entpacken(b"XX").is_err());
        assert!(entpacken(&b[..b.len() - 1]).is_err());
        assert!(entpacken(KENNUNG).unwrap().is_empty());
    }

    #[test]
    fn umrechnen_auf_bildgroesse() {
        let w = registerwerk_ocr::Wort {
            text: "Rechnung".into(),
            links: 100,
            oben: 200,
            rechts: 300,
            unten: 250,
            grund_links: 240.0,
            grund_rechts: 242.0,
            zeilenende: true,
        };
        let l = aus_tesseract(&[w], 1000, 2000);
        assert_eq!((l[0].links, l[0].oben, l[0].rechts, l[0].unten), (0.1, 0.1, 0.3, 0.125));
        assert_eq!((l[0].grund_links, l[0].grund_rechts), (0.12, 0.121));
    }
}
