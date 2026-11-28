//! Erkannten Text als unsichtbare Textebene in Scans zurückschreiben
//! (Konzept, Abschnitt 7; Stufe 5, zweiter Teil in v0.0.5).
//!
//! Betroffen sind nur Scan-Seiten ohne Text (höchstens 10 Zeichen), deren
//! erkannter Text übernommen wurde. Digital erzeugte PDFs bleiben unverändert,
//! vorhandener Text wird nie überschrieben.
//!
//! Die Textebene entsteht so, wie Tesseract sie in eigene PDFs legt: je Wort
//! ein Textobjekt im unsichtbaren Modus (`3 Tr`) an der erkannten Stelle, auf
//! die Breite des Worts gestreckt, in einer eingebetteten Schrift ohne
//! sichtbare Zeichen. Markieren und Suchen funktionieren damit in jedem
//! PDF-Programm.
//!
//! Geschrieben wird als **inkrementelle Änderung** (ISO 32000, 7.5.6): Die
//! Bytes der Originaldatei bleiben unverändert, die neuen Objekte werden
//! angehängt. Die Scanbilder können sich dadurch nicht verändern.
//!
//! Ablauf je Datei (Konzept, Abschnitt 7, "Zurückschreiben in die PDF"):
//!
//! 1. Die neue Fassung entsteht als temporäre Datei im selben Ordner.
//! 2. Prüfung: Original unverändert am Anfang der neuen Fassung, gleiche
//!    Seitenzahl, die Seiten sehen gerendert gleich aus, Text vorhanden, die
//!    Datei lässt sich öffnen.
//! 3. Austausch in einem Schritt (Umbenennen), Änderungsdatum wie vorher.
//! 4. Protokoll mit Pfad, Seiten und Zeitpunkt.
//! 5. Register nachführen: neue Größe, neu gelesener Text, Vorschauen und
//!    OCR bleiben gültig.
//!
//! Im Probelauf entstehen die neuen Fassungen als Kopien in einem eigenen
//! Ordner. Originale und Register bleiben unberührt.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use lopdf::{dictionary, Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, StringFormat};
use rusqlite::params;
use serde::Serialize;

use crate::inhalte::Fortschritt;
use crate::ocr::OCR_VERFAHREN;
use crate::pdf::{self, Inhalt};
use crate::register::Register;
use crate::woerter::{self, LageWort};
use crate::{guete, Ergebnis, Konfiguration};

/// Schrift ohne sichtbare Zeichen aus Tesseract (Apache 2.0).
const SCHRIFT: &[u8] = include_bytes!("../schrift/glyphless.ttf");
/// Breite jedes Zeichens der Schrift in Einheiten der Schriftgröße (`/DW 500`).
const ZEICHENBREITE: f64 = 0.5;
/// Name der Schrift in den Ressourcen der Seite.
const SCHRIFT_NAME: &str = "RwOcr";
/// Protokoll der geänderten Dateien, neben dem Register.
pub const PROTOKOLL: &str = "textebene.log";

/// ToUnicode-Tabelle: Jeder Code ist das gleichnamige UTF-16-Zeichen.
const TOUNICODE: &str = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
1 beginbfrange
<0000> <FFFF> <0000>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end
";

/// Die Textebene einer Seite.
#[derive(Debug, Clone, PartialEq)]
pub struct SeitenEbene {
    /// Seitennummer ab 1.
    pub nr: u32,
    pub woerter: Vec<LageWort>,
}

/// Abbildung von der gerenderten Seite (0 bis 1, links oben) in den
/// Koordinatenraum der PDF-Seite. Berücksichtigt den sichtbaren Ausschnitt
/// (CropBox) und die Drehung der Seite (`/Rotate`), so wie PDFium die Seite
/// für die OCR gerendert hat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Abbildung {
    ursprung: (f64, f64),
    /// Richtung und Länge von u = 0 nach u = 1 (nach rechts im Bild).
    eu: (f64, f64),
    /// Richtung und Länge von v = 0 nach v = 1 (nach unten im Bild).
    ev: (f64, f64),
}

impl Abbildung {
    /// `kasten`: sichtbarer Ausschnitt [x0, y0, x1, y1] in PDF-Punkten,
    /// `drehung`: `/Rotate` der Seite im Uhrzeigersinn.
    pub fn neu(kasten: [f64; 4], drehung: i64) -> Result<Self, String> {
        let (x0, x1) = (kasten[0].min(kasten[2]), kasten[0].max(kasten[2]));
        let (y0, y1) = (kasten[1].min(kasten[3]), kasten[1].max(kasten[3]));
        let (b, h) = (x1 - x0, y1 - y0);
        if b <= 0.0 || h <= 0.0 {
            return Err("Seite ohne gültige Maße".into());
        }
        let (ursprung, eu, ev) = match drehung.rem_euclid(360) {
            0 => ((x0, y1), (b, 0.0), (0.0, -h)),
            90 => ((x0, y0), (0.0, h), (b, 0.0)),
            180 => ((x1, y0), (-b, 0.0), (0.0, h)),
            270 => ((x1, y1), (0.0, -h), (-b, 0.0)),
            andere => return Err(format!("Drehung der Seite um {andere}° nicht unterstützt")),
        };
        Ok(Self { ursprung, eu, ev })
    }

    pub fn punkt(&self, u: f64, v: f64) -> (f64, f64) {
        (self.ursprung.0 + u * self.eu.0 + v * self.ev.0, self.ursprung.1 + u * self.eu.1 + v * self.ev.1)
    }
}

fn zahl(v: f64) -> String {
    let t = format!("{v:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() {
        "0".into()
    } else {
        t.into()
    }
}

/// UTF-16BE als Hex-Zeichenkette. Zeichen außerhalb der Grundebene werden
/// ausgelassen, die Schrift kennt nur 2-Byte-Codes. Liefert auch die Zahl der
/// Zeichen.
fn utf16_hex(text: &str) -> (String, usize) {
    let mut hex = String::with_capacity(text.len() * 4);
    let mut n = 0;
    for c in text.chars().filter(|c| (*c as u32) <= 0xFFFF && !c.is_control()) {
        hex.push_str(&format!("{:04X}", c as u32));
        n += 1;
    }
    (hex, n)
}

/// Inhaltsstrom der Textebene einer Seite, unkomprimiert. Beginnt mit `Q`,
/// das ein vorangestelltes `q` schließt: Der Grafikzustand des vorhandenen
/// Seiteninhalts wirkt so nicht auf die Textebene.
pub fn inhaltsstrom(woerter: &[LageWort], abb: &Abbildung, schrift: &str) -> Vec<u8> {
    let mut s = String::from("Q\nBT\n3 Tr\n");
    for w in woerter {
        let (u0, u1) = (w.links as f64, w.rechts as f64);
        let (g0, g1) = (w.grund_links as f64, w.grund_rechts as f64);
        let p0 = abb.punkt(u0, g0);
        let p1 = abb.punkt(u1, g1);
        let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
        let laenge = (dx * dx + dy * dy).sqrt();
        // Höhe über der Grundlinie bis zur Oberkante des Worts
        let oben = abb.punkt(u0, w.oben as f64);
        let hoehe = ((oben.0 - p0.0).powi(2) + (oben.1 - p0.1).powi(2)).sqrt();
        let mut text = w.text.clone();
        if !w.zeilenende {
            text.push(' ');
        }
        let (hex, zeichen) = utf16_hex(&text);
        // Das Leerzeichen am Ende zählt nicht zur Breite des Worts
        let zeichen_wort = utf16_hex(w.text.trim()).1;
        if laenge < 0.01 || zeichen_wort == 0 || zeichen == 0 {
            continue;
        }
        let groesse = hoehe.max(1.0);
        let (ax, ay) = (dx / laenge, dy / laenge);
        // Senkrecht zur Grundlinie, nach oben (Drehung um +90°)
        let (nx, ny) = (-ay, ax);
        let streckung = 100.0 * laenge / (groesse * zeichen_wort as f64 * ZEICHENBREITE);
        s.push_str(&format!(
            "/{schrift} {} Tf {} Tz {} {} {} {} {} {} Tm <{hex}> Tj\n",
            zahl(groesse),
            zahl(streckung),
            zahl(ax),
            zahl(ay),
            zahl(nx),
            zahl(ny),
            zahl(p0.0),
            zahl(p0.1),
        ));
    }
    s.push_str("ET\n");
    s.into_bytes()
}

fn komprimiert(dict: Dictionary, daten: &[u8]) -> Stream {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(daten).expect("Schreiben in den Speicher");
    let mut dict = dict;
    dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    Stream::new(dict, z.finish().expect("Schreiben in den Speicher"))
}

/// Legt die Objekte der Schrift an und liefert den Verweis auf die Schrift.
fn schrift_anlegen(doc: &mut Document) -> ObjectId {
    let datei = doc.add_object(komprimiert(dictionary! { "Length1" => SCHRIFT.len() as i64 }, SCHRIFT));
    let cid_zu_gid: Vec<u8> = (0..65536).flat_map(|_| [0u8, 1]).collect();
    let cid_zu_gid = doc.add_object(komprimiert(Dictionary::new(), &cid_zu_gid));
    let tounicode = doc.add_object(komprimiert(Dictionary::new(), TOUNICODE.as_bytes()));
    let beschreibung = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "GlyphLessFont",
        "Flags" => 5,
        "FontBBox" => vec![0.into(), 0.into(), 500.into(), 1000.into()],
        "ItalicAngle" => 0,
        "Ascent" => 1000,
        "Descent" => -1,
        "CapHeight" => 1000,
        "StemV" => 80,
        "FontFile2" => datei,
    });
    let cid_schrift = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "GlyphLessFont",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::String(b"Adobe".to_vec(), StringFormat::Literal),
            "Ordering" => Object::String(b"Identity".to_vec(), StringFormat::Literal),
            "Supplement" => 0,
        },
        "FontDescriptor" => beschreibung,
        "DW" => 500,
        "CIDToGIDMap" => cid_zu_gid,
    });
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "GlyphLessFont",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid_schrift.into()],
        "ToUnicode" => tounicode,
    })
}

/// Sucht einen Eintrag der Seite, auch geerbt aus dem Seitenbaum.
fn geerbt<'a>(doc: &'a Document, seite: ObjectId, schluessel: &[u8]) -> Option<&'a Object> {
    let mut knoten = doc.get_object(seite).and_then(Object::as_dict).ok()?;
    for _ in 0..32 {
        if let Ok(wert) = knoten.get(schluessel) {
            return doc.dereference(wert).ok().map(|(_, o)| o);
        }
        let eltern = knoten.get(b"Parent").and_then(Object::as_reference).ok()?;
        knoten = doc.get_object(eltern).and_then(Object::as_dict).ok()?;
    }
    None
}

fn kasten(doc: &Document, wert: Option<&Object>) -> Option<[f64; 4]> {
    let feld = wert?.as_array().ok()?;
    if feld.len() != 4 {
        return None;
    }
    let mut k = [0.0; 4];
    for (i, o) in feld.iter().enumerate() {
        let (_, o) = doc.dereference(o).ok()?;
        k[i] = match o {
            Object::Integer(v) => *v as f64,
            Object::Real(v) => *v as f64,
            _ => return None,
        };
    }
    Some(k)
}

/// Abbildung einer Seite aus MediaBox, CropBox und Rotate.
fn abbildung(doc: &Document, seite: ObjectId) -> Result<Abbildung, String> {
    let media = kasten(doc, geerbt(doc, seite, b"MediaBox")).ok_or("Seite ohne MediaBox")?;
    let sichtbar = match kasten(doc, geerbt(doc, seite, b"CropBox")) {
        // Der sichtbare Ausschnitt ist auf die MediaBox begrenzt
        Some(c) => {
            let n = |k: [f64; 4]| [k[0].min(k[2]), k[1].min(k[3]), k[0].max(k[2]), k[1].max(k[3])];
            let (c, m) = (n(c), n(media));
            let s = [c[0].max(m[0]), c[1].max(m[1]), c[2].min(m[2]), c[3].min(m[3])];
            if s[0] < s[2] && s[1] < s[3] {
                s
            } else {
                m
            }
        }
        None => media,
    };
    let drehung = match geerbt(doc, seite, b"Rotate") {
        Some(Object::Integer(r)) => *r,
        Some(Object::Real(r)) => *r as i64,
        _ => 0,
    };
    Abbildung::neu(sichtbar, drehung)
}

/// Bisheriger Inhalt der Seite als Liste von Verweisen bzw. Objekten.
fn inhalte_der_seite(doc: &Document, seite: &Dictionary) -> Vec<Object> {
    match seite.get(b"Contents") {
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            // Verweis auf ein Feld von Strömen
            Ok(Object::Array(feld)) => feld.clone(),
            _ => vec![Object::Reference(*id)],
        },
        Ok(Object::Array(feld)) => feld.clone(),
        _ => Vec::new(),
    }
}

/// Fügt die Schrift den Ressourcen der Seite hinzu und liefert ihren Namen.
fn schrift_eintragen(inc: &mut IncrementalDocument, seite: ObjectId, schrift: ObjectId) -> Result<String, String> {
    let f = |e: lopdf::Error| format!("Ressourcen der Seite: {e}");
    let ressourcen = inc.get_or_create_resources(seite).map_err(f)?;
    let ressourcen = match ressourcen {
        Object::Dictionary(d) => d,
        _ => return Err("Ressourcen der Seite sind kein Verzeichnis".into()),
    };
    if !ressourcen.has(b"Font") {
        ressourcen.set("Font", Dictionary::new());
    }
    let schriften_verweis = match ressourcen.get(b"Font") {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    let schriften = match schriften_verweis {
        Some(id) => {
            inc.opt_clone_object_to_new_document(id).map_err(f)?;
            inc.new_document.get_object_mut(id).map_err(f)?
        }
        None => inc
            .get_or_create_resources(seite)
            .map_err(f)?
            .as_dict_mut()
            .map_err(f)?
            .get_mut(b"Font")
            .map_err(f)?,
    };
    let schriften = schriften.as_dict_mut().map_err(|_| "Schriften der Seite sind kein Verzeichnis".to_string())?;
    // Gleicher Name schon vergeben (etwa bei gemeinsamen Ressourcen): wiederverwenden,
    // wenn er auf unsere Schrift zeigt, sonst einen freien Namen suchen.
    let mut name = SCHRIFT_NAME.to_string();
    let mut i = 1;
    loop {
        match schriften.get(name.as_bytes()) {
            Ok(Object::Reference(id)) if *id == schrift => break,
            Ok(_) => {
                name = format!("{SCHRIFT_NAME}{i}");
                i += 1;
            }
            Err(_) => {
                schriften.set(name.clone(), Object::Reference(schrift));
                break;
            }
        }
    }
    Ok(name)
}

/// Fehler an einer Seite, mit ihrer Nummer.
fn seite_fehler<E: std::fmt::Display>(nr: u32) -> impl Fn(E) -> String {
    move |e| format!("Seite {nr}: {e}")
}

/// Bettet die Textebene der genannten Seiten ein. Liefert die neue Fassung:
/// das unveränderte Original mit angehängter Änderung.
pub fn einbetten(original: &[u8], seiten: &[SeitenEbene]) -> Result<Vec<u8>, String> {
    let doc = Document::load_mem(original).map_err(|e| format!("PDF nicht lesbar: {e}"))?;
    if doc.is_encrypted() || doc.trailer.has(b"Encrypt") {
        return Err("PDF ist verschlüsselt".into());
    }
    let seiten_ids = doc.get_pages();
    // Geometrie und bisheriger Inhalt vorab aus dem unveränderten Dokument
    let mut plan = Vec::with_capacity(seiten.len());
    for s in seiten {
        let id = *seiten_ids.get(&s.nr).ok_or_else(|| format!("Seite {} fehlt", s.nr))?;
        let abb = abbildung(&doc, id).map_err(seite_fehler(s.nr))?;
        let dict = doc.get_object(id).and_then(Object::as_dict).map_err(seite_fehler(s.nr))?;
        plan.push((s, id, abb, inhalte_der_seite(&doc, dict)));
    }
    let mut inc = IncrementalDocument::create_from(original.to_vec(), doc);
    let schrift = schrift_anlegen(&mut inc.new_document);
    let anfang = inc.new_document.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    for (s, id, abb, bisher) in plan {
        let name = schrift_eintragen(&mut inc, id, schrift).map_err(seite_fehler(s.nr))?;
        let strom = inhaltsstrom(&s.woerter, &abb, &name);
        let ebene = inc.new_document.add_object(komprimiert(Dictionary::new(), &strom));
        let mut inhalte = Vec::with_capacity(bisher.len() + 2);
        inhalte.push(Object::Reference(anfang));
        inhalte.extend(bisher);
        inhalte.push(Object::Reference(ebene));
        inc.opt_clone_object_to_new_document(id).map_err(seite_fehler(s.nr))?;
        let seite = inc.new_document.get_object_mut(id).and_then(Object::as_dict_mut).map_err(seite_fehler(s.nr))?;
        seite.set("Contents", Object::Array(inhalte));
    }
    let mut neu = Vec::with_capacity(original.len() + 16 * 1024);
    inc.save_to(&mut neu).map_err(|e| format!("Schreiben: {e}"))?;
    if !neu.starts_with(original) {
        return Err("neue Fassung beginnt nicht mit dem unveränderten Original".into());
    }
    Ok(neu)
}

// --- Lauf über das Register ---

/// Eine PDF mit Seiten, deren Textebene geschrieben werden soll.
#[derive(Debug, Clone, PartialEq)]
pub struct TextebeneAuftrag {
    pub id: i64,
    pub pfad: String,
    pub nur_online: bool,
    pub geaendert: i64,
    pub groesse: Option<i64>,
    pub seiten: Vec<u32>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TextebeneErgebnis {
    pub dauer_ms: u64,
    pub probelauf: bool,
    /// Ordner der Kopien im Probelauf.
    pub ziel: Option<String>,
    /// PDFs mit ausstehenden Seiten zu Beginn des Laufs.
    pub ausstehend: u64,
    pub geschrieben: u64,
    pub seiten: u64,
    pub fehler: u64,
    pub nur_online: u64,
    /// Übergangen, weil sich die Datei seit dem Einlesen geändert hat.
    pub veraendert: u64,
    pub abgebrochen: bool,
    /// Größe der geschriebenen Dateien vorher und nachher, in Byte.
    pub bytes_vorher: u64,
    pub bytes_nachher: u64,
    /// Fehler dieses Laufs, höchstens 50.
    pub fehler_liste: Vec<crate::register::FehlerZeile>,
}

/// Stand der Textebene im Register.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TextebeneStand {
    pub dateien_ausstehend: u64,
    pub seiten_ausstehend: u64,
    pub dateien_ausstehend_online: u64,
    pub dateien_geschrieben: u64,
    pub seiten_in_pdf: u64,
    pub fehler: u64,
    pub fehler_liste: Vec<crate::register::FehlerZeile>,
}

/// Seiten, die eine Textebene bekommen: Scan, ohne Text, erkannter Text
/// übernommen, Lage der Wörter vorhanden, noch nicht in der PDF.
fn bedingung_seite() -> String {
    format!(
        "o.uebernommen = 1 AND o.in_pdf = 0 AND o.woerter IS NOT NULL
         AND s.zeichen < {min} AND s.bildanteil >= {scan}",
        min = guete::MIN_ZEICHEN,
        scan = pdf::SCAN_FLAECHE
    )
}

/// PDFs, deren Register-Stand zur Datei passt und deren OCR aktuell ist.
pub(crate) fn bedingung_datei() -> String {
    format!(
        "d.endung = 'pdf' AND d.textebene_fehler IS NULL
         AND d.ocr_verfahren = {OCR_VERFAHREN} AND d.ocr_fehler IS NULL
         AND d.ocr_geaendert IS d.inhalt_geaendert AND d.inhalt_geaendert IS d.geaendert
         AND (d.groesse IS NULL OR d.inhalt_groesse IS d.groesse)
         AND EXISTS (SELECT 1 FROM ocr o JOIN seite s ON s.datei = o.datei AND s.nr = o.nr
                     WHERE o.datei = d.id AND {})",
        bedingung_seite()
    )
}

impl Register {
    pub fn textebene_ausstehend(&self) -> Ergebnis<Vec<TextebeneAuftrag>> {
        let sql = format!(
            "SELECT d.id, d.pfad, d.nur_online, d.geaendert, d.groesse FROM datei d WHERE {} ORDER BY d.pfad",
            bedingung_datei()
        );
        let mut liste = self
            .db
            .prepare(&sql)?
            .query_map([], |z| {
                Ok(TextebeneAuftrag {
                    id: z.get(0)?,
                    pfad: z.get(1)?,
                    nur_online: z.get(2)?,
                    geaendert: z.get(3)?,
                    groesse: z.get(4)?,
                    seiten: Vec::new(),
                })
            })?
            .filter(|a| a.as_ref().map_or(true, |a| self.in_auswahl(a.id)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seiten = self.db.prepare(&format!(
            "SELECT o.nr FROM ocr o JOIN seite s ON s.datei = o.datei AND s.nr = o.nr
             WHERE o.datei = ?1 AND {} ORDER BY o.nr",
            bedingung_seite()
        ))?;
        for a in &mut liste {
            a.seiten = seiten.query_map([a.id], |z| z.get(0))?.collect::<Result<Vec<_>, _>>()?;
        }
        Ok(liste)
    }

    /// Wortlagen der genannten Seiten einer Datei.
    pub fn textebene_seiten(&self, id: i64, seiten: &[u32]) -> Ergebnis<Result<Vec<SeitenEbene>, String>> {
        let mut abfrage = self.db.prepare("SELECT woerter FROM ocr WHERE datei = ?1 AND nr = ?2")?;
        let mut liste = Vec::with_capacity(seiten.len());
        for &nr in seiten {
            let block: Option<Vec<u8>> = abfrage.query_row(params![id, nr], |z| z.get(0))?;
            let Some(block) = block else { return Ok(Err(format!("Seite {nr}: Lage der Wörter fehlt"))) };
            match woerter::entpacken(&block) {
                Ok(w) => liste.push(SeitenEbene { nr, woerter: w }),
                Err(e) => return Ok(Err(format!("Seite {nr}: {e}"))),
            }
        }
        Ok(Ok(liste))
    }

    pub fn textebene_fehler_speichern(&mut self, id: i64, meldung: &str) -> Ergebnis<()> {
        self.db.execute("UPDATE datei SET textebene_fehler = ?2 WHERE id = ?1", params![id, meldung])?;
        Ok(())
    }

    /// Führt das Register nach dem Austausch einer Datei nach. Die Datei hat
    /// jetzt die Größe `groesse` und das Änderungsdatum `geaendert`. Alle
    /// Stände, die zur alten Fassung passten, gelten für die neue. Der Text
    /// der Seiten wird aus `inhalt` übernommen, die Erkennung bleibt.
    pub fn textebene_eintragen(
        &mut self,
        id: i64,
        geaendert: i64,
        groesse: u64,
        inhalt: &Inhalt,
        seiten: &[u32],
    ) -> Ergebnis<()> {
        let tx = self.db.transaction()?;
        let (alt_g, alt_gr): (i64, Option<i64>) =
            tx.query_row("SELECT geaendert, groesse FROM datei WHERE id = ?1", [id], |z| Ok((z.get(0)?, z.get(1)?)))?;
        let groesse = groesse as i64;
        tx.execute(
            "UPDATE datei SET groesse = ?2, geaendert = ?3, nur_online = 0,
                inhalt_geaendert = ?3, inhalt_groesse = ?2,
                vorschau_groesse   = CASE WHEN vorschau_geaendert IS ?4 AND vorschau_groesse IS ?5 THEN ?2 ELSE vorschau_groesse END,
                vorschau_geaendert = CASE WHEN vorschau_geaendert IS ?4 AND vorschau_groesse IS ?5 THEN ?3 ELSE vorschau_geaendert END,
                ocr_geaendert = ?3, ocr_groesse = ?2,
                textebene_geschrieben = ?6, textebene_fehler = NULL
             WHERE id = ?1",
            params![id, groesse, geaendert, alt_g, alt_gr, jetzt()],
        )?;
        {
            let mut text = tx.prepare(
                "UPDATE seite SET text = ?3, zeichen = ?4, tokens = ?5, wortartig = ?6 WHERE datei = ?1 AND nr = ?2",
            )?;
            for s in &inhalt.seiten {
                let z = guete::zaehlen(&s.text);
                text.execute(params![id, s.nr, s.text, z.zeichen as i64, z.tokens as i64, z.wortartig as i64])?;
            }
            let mut ocr = tx.prepare("UPDATE ocr SET in_pdf = 1, woerter = NULL WHERE datei = ?1 AND nr = ?2")?;
            for nr in seiten {
                ocr.execute(params![id, nr])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn textebene_stand(&self) -> Ergebnis<TextebeneStand> {
        let zahl = |z: &rusqlite::Row, i: usize| -> rusqlite::Result<u64> {
            Ok(z.get::<_, Option<i64>>(i)?.unwrap_or(0) as u64)
        };
        let mut s = TextebeneStand::default();
        (s.dateien_ausstehend, s.dateien_ausstehend_online) = self.db.query_row(
            &format!("SELECT count(*), sum(d.nur_online) FROM datei d WHERE {}", bedingung_datei()),
            [],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?)),
        )?;
        s.seiten_ausstehend = self.db.query_row(
            &format!(
                "SELECT count(*) FROM ocr o JOIN seite s ON s.datei = o.datei AND s.nr = o.nr
                 JOIN datei d ON d.id = o.datei WHERE {} AND {}",
                bedingung_datei(),
                bedingung_seite()
            ),
            [],
            |z| zahl(z, 0),
        )?;
        (s.dateien_geschrieben, s.fehler) = self.db.query_row(
            "SELECT sum(textebene_geschrieben IS NOT NULL), sum(textebene_fehler IS NOT NULL) FROM datei",
            [],
            |z| Ok((zahl(z, 0)?, zahl(z, 1)?)),
        )?;
        s.seiten_in_pdf = self.db.query_row("SELECT sum(in_pdf) FROM ocr", [], |z| zahl(z, 0))?;
        s.fehler_liste = self
            .db
            .prepare("SELECT pfad, textebene_fehler FROM datei WHERE textebene_fehler IS NOT NULL ORDER BY pfad LIMIT 50")?
            .query_map([], |z| Ok(crate::register::FehlerZeile { pfad: z.get(0)?, meldung: z.get(1)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(s)
    }
}

fn jetzt() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn sekunden(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Zeitpunkt als `JJJJ-MM-TT hh:mm:ss UTC`.
fn zeitpunkt(s: i64) -> String {
    let t = s.rem_euclid(86_400);
    format!("{} {:02}:{:02}:{:02} UTC", crate::datum::aus_zeitstempel(s), t / 3600, t % 3600 / 60, t % 60)
}

/// Prüft die neue Fassung `neu` gegen das Original `alt` und liest ihren
/// Inhalt. `seiten` sind die Seiten mit neuer Textebene.
pub type Pruefer<'a> = dyn Fn(&Path, &Path, &[u32]) -> Result<Inhalt, String> + 'a;

/// Name der temporären Datei neben dem Original. Der Cloud-Client übergeht
/// Dateien, die mit `.~` beginnen.
fn temporaer(pfad: &Path) -> PathBuf {
    let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    pfad.with_file_name(format!(".~registerwerk-{name}.tmp"))
}

/// Ergebnis einer Datei.
enum Ausgang {
    Geschrieben { vorher: u64, nachher: u64 },
    Uebergangen { veraendert: bool },
    Fehler(String),
}

/// Schreibt die Textebene aller ausstehenden PDFs.
///
/// - `pruefer` prüft jede neue Fassung (in der Anwendung mit PDFium).
/// - `probelauf`: Ordner für Kopien. Dann bleiben Originale und Register
///   unberührt.
/// - `protokoll`: Datei, an die je geänderter Datei eine Zeile angehängt wird.
/// - `hoechstens`: endet, sobald so viele Dateien geschrieben sind. Zum
///   Ausprobieren an einer einzelnen Datei.
#[allow(clippy::too_many_arguments)]
pub fn schreiben(
    konfig: &Konfiguration,
    register: &mut Register,
    pruefer: &Pruefer,
    probelauf: Option<&Path>,
    protokoll: Option<&Path>,
    hoechstens: Option<u64>,
    mut fortschritt: impl FnMut(&Fortschritt),
    abbrechen: &AtomicBool,
) -> Ergebnis<TextebeneErgebnis> {
    let start = Instant::now();
    let liste = register.textebene_ausstehend()?;
    let mut e = TextebeneErgebnis {
        ausstehend: liste.len() as u64,
        probelauf: probelauf.is_some(),
        ziel: probelauf.map(|p| p.display().to_string()),
        ..Default::default()
    };
    for (i, a) in liste.iter().enumerate() {
        if hoechstens.is_some_and(|n| e.geschrieben >= n) {
            break;
        }
        if abbrechen.load(Ordering::Relaxed) {
            e.abgebrochen = true;
            break;
        }
        let ausgang = match register.textebene_seiten(a.id, &a.seiten)? {
            Err(f) => Ausgang::Fehler(f),
            Ok(seiten) => datei_schreiben(konfig, register, a, &seiten, pruefer, probelauf, protokoll)?,
        };
        match ausgang {
            Ausgang::Geschrieben { vorher, nachher } => {
                e.geschrieben += 1;
                e.seiten += a.seiten.len() as u64;
                e.bytes_vorher += vorher;
                e.bytes_nachher += nachher;
            }
            Ausgang::Uebergangen { veraendert: true } => e.veraendert += 1,
            Ausgang::Uebergangen { veraendert: false } => e.nur_online += 1,
            Ausgang::Fehler(f) => {
                e.fehler += 1;
                if e.fehler_liste.len() < 50 {
                    e.fehler_liste.push(crate::register::FehlerZeile { pfad: a.pfad.clone(), meldung: f.clone() });
                }
                if probelauf.is_none() {
                    register.textebene_fehler_speichern(a.id, &f)?;
                }
            }
        }
        fortschritt(&Fortschritt { erledigt: i as u64 + 1, gesamt: e.ausstehend, datei: a.pfad.clone() });
    }
    e.dauer_ms = start.elapsed().as_millis() as u64;
    Ok(e)
}

fn datei_schreiben(
    konfig: &Konfiguration,
    register: &mut Register,
    a: &TextebeneAuftrag,
    seiten: &[SeitenEbene],
    pruefer: &Pruefer,
    probelauf: Option<&Path>,
    protokoll: Option<&Path>,
) -> Ergebnis<Ausgang> {
    let pfad = konfig.ort_oder_nichts(&a.pfad);
    let Ok(meta) = std::fs::metadata(&pfad) else { return Ok(Ausgang::Uebergangen { veraendert: true }) };
    if a.nur_online || meta.len() == 0 || crate::dateibaum::platzhalter(&pfad, &meta) {
        return Ok(Ausgang::Uebergangen { veraendert: false });
    }
    let Ok(mtime) = meta.modified() else { return Ok(Ausgang::Fehler("Änderungsdatum nicht lesbar".into())) };
    if sekunden(mtime) != a.geaendert || a.groesse.is_some_and(|g| g as u64 != meta.len()) {
        return Ok(Ausgang::Uebergangen { veraendert: true });
    }
    let original = match std::fs::read(&pfad) {
        Ok(b) => b,
        Err(f) => return Ok(Ausgang::Fehler(format!("Lesen: {f}"))),
    };
    if original.len() as u64 != meta.len() {
        return Ok(Ausgang::Uebergangen { veraendert: true });
    }
    let neu = match einbetten(&original, seiten) {
        Ok(n) => n,
        Err(f) => return Ok(Ausgang::Fehler(f)),
    };
    let ziel = match probelauf {
        Some(ordner) => ordner.join(&a.pfad),
        None => pfad.clone(),
    };
    if let Some(eltern) = ziel.parent() {
        std::fs::create_dir_all(eltern)?;
    }
    let tmp = temporaer(&ziel);
    let geschrieben = (|| -> std::io::Result<()> {
        // Neu anlegen statt überschreiben (v0.1.4): Ein Rest eines
        // abgebrochenen Laufs wird entfernt, eine Verknüpfung unter dem Namen
        // wird nicht verfolgt (`remove_file` entfernt nur die Verknüpfung)
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(&neu)?;
        f.sync_all()?;
        // Änderungsdatum wie beim Original. Das Umbenennen behält es bei.
        f.set_modified(mtime)?;
        Ok(())
    })();
    let verwerfen = |tmp: &Path| {
        let _ = std::fs::remove_file(tmp);
    };
    if let Err(f) = geschrieben {
        verwerfen(&tmp);
        return Ok(Ausgang::Fehler(format!("Temporäre Datei: {f}")));
    }
    let inhalt = match pruefer(&pfad, &tmp, &a.seiten) {
        Ok(i) => i,
        Err(f) => {
            verwerfen(&tmp);
            return Ok(Ausgang::Fehler(format!("Prüfung: {f}")));
        }
    };
    // Kurz vor dem Austausch: Ist das Original noch dasselbe?
    let noch_gleich = std::fs::metadata(&pfad)
        .ok()
        .is_some_and(|m| m.len() == meta.len() && m.modified().ok() == Some(mtime));
    if !noch_gleich {
        verwerfen(&tmp);
        return Ok(Ausgang::Uebergangen { veraendert: true });
    }
    if let Err(f) = std::fs::rename(&tmp, &ziel) {
        verwerfen(&tmp);
        return Ok(Ausgang::Fehler(format!("Austausch: {f}")));
    }
    let nachher = neu.len() as u64;
    if probelauf.is_some() {
        return Ok(Ausgang::Geschrieben { vorher: original.len() as u64, nachher });
    }
    // Ab hier ist das Original ersetzt
    let geaendert = std::fs::metadata(&pfad).ok().and_then(|m| m.modified().ok()).map(sekunden).unwrap_or(a.geaendert);
    if let Some(datei) = protokoll {
        let seiten_text: Vec<String> = a.seiten.iter().map(u32::to_string).collect();
        let zeile = format!(
            "{}\t{}\tSeiten {}\t{} -> {} Byte\n",
            zeitpunkt(jetzt()),
            a.pfad,
            seiten_text.join(","),
            original.len(),
            nachher
        );
        std::fs::OpenOptions::new().create(true).append(true).open(datei)?.write_all(zeile.as_bytes())?;
    }
    register.textebene_eintragen(a.id, geaendert, nachher, &inhalt, &a.seiten)?;
    Ok(Ausgang::Geschrieben { vorher: original.len() as u64, nachher })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wort(text: &str, links: f32, rechts: f32, zeilenende: bool) -> LageWort {
        LageWort { text: text.into(), links, oben: 0.10, rechts, unten: 0.12, grund_links: 0.115, grund_rechts: 0.115, zeilenende }
    }

    #[test]
    fn abbildung_je_drehung() {
        let k = [0.0, 0.0, 600.0, 800.0];
        let a = Abbildung::neu(k, 0).unwrap();
        assert_eq!(a.punkt(0.0, 0.0), (0.0, 800.0));
        assert_eq!(a.punkt(1.0, 1.0), (600.0, 0.0));
        // 90° im Uhrzeigersinn: links oben im Bild ist links unten auf der Seite
        let a = Abbildung::neu(k, 90).unwrap();
        assert_eq!(a.punkt(0.0, 0.0), (0.0, 0.0));
        assert_eq!(a.punkt(1.0, 0.0), (0.0, 800.0));
        assert_eq!(a.punkt(0.0, 1.0), (600.0, 0.0));
        let a = Abbildung::neu(k, 180).unwrap();
        assert_eq!(a.punkt(0.0, 0.0), (600.0, 0.0));
        let a = Abbildung::neu(k, -90).unwrap();
        assert_eq!(a.punkt(0.0, 0.0), (600.0, 800.0));
        assert_eq!(a.punkt(1.0, 0.0), (600.0, 0.0));
        assert!(Abbildung::neu(k, 45).is_err());
        assert!(Abbildung::neu([0.0, 0.0, 0.0, 10.0], 0).is_err());
    }

    #[test]
    fn inhaltsstrom_unsichtbar_und_gestreckt() {
        let a = Abbildung::neu([0.0, 0.0, 600.0, 800.0], 0).unwrap();
        let s = String::from_utf8(inhaltsstrom(&[wort("Grüße", 0.1, 0.3, false), wort("€", 0.4, 0.45, true)], &a, "RwOcr"))
            .unwrap();
        assert!(s.starts_with("Q\nBT\n3 Tr\n"), "{s}");
        assert!(s.ends_with("ET\n"));
        // "Grüße " in UTF-16BE, mit Leerzeichen, weil nicht am Zeilenende
        assert!(s.contains("<0047007200FC00DF00650020> Tj"), "{s}");
        assert!(s.contains("<20AC> Tj"), "{s}");
        // Grundlinie bei v = 0.115: y = 800 - 92 = 708, x = 60; Höhe 0.015 × 800 = 12
        assert!(s.contains("/RwOcr 12 Tf"), "{s}");
        assert!(s.contains("1 0 0 1 60 708 Tm"), "{s}");
        // Breite 120 pt für 5 Zeichen à 6 pt: Streckung 400 %
        assert!(s.contains(" 400 Tz"), "{s}");
    }

    // --- Einbetten in eine echte PDF ---

    /// Eine Scan-PDF wie aus der Scan-App: je Seite ein Graustufenbild über
    /// die ganze Fläche, kein Text. `drehung` setzt `/Rotate` der ersten
    /// Seite. Die zweite Seite hat gemeinsame Ressourcen mit der ersten und
    /// einen Inhalt als Feld.
    pub(crate) fn scan_pdf(drehung: i64) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pixel: Vec<u8> = (0..200 * 280).map(|i| if i % 7 == 0 { 0 } else { 255 }).collect();
        let bild = doc.add_object(komprimiert(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 200, "Height" => 280,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            &pixel,
        ));
        let ressourcen = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild } });
        let inhalt = doc.add_object(Stream::new(Dictionary::new(), b"q 595 0 0 842 0 0 cm /Im0 Do Q".to_vec()));
        let teil1 = doc.add_object(Stream::new(Dictionary::new(), b"q 595 0 0 842 0 0 cm".to_vec()));
        let teil2 = doc.add_object(Stream::new(Dictionary::new(), b"/Im0 Do Q".to_vec()));
        let seiten_id = doc.new_object_id();
        let s1 = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => seiten_id, "Contents" => inhalt, "Resources" => ressourcen,
            "Rotate" => drehung,
        });
        let s2 = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => seiten_id, "Contents" => vec![teil1.into(), teil2.into()],
            "Resources" => ressourcen,
        });
        doc.objects.insert(
            seiten_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![s1.into(), s2.into()], "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let katalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => seiten_id });
        doc.trailer.set("Root", katalog);
        let mut b = Vec::new();
        doc.save_to(&mut b).unwrap();
        b
    }

    /// Ändert die Test-PDF: `aendern` bekommt das Dokument und die erste Seite.
    fn geaendert(aendern: impl Fn(&mut Document, ObjectId)) -> Vec<u8> {
        let mut doc = Document::load_mem(&scan_pdf(0)).unwrap();
        let seite = *doc.get_pages().get(&1).unwrap();
        aendern(&mut doc, seite);
        let mut b = Vec::new();
        doc.save_to(&mut b).unwrap();
        b
    }

    /// CropBox begrenzt die Seite, ohne MediaBox ist die Seite unbrauchbar (v0.1.2).
    #[test]
    fn cropbox_und_fehlende_mediabox() {
        let mit_crop = geaendert(|doc, seite| {
            let d = doc.get_object_mut(seite).and_then(Object::as_dict_mut).unwrap();
            d.set("CropBox", vec![600.into(), 800.into(), 50.into(), 40.into()]);
        });
        assert!(einbetten(&mit_crop, &[ebene(1)]).is_ok());
        // Ausschnitt außerhalb der MediaBox: es gilt die MediaBox
        let daneben = geaendert(|doc, seite| {
            let d = doc.get_object_mut(seite).and_then(Object::as_dict_mut).unwrap();
            d.set("CropBox", vec![700.into(), 900.into(), 800.into(), 1000.into()]);
        });
        assert!(einbetten(&daneben, &[ebene(1)]).is_ok());
        let ohne = geaendert(|doc, _| {
            for (_, o) in doc.objects.iter_mut() {
                if let Ok(d) = o.as_dict_mut() {
                    d.remove(b"MediaBox");
                }
            }
        });
        assert_eq!(einbetten(&ohne, &[ebene(1)]).unwrap_err(), "Seite 1: Seite ohne MediaBox");
        assert!(einbetten(&scan_pdf(0), &[ebene(9)]).unwrap_err().contains("Seite 9 fehlt"));
    }

    pub(crate) fn ebene(nr: u32) -> SeitenEbene {
        SeitenEbene {
            nr,
            woerter: vec![
                wort("Rechnung", 0.1, 0.3, false),
                wort("über", 0.32, 0.4, false),
                wort("123,45", 0.42, 0.55, true),
                LageWort { text: "Stuttgart".into(), links: 0.1, oben: 0.2, rechts: 0.3, unten: 0.22, grund_links: 0.215, grund_rechts: 0.215, zeilenende: true },
            ],
        }
    }

    #[test]
    fn einbetten_haengt_nur_an() {
        let original = scan_pdf(0);
        let neu = einbetten(&original, &[ebene(1), ebene(2)]).unwrap();
        assert!(neu.starts_with(&original));
        assert!(neu.len() - original.len() < 8000, "{}", neu.len() - original.len());
        let doc = Document::load_mem(&neu).unwrap();
        let seiten = doc.get_pages();
        for (nr, anzahl) in [(1u32, 3usize), (2, 4)] {
            let seite = doc.get_object(seiten[&nr]).unwrap().as_dict().unwrap();
            let inhalte = seite.get(b"Contents").unwrap().as_array().unwrap();
            assert_eq!(inhalte.len(), anzahl, "Seite {nr}");
        }
        // Gemeinsame Ressourcen: die Schrift steht einmal darin
        let r = doc.get_object(seiten[&1]).unwrap().as_dict().unwrap().get(b"Resources").unwrap().as_reference().unwrap();
        let fonts = doc.get_object(r).unwrap().as_dict().unwrap().get(b"Font").unwrap().as_dict().unwrap();
        assert_eq!(fonts.len(), 1);
        assert!(fonts.has(SCHRIFT_NAME.as_bytes()));
        // Ein zweites Einbetten hängt eine weitere Änderung an
        let noch = einbetten(&neu, &[ebene(1)]).unwrap();
        assert!(noch.starts_with(&neu));
        assert!(einbetten(&original, &[ebene(3)]).is_err());
        assert!(einbetten(b"%PDF-1.4 kaputt", &[ebene(1)]).is_err());
    }

    /// Prüft die neue Fassung mit PDFium wie in der Anwendung.
    #[test]
    fn neue_fassung_besteht_die_pruefung() {
        let pdfium = crate::testumgebung::pdfium();
        let tmp = tempfile::tempdir().unwrap();
        for drehung in [0, 90, 180, 270] {
            let alt = tmp.path().join(format!("alt{drehung}.pdf"));
            let neu = tmp.path().join(format!("neu{drehung}.pdf"));
            let original = scan_pdf(drehung);
            std::fs::write(&alt, &original).unwrap();
            std::fs::write(&neu, einbetten(&original, &[ebene(1), ebene(2)]).unwrap()).unwrap();
            let inhalt = pdf::textebene_pruefen(pdfium, &alt, &neu, &[1, 2]).unwrap();
            for s in &inhalt.seiten {
                assert!(s.text.contains("Rechnung über 123,45"), "Drehung {drehung}, Seite {}: {:?}", s.nr, s.text);
                assert!(s.text.contains("Stuttgart"));
            }
            // Die Wörter liegen dort, wo sie erkannt wurden: "Rechnung" links oben
            let doc = pdfium.load_pdf_from_file(&neu, None).unwrap();
            let seite = doc.pages().get(0).unwrap();
            let text = seite.text().unwrap();
            let r = text.chars().iter().find(|c| c.unicode_char() == Some('R')).unwrap().loose_bounds().unwrap();
            let (x, y) = (r.left().value, r.bottom().value);
            let erwartet = Abbildung::neu([0.0, 0.0, 595.0, 842.0], drehung).unwrap().punkt(0.1, 0.115);
            assert!((x as f64 - erwartet.0).abs() < 15.0 && (y as f64 - erwartet.1).abs() < 15.0 || drehung != 0,
                "Drehung {drehung}: ({x}, {y}) statt {erwartet:?}");
        }
        // Eine Seite mit Text darf keine weitere Textebene bekommen
        let mit_text = tmp.path().join("neu0.pdf");
        let nochmal = tmp.path().join("nochmal.pdf");
        std::fs::write(&nochmal, einbetten(&std::fs::read(&mit_text).unwrap(), &[ebene(1)]).unwrap()).unwrap();
        let f = pdf::textebene_pruefen(pdfium, &mit_text, &nochmal, &[1]).unwrap_err();
        assert!(f.contains("schon Text"), "{f}");
    }

    // --- Lauf über das Register, Prüfung durch einen Ersatz für PDFium ---

    use crate::ocr::{Fall, OcrErgebnisSeite};
    use crate::pdf::Seite;
    use std::fs;
    use std::time::Duration;

    const GUT: &str = "Sehr geehrter Herr Sieber, anbei die Rechnung über 123,45 EUR.";

    fn testleser(pfad: &Path) -> Result<Inhalt, String> {
        let name = pfad.file_name().unwrap().to_string_lossy().to_string();
        let seite = |nr, text: &str, bildanteil| Seite { nr, text: text.into(), bildanteil };
        let seiten = match name.as_str() {
            // Seite 1 Scan ohne Text, Seite 2 Scan mit Text
            "scan.pdf" | "kaputt.pdf" | "anders.pdf" => vec![seite(1, "", 1.0), seite(2, GUT, 1.0)],
            // digital ohne Text: nur ins Register
            "digital.pdf" => vec![seite(1, "", 0.0), seite(2, "", 0.0)],
            // 1 bis 10 Zeichen gelten als ohne Text
            "nummer.pdf" => vec![seite(1, "- 1 -", 1.0), seite(2, GUT, 1.0)],
            _ => vec![seite(1, GUT, 1.0)],
        };
        Ok(Inhalt { seiten, erstellt: None, grob: false })
    }

    /// Ersatz für die Prüfung mit PDFium: liest den Text aus `testleser` und
    /// setzt auf den geschriebenen Seiten den erkannten Text. Für
    /// `kaputt.pdf` schlägt die Prüfung fehl.
    fn testpruefer(alt: &Path, neu: &Path, seiten: &[u32]) -> Result<Inhalt, String> {
        assert!(fs::read(neu).unwrap().starts_with(&fs::read(alt).unwrap()));
        if alt.ends_with("kaputt.pdf") {
            return Err("Seite 1 sieht in der neuen Fassung anders aus".into());
        }
        let mut inhalt = testleser(alt)?;
        for s in &mut inhalt.seiten {
            if seiten.contains(&s.nr) {
                s.text = "Rechnung über 123,45 Stuttgart".into();
            }
        }
        Ok(inhalt)
    }

    const VORHER: u64 = 1_700_000_000;

    fn aufbauen() -> (tempfile::TempDir, Konfiguration, Register) {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("A");
        fs::create_dir_all(&a).unwrap();
        let pdf = scan_pdf(0);
        for name in ["scan.pdf", "kaputt.pdf", "anders.pdf", "digital.pdf", "nummer.pdf", "gut.pdf"] {
            let pfad = a.join(name);
            fs::write(&pfad, &pdf).unwrap();
            let t = UNIX_EPOCH + Duration::from_secs(VORHER);
            fs::File::options().write(true).open(&pfad).unwrap().set_modified(t).unwrap();
        }
        // Ast "A" liegt im Ordner A: Die Pfade im Register heißen "A/…"
        let konfig = Konfiguration::mit_ast("A", &tmp.path().join("A"));
        let mut register = Register::im_speicher().unwrap();
        crate::dateibaum_einlesen(&konfig, &crate::Regeln::voll(&["A"]), &mut register, true).unwrap();
        crate::inhalte::lesen(&konfig, &mut register, testleser, |_| {}, &AtomicBool::new(false)).unwrap();
        // OCR wie im Lauf: Seiten ohne Text erkannt und übernommen, mit Wortlagen
        for auftrag in register.ocr_ausstehend().unwrap() {
            let seiten = auftrag
                .seiten
                .iter()
                .map(|s| OcrErgebnisSeite {
                    nr: s.nr,
                    fall: Fall::OhneText,
                    text: "Rechnung über 123,45 Stuttgart".into(),
                    zaehlung: guete::zaehlen("Rechnung über 123,45 Stuttgart"),
                    konfidenz: 90,
                    dauer_ms: 1,
                    uebernommen: true,
                    woerter: Some(woerter::packen(&ebene(s.nr).woerter)),
                })
                .collect();
            register
                .ocr_speichern(auftrag.id, auftrag.inhalt_geaendert, auftrag.inhalt_groesse, &Ok(seiten))
                .unwrap();
        }
        (tmp, konfig, register)
    }

    fn lauf(konfig: &Konfiguration, register: &mut Register, probelauf: Option<&Path>, protokoll: Option<&Path>) -> TextebeneErgebnis {
        schreiben(konfig, register, &testpruefer, probelauf, protokoll, None, |_| {}, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn waehlt_nur_scan_seiten_ohne_text() {
        let (_tmp, _konfig, register) = aufbauen();
        let liste = register.textebene_ausstehend().unwrap();
        let pfade: Vec<_> = liste.iter().map(|a| (a.pfad.as_str(), a.seiten.clone())).collect();
        assert_eq!(
            pfade,
            vec![("A/anders.pdf", vec![1]), ("A/kaputt.pdf", vec![1]), ("A/nummer.pdf", vec![1]), ("A/scan.pdf", vec![1])]
        );
        let s = register.stand().unwrap().textebene;
        assert_eq!((s.dateien_ausstehend, s.seiten_ausstehend, s.dateien_geschrieben), (4, 4, 0));
    }

    #[test]
    fn probelauf_laesst_originale_und_register_unberuehrt() {
        let (tmp, konfig, mut register) = aufbauen();
        let ziel = tmp.path().join("probe");
        let original = fs::read(tmp.path().join("A/scan.pdf")).unwrap();
        let e = lauf(&konfig, &mut register, Some(&ziel), None);
        assert!(e.probelauf);
        assert_eq!((e.ausstehend, e.geschrieben, e.fehler, e.seiten), (4, 3, 1, 3));
        assert_eq!(e.fehler_liste[0].pfad, "A/kaputt.pdf");
        assert_eq!(fs::read(tmp.path().join("A/scan.pdf")).unwrap(), original);
        let kopie = fs::read(ziel.join("A/scan.pdf")).unwrap();
        assert!(kopie.starts_with(&original) && kopie.len() > original.len());
        assert!(!ziel.join("A/kaputt.pdf").exists());
        // keine temporären Dateien übrig
        assert_eq!(fs::read_dir(ziel.join("A")).unwrap().count(), 3);
        assert_eq!(register.textebene_ausstehend().unwrap().len(), 4);
        assert_eq!(register.stand().unwrap().textebene.fehler, 0);
    }

    #[test]
    fn schreibt_prueft_tauscht_und_fuehrt_das_register_nach() {
        let (tmp, konfig, mut register) = aufbauen();
        let protokoll = tmp.path().join(PROTOKOLL);
        let scan = tmp.path().join("A/scan.pdf");
        let original = fs::read(&scan).unwrap();
        // anders.pdf ändert sich nach dem Einlesen
        let anders = tmp.path().join("A/anders.pdf");
        fs::write(&anders, b"%PDF neu").unwrap();

        let e = lauf(&konfig, &mut register, None, Some(&protokoll));
        assert!(!e.probelauf);
        assert_eq!((e.geschrieben, e.fehler, e.veraendert, e.seiten), (2, 1, 1, 2));
        assert!(e.bytes_nachher > e.bytes_vorher);

        // Original ersetzt: alte Bytes am Anfang, Änderungsdatum wie vorher
        let neu = fs::read(&scan).unwrap();
        assert!(neu.starts_with(&original) && neu.len() > original.len());
        let mtime = fs::metadata(&scan).unwrap().modified().unwrap();
        assert_eq!(sekunden(mtime), VORHER as i64);
        // kaputt.pdf unverändert, Fehler festgehalten, keine temporären Dateien
        assert_eq!(fs::read(tmp.path().join("A/kaputt.pdf")).unwrap(), original);
        let namen: Vec<String> =
            fs::read_dir(tmp.path().join("A")).unwrap().map(|d| d.unwrap().file_name().to_string_lossy().into()).collect();
        assert!(namen.iter().all(|n| !n.starts_with(".~")), "{namen:?}");

        let protokolliert = fs::read_to_string(&protokoll).unwrap();
        assert_eq!(protokolliert.lines().count(), 2);
        assert!(protokolliert.contains("A/scan.pdf\tSeiten 1\t"), "{protokolliert}");

        // Register: neue Größe, Seite in der PDF, Wortlagen entfernt
        let id: i64 = register.db.query_row("SELECT id FROM datei WHERE pfad = 'A/scan.pdf'", [], |z| z.get(0)).unwrap();
        let (groesse, inhalt_groesse, ocr_groesse, geschrieben): (i64, i64, i64, Option<i64>) = register
            .db
            .query_row(
                "SELECT groesse, inhalt_groesse, ocr_groesse, textebene_geschrieben FROM datei WHERE id = ?1",
                [id],
                |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?, z.get(3)?)),
            )
            .unwrap();
        assert_eq!((groesse, inhalt_groesse, ocr_groesse), (neu.len() as i64, neu.len() as i64, neu.len() as i64));
        assert!(geschrieben.is_some());
        let (in_pdf, woerter): (bool, Option<Vec<u8>>) = register
            .db
            .query_row("SELECT in_pdf, woerter FROM ocr WHERE datei = ?1 AND nr = 1", [id], |z| Ok((z.get(0)?, z.get(1)?)))
            .unwrap();
        assert!(in_pdf && woerter.is_none());
        let zeichen: i64 = register.db.query_row("SELECT zeichen FROM seite WHERE datei = ?1 AND nr = 1", [id], |z| z.get(0)).unwrap();
        assert!(zeichen >= guete::MIN_ZEICHEN as i64);

        let s = register.stand().unwrap();
        assert_eq!((s.textebene.dateien_geschrieben, s.textebene.seiten_in_pdf, s.textebene.fehler), (2, 2, 1));
        assert_eq!(s.textebene.fehler_liste[0].pfad, "A/kaputt.pdf");
        // Nichts gilt als veraltet: Dateibaum unverändert, Inhalt und OCR aktuell
        let abgleich = crate::dateibaum_einlesen(&konfig, &crate::Regeln::voll(&["A"]), &mut register, true).unwrap();
        assert_eq!(abgleich.geaendert, 1, "nur anders.pdf");
        assert!(register.ausstehend().unwrap().iter().all(|a| a.pfad == "A/anders.pdf"));
        assert!(register.ocr_ausstehend().unwrap().is_empty());
        // Zweiter Lauf: nichts mehr zu tun außer der geänderten Datei, die
        // erst neu gelesen und erkannt werden muss
        let e = lauf(&konfig, &mut register, None, Some(&protokoll));
        assert_eq!(e.ausstehend, 0);
    }

    #[test]
    fn hoechstens_eine_datei() {
        let (_tmp, konfig, mut register) = aufbauen();
        let e = schreiben(&konfig, &mut register, &testpruefer, None, None, Some(1), |_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(e.geschrieben, 1);
        // anders.pdf ist die erste Datei und wurde geschrieben, kaputt.pdf nicht versucht
        assert_eq!(e.fehler, 0);
        assert_eq!(register.textebene_ausstehend().unwrap().len(), 3);
    }

    #[test]
    fn nur_online_wird_uebergangen() {
        let (tmp, konfig, mut register) = aufbauen();
        let scan = tmp.path().join("A/scan.pdf");
        crate::dateibaum::platzhalter_anlegen(&scan);
        fs::File::options().write(true).open(&scan).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs(VORHER)).unwrap();
        let e = lauf(&konfig, &mut register, None, None);
        assert_eq!(e.nur_online, 1);
    }

    #[test]
    fn zahlen_kurz() {
        assert_eq!(zahl(1.0), "1");
        assert_eq!(zahl(-0.0001), "0");
        assert_eq!(zahl(12.3456), "12.346");
        assert_eq!(zahl(0.5), "0.5");
    }
}
