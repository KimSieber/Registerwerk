//! Text aus Office- und Textdateien (v0.0.6).
//!
//! Gelesen wird ohne fremdes Programm: Word, Excel, PowerPoint und
//! OpenDocument sind ZIP-Archive mit XML darin.
//!
//! | Format | Seiten im Register |
//! |---|---|
//! | Word (docx, docm, dotx, dotm), OpenDocument (odt, ods, odp) | eine Seite mit dem ganzen Text |
//! | Excel (xlsx, xlsm, xltx, xltm) | eine Seite je Tabellenblatt, Zellen mit Tab, Zeilen mit Zeilenumbruch |
//! | PowerPoint (pptx, pptm) | eine Seite je Folie |
//! | Text (txt, md) | eine Seite, UTF-8, sonst Latin-1 |
//! | Pages, Keynote (ab 2013) | eine Seite mit dem ganzen Text |
//!
//! Pages und Keynote speichern ihren Inhalt als Snappy-komprimierte
//! Protobuf-Archive (IWA). Gelesen werden nur die Textspeicher (Typ 2001,
//! `TSWP.StorageArchive`), ohne Bibliothek für das ganze Format.
//!
//! Die alten Binärformate (doc, xls, ppt), Numbers und Pages vor 2013 werden
//! nicht gelesen, sie bleiben bei Name und Datum. Der Text einer Seite ist auf
//! [`MAX_ZEICHEN`] begrenzt, damit große Tabellen den Suchindex nicht sprengen.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::pdf::{Inhalt, Seite};

/// Höchstzahl der Zeichen je Seite. **Annahme**
pub const MAX_ZEICHEN: usize = 500_000;

/// Liest den Text einer Office- oder Textdatei.
pub fn lesen(pfad: &Path) -> Result<Inhalt, String> {
    let endung = pfad.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let seiten: Vec<String> = match endung.as_str() {
        "txt" | "md" => vec![text_datei(&std::fs::read(pfad).map_err(|e| e.to_string())?)],
        "docx" | "docm" | "dotx" | "dotm" => {
            let mut zip = oeffnen(pfad)?;
            vec![xml_text(&eintrag(&mut zip, "word/document.xml")?, &WORD)?]
        }
        "pptx" | "pptm" => {
            let mut zip = oeffnen(pfad)?;
            let mut folien: Vec<(u32, String)> = zip
                .file_names()
                .filter_map(|n| nummer(n, "ppt/slides/slide", ".xml").map(|z| (z, n.to_string())))
                .collect();
            folien.sort();
            folien.iter().map(|(_, n)| xml_text(&eintrag(&mut zip, n)?, &FOLIE)).collect::<Result<_, _>>()?
        }
        "xlsx" | "xlsm" | "xltx" | "xltm" => excel(&mut oeffnen(pfad)?)?,
        "pages" | "key" => vec![iwork(&mut oeffnen(pfad)?)?],
        "odt" | "ods" | "odp" => {
            let mut zip = oeffnen(pfad)?;
            vec![xml_text(&eintrag(&mut zip, "content.xml")?, &OPEN_DOCUMENT)?]
        }
        andere => return Err(format!("Format {andere} wird nicht gelesen")),
    };
    Ok(Inhalt {
        seiten: seiten
            .into_iter()
            .enumerate()
            .map(|(i, text)| Seite { nr: i as u32 + 1, text: kuerzen(text.trim().to_string()), bildanteil: 0.0 })
            .collect(),
        erstellt: None,
    })
}

type Zip = zip::ZipArchive<std::fs::File>;

fn oeffnen(pfad: &Path) -> Result<Zip, String> {
    let datei = std::fs::File::open(pfad).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(datei).map_err(|e| format!("kein gültiges Office-Dokument: {e}"))
}

fn eintrag(zip: &mut Zip, name: &str) -> Result<Vec<u8>, String> {
    let mut e = zip.by_name(name).map_err(|_| format!("{name} fehlt im Dokument"))?;
    let mut daten = Vec::new();
    e.read_to_end(&mut daten).map_err(|e| e.to_string())?;
    Ok(daten)
}

/// Nummer in einem Namen wie `ppt/slides/slide12.xml`.
fn nummer(name: &str, vorn: &str, hinten: &str) -> Option<u32> {
    name.strip_prefix(vorn)?.strip_suffix(hinten)?.parse().ok()
}

fn kuerzen(mut text: String) -> String {
    if text.len() > MAX_ZEICHEN {
        let mut ende = MAX_ZEICHEN;
        while !text.is_char_boundary(ende) {
            ende -= 1;
        }
        text.truncate(ende);
    }
    text
}

/// Text einer Textdatei: UTF-8, sonst Latin-1 (ältere Windows-Dateien).
fn text_datei(daten: &[u8]) -> String {
    let daten = daten.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(daten);
    match std::str::from_utf8(daten) {
        Ok(s) => s.to_string(),
        Err(_) => daten.iter().map(|&b| b as char).collect(),
    }
}

/// Welche Elemente Text tragen und wie Absätze, Tabulatoren und Umbrüche
/// aussehen. Verglichen wird der Name ohne Namensraum.
struct Regeln {
    /// Text zählt nur innerhalb dieser Elemente. Leer: überall.
    text_in: &'static [&'static [u8]],
    absatz: &'static [&'static [u8]],
    tab: &'static [&'static [u8]],
    umbruch: &'static [&'static [u8]],
}

const WORD: Regeln = Regeln { text_in: &[b"t"], absatz: &[b"p", b"tr"], tab: &[b"tab", b"tc"], umbruch: &[b"br", b"cr"] };
const FOLIE: Regeln = Regeln { text_in: &[b"t"], absatz: &[b"p"], tab: &[b"tab"], umbruch: &[b"br"] };
const OPEN_DOCUMENT: Regeln =
    Regeln { text_in: &[b"body"], absatz: &[b"p", b"h", b"table-row"], tab: &[b"tab", b"table-cell"], umbruch: &[b"line-break"] };

fn xml_text(xml: &[u8], r: &Regeln) -> Result<String, String> {
    let mut leser = Reader::from_reader(xml);
    let mut puffer = Vec::new();
    let mut text = String::new();
    let mut innen = 0usize;
    let leer = |e: &BytesStart, text: &mut String| {
        let name = e.local_name();
        let n = name.as_ref();
        if r.tab.contains(&n) {
            text.push('\t');
        } else if r.umbruch.contains(&n) || r.absatz.contains(&n) {
            text.push('\n');
        } else if n == b"s" {
            // OpenDocument: mehrere Leerzeichen
            let anzahl = e
                .try_get_attribute("text:c")
                .ok()
                .flatten()
                .and_then(|a| std::str::from_utf8(&a.value).ok().and_then(|v| v.parse().ok()))
                .unwrap_or(1usize);
            text.push_str(&" ".repeat(anzahl.min(100)));
        }
    };
    loop {
        match leser.read_event_into(&mut puffer).map_err(|e| format!("XML: {e}"))? {
            Event::Start(e) => {
                let n = e.local_name();
                if r.text_in.contains(&n.as_ref()) {
                    innen += 1;
                }
                if r.tab.contains(&n.as_ref()) && n.as_ref() != b"tc" && n.as_ref() != b"table-cell" {
                    text.push('\t');
                }
            }
            Event::Empty(e) => leer(&e, &mut text),
            Event::End(e) => {
                let n = e.local_name();
                if r.text_in.contains(&n.as_ref()) {
                    innen = innen.saturating_sub(1);
                }
                if r.absatz.contains(&n.as_ref()) {
                    text.push('\n');
                } else if n.as_ref() == b"tc" || n.as_ref() == b"table-cell" {
                    text.push('\t');
                }
            }
            Event::Text(t) if innen > 0 || r.text_in.is_empty() => {
                text.push_str(&t.unescape().map_err(|e| format!("XML: {e}"))?);
            }
            Event::CData(t) if innen > 0 => text.push_str(&String::from_utf8_lossy(&t)),
            Event::Eof => break,
            _ => {}
        }
        if text.len() > MAX_ZEICHEN {
            break;
        }
        puffer.clear();
    }
    Ok(text)
}

/// Pages und Keynote: Text aller Textspeicher in den IWA-Archiven unter
/// `Index/`, ohne Stilvorlagen und Verwaltungsdaten.
fn iwork(zip: &mut Zip) -> Result<String, String> {
    let mut namen: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("Index/") && n.ends_with(".iwa"))
        .filter(|n| !["Stylesheet", "ViewState", "Metadata", "Annotation", "CalculationEngine"].iter().any(|x| n.contains(x)))
        .map(str::to_string)
        .collect();
    if namen.is_empty() {
        return Err("Pages/Keynote vor 2013 wird nicht gelesen".into());
    }
    // Document.iwa zuerst, der Rest in Namensfolge
    namen.sort_by_key(|n| (!n.ends_with("/Document.iwa"), n.clone()));
    let mut text = String::new();
    for n in namen {
        let daten = iwa_entpacken(&eintrag(zip, &n)?)?;
        for t in iwa_texte(&daten)? {
            text.push_str(&t);
            text.push('\n');
            if text.len() > MAX_ZEICHEN {
                return Ok(text);
            }
        }
    }
    Ok(text.replace(['\u{2028}', '\u{2029}'], "\n").replace('\u{fffc}', ""))
}

/// IWA: Blöcke aus 1 Byte Typ, 3 Byte Länge (little endian) und Snappy-Daten ohne Rahmen.
fn iwa_entpacken(daten: &[u8]) -> Result<Vec<u8>, String> {
    let mut aus = Vec::new();
    let mut i = 0;
    while i + 4 <= daten.len() {
        let n = daten[i + 1] as usize | (daten[i + 2] as usize) << 8 | (daten[i + 3] as usize) << 16;
        let block = daten.get(i + 4..i + 4 + n).ok_or("IWA: Block abgeschnitten")?;
        aus.extend(snap::raw::Decoder::new().decompress_vec(block).map_err(|e| format!("IWA: {e}"))?);
        i += 4 + n;
    }
    Ok(aus)
}

fn varint(b: &[u8], i: &mut usize) -> Result<u64, String> {
    let mut wert = 0u64;
    for schiebe in (0..64).step_by(7) {
        let c = *b.get(*i).ok_or("IWA: Daten abgeschnitten")?;
        *i += 1;
        wert |= ((c & 0x7f) as u64) << schiebe;
        if c < 0x80 {
            return Ok(wert);
        }
    }
    Err("IWA: Zahl zu lang".into())
}

/// Felder einer Protobuf-Nachricht: (Nummer, Zahl, Bytes). Bei Zahlen sind die
/// Bytes leer, bei Bytes die Zahl 0.
type Feld<'a> = (u64, u64, &'a [u8]);

fn felder(b: &[u8]) -> Result<Vec<Feld<'_>>, String> {
    let (mut liste, mut i) = (Vec::new(), 0);
    while i < b.len() {
        let schluessel = varint(b, &mut i)?;
        let (nr, art) = (schluessel >> 3, schluessel & 7);
        match art {
            0 => liste.push((nr, varint(b, &mut i)?, &b[0..0])),
            1 => i += 8,
            5 => i += 4,
            2 => {
                let n = varint(b, &mut i)? as usize;
                liste.push((nr, 0, b.get(i..i + n).ok_or("IWA: Feld abgeschnitten")?));
                i += n;
            }
            _ => return Err(format!("IWA: unbekannte Feldart {art}")),
        }
    }
    Ok(liste)
}

/// Texte der Textspeicher (Typ 2001, Feld 3) in einem entpackten Archiv. Das
/// Archiv ist eine Folge aus Länge, `ArchiveInfo` und den Nachrichten, deren
/// Typ und Länge in `ArchiveInfo.message_infos` (Feld 2) stehen.
fn iwa_texte(b: &[u8]) -> Result<Vec<String>, String> {
    let (mut texte, mut i) = (Vec::new(), 0);
    while i < b.len() {
        let n = varint(b, &mut i)? as usize;
        let info = b.get(i..i + n).ok_or("IWA: Kopf abgeschnitten")?;
        i += n;
        for (nr, _, nachricht) in felder(info)? {
            if nr != 2 {
                continue;
            }
            let angaben = felder(nachricht)?;
            let typ = angaben.iter().find(|f| f.0 == 1).map(|f| f.1).unwrap_or(0);
            let laenge = angaben.iter().find(|f| f.0 == 3).map(|f| f.1 as usize).unwrap_or(0);
            let inhalt = b.get(i..i + laenge).ok_or("IWA: Nachricht abgeschnitten")?;
            i += laenge;
            if typ == 2001 {
                for (nr, _, t) in felder(inhalt)? {
                    if nr == 3 {
                        texte.push(String::from_utf8_lossy(t).into_owned());
                    }
                }
            }
        }
    }
    Ok(texte)
}

/// Excel: eine Seite je Tabellenblatt, beginnend mit dem Namen des Blatts.
fn excel(zip: &mut Zip) -> Result<Vec<String>, String> {
    let geteilt = match eintrag(zip, "xl/sharedStrings.xml") {
        Ok(xml) => geteilte_texte(&xml)?,
        Err(_) => Vec::new(),
    };
    // Blätter in der Reihenfolge der Arbeitsmappe, Datei über die Beziehungen
    let blaetter = blaetter(&eintrag(zip, "xl/workbook.xml")?)?;
    let ziele = beziehungen(&eintrag(zip, "xl/_rels/workbook.xml.rels").unwrap_or_default())?;
    let mut seiten = Vec::new();
    for (name, rid) in blaetter {
        let Some(ziel) = ziele.get(&rid) else { continue };
        let pfad = match ziel.strip_prefix('/') {
            Some(absolut) => absolut.to_string(),
            None => format!("xl/{ziel}"),
        };
        let Ok(xml) = eintrag(zip, &pfad) else { continue };
        seiten.push(format!("{name}\n{}", blatt(&xml, &geteilt)?));
    }
    Ok(seiten)
}

fn attribut(e: &BytesStart, name: &str) -> Option<String> {
    e.try_get_attribute(name).ok().flatten().and_then(|a| a.unescape_value().ok().map(|v| v.to_string()))
}

fn geteilte_texte(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut leser = Reader::from_reader(xml);
    let mut puffer = Vec::new();
    let (mut liste, mut aktuell, mut in_t) = (Vec::new(), String::new(), false);
    loop {
        match leser.read_event_into(&mut puffer).map_err(|e| format!("XML: {e}"))? {
            Event::Start(e) if e.local_name().as_ref() == b"t" => in_t = true,
            Event::End(e) if e.local_name().as_ref() == b"t" => in_t = false,
            Event::End(e) if e.local_name().as_ref() == b"si" => liste.push(std::mem::take(&mut aktuell)),
            Event::Text(t) if in_t => aktuell.push_str(&t.unescape().map_err(|e| format!("XML: {e}"))?),
            Event::Eof => break,
            _ => {}
        }
        puffer.clear();
    }
    Ok(liste)
}

/// Name und Beziehungs-Id der Blätter aus `workbook.xml`.
fn blaetter(xml: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut leser = Reader::from_reader(xml);
    let mut puffer = Vec::new();
    let mut liste = Vec::new();
    loop {
        match leser.read_event_into(&mut puffer).map_err(|e| format!("XML: {e}"))? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                if let (Some(name), Some(rid)) = (attribut(&e, "name"), attribut(&e, "r:id")) {
                    liste.push((name, rid));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        puffer.clear();
    }
    Ok(liste)
}

/// Beziehungen Id → Ziel aus einer `.rels`-Datei.
fn beziehungen(xml: &[u8]) -> Result<HashMap<String, String>, String> {
    let mut leser = Reader::from_reader(xml);
    let mut puffer = Vec::new();
    let mut ziele = HashMap::new();
    loop {
        match leser.read_event_into(&mut puffer).map_err(|e| format!("XML: {e}"))? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if let (Some(id), Some(ziel)) = (attribut(&e, "Id"), attribut(&e, "Target")) {
                    ziele.insert(id, ziel);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        puffer.clear();
    }
    Ok(ziele)
}

/// Zellen eines Blatts: Zellen mit Tab, Zeilen mit Zeilenumbruch.
fn blatt(xml: &[u8], geteilt: &[String]) -> Result<String, String> {
    let mut leser = Reader::from_reader(xml);
    let mut puffer = Vec::new();
    let mut text = String::new();
    let (mut typ, mut wert, mut in_wert, mut zellen) = (String::new(), String::new(), false, Vec::<String>::new());
    loop {
        match leser.read_event_into(&mut puffer).map_err(|e| format!("XML: {e}"))? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"c" => {
                    typ = attribut(&e, "t").unwrap_or_default();
                    wert.clear();
                }
                b"v" | b"t" => in_wert = true,
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                b"v" | b"t" => in_wert = false,
                b"c" => {
                    let w = match typ.as_str() {
                        "s" => wert.trim().parse::<usize>().ok().and_then(|i| geteilt.get(i)).cloned().unwrap_or_default(),
                        "b" => String::new(),
                        _ => wert.clone(),
                    };
                    if !w.trim().is_empty() {
                        zellen.push(w);
                    }
                }
                b"row" if !zellen.is_empty() => {
                    text.push_str(&zellen.join("\t"));
                    text.push('\n');
                    zellen.clear();
                }
                _ => {}
            },
            Event::Text(t) if in_wert => wert.push_str(&t.unescape().map_err(|e| format!("XML: {e}"))?),
            Event::Eof => break,
            _ => {}
        }
        if text.len() > MAX_ZEICHEN {
            break;
        }
        puffer.clear();
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_schreiben(pfad: &Path, eintraege: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(pfad).unwrap());
        for (name, inhalt) in eintraege {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(inhalt.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    fn texte(i: &Inhalt) -> Vec<&str> {
        i.seiten.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn word() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Brief.docx");
        zip_schreiben(
            &p,
            &[(
                "word/document.xml",
                r#"<w:document xmlns:w="w"><w:body>
                   <w:p><w:r><w:t>Sehr geehrte</w:t></w:r><w:r><w:t xml:space="preserve"> Damen &amp; Herren,</w:t></w:r></w:p>
                   <w:p><w:r><w:instrText>PAGE</w:instrText><w:t>Betrag</w:t><w:tab/><w:t>12,00</w:t></w:r></w:p>
                   </w:body></w:document>"#,
            )],
        );
        let i = lesen(&p).unwrap();
        assert_eq!(texte(&i), ["Sehr geehrte Damen & Herren,\nBetrag\t12,00"]);
    }

    #[test]
    fn excel_je_blatt() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Haushalt.xlsm");
        zip_schreiben(
            &p,
            &[
                (
                    "xl/workbook.xml",
                    r#"<workbook xmlns:r="r"><sheets><sheet name="Strom" sheetId="1" r:id="rId1"/><sheet name="Gas" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<Relationships><Relationship Id="rId2" Target="worksheets/sheet2.xml"/><Relationship Id="rId1" Target="/xl/worksheets/sheet1.xml"/></Relationships>"#,
                ),
                ("xl/sharedStrings.xml", r#"<sst><si><t>EnBW</t></si><si><r><t>Abschlag </t></r><r><t>Juli</t></r></si></sst>"#),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row><c t="s"><v>0</v></c><c t="s"><v>1</v></c><c><v>89.5</v></c></row>
                       <row><c t="inlineStr"><is><t>Summe</t></is></c><c t="b"><v>1</v></c></row></sheetData></worksheet>"#,
                ),
                ("xl/worksheets/sheet2.xml", r#"<worksheet><sheetData><row><c t="str"><v>Stadtwerke</v></c></row></sheetData></worksheet>"#),
            ],
        );
        let i = lesen(&p).unwrap();
        assert_eq!(texte(&i), ["Strom\nEnBW\tAbschlag Juli\t89.5\nSumme", "Gas\nStadtwerke"]);
        assert_eq!(i.seiten[1].nr, 2);
    }

    #[test]
    fn powerpoint_je_folie_in_reihenfolge() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Vortrag.pptx");
        zip_schreiben(
            &p,
            &[
                ("ppt/slides/slide10.xml", r#"<p:sld><a:p><a:t>Zehn</a:t></a:p></p:sld>"#),
                ("ppt/slides/slide2.xml", r#"<p:sld><a:p><a:t>Zwei</a:t></a:p><a:p><a:t>b</a:t></a:p></p:sld>"#),
            ],
        );
        assert_eq!(texte(&lesen(&p).unwrap()), ["Zwei\nb", "Zehn"]);
    }

    #[test]
    fn open_document_und_text() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Notiz.odt");
        zip_schreiben(
            &p,
            &[(
                "content.xml",
                r#"<office:document-content><office:automatic-styles><style:style/></office:automatic-styles>
                   <office:body><office:text><text:h>Titel</text:h><text:p>a<text:s text:c="2"/>b<text:span>c</text:span></text:p></office:text></office:body></office:document-content>"#,
            )],
        );
        assert_eq!(texte(&lesen(&p).unwrap()), ["Titel\na  bc"]);

        let t = tmp.path().join("alt.txt");
        std::fs::write(&t, b"Gr\xfc\xdfe").unwrap();
        assert_eq!(texte(&lesen(&t).unwrap()), ["Grüße"]);
        let m = tmp.path().join("neu.md");
        std::fs::write(&m, "\u{feff}# Überschrift\n").unwrap();
        assert_eq!(texte(&lesen(&m).unwrap()), ["# Überschrift"]);
    }

    /// Baut ein IWA-Archiv mit einem Textspeicher und einer anderen Nachricht.
    fn iwa(texte: &[&str]) -> Vec<u8> {
        fn v(mut n: u64, aus: &mut Vec<u8>) {
            loop {
                let b = (n & 0x7f) as u8;
                n >>= 7;
                if n == 0 {
                    aus.push(b);
                    break;
                }
                aus.push(b | 0x80);
            }
        }
        let mut speicher = Vec::new();
        for t in texte {
            speicher.push(3 << 3 | 2);
            v(t.len() as u64, &mut speicher);
            speicher.extend_from_slice(t.as_bytes());
        }
        let anderes = b"\x08\x05".to_vec();
        let info_fuer = |typ: u64, laenge: usize| {
            let mut m = Vec::new();
            m.push(1 << 3);
            v(typ, &mut m);
            m.push(3 << 3);
            v(laenge as u64, &mut m);
            m
        };
        let mut info = Vec::new();
        for m in [info_fuer(2001, speicher.len()), info_fuer(7, anderes.len())] {
            info.push(2 << 3 | 2);
            v(m.len() as u64, &mut info);
            info.extend(m);
        }
        let mut roh = Vec::new();
        v(info.len() as u64, &mut roh);
        roh.extend(info);
        roh.extend(speicher);
        roh.extend(anderes);
        let gepackt = snap::raw::Encoder::new().compress_vec(&roh).unwrap();
        let mut datei = vec![0u8, gepackt.len() as u8, (gepackt.len() >> 8) as u8, (gepackt.len() >> 16) as u8];
        datei.extend(gepackt);
        datei
    }

    #[test]
    fn pages() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Brief.pages");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        for (name, daten) in [
            ("Index/Document.iwa", iwa(&["Sehr geehrte\u{2028}Damen\u{fffc}", "Fußzeile"])),
            ("Index/DocumentStylesheet-1.iwa", iwa(&["Stilname"])),
        ] {
            z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&daten).unwrap();
        }
        z.finish().unwrap();
        assert_eq!(texte(&lesen(&p).unwrap()), ["Sehr geehrte\nDamen\nFußzeile"]);
    }

    #[test]
    fn fehler_und_grenzen() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("kaputt.docx");
        std::fs::write(&p, b"kein zip").unwrap();
        assert!(lesen(&p).unwrap_err().contains("kein gültiges Office-Dokument"));
        assert!(lesen(Path::new("x.doc")).is_err());
        assert_eq!(kuerzen("ä".repeat(MAX_ZEICHEN)).len(), MAX_ZEICHEN);
    }
}
