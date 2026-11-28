//! Text aus RTF, HTML und Textdateien mit falscher Endung, dazu der grobe
//! Rückfall für Dateien, die kein Leser versteht (v0.0.9).
//!
//! - **RTF:** eigener Leser. Steuerwörter für Absätze und Tabulatoren werden
//!   übersetzt, Schrifttabellen, Bilder und Feldanweisungen übergangen.
//! - **HTML:** Tags entfernt, `script` und `style` übergangen, die häufigen
//!   Zeichenreferenzen aufgelöst.
//! - **Grob:** lesbare Zeichenfolgen direkt aus der Datei, als 8-Bit-Text und
//!   als UTF-16, ähnlich `strings`. Der Text ist ungeordnet, die Datei wird aber
//!   über die Suche gefunden. Im Register mit dem Vermerk „Text nur grob gelesen“.

/// Windows-1252: wie Latin-1, nur der Bereich 0x80 bis 0x9F ist belegt.
pub fn cp1252(b: u8) -> char {
    const OBEN: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => OBEN[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Text als UTF-8, sonst Windows-1252. Ohne BOM.
pub fn dekodieren(daten: &[u8]) -> String {
    let daten = daten.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(daten);
    match std::str::from_utf8(daten) {
        Ok(s) => s.to_string(),
        Err(_) => daten.iter().map(|&b| cp1252(b)).collect(),
    }
}

/// Höchstens drei Zeilenumbrüche in Folge, Zeilenenden ohne Leerzeichen.
fn aufraeumen(text: &str) -> String {
    let mut aus = String::with_capacity(text.len());
    let mut leer = 0;
    for zeile in text.lines() {
        let zeile = zeile.trim_end();
        if zeile.trim().is_empty() {
            leer += 1;
            if leer > 1 {
                continue;
            }
        } else {
            leer = 0;
        }
        aus.push_str(zeile);
        aus.push('\n');
    }
    aus.trim().to_string()
}

// ---------- Erkennen ----------

/// Beginnt die Datei als RTF?
pub fn ist_rtf(daten: &[u8]) -> bool {
    daten.starts_with(b"{\\rtf")
}

/// Ist die Datei HTML? Erlaubt Leerraum und BOM am Anfang.
pub fn ist_html(daten: &[u8]) -> bool {
    let anfang: String = dekodieren(&daten[..daten.len().min(1024)]).trim_start().to_lowercase();
    anfang.starts_with("<!doctype html") || anfang.starts_with("<html") || (anfang.starts_with('<') && anfang.contains("<html"))
}

/// Ist die Datei reiner Text? Keine Nullbytes und fast nur druckbare
/// Zeichen im Anfang. Etwa eine Seriendruck-Datenquelle mit Endung `.doc`.
pub fn ist_text(daten: &[u8]) -> bool {
    let probe = &daten[..daten.len().min(8192)];
    if probe.is_empty() || probe.contains(&0) {
        return false;
    }
    let lesbar = probe.iter().filter(|&&b| b >= 0x20 || b == b'\n' || b == b'\r' || b == b'\t').count();
    lesbar * 100 >= probe.len() * 97
}

// ---------- RTF ----------

/// Gruppen, deren Inhalt kein Text des Dokuments ist.
const RTF_UEBERGEHEN: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "objdata", "fldinst", "datafield", "themedata",
    "colorschememapping", "latentstyles", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl",
    "mmathPr", "filetbl", "revtbl", "pgdsctbl", "bkmkstart", "bkmkend", "datastore", "xmlopen", "wgrffmtfilter",
    "shppict", "nonshppict", "blipuid", "header", "footer", "headerl", "headerr", "headerf", "footerl", "footerr",
    "footerf", "pnseclvl", "listpicture", "fchars", "lchars", "atnid", "atnauthor", "template", "comment",
    "keywords", "title", "subject", "author", "operator", "doccomm", "category", "company",
];

/// Text eines RTF-Dokuments.
pub fn rtf(daten: &[u8]) -> String {
    #[derive(Clone, Copy)]
    struct Stand {
        uebergehen: bool,
        /// Zeichen, die nach `\u` übersprungen werden
        uc: usize,
    }
    let mut stapel: Vec<Stand> = Vec::new();
    let mut stand = Stand { uebergehen: false, uc: 1 };
    let mut text = String::new();
    // Nach \u noch zu überspringende Ersatzzeichen
    let mut ueberspringen = 0usize;
    // Am Gruppenanfang: Das erste Steuerwort kann ein Ziel sein
    let mut gruppenanfang = false;
    let mut i = 0;
    let n = daten.len();
    let zeichen = |text: &mut String, z: char, stand: &Stand, ueberspringen: &mut usize| {
        if *ueberspringen > 0 {
            *ueberspringen -= 1;
        } else if !stand.uebergehen {
            text.push(z);
        }
    };
    while i < n {
        let b = daten[i];
        match b {
            b'{' => {
                stapel.push(stand);
                gruppenanfang = true;
                i += 1;
                continue;
            }
            b'}' => {
                stand = stapel.pop().unwrap_or(stand);
                i += 1;
            }
            b'\\' => {
                let Some(&c) = daten.get(i + 1) else { break };
                if c.is_ascii_alphabetic() {
                    let mut j = i + 1;
                    while j < n && daten[j].is_ascii_alphabetic() && j - i < 40 {
                        j += 1;
                    }
                    let wort = std::str::from_utf8(&daten[i + 1..j]).unwrap_or("");
                    let mut k = j;
                    if k < n && (daten[k] == b'-' || daten[k].is_ascii_digit()) {
                        k += 1;
                        while k < n && daten[k].is_ascii_digit() && k - j < 12 {
                            k += 1;
                        }
                    }
                    let param: Option<i64> = std::str::from_utf8(&daten[j..k]).ok().and_then(|p| p.parse().ok());
                    if k < n && daten[k] == b' ' {
                        k += 1;
                    }
                    i = k;
                    if gruppenanfang && RTF_UEBERGEHEN.contains(&wort) {
                        stand.uebergehen = true;
                    }
                    gruppenanfang = false;
                    match wort {
                        "par" | "line" | "sect" | "page" | "row" => zeichen(&mut text, '\n', &stand, &mut 0),
                        "tab" | "cell" => zeichen(&mut text, '\t', &stand, &mut 0),
                        "emdash" => zeichen(&mut text, '—', &stand, &mut ueberspringen),
                        "endash" => zeichen(&mut text, '–', &stand, &mut ueberspringen),
                        "bullet" => zeichen(&mut text, '•', &stand, &mut ueberspringen),
                        "lquote" => zeichen(&mut text, '‘', &stand, &mut ueberspringen),
                        "rquote" => zeichen(&mut text, '’', &stand, &mut ueberspringen),
                        "ldblquote" => zeichen(&mut text, '“', &stand, &mut ueberspringen),
                        "rdblquote" => zeichen(&mut text, '”', &stand, &mut ueberspringen),
                        "uc" => stand.uc = param.unwrap_or(1).clamp(0, 10) as usize,
                        "u" => {
                            let wert = param.unwrap_or(0);
                            let wert = if wert < 0 { wert + 65536 } else { wert };
                            if let Some(z) = char::from_u32(wert as u32) {
                                zeichen(&mut text, z, &stand, &mut 0);
                            }
                            ueberspringen = stand.uc;
                        }
                        "bin" => i = i.saturating_add(param.unwrap_or(0).max(0) as usize),
                        _ => {}
                    }
                    continue;
                }
                gruppenanfang = false;
                match c {
                    b'\'' => {
                        let hex = daten.get(i + 2..i + 4).and_then(|h| std::str::from_utf8(h).ok());
                        if let Some(wert) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                            zeichen(&mut text, cp1252(wert), &stand, &mut ueberspringen);
                        }
                        i += 4;
                    }
                    b'*' => {
                        stand.uebergehen = true;
                        i += 2;
                    }
                    b'~' => {
                        zeichen(&mut text, ' ', &stand, &mut ueberspringen);
                        i += 2;
                    }
                    b'_' => {
                        zeichen(&mut text, '-', &stand, &mut ueberspringen);
                        i += 2;
                    }
                    b'\n' | b'\r' => {
                        zeichen(&mut text, '\n', &stand, &mut 0);
                        i += 2;
                    }
                    b'\\' | b'{' | b'}' => {
                        zeichen(&mut text, c as char, &stand, &mut ueberspringen);
                        i += 2;
                    }
                    _ => i += 2,
                }
                continue;
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                zeichen(&mut text, cp1252(b), &stand, &mut ueberspringen);
                i += 1;
            }
        }
        gruppenanfang = false;
    }
    aufraeumen(&text)
}

// ---------- HTML ----------

/// Text einer HTML-Datei.
/// Stelle von `nadel` (ASCII, klein geschrieben) in `text`, Groß- und
/// Kleinschreibung egal. Die Stelle liegt immer auf einer Zeichengrenze, weil
/// die Nadel mit `<` beginnt.
fn finde_ohne_gross(text: &str, nadel: &str) -> Option<usize> {
    let n = nadel.as_bytes();
    text.as_bytes().windows(n.len().max(1)).position(|w| w.eq_ignore_ascii_case(n))
}

pub fn html(daten: &[u8]) -> String {
    let quelle = dekodieren(daten);
    let mut text = String::new();
    let mut rest = quelle.as_str();
    while let Some(p) = rest.find('<') {
        text.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(ende) = rest.find('>') else { break };
        let tag = rest[1..ende].trim().to_ascii_lowercase();
        let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        rest = &rest[ende + 1..];
        if (name == "script" || name == "style") && !tag.starts_with('/') {
            let schluss = format!("</{name}");
            // Ohne Kopie in Kleinbuchstaben, die kostete bei großen Dateien je
            // Script-Block den ganzen Rest (v0.1.5)
            match finde_ohne_gross(rest, &schluss) {
                Some(p) => rest = &rest[p..],
                None => break,
            }
            continue;
        }
        if tag.starts_with("!--") {
            continue;
        }
        match name.as_str() {
            "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "title" | "table" => {
                text.push('\n')
            }
            "td" | "th" => text.push('\t'),
            _ => {}
        }
    }
    text.push_str(rest);
    let text = entitaeten(&text);
    // Leerraum innerhalb einer Zeile zusammenfassen
    let zeilen: Vec<String> = text
        .lines()
        .map(|z| z.split([' ', '\u{a0}', '\t']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" "))
        .collect();
    aufraeumen(&zeilen.join("\n"))
}

fn entitaeten(text: &str) -> String {
    let mut aus = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(p) = rest.find('&') {
        aus.push_str(&rest[..p]);
        rest = &rest[p..];
        // Nur in den ersten zwölf Zeichen, nicht Bytes: Umlaute haben zwei Bytes
        let ende = rest.char_indices().take(12).find(|(_, z)| *z == ';').map(|(i, _)| i);
        let Some(ende) = ende else {
            aus.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..ende];
        let z = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "auml" => Some('ä'),
            "ouml" => Some('ö'),
            "uuml" => Some('ü'),
            "Auml" => Some('Ä'),
            "Ouml" => Some('Ö'),
            "Uuml" => Some('Ü'),
            "szlig" => Some('ß'),
            "euro" => Some('€'),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            n if n.starts_with("#x") || n.starts_with("#X") => {
                u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32)
            }
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match z {
            Some(z) => {
                aus.push(z);
                rest = &rest[ende + 1..];
            }
            None => {
                aus.push('&');
                rest = &rest[1..];
            }
        }
    }
    aus.push_str(rest);
    aus
}

// ---------- Visitenkarten ----------

/// Eigenschaften einer Visitenkarte, die nur Verwaltung sind.
const VCF_UEBERGEHEN: &[&str] =
    &["BEGIN", "END", "VERSION", "PRODID", "UID", "REV", "PHOTO", "LOGO", "SOUND", "KEY", "X-ABUID", "X-ABLABEL"];

/// Quoted-Printable dekodieren (vCard 2.1): `=C3=BC` wird zu Bytes.
fn qp(wert: &str) -> Vec<u8> {
    let b = wert.as_bytes();
    let mut aus = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            if let Some(z) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                aus.push(z);
                i += 3;
                continue;
            }
        }
        aus.push(b[i]);
        i += 1;
    }
    aus
}

/// Text einer Visitenkarte (vCard, auch mehrere in einer Datei): je
/// Eigenschaft eine Zeile mit dem Wert, ohne Verwaltungsangaben und Bilder.
pub fn vcf(daten: &[u8]) -> String {
    let quelle = dekodieren(daten).replace("\r\n", "\n");
    // Fortsetzungszeilen (Leerzeichen am Anfang) und weiche Umbrüche von
    // Quoted-Printable (= am Zeilenende) anhängen
    let mut zeilen: Vec<String> = Vec::new();
    for z in quelle.split('\n') {
        match zeilen.last_mut() {
            Some(letzte) if z.starts_with(' ') || z.starts_with('\t') => letzte.push_str(&z[1..]),
            Some(letzte) if letzte.ends_with('=') && letzte.to_uppercase().contains("QUOTED-PRINTABLE") => {
                letzte.pop();
                letzte.push_str(z);
            }
            _ => zeilen.push(z.to_string()),
        }
    }
    let mut text = String::new();
    for z in zeilen {
        let Some((kopf, wert)) = z.split_once(':') else { continue };
        let teile: Vec<&str> = kopf.split(';').collect();
        let name = teile[0].rsplit('.').next().unwrap_or("").to_uppercase();
        if VCF_UEBERGEHEN.contains(&name.as_str()) || wert.trim().is_empty() {
            if name == "END" {
                text.push('\n');
            }
            continue;
        }
        let kopf_gross = kopf.to_uppercase();
        let wert = if kopf_gross.contains("QUOTED-PRINTABLE") {
            let roh = qp(wert);
            if kopf_gross.contains("CHARSET=UTF-8") || std::str::from_utf8(&roh).is_ok() {
                String::from_utf8_lossy(&roh).into_owned()
            } else {
                roh.iter().map(|&b| cp1252(b)).collect()
            }
        } else {
            wert.to_string()
        };
        // Strukturierte Werte (Name, Adresse) mit ; getrennt, \, und \n entschärfen
        let wert = wert.replace("\\n", " ").replace("\\,", ",").replace("\\;", ";");
        let wert: Vec<&str> = wert.split(';').map(str::trim).filter(|t| !t.is_empty()).collect();
        if !wert.is_empty() {
            text.push_str(&wert.join(" "));
            text.push('\n');
        }
    }
    aufraeumen(&text)
}

// ---------- Grob ----------

/// Vermerk im Register für Dateien, deren Text nur grob gelesen ist.
pub const GROB: &str = "Text nur grob gelesen";

/// Mindestens so viele Buchstaben, damit der grobe Text etwas taugt.
const GROB_MIN_BUCHSTABEN: usize = 20;

/// Verwaltungsnamen der Verbunddateien, kein Text des Dokuments.
const GROB_RAUSCHEN: &[&str] = &[
    "Root Entry", "SummaryInformation", "DocumentSummaryInformation", "CompObj", "WordDocument", "ObjectPool",
    "PowerPoint Document", "Current User", "Workbook", "Microsoft Word", "Microsoft Excel", "Normal.dot",
    "Times New Roman", "Arial", "Courier New", "Symbol", "Wingdings", "MSWordDoc", "Word.Document.6",
    "Word.Document.8", "Word.Picture.6", "Word.Picture.8", "Microsoft Word Dokument", "Microsoft Word-Dokument",
    "Microsoft Word-Grafik", "Microsoft Excel-Arbeitsblatt", "Excel.Sheet.5", "Excel.Sheet.8", "Biff5", "Biff8",
    "Microsoft Excel 97-Tabelle", "Desktop", "-keine-", "Tabelle1", "Tabelle2", "Tabelle3", "Standard",
];

/// Taugt die Zeichenfolge als Text? Sie braucht ein Wort aus mindestens vier
/// Buchstaben mit Vokal, fast nur Buchstaben, Ziffern und Leerzeichen und
/// keine langen Wiederholungen (Füllmuster in Binärdaten).
fn taugt(lauf: &str) -> bool {
    let zeichen = lauf.chars().count();
    let gemischt = lauf.chars().filter(|z| z.is_alphanumeric() || *z == ' ').count() * 10 >= zeichen * 8;
    let wort = lauf.split(|z: char| !z.is_alphabetic()).any(|w| {
        w.chars().count() >= 4
            && w.chars().any(|z| "aeiouyäöüAEIOUYÄÖÜ".contains(z))
            && w.chars().any(|z| !"aeiouyäöüAEIOUYÄÖÜ".contains(z))
    });
    // Muster wie "fbfbfbfb" oder ">>>>": wenige verschiedene Zeichen auf viel Länge
    let mut verschieden: Vec<char> = lauf.chars().collect();
    verschieden.sort_unstable();
    verschieden.dedup();
    let abwechslung = verschieden.len() * 4 >= zeichen.min(40);
    gemischt && wort && abwechslung && !GROB_RAUSCHEN.iter().any(|r| lauf.trim().eq_ignore_ascii_case(r))
}

/// Zeichen, die in grob gelesenem Text vorkommen dürfen: Buchstaben ohne
/// Akzente, die deutschen Umlaute und wenige Akzentbuchstaben, Ziffern und
/// übliche Satzzeichen. Seltene Zeichen wie þ, ³ oder ÿ stehen in Binärdaten
/// ständig und würden Zufallsfolgen zu „Text“ machen.
fn lesbar(z: char) -> bool {
    z.is_ascii_alphanumeric() || "äöüÄÖÜßéèêàáâçñ€ .,;:-_/()'\"!?%&+$§@#*=<>[]„“”‚‘’–—…•".contains(z)
}

/// Lesbare Zeichenfolgen der Datei: 8-Bit-Text (Windows-1252) und UTF-16.
/// `None`, wenn kaum Text darin steht.
pub fn grob(daten: &[u8]) -> Option<String> {
    const MIN_LAUF: usize = 5;
    let mut laeufe: Vec<String> = Vec::new();
    // 8 Bit
    let mut lauf = String::new();
    for &b in daten {
        let z = cp1252(b);
        if b >= 0x20 && lesbar(z) {
            lauf.push(z);
        } else {
            if lauf.chars().count() >= MIN_LAUF && taugt(&lauf) {
                laeufe.push(lauf.trim().to_string());
            }
            lauf.clear();
        }
    }
    if lauf.chars().count() >= MIN_LAUF && taugt(&lauf) {
        laeufe.push(lauf.trim().to_string());
    }
    // UTF-16, an beiden Ausrichtungen
    for versatz in 0..2 {
        let mut lauf = String::new();
        let einheiten = daten.get(versatz..).unwrap_or(&[]).chunks_exact(2).map(|z| u16::from_le_bytes([z[0], z[1]]));
        for z in char::decode_utf16(einheiten) {
            match z {
                Ok(z) if (z as u32) >= 0x20 && lesbar(z) => lauf.push(z),
                _ => {
                    if lauf.chars().count() >= MIN_LAUF && taugt(&lauf) {
                        laeufe.push(lauf.trim().to_string());
                    }
                    lauf.clear();
                }
            }
        }
    }
    let mut gesehen = std::collections::HashSet::new();
    laeufe.retain(|l| gesehen.insert(l.clone()));
    let buchstaben: usize = laeufe.iter().map(|l| l.chars().filter(|z| z.is_alphabetic()).count()).sum();
    if buchstaben < GROB_MIN_BUCHSTABEN {
        return None;
    }
    Some(laeufe.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liest_rtf() {
        let d = br"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\froman Times New Roman;}}{\colortbl;\red0\green0\blue0;}
{\*\generator Word;}{\info{\title Titel}{\author Kim}}\pard\plain\f0 Liebe Lucia,\par
gr\'fc\'dfe aus Stuttgart \endash  bis bald.\tab Ende\par
{\field{\*\fldinst PAGE}{\fldrslt 1}}\par
Unicode: \u8364?  und \{Klammern\}\par}";
        let t = rtf(d);
        assert_eq!(t, "Liebe Lucia,\ngrüße aus Stuttgart – bis bald.\tEnde\n1\nUnicode: €  und {Klammern}");
        assert!(ist_rtf(d));
    }

    #[test]
    fn liest_html() {
        let d = b"<!DOCTYPE html><html><head><title>Der Zauberlehrling</title><style>p{color:red}</style></head>\
<body><p>Hat der alte Hexenmeister&nbsp;sich doch einmal wegbegeben!</p><script>x=1</script>\
<table><tr><td>A&amp;B</td><td>Gr&uuml;n &#8364;</td></tr></table></body></html>";
        assert!(ist_html(d));
        assert_eq!(html(d), "Der Zauberlehrling\n\nHat der alte Hexenmeister sich doch einmal wegbegeben!\n\nA&B Grün €");
    }

    #[test]
    fn liest_visitenkarten() {
        let d = b"BEGIN:VCARD\r\nVERSION:2.1\r\nN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:M=C3=BCller;Anna;;;\r\n\
FN:Anna M\xc3\xbcller\r\nTEL;CELL:0711 123\r\nADR;HOME:;;Offenbachstr. 20;Stuttgart;;70195;\r\n\
NOTE:Lange Notiz\r\n  mit Fortsetzung\r\nPHOTO;ENCODING=BASE64:AAAA\r\nEND:VCARD\r\n";
        assert_eq!(vcf(d), "Müller Anna\nAnna Müller\n0711 123\nOffenbachstr. 20 Stuttgart 70195\nLange Notiz mit Fortsetzung");
    }

    #[test]
    fn html_mit_umlauten_an_jeder_stelle() {
        // Bis v0.0.9 brach das Lesen hier ab: Byte-Grenze mitten in einem Umlaut
        let d = "<p>Gr&ouml;&szlig;e & äöüäöüäöüäöü; Straße &amp; <SCRİPT>x</SCRİPT> İİİ<script>y</script>Ende</p>";
        let t = html(d.as_bytes());
        assert!(t.contains("Größe & äöüäöüäöüäöü; Straße &") && t.ends_with("Ende"), "{t}");
        for n in 0..d.len() {
            let _ = html(&d.as_bytes()[..n]);
        }
    }

    #[test]
    fn erkennt_text() {
        assert!(ist_text(b"\"Name\"\t\"Vorname\"\r\n\"M\xfcller\"\t\"Anna\"\r\n"));
        assert!(!ist_text(b"\xD0\xCF\x11\xE0\x00\x00abc"));
        assert!(!ist_text(b""));
    }

    #[test]
    fn grob_findet_text_in_binaerdaten() {
        let mut d = vec![0u8; 200];
        d.extend(b"\xff\xff\xff\xffRoot Entry\x00\x00Abrechnung Urlaub Fuerteventura 1996\x00\x01\x02");
        d.extend("Betrag in Peseten".encode_utf16().flat_map(u16::to_le_bytes));
        d.extend([0u8; 10]);
        d.extend(b">>>>>>>>>>>>\x00");
        let t = grob(&d).unwrap();
        assert_eq!(t, "Abrechnung Urlaub Fuerteventura 1996\nBetrag in Peseten");
        assert!(grob(&[0u8; 500]).is_none());
    }
}
