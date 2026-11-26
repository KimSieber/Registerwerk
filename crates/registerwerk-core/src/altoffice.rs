//! Text aus den alten Binärformaten von Office (v0.0.7): doc, xls, ppt.
//!
//! Die Dateien sind OLE-Verbunddateien mit je einem Datenstrom für den Inhalt.
//! Seit v0.0.9 liest sie ein eigener, toleranter Leser (`ole.rs`), weil die
//! Bibliothek `cfb` Dateien mit kleinen Fehlern ablehnt. Gelesen wird nur der
//! Text, ohne fremdes Programm:
//!
//! | Format | Strom | Seiten im Register |
//! |---|---|---|
//! | Word (doc, dot) | `WordDocument`, Stücktabelle in `0Table` oder `1Table` | eine Seite mit dem ganzen Text |
//! | Excel (xls, xlt) | über `calamine` | eine Seite je Tabellenblatt, wie bei xlsx |
//! | PowerPoint (ppt, pps) | `PowerPoint Document`, Textatome | eine Seite mit dem ganzen Text |
//!
//! Dazu seit v0.0.9: Word für Windows 2 (ohne Verbunddatei) und Word-6-Dateien,
//! deren Kopf um zwei Byte versetzt ist. Excel 5 und 95 liest `biff.rs`, wenn
//! `calamine` scheitert. Verschlüsselte Dateien werden nicht gelesen.

use std::path::Path;

use crate::dokumente::MAX_ZEICHEN;

/// Mehr wird aus einem Strom nicht gelesen. Text ist weit kleiner, die
/// Grenze schützt nur vor beschädigten Dateien.
const MAX_STROM: u64 = 256 * 1024 * 1024;

use crate::ole::Verbund;

fn oeffnen(pfad: &Path) -> Result<Verbund, String> {
    let daten = std::fs::read(pfad).map_err(|e| e.to_string())?;
    if daten.starts_with(b"{\\rtf") {
        return Err("RTF mit der Endung eines Office-Dokuments wird nicht gelesen".into());
    }
    // Word, Excel und PowerPoint 97 bis 2003 sind OLE-Verbunddateien. Ohne
    // deren Kennung ist es ein älteres Format (vor 1997) oder ein fremdes.
    if !daten.starts_with(&crate::ole::KENNUNG) {
        return Err(VOR_97.into());
    }
    Verbund::oeffnen(daten).map_err(|e| format!("Office-Datei beschädigt oder nicht lesbar ({e})"))
}

/// Meldung für Office-Dateien ohne OLE-Kennung (v0.0.9).
pub const VOR_97: &str = "älteres Office-Format (vor 97) oder fremdes Format, wird nicht gelesen";

fn strom(ole: &Verbund, name: &str) -> Result<Vec<u8>, String> {
    let mut daten = ole.strom(name).ok_or_else(|| format!("{name} fehlt im Dokument"))?;
    daten.truncate(MAX_STROM as usize);
    Ok(daten)
}

fn u16_bei(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(i..i + 2)?.try_into().ok()?))
}

fn u32_bei(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

/// Windows-1252: wie Latin-1, nur der Bereich 0x80 bis 0x9F ist belegt.
fn cp1252(b: u8) -> char {
    crate::textformate::cp1252(b)
}

fn utf16(daten: &[u8]) -> impl Iterator<Item = char> + '_ {
    char::decode_utf16(daten.chunks_exact(2).map(|z| u16::from_le_bytes([z[0], z[1]])))
        .map(|z| z.unwrap_or(char::REPLACEMENT_CHARACTER))
}

// ---------- Word ----------

/// Text eines Word-Dokuments (Word 6 bis 2003): Haupttext, Fußnoten, Kopf-
/// und Fußzeilen, in der Reihenfolge der Datei.
pub fn word(pfad: &Path) -> Result<String, String> {
    let ole = oeffnen(pfad)?;
    let ergebnis = strom(&ole, "WordDocument").and_then(|dok| word_text(&dok, |name| strom(&ole, name)));
    match ergebnis {
        Ok(text) => Ok(text),
        Err(meldung) if !meldung.contains("verschlüsselt") && !meldung.contains("vor Word 6") => {
            // Beschädigtes Verzeichnis der Verbunddatei: Den Kopf des Dokuments
            // (FIB) direkt an den Sektorgrenzen suchen. Kleine Dokumente stehen
            // meist zusammenhängend in der Datei (v0.0.9).
            let roh = std::fs::read(pfad).map_err(|e| e.to_string())?;
            word_suchen(&roh).ok_or(meldung)
        }
        Err(meldung) => Err(meldung),
    }
}

/// Sucht den Kopf eines Word-6/95-Dokuments an den Sektorgrenzen und liest
/// den Text ab dort. Word 97 braucht einen zweiten Strom und bleibt außen vor.
fn word_suchen(roh: &[u8]) -> Option<String> {
    let mut stelle = 512;
    while stelle + 0x20 < roh.len() {
        for versatz in [0usize, 2] {
            let k = stelle + versatz;
            let kennung = u16_bei(roh, k);
            let n_fib = u16_bei(roh, k + 2).unwrap_or(0);
            if matches!(kennung, Some(0xA5DC | 0xA5EC)) && (0x0065..0x00C1).contains(&n_fib) {
                // Die Stellen im Kopf zählen ab dem Anfang des Stroms
                let dok = &roh[stelle..];
                let kein_strom = |_: &str| Err::<Vec<u8>, String>("kein Tabellenstrom".into());
                if let Ok(text) = word_text(dok, kein_strom) {
                    if text.chars().filter(|z| z.is_alphabetic()).count() >= 10 {
                        return Some(text);
                    }
                }
            }
        }
        stelle += 512;
    }
    None
}

/// Text aus dem Strom `WordDocument`. `tabelle` liefert für Word 97 den
/// Tabellenstrom.
fn word_text(dok: &[u8], tabelle: impl Fn(&str) -> Result<Vec<u8>, String>) -> Result<String, String> {
    let dok = dok.to_vec();
    let kaputt = || "Word-Dokument ist beschädigt".to_string();
    // Manche Word-6-Dateien tragen zwei Nullbytes vor dem Kopf. Die Stellen
    // des Textes zählen trotzdem ab dem Anfang des Stroms (v0.0.9).
    let kopf = if u16_bei(&dok, 0) == Some(0) && matches!(u16_bei(&dok, 2), Some(0xA5DC | 0xA5EC)) { 2 } else { 0 };
    let n_fib = u16_bei(&dok, kopf + 2).ok_or_else(kaputt)?;
    let merkmale = u16_bei(&dok, kopf + 0x0A).ok_or_else(kaputt)?;
    if merkmale & 0x0100 != 0 {
        return Err("Word-Dokument ist verschlüsselt".into());
    }
    let roh: Vec<char> = if n_fib >= 0x00C1 {
        // Word 97 bis 2003: Stücktabelle im Tabellenstrom
        let tabelle = tabelle(if merkmale & 0x0200 != 0 { "1Table" } else { "0Table" })?;
        let fc = u32_bei(&dok, 0x01A2).ok_or_else(kaputt)? as usize;
        let lcb = u32_bei(&dok, 0x01A6).ok_or_else(kaputt)? as usize;
        let clx = tabelle.get(fc..fc.saturating_add(lcb)).ok_or_else(kaputt)?;
        stuecke(clx, &dok, true).ok_or_else(kaputt)?
    } else if n_fib >= 0x0065 {
        // Word 6 und 95: 8-Bit-Text, zusammenhängend oder über eine
        // Stücktabelle im selben Strom
        if merkmale & 0x0004 != 0 {
            let fc = u32_bei(&dok, kopf + 0x0160).ok_or_else(kaputt)? as usize;
            let lcb = u32_bei(&dok, kopf + 0x0164).ok_or_else(kaputt)? as usize;
            let clx = dok.get(fc..fc.saturating_add(lcb)).ok_or_else(kaputt)?;
            stuecke(clx, &dok, false).ok_or_else(kaputt)?
        } else {
            let von = u32_bei(&dok, kopf + 0x18).ok_or_else(kaputt)? as usize;
            let bis = u32_bei(&dok, kopf + 0x1C).ok_or_else(kaputt)? as usize;
            dok.get(von..bis.min(dok.len())).ok_or_else(kaputt)?.iter().map(|&b| cp1252(b)).collect()
        }
    } else {
        return Err("Word-Dokumente vor Word 6 werden nicht gelesen".into());
    };
    Ok(word_bereinigen(roh))
}

/// Ist es Word für Windows 2 (ohne Verbunddatei, Kennung 0xA5DB)?
pub fn ist_word2(daten: &[u8]) -> bool {
    u16_bei(daten, 0) == Some(0xA5DB)
}

/// Text eines Dokuments aus Word für Windows 2 (v0.0.9): 8-Bit-Text,
/// zusammenhängend zwischen `fcMin` und `fcMac`. Schnell gespeicherte
/// Dokumente (mit Stücktabelle) werden nicht gelesen.
pub fn word2(daten: &[u8]) -> Result<String, String> {
    let kaputt = || "Word-2-Dokument ist beschädigt".to_string();
    let merkmale = u16_bei(daten, 0x0A).ok_or_else(kaputt)?;
    if merkmale & 0x0100 != 0 {
        return Err("Word-Dokument ist verschlüsselt".into());
    }
    if merkmale & 0x0004 != 0 {
        return Err("schnell gespeichertes Word-2-Dokument wird nicht gelesen".into());
    }
    let von = u32_bei(daten, 0x18).ok_or_else(kaputt)? as usize;
    let bis = u32_bei(daten, 0x1C).ok_or_else(kaputt)? as usize;
    let roh = daten.get(von..bis.min(daten.len())).filter(|_| von < bis).ok_or_else(kaputt)?;
    Ok(word_bereinigen(roh.iter().map(|&b| cp1252(b)).collect()))
}

/// Liest den Text über die Stücktabelle (Clx): Jedes Stück nennt eine Stelle
/// im Strom `WordDocument`, an der ein Abschnitt des Textes steht, als 8-Bit-
/// Text oder als UTF-16. Mit `kennung` (Word 97) sagt ein Bit der Stelle,
/// welches von beiden gilt. `None` bei beschädigter Tabelle.
fn stuecke(clx: &[u8], dok: &[u8], kennung: bool) -> Option<Vec<char>> {
    let mut i = 0;
    // Vor der Tabelle können Formatangaben stehen (Kennzeichen 1)
    while *clx.get(i)? == 1 {
        i += 3 + u16_bei(clx, i + 1)? as usize;
    }
    if *clx.get(i)? != 2 {
        return None;
    }
    let laenge = u32_bei(clx, i + 1)? as usize;
    let plc = clx.get(i + 5..(i + 5).checked_add(laenge)?)?;
    // n + 1 Zeichenpositionen zu je 4 Byte, dann n Einträge zu je 8 Byte
    let n = plc.len().checked_sub(4)? / 12;
    let mut text = Vec::new();
    for s in 0..n {
        let von = u32_bei(plc, s * 4)?;
        let bis = u32_bei(plc, s * 4 + 4)?;
        let zeichen = bis.checked_sub(von)? as usize;
        let fc = u32_bei(plc, (n + 1) * 4 + s * 8 + 2)?;
        if kennung && fc & 0x4000_0000 != 0 {
            let ort = ((fc & 0x3FFF_FFFF) / 2) as usize;
            text.extend(dok.get(ort..ort.checked_add(zeichen)?)?.iter().map(|&b| cp1252(b)));
        } else if kennung {
            let ort = fc as usize;
            text.extend(utf16(dok.get(ort..ort.checked_add(zeichen.checked_mul(2)?)?)?));
        } else {
            let ort = fc as usize;
            text.extend(dok.get(ort..ort.checked_add(zeichen)?)?.iter().map(|&b| cp1252(b)));
        }
        if text.len() > MAX_ZEICHEN {
            break;
        }
    }
    Some(text)
}

/// Ersetzt die Steuerzeichen von Word: Absatz- und Zellenmarken, Umbrüche.
/// Von Feldern (Seitenzahl, Verweis, Serienbrief) bleibt das Ergebnis, die
/// Anweisung entfällt.
fn word_bereinigen(roh: Vec<char>) -> String {
    let mut text = String::with_capacity(roh.len());
    // Je offenem Feld: Sind wir noch in der Anweisung?
    let mut felder: Vec<bool> = Vec::new();
    // Das letzte Zeichen war eine Zellenmarke
    let mut zelle = false;
    for z in roh {
        match z {
            '\u{13}' => felder.push(true),
            '\u{14}' => {
                if let Some(anweisung) = felder.last_mut() {
                    *anweisung = false;
                }
            }
            '\u{15}' => {
                felder.pop();
            }
            _ if felder.contains(&true) => {}
            '\r' | '\u{0B}' | '\u{0C}' | '\n' => {
                // höchstens eine Leerzeile
                if !text.ends_with("\n\n") {
                    text.push('\n');
                }
                zelle = false;
            }
            // Zellenmarke. Die zweite in Folge beendet die Tabellenzeile.
            '\u{07}' if zelle => {
                text.pop();
                text.push('\n');
                zelle = false;
            }
            '\u{07}' => {
                text.push('\t');
                zelle = true;
            }
            '\t' => text.push('\t'),
            '\u{1E}' => text.push('-'),
            '\u{A0}' => text.push(' '),
            z if z < ' ' || z == '\u{AD}' => {}
            z => {
                text.push(z);
                zelle = false;
            }
        }
    }
    text
}

// ---------- PowerPoint ----------

/// Text einer PowerPoint-Datei (97 bis 2003): alle Textatome der Folien und
/// Notizen, ohne die Folienmaster. Gleicher Text steht nur einmal.
pub fn powerpoint(pfad: &Path) -> Result<String, String> {
    let ole = oeffnen(pfad)?;
    if ole.hat("EncryptedSummary") {
        return Err("PowerPoint-Datei ist verschlüsselt".into());
    }
    if ole.hat("PP40") && !ole.hat("PowerPoint Document") {
        return Err("PowerPoint 4 wird nicht gelesen".into());
    }
    let dok = strom(&ole, "PowerPoint Document")?;
    let mut texte: Vec<String> = Vec::new();
    ppt_lesen(&dok, 0, &mut texte);
    Ok(texte.join("\n"))
}

/// Läuft durch die Datensätze: 8 Byte Kopf (Art des Satzes, Typ, Länge),
/// dann der Inhalt. Behälter enthalten wieder Datensätze.
fn ppt_lesen(d: &[u8], tiefe: usize, texte: &mut Vec<String>) {
    const TEXT_UTF16: u16 = 0x0FA0;
    const TEXT_8BIT: u16 = 0x0FA8;
    const MASTER: u16 = 0x03F8;
    let mut i = 0;
    let mut zeichen: usize = texte.iter().map(String::len).sum();
    while let (Some(art), Some(typ), Some(laenge)) = (u16_bei(d, i), u16_bei(d, i + 2), u32_bei(d, i + 4)) {
        let Some(inhalt) = (i + 8).checked_add(laenge as usize).and_then(|ende| d.get(i + 8..ende)) else { break };
        i += 8 + inhalt.len();
        let roh: Option<String> = match typ {
            TEXT_UTF16 => Some(utf16(inhalt).collect()),
            TEXT_8BIT => Some(inhalt.iter().map(|&b| b as char).collect()),
            MASTER => None,
            _ => {
                if art & 0x000F == 0x000F && tiefe < 40 {
                    ppt_lesen(inhalt, tiefe + 1, texte);
                    zeichen = texte.iter().map(String::len).sum();
                }
                None
            }
        };
        if let Some(roh) = roh {
            let text: String = roh
                .chars()
                .map(|z| if z == '\r' || z == '\u{0B}' { '\n' } else { z })
                .filter(|z| *z >= ' ' || *z == '\n' || *z == '\t')
                .collect();
            let text = text.trim();
            // "*" steht für die Foliennummer
            if !text.is_empty() && text != "*" && !texte.iter().any(|t| t == text) {
                zeichen += text.len();
                texte.push(text.to_string());
            }
        }
        if zeichen > MAX_ZEICHEN {
            break;
        }
    }
}

// ---------- Excel ----------

/// Excel (97 bis 2003): eine Seite je Tabellenblatt, beginnend mit dem Namen
/// des Blatts. Zellen mit Tab, Zeilen mit Zeilenumbruch, leere entfallen.
pub fn excel(pfad: &Path) -> Result<Vec<String>, String> {
    use calamine::{Data, Reader};
    // Gleiche Prüfung und Meldung wie bei Word und PowerPoint
    let ole = oeffnen(pfad)?;
    // Excel 5 und 95 (Strom "Book" oder "Workbook", BIFF5) liest der eigene
    // Leser (v0.0.9). Seit v0.1.2 vor calamine: calamine öffnet manche solche
    // Mappe ohne Fehler, liefert aber keine Blätter. Der eigene Leser kam
    // dann nie zum Zug.
    let biff5 = ole.strom("Workbook").or_else(|| ole.strom("Book")).filter(|d| {
        crate::biff::ist_biff(d) && u16_bei(d, 4).is_some_and(|version| version < 0x0600)
    });
    if let Some(d) = biff5 {
        return crate::biff::excel_alt(&d);
    }
    let mut mappe: calamine::Xls<_> = calamine::open_workbook(pfad).map_err(|e: calamine::XlsError| {
        if e.to_string().contains("password") {
            "Excel-Datei ist verschlüsselt (Kennwortschutz)".to_string()
        } else {
            format!("Excel-Datei beschädigt oder nicht lesbar ({e})")
        }
    })?;
    let mut seiten = Vec::new();
    for name in mappe.sheet_names() {
        let Ok(bereich) = mappe.worksheet_range(&name) else { continue };
        let mut text = name.clone();
        for zeile in bereich.rows() {
            let zellen: Vec<String> = zeile
                .iter()
                .filter_map(|z| match z {
                    Data::Empty | Data::Bool(_) | Data::Error(_) => None,
                    Data::String(s) => Some(s.trim().to_string()),
                    // ganze Zahlen ohne Nachkommastelle, wie in der Anzeige
                    Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => Some(format!("{}", *f as i64)),
                    andere => Some(andere.to_string()),
                })
                .filter(|s| !s.is_empty())
                .collect();
            if !zellen.is_empty() {
                text.push('\n');
                text.push_str(&zellen.join("\t"));
            }
            if text.len() > MAX_ZEICHEN {
                break;
            }
        }
        seiten.push(text);
    }
    Ok(seiten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ole_schreiben(pfad: &Path, stroeme: &[(&str, &[u8])]) {
        let mut ole = cfb::create(pfad).unwrap();
        for (name, daten) in stroeme {
            ole.create_stream(name).unwrap().write_all(daten).unwrap();
        }
        ole.flush().unwrap();
    }

    fn utf16_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// Kopf eines Word-Dokuments (FIB) mit Version und Merkmalen.
    fn fib(n_fib: u16, merkmale: u16) -> Vec<u8> {
        let mut d = vec![0u8; 0x200];
        d[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
        d[2..4].copy_from_slice(&n_fib.to_le_bytes());
        d[0x0A..0x0C].copy_from_slice(&merkmale.to_le_bytes());
        d
    }

    /// Stücktabelle für Stücke aus (Zeichenzahl, Stelle im Strom).
    fn clx(stuecke: &[(u32, u32)]) -> Vec<u8> {
        let mut plc = Vec::new();
        let mut cp = 0u32;
        plc.extend(cp.to_le_bytes());
        for (zeichen, _) in stuecke {
            cp += zeichen;
            plc.extend(cp.to_le_bytes());
        }
        for (_, fc) in stuecke {
            plc.extend([0u8, 0]);
            plc.extend(fc.to_le_bytes());
            plc.extend([0u8, 0]);
        }
        // davor eine Formatangabe, die übersprungen wird
        let mut c = vec![1u8, 2, 0, 0xAA, 0xBB, 2];
        c.extend((plc.len() as u32).to_le_bytes());
        c.extend(plc);
        c
    }

    #[test]
    fn word_97_mit_stuecken_feldern_und_tabelle() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("brief.doc");
        let mut dok = fib(0x00C1, 0x0200);
        // Stück 1: 8-Bit, mit Euro-Zeichen in Windows-1252
        let eins = b"Miete 1.250 \x80 \x84kalt\x93\r";
        let ort1 = dok.len() as u32;
        dok.extend(eins);
        dok.push(0);
        // Stück 2: UTF-16 mit Feld (Seitenzahl) und Tabellenzeile
        let zwei = utf16_bytes("Größe\u{13} PAGE \u{14}7\u{15}\rPosten\u{07}12,00\u{07}\u{07}Ende");
        let ort2 = dok.len() as u32;
        dok.extend(&zwei);
        let tabelle_vorn = [0u8; 4];
        let c = clx(&[(eins.len() as u32, (ort1 * 2) | 0x4000_0000), ((zwei.len() / 2) as u32, ort2)]);
        dok[0x01A2..0x01A6].copy_from_slice(&4u32.to_le_bytes());
        dok[0x01A6..0x01AA].copy_from_slice(&(c.len() as u32).to_le_bytes());
        let tabelle = [&tabelle_vorn[..], &c].concat();
        ole_schreiben(&p, &[("WordDocument", &dok), ("1Table", &tabelle)]);
        assert_eq!(word(&p).unwrap(), "Miete 1.250 € „kalt“\nGröße7\nPosten\t12,00\nEnde");

        // über dokumente::lesen als eine Seite
        let i = crate::dokumente::lesen(&p).unwrap();
        assert_eq!((i.seiten.len(), i.seiten[0].text.starts_with("Miete")), (1, true));
    }

    #[test]
    fn word_6_und_fehler() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("alt.doc");
        let mut dok = fib(0x0065, 0);
        let text = b"Alter Brief \xFCber Gr\xF6\xDFe\rZeile zwei";
        dok[0x18..0x1C].copy_from_slice(&0x200u32.to_le_bytes());
        dok[0x1C..0x20].copy_from_slice(&(0x200 + text.len() as u32).to_le_bytes());
        dok.extend(text);
        ole_schreiben(&p, &[("WordDocument", &dok)]);
        assert_eq!(word(&p).unwrap(), "Alter Brief über Größe\nZeile zwei");

        ole_schreiben(&p, &[("WordDocument", &fib(0x00C1, 0x0100))]);
        assert!(word(&p).unwrap_err().contains("verschlüsselt"));
        ole_schreiben(&p, &[("WordDocument", &fib(0x0021, 0))]);
        assert!(word(&p).unwrap_err().contains("vor Word 6"));
        // Stücktabelle zeigt ins Leere
        let mut dok = fib(0x00C1, 0);
        dok[0x01A6..0x01AA].copy_from_slice(&500u32.to_le_bytes());
        ole_schreiben(&p, &[("WordDocument", &dok), ("0Table", &[0u8; 10])]);
        assert!(word(&p).unwrap_err().contains("beschädigt"));
        ole_schreiben(&p, &[("Anderes", b"x")]);
        assert!(word(&p).unwrap_err().contains("WordDocument fehlt"));
        std::fs::write(&p, b"{\\rtf1 Hallo}").unwrap();
        assert!(word(&p).unwrap_err().contains("RTF"));
        std::fs::write(&p, b"kein Office").unwrap();
        assert!(word(&p).is_err());
    }

    /// Word 6 mit Stücktabelle im selben Strom (schnell gespeichert) und mit
    /// zwei Nullbytes vor dem Kopf (v0.1.2).
    #[test]
    fn word_6_mit_stuecken_und_versatz() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("schnell.doc");
        let mut dok = fib(0x0065, 0x0004);
        let text = b"Schnell gespeichert\r";
        let ort = dok.len() as u32;
        dok.extend(text);
        let c = clx(&[(text.len() as u32, ort)]);
        let fc = dok.len() as u32;
        dok.extend(&c);
        dok[0x160..0x164].copy_from_slice(&fc.to_le_bytes());
        dok[0x164..0x168].copy_from_slice(&(c.len() as u32).to_le_bytes());
        ole_schreiben(&p, &[("WordDocument", &dok)]);
        assert_eq!(word(&p).unwrap(), "Schnell gespeichert\n");

        let mut dok = vec![0u8, 0];
        dok.extend(fib(0x0065, 0));
        let text = b"Mit Versatz";
        let von = dok.len() as u32;
        dok[2 + 0x18..2 + 0x1C].copy_from_slice(&von.to_le_bytes());
        dok[2 + 0x1C..2 + 0x20].copy_from_slice(&(von + text.len() as u32).to_le_bytes());
        dok.extend(text);
        ole_schreiben(&p, &[("WordDocument", &dok)]);
        assert_eq!(word(&p).unwrap(), "Mit Versatz");
    }

    /// Beschädigtes Verzeichnis: Fehlt der Strom, wird der Kopf eines
    /// Word-6-Dokuments an den Sektorgrenzen gesucht (v0.0.9).
    #[test]
    fn word_6_ohne_verzeichnis_gefunden() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("kaputt.doc");
        let mut dok = fib(0x0065, 0);
        let text = b"Gefunden an der Sektorgrenze";
        dok[0x18..0x1C].copy_from_slice(&0x200u32.to_le_bytes());
        dok[0x1C..0x20].copy_from_slice(&(0x200 + text.len() as u32).to_le_bytes());
        dok.extend(text);
        // Ab 4096 Byte liegt ein Strom in Sektoren zu 512 Byte
        dok.resize(5000, 0);
        ole_schreiben(&p, &[("Anderer Name", &dok)]);
        assert_eq!(word(&p).unwrap(), "Gefunden an der Sektorgrenze");
        // Ohne Kopf bleibt die ursprüngliche Meldung
        assert!(word_suchen(&[0u8; 2048]).is_none());
    }

    #[test]
    fn word_2() {
        let mut d = vec![0u8; 0x80];
        d[0..2].copy_from_slice(&0xA5DBu16.to_le_bytes());
        let text = b"Brief aus Word 2\rZweite Zeile";
        d[0x18..0x1C].copy_from_slice(&0x80u32.to_le_bytes());
        d[0x1C..0x20].copy_from_slice(&(0x80 + text.len() as u32).to_le_bytes());
        d.extend(text);
        assert!(ist_word2(&d));
        assert_eq!(word2(&d).unwrap(), "Brief aus Word 2\nZweite Zeile");

        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("w2.doc");
        std::fs::write(&p, &d).unwrap();
        assert_eq!(crate::dokumente::lesen(&p).unwrap().seiten[0].text, "Brief aus Word 2\nZweite Zeile");

        let mit = |merkmale: u16| {
            let mut x = d.clone();
            x[0x0A..0x0C].copy_from_slice(&merkmale.to_le_bytes());
            x
        };
        assert!(word2(&mit(0x0100)).unwrap_err().contains("verschlüsselt"));
        assert!(word2(&mit(0x0004)).unwrap_err().contains("schnell gespeichert"));
        let mut leer = d.clone();
        leer[0x1C..0x20].copy_from_slice(&0x80u32.to_le_bytes());
        assert!(word2(&leer).unwrap_err().contains("beschädigt"));
        assert!(word2(&[0xDB, 0xA5]).unwrap_err().contains("beschädigt"));
    }

    #[test]
    fn steuerzeichen_von_word() {
        let roh: Vec<char> = "a\u{0B}b\u{0C}c\r\r\rd\u{1E}e\u{A0}f\u{AD}g\u{01}h".chars().collect();
        assert_eq!(word_bereinigen(roh), "a\nb\nc\n\nd-e fgh");
        // Verschachtelte Felder: nur das äußere Ergebnis
        let roh: Vec<char> = "x\u{13}A\u{13}B\u{14}C\u{15}\u{14}D\u{15}y".chars().collect();
        assert_eq!(word_bereinigen(roh), "xDy");
    }

    #[test]
    fn verbunddateien_mit_fehlern() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.ppt");
        ole_schreiben(&p, &[("PP40", b"x")]);
        assert!(powerpoint(&p).unwrap_err().contains("PowerPoint 4"));
        ole_schreiben(&p, &[("Anderes", b"x")]);
        assert!(powerpoint(&p).unwrap_err().contains("fehlt"));
        // Kennung der Verbunddatei, dahinter nichts Brauchbares
        let mut kaputt = crate::ole::KENNUNG.to_vec();
        kaputt.extend([0u8; 100]);
        std::fs::write(&p, &kaputt).unwrap();
        assert!(powerpoint(&p).is_err());
        assert!(powerpoint(&tmp.path().join("fehlt.ppt")).is_err());
    }

    /// Excel 5 im Strom `Book` liest der eigene Leser, eine unlesbare
    /// Arbeitsmappe meldet calamine (v0.1.2).
    #[test]
    fn excel_5_und_unlesbar() {
        let satz5 = |art: u16, inhalt: &[u8]| {
            let mut d = art.to_le_bytes().to_vec();
            d.extend((inhalt.len() as u16).to_le_bytes());
            d.extend(inhalt);
            d
        };
        let mut book = satz5(0x0809, &[0, 5, 0x10, 0, 0, 0, 0, 0]);
        let mut l = vec![0, 0, 0, 0, 0, 0, 6, 0];
        l.extend(b"Posten");
        book.extend(satz5(0x0204, &l));
        book.extend(satz5(0x000A, &[]));
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("alt.xls");
        ole_schreiben(&p, &[("Book", &book)]);
        assert!(excel(&p).unwrap().join("\n").contains("Posten"));

        ole_schreiben(&p, &[("Workbook", b"kein Excel")]);
        assert!(excel(&p).unwrap_err().contains("nicht lesbar"));
    }

    fn satz(art: u16, typ: u16, inhalt: &[u8]) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend(art.to_le_bytes());
        s.extend(typ.to_le_bytes());
        s.extend((inhalt.len() as u32).to_le_bytes());
        s.extend(inhalt);
        s
    }

    #[test]
    fn powerpoint_textatome_ohne_master() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("folien.ppt");
        let liste = [
            satz(0, 0x0FA0, &utf16_bytes("Jahresrückblick Ärger")),
            satz(0, 0x0FA8, b"Punkt eins\rPunkt zwei"),
            satz(0, 0x0FA8, b"*"),
            // derselbe Text ein zweites Mal
            satz(0, 0x0FA8, b"Punkt eins\rPunkt zwei"),
        ]
        .concat();
        let dok = [
            satz(0x000F, 0x03E8, &satz(0x000F, 0x0FF0, &liste)),
            satz(0x000F, 0x03F8, &satz(0, 0x0FA8, b"Mastertitel bearbeiten")),
            satz(0x000F, 0x03EE, &satz(0x000F, 0xF00D, &satz(0, 0x0FA8, b"Freies Textfeld"))),
            // abgeschnittener Satz am Ende
            vec![0x0F, 0, 0xEE, 0x03, 0xFF, 0xFF, 0, 0, 1, 2],
        ]
        .concat();
        ole_schreiben(&p, &[("PowerPoint Document", &dok)]);
        assert_eq!(powerpoint(&p).unwrap(), "Jahresrückblick Ärger\nPunkt eins\nPunkt zwei\nFreies Textfeld");
        ole_schreiben(&p, &[("EncryptedSummary", b"x"), ("PowerPoint Document", &dok)]);
        assert!(powerpoint(&p).unwrap_err().contains("verschlüsselt"));
    }

    #[test]
    fn excel_je_blatt() {
        // Mit LibreOffice erzeugte Datei: zwei Blätter, Formel, leere Zeile
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/daten/haushalt.xls");
        let seiten = excel(&p).unwrap();
        assert_eq!(
            seiten,
            ["Strom\nAnbieter\tAbschlag\tDatum\nEnBW\t89.5\tJuli\nStadtwerke Größe\t120\nSumme\t209.5", "Gas\nZählerstand\t12345"]
        );
        let i = crate::dokumente::lesen(&p).unwrap();
        assert_eq!((i.seiten.len(), i.seiten[1].nr), (2, 2));
    }
}
