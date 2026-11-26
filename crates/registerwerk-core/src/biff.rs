//! Zellentexte alter Tabellenformate ohne `calamine` (v0.0.9).
//!
//! | Format | Woran erkannt |
//! |---|---|
//! | Excel 2 bis 4 (BIFF2–4), ohne Verbunddatei | Kennung des ersten Datensatzes 0x0009, 0x0209, 0x0409 |
//! | Excel 5 und 95 (BIFF5), in der Verbunddatei (Strom `Book`) oder ohne | Kennung 0x0809, Version 0x0500 |
//! | Lotus 1-2-3 (WK1) mit Endung `.xls` | Kennung 0x0000 mit Version 0x0404 bis 0x0406 |
//!
//! Gelesen werden Texte und Zahlen der Zellen, je Tabellenblatt eine Seite.
//! Excel 97 bis 2003 (BIFF8) liest weiter `calamine`. Verschlüsselte Mappen
//! (Datensatz FILEPASS) werden nicht gelesen.

use std::collections::BTreeMap;

use crate::dokumente::MAX_ZEICHEN;
use crate::textformate::cp1252;

fn u16_bei(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(i..i + 2)?.try_into().ok()?))
}

fn u32_bei(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

fn f64_bei(d: &[u8], i: usize) -> Option<f64> {
    Some(f64::from_le_bytes(d.get(i..i + 8)?.try_into().ok()?))
}

/// Zahl wie in der Anzeige: ganze Zahlen ohne Nachkommastellen.
fn zahl(f: f64) -> Option<String> {
    if !f.is_finite() {
        return None;
    }
    Some(if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { format!("{f}") })
}

/// RK-Wert: komprimierte Zahl in 32 Bit.
fn rk(wert: u32) -> f64 {
    let mut f = if wert & 0x02 != 0 {
        ((wert as i32) >> 2) as f64
    } else {
        f64::from_bits(((wert & 0xFFFF_FFFC) as u64) << 32)
    };
    if wert & 0x01 != 0 {
        f /= 100.0;
    }
    f
}

fn text8(b: &[u8]) -> String {
    b.iter().map(|&z| cp1252(z)).collect()
}

/// Ist es ein Datenstrom alter Excel-Formate (BIFF2 bis 5)?
pub fn ist_biff(daten: &[u8]) -> bool {
    matches!(u16_bei(daten, 0), Some(0x0009 | 0x0209 | 0x0409 | 0x0809))
}

/// Ist es eine Lotus-1-2-3-Datei (WK1, WKS)?
pub fn ist_lotus(daten: &[u8]) -> bool {
    u16_bei(daten, 0) == Some(0) && u16_bei(daten, 2) == Some(2) && matches!(u16_bei(daten, 4), Some(0x0404..=0x0406))
}

/// Seiten aus Zellen: Zeilen mit Tabulator zwischen den Zellen.
fn seite(name: &str, zellen: &BTreeMap<(u16, u16), String>) -> String {
    let mut text = name.to_string();
    let mut zeile: Option<u16> = None;
    for ((r, _), wert) in zellen {
        if zeile == Some(*r) {
            text.push('\t');
        } else {
            text.push('\n');
            zeile = Some(*r);
        }
        text.push_str(wert.trim());
        if text.len() > MAX_ZEICHEN {
            break;
        }
    }
    text.trim().to_string()
}

/// Ist an Stelle `i` ein glaubwürdiger Datensatz, gefolgt von zwei weiteren?
fn glaubwuerdig(daten: &[u8], mut i: usize) -> bool {
    for _ in 0..3 {
        let (Some(art), Some(laenge)) = (u16_bei(daten, i), u16_bei(daten, i + 2)) else { return i >= daten.len() };
        if art == 0 || art > 0x0900 || laenge > 8224 || i + 4 + laenge as usize > daten.len() {
            return false;
        }
        i += 4 + laenge as usize;
    }
    true
}

/// Nach einem beschädigten Datensatz die nächste Stelle, an der die Kette
/// wieder stimmt. Excel repariert solche Dateien beim Öffnen still.
fn wiederfinden(daten: &[u8], ab: usize) -> Option<usize> {
    (ab..daten.len().saturating_sub(4)).find(|&i| glaubwuerdig(daten, i))
}

/// Excel 2 bis 5: eine Seite je Tabellenblatt.
pub fn excel_alt(daten: &[u8]) -> Result<Vec<String>, String> {
    let mut namen: Vec<String> = Vec::new();
    // Je Abschnitt (BOF bis EOF) die Zellen. In BIFF5 ist der erste Abschnitt
    // die Arbeitsmappe, die weiteren sind die Blätter in der Folge der Namen.
    let mut abschnitte: Vec<BTreeMap<(u16, u16), String>> = Vec::new();
    let mut zellen: BTreeMap<(u16, u16), String> = BTreeMap::new();
    // Formel mit Text als Ergebnis: Der Text folgt im Datensatz STRING
    let mut formel_text: Option<(u16, u16)> = None;
    let mut biff2 = false;
    let mut i = 0;
    let mut bofs = 0;
    while let (Some(art), Some(laenge)) = (u16_bei(daten, i), u16_bei(daten, i + 2)) {
        // Beschädigte Kette: weiter an der nächsten glaubwürdigen Stelle
        if bofs > 0 && !glaubwuerdig(daten, i) && daten.len() > i + 4 {
            match wiederfinden(daten, i + 1) {
                Some(neu) => {
                    i = neu;
                    continue;
                }
                None => break,
            }
        }
        let Some(d) = daten.get(i + 4..i + 4 + laenge as usize) else { break };
        i += 4 + laenge as usize;
        let rc = || Some((u16_bei(d, 0)?, u16_bei(d, 2)?));
        match art {
            0x0009 | 0x0209 | 0x0409 | 0x0809 => {
                biff2 = art == 0x0009;
                // Neuer Abschnitt: Arbeitsmappe oder Blatt
                if bofs > 0 {
                    abschnitte.push(std::mem::take(&mut zellen));
                }
                bofs += 1;
            }
            0x002F => return Err("Excel-Datei ist verschlüsselt".into()),
            // BOUNDSHEET (BIFF5): Name eines Blatts
            0x0085 => {
                let n = *d.get(6).unwrap_or(&0) as usize;
                if let Some(name) = d.get(7..7 + n) {
                    namen.push(text8(name));
                }
            }
            // LABEL, RSTRING
            0x0004 | 0x0204 | 0x00D6 => {
                let wert = if art == 0x0004 {
                    let n = *d.get(7).unwrap_or(&0) as usize;
                    d.get(8..8 + n).map(text8)
                } else {
                    let n = u16_bei(d, 6).unwrap_or(0) as usize;
                    d.get(8..8 + n).map(text8)
                };
                if let (Some(pos), Some(wert)) = (rc(), wert) {
                    zellen.insert(pos, wert);
                }
            }
            // NUMBER
            0x0003 | 0x0203 => {
                let f = if art == 0x0003 { f64_bei(d, 7) } else { f64_bei(d, 6) };
                if let (Some(pos), Some(z)) = (rc(), f.and_then(zahl)) {
                    zellen.insert(pos, z);
                }
            }
            // INTEGER (BIFF2)
            0x0002 => {
                if let (Some(pos), Some(z)) = (rc(), u16_bei(d, 7)) {
                    zellen.insert(pos, z.to_string());
                }
            }
            // RK
            0x007E | 0x027E => {
                if let (Some(pos), Some(w)) = (rc(), u32_bei(d, 6)) {
                    zellen.insert(pos, zahl(rk(w)).unwrap_or_default());
                }
            }
            // MULRK: mehrere RK in einer Zeile
            0x00BD => {
                if let (Some(r), Some(erste)) = (u16_bei(d, 0), u16_bei(d, 2)) {
                    let mut j = 4;
                    let mut c = erste;
                    while j + 6 <= d.len().saturating_sub(2) {
                        if let Some(w) = u32_bei(d, j + 2) {
                            zellen.insert((r, c), zahl(rk(w)).unwrap_or_default());
                        }
                        j += 6;
                        c = c.saturating_add(1);
                    }
                }
            }
            // FORMULA: Ergebnis als Zahl, oder Text im folgenden STRING
            0x0006 | 0x0206 | 0x0406 => {
                let ergebnis = if biff2 { 7 } else { 6 };
                if let (Some(pos), Some(roh)) = (rc(), d.get(ergebnis..ergebnis + 8)) {
                    if roh[6] == 0xFF && roh[7] == 0xFF {
                        if roh[0] == 0 {
                            formel_text = Some(pos);
                        }
                    } else if let Some(z) = f64_bei(roh, 0).and_then(zahl) {
                        zellen.insert(pos, z);
                    }
                }
            }
            // STRING: Ergebnis einer Formel
            0x0007 | 0x0207 => {
                let wert =
                    if art == 0x0007 { d.get(1..1 + *d.first().unwrap_or(&0) as usize) } else { d.get(2..2 + u16_bei(d, 0).unwrap_or(0) as usize) };
                if let (Some(pos), Some(wert)) = (formel_text.take(), wert) {
                    zellen.insert(pos, text8(wert));
                }
            }
            _ => {}
        }
    }
    if bofs == 0 {
        return Err("keine Excel-Tabelle".into());
    }
    abschnitte.push(zellen);
    if abschnitte.iter().all(|z| z.is_empty()) {
        return Err("Excel-Tabelle ohne lesbare Zellen".into());
    }
    // Mit Blattnamen beginnen die Blätter nach der Arbeitsmappe
    let versatz = usize::from(!namen.is_empty() && abschnitte.len() > namen.len());
    Ok(abschnitte
        .iter()
        .enumerate()
        .filter(|(_, z)| !z.is_empty())
        .map(|(n, z)| seite(n.checked_sub(versatz).and_then(|n| namen.get(n)).map(String::as_str).unwrap_or(""), z))
        .collect())
}

/// Lotus 1-2-3 (WK1): eine Seite.
pub fn lotus(daten: &[u8]) -> Result<Vec<String>, String> {
    let mut zellen: BTreeMap<(u16, u16), String> = BTreeMap::new();
    let mut i = 0;
    while let (Some(art), Some(laenge)) = (u16_bei(daten, i), u16_bei(daten, i + 2)) {
        let Some(d) = daten.get(i + 4..i + 4 + laenge as usize) else { break };
        i += 4 + laenge as usize;
        // Format (1 Byte), Spalte, Zeile
        let pos = || Some((u16_bei(d, 3)?, u16_bei(d, 1)?));
        match art {
            0x0001 => break,
            // LABEL: Text mit Ausrichtungszeichen davor, mit Null beendet
            0x000F => {
                let roh = d.get(5..).unwrap_or(&[]);
                let ende = roh.iter().position(|&b| b == 0).unwrap_or(roh.len());
                let text = text8(&roh[..ende]);
                let text = text.strip_prefix(['\'', '"', '^', '\\', '|']).unwrap_or(&text).to_string();
                if let Some(p) = pos() {
                    zellen.insert(p, text);
                }
            }
            // NUMBER, FORMULA (Ergebnis vorn)
            0x000E | 0x0010 => {
                if let (Some(p), Some(z)) = (pos(), f64_bei(d, 5).and_then(zahl)) {
                    zellen.insert(p, z);
                }
            }
            // INTEGER
            0x000D => {
                if let (Some(p), Some(z)) = (pos(), u16_bei(d, 5)) {
                    zellen.insert(p, (z as i16).to_string());
                }
            }
            _ => {}
        }
    }
    Ok(vec![seite("", &zellen)].into_iter().filter(|s| !s.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn satz(art: u16, inhalt: &[u8]) -> Vec<u8> {
        let mut d = art.to_le_bytes().to_vec();
        d.extend((inhalt.len() as u16).to_le_bytes());
        d.extend(inhalt);
        d
    }

    fn label5(r: u16, c: u16, text: &str) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(r.to_le_bytes());
        d.extend(c.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend((text.len() as u16).to_le_bytes());
        d.extend(text.bytes());
        satz(0x0204, &d)
    }

    #[test]
    fn liest_biff5_mit_zwei_blaettern() {
        let mut d = satz(0x0809, &[0, 5, 5, 0, 0, 0, 0, 0]);
        let blatt = |name: &str| {
            let mut b = vec![0, 0, 0, 0, 0, 0, name.len() as u8];
            b.extend(name.bytes());
            satz(0x0085, &b)
        };
        d.extend(blatt("Kosten"));
        d.extend(blatt("Leer"));
        d.extend(satz(0x000A, &[]));
        d.extend(satz(0x0809, &[0, 5, 0x10, 0, 0, 0, 0, 0]));
        d.extend(label5(0, 0, "Posten"));
        d.extend(label5(0, 1, "Betrag"));
        d.extend(label5(1, 0, "Reise"));
        let mut n = vec![1, 0, 1, 0, 0, 0];
        n.extend(12.5f64.to_le_bytes());
        d.extend(satz(0x0203, &n));
        // RK: 300 als ganze Zahl
        let mut r = vec![2, 0, 1, 0, 0, 0];
        r.extend(((300u32 << 2) | 2).to_le_bytes());
        d.extend(satz(0x027E, &r));
        d.extend(satz(0x000A, &[]));
        d.extend(satz(0x0809, &[0, 5, 0x10, 0, 0, 0, 0, 0]));
        d.extend(satz(0x000A, &[]));
        assert!(ist_biff(&d));
        assert_eq!(excel_alt(&d).unwrap(), vec!["Kosten\nPosten\tBetrag\nReise\t12.5\n300".to_string()]);
    }

    /// Zelle in BIFF2: Zeile, Spalte, 3 Byte Attribute, dann der Wert.
    fn zelle2(art: u16, r: u16, c: u16, wert: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(r.to_le_bytes());
        d.extend(c.to_le_bytes());
        d.extend([0, 0, 0]);
        d.extend(wert);
        satz(art, &d)
    }

    /// Excel 2: Text, Zahl, ganze Zahl, Formel mit Zahl und mit Text, RK
    /// geteilt durch 100 (v0.1.2).
    #[test]
    fn liest_biff2() {
        let mut d = satz(0x0009, &[0, 0, 0x10, 0]);
        let mut label = vec![5u8];
        label.extend(b"Konto");
        d.extend(zelle2(0x0004, 0, 0, &label));
        d.extend(zelle2(0x0003, 0, 1, &1.25f64.to_le_bytes()));
        d.extend(zelle2(0x0002, 1, 0, &7u16.to_le_bytes()));
        d.extend(zelle2(0x0006, 1, 1, &2.0f64.to_le_bytes()));
        // Formel mit Text: Ergebnis 0 … FF FF, der Text folgt in STRING
        d.extend(zelle2(0x0006, 2, 0, &[0, 0, 0, 0, 0, 0, 0xFF, 0xFF]));
        let mut text = vec![5u8];
        text.extend(b"Summe");
        d.extend(satz(0x0007, &text));
        // RK 12345 als ganze Zahl, geteilt durch 100
        let mut rk_satz = vec![2, 0, 1, 0, 0, 0];
        rk_satz.extend(((12345u32 << 2) | 3).to_le_bytes());
        d.extend(satz(0x007E, &rk_satz));
        d.extend(satz(0x000A, &[]));
        assert!(ist_biff(&d));
        assert_eq!(excel_alt(&d).unwrap(), vec!["Konto\t1.25\n7\t2\nSumme\t123.45".to_string()]);
    }

    /// MULRK mit Gleitkommazahl, RSTRING, eine beschädigte Stelle in der Kette
    /// und Formel mit Text in BIFF5 (v0.1.2).
    #[test]
    fn liest_mulrk_und_ueberspringt_beschaedigtes() {
        let mut d = satz(0x0809, &[0, 5, 0x10, 0, 0, 0, 0, 0]);
        // Müll mitten in der Kette. Die Kette gilt erst ab drei stimmigen
        // Datensätzen wieder, deshalb steht er direkt nach dem Anfang.
        d.extend([0xFF; 5]);
        // MULRK: Zeile 0, ab Spalte 0, 1,5 als Gleitkomma und 2 als ganze Zahl
        let mut m = vec![0, 0, 0, 0];
        m.extend([0, 0]);
        m.extend(0x3FF8_0000u32.to_le_bytes());
        m.extend([0, 0]);
        m.extend(((2u32 << 2) | 2).to_le_bytes());
        m.extend(1u16.to_le_bytes());
        d.extend(satz(0x00BD, &m));
        d.extend(label5(1, 0, "danach"));
        let mut rs = vec![1, 0, 1, 0, 0, 0, 3, 0];
        rs.extend(b"RTF");
        d.extend(satz(0x00D6, &rs));
        let mut f = vec![2, 0, 0, 0, 0, 0];
        f.extend([0, 0, 0, 0, 0, 0, 0xFF, 0xFF]);
        d.extend(satz(0x0206, &f));
        let mut st = 4u16.to_le_bytes().to_vec();
        st.extend(b"Text");
        d.extend(satz(0x0207, &st));
        d.extend(satz(0x000A, &[]));
        assert_eq!(excel_alt(&d).unwrap(), vec!["1.5\t2\ndanach\tRTF\nText".to_string()]);
    }

    #[test]
    fn leere_und_fremde_daten() {
        assert_eq!(excel_alt(&satz(0x0204, &[0; 8])).unwrap_err(), "keine Excel-Tabelle");
        let mut leer = satz(0x0809, &[0, 5, 0x10, 0, 0, 0, 0, 0]);
        leer.extend(satz(0x000A, &[]));
        assert_eq!(excel_alt(&leer).unwrap_err(), "Excel-Tabelle ohne lesbare Zellen");
        assert!(!ist_biff(b"x") && !ist_lotus(b"\0\0\x02\0\x00\x01"));
        assert_eq!(zahl(f64::NAN), None);
        assert_eq!(zahl(0.5).as_deref(), Some("0.5"));
        assert!((rk(0x3FF8_0000 | 1) - 0.015).abs() < 1e-12);
    }

    #[test]
    fn meldet_verschluesselung() {
        let mut d = satz(0x0809, &[0, 5, 5, 0, 0, 0, 0, 0]);
        d.extend(satz(0x002F, &[0, 0, 0, 0]));
        assert_eq!(excel_alt(&d).unwrap_err(), "Excel-Datei ist verschlüsselt");
    }

    #[test]
    fn liest_lotus() {
        let mut d = satz(0x0000, &[0x04, 0x04]);
        let mut l = vec![0xFF, 0, 0, 0, 0];
        l.extend(b"'Abrechnung Urlaub\0");
        d.extend(satz(0x000F, &l));
        let mut n = vec![0xFF, 1, 0, 0, 0];
        n.extend(42.0f64.to_le_bytes());
        d.extend(satz(0x000E, &n));
        // INTEGER negativ, FORMULA mit Ergebnis vorn
        d.extend(satz(0x000D, &[0xFF, 0, 0, 1, 0, 0xFE, 0xFF]));
        let mut f = vec![0xFF, 1, 0, 1, 0];
        f.extend(3.5f64.to_le_bytes());
        d.extend(satz(0x0010, &f));
        d.extend(satz(0x0001, &[]));
        d.extend(satz(0x000F, &[0xFF, 0, 0, 5, 0, b'x', 0]));
        assert!(ist_lotus(&d));
        assert_eq!(lotus(&d).unwrap(), vec!["Abrechnung Urlaub\t42\n-2\t3.5".to_string()]);
        // Ohne Zellen keine Seite
        assert!(lotus(&satz(0x0000, &[0x04, 0x04])).unwrap().is_empty());
    }
}
