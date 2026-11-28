//! Welche Dateitypen gelesen werden und welche eine Vorschau bekommen (v0.0.6).
//!
//! - PDFs: Text je Seite, Vorschau je Seite, OCR, Textebene
//! - Office und Text: nur der Text (`dokumente.rs`, altes Office in
//!   `altoffice.rs`), keine Vorschau
//! - Bilder: eine Seite ohne Text, Vorschau und OCR (`bild.rs`), der erkannte
//!   Text kommt nur ins Register. HEIC und HEIF seit v0.0.7
//! - Textdateien (txt, md, seit v0.0.9 rtf, csv, tsv, log, htm, html, vcf): dazu eine gezeichnete
//!   Vorschau (`textbild.rs`)
//! - alle anderen: nur Name, Ordner, Größe und Datum (seit v0.1.5 auch
//!   Quelltext, Programme und Unbekanntes)
//!
//! Die Listen stehen als Makros, damit sie in SQL-Bedingungen als Text
//! eingesetzt werden können.

/// Endungen, deren Text gelesen wird, als SQL-Liste.
#[macro_export]
macro_rules! lesbar_sql {
    () => {
        "('pdf','docx','docm','dotx','dotm','xlsx','xlsm','xltx','xltm','pptx','pptm','odt','ods','odp','txt','md','rtf','csv','tsv','log','htm','html','vcf','pages','key','doc','dot','xls','xlt','ppt','pps','jpg','jpeg','png','tif','tiff','bmp','gif','webp','heic','heif')"
    };
}

/// Endungen, die eine Vorschau bekommen, als SQL-Liste: PDFs, Bilder und
/// Textdateien (deren Vorschau zeichnet `textbild.rs`).
#[macro_export]
macro_rules! vorschau_sql {
    () => {
        "('pdf','jpg','jpeg','png','tif','tiff','bmp','gif','webp','heic','heif','txt','md','rtf','csv','tsv','log','htm','html','vcf')"
    };
}

/// Endungen, deren Seiten ohne Text erkannt werden (OCR), als SQL-Liste:
/// PDFs und Bilder.
#[macro_export]
macro_rules! ocr_sql {
    () => {
        "('pdf','jpg','jpeg','png','tif','tiff','bmp','gif','webp','heic','heif')"
    };
}

/// Office- und Textformate, deren Text gelesen wird (ohne PDF).
pub const DOKUMENTE: &[&str] = &[
    "docx", "docm", "dotx", "dotm", "xlsx", "xlsm", "xltx", "xltm", "pptx", "pptm", "odt", "ods", "odp", "txt", "md",
    "rtf", "csv", "tsv", "log", "htm", "html", "vcf", "pages", "key", "doc", "dot", "xls", "xlt", "ppt", "pps",
];

/// Bildformate, die eine Vorschau bekommen.
pub const BILDER: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "bmp", "gif", "webp", "heic", "heif"];

/// Textdateien: Ihre Vorschau wird aus dem Text gezeichnet (`textbild.rs`).
/// Seit v0.0.9 auch RTF, Tabellen als Text, Protokolle, HTML und Visitenkarten.
pub const TEXT: &[&str] = &["txt", "md", "rtf", "csv", "tsv", "log", "htm", "html", "vcf"];

/// Wie eine Datei in einem Ordner der Stufe Voll behandelt wird. Bis v0.1.4
/// gab es dazu „ignoriert“ für Quelltext, Programme und Unbekanntes. Seit
/// v0.1.5 kommt jede Datei wenigstens mit Namen ins Register, damit sich auch
/// Programme über den Namen finden lassen (Kim).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Klasse {
    /// Text wird gelesen, PDFs und Bilder bekommen Vorschau und OCR.
    Gelesen,
    /// Nur Name, Ordner, Größe und Datum: Archive, Medien, Quelltext,
    /// Programme und alles Unbekannte.
    NurName,
}

/// Klasse einer Endung (Kleinbuchstaben, ohne Punkt).
pub fn klasse(endung: &str) -> Klasse {
    if endung == "pdf" || DOKUMENTE.contains(&endung) || BILDER.contains(&endung) {
        Klasse::Gelesen
    } else {
        Klasse::NurName
    }
}

fn endung(pfad: &std::path::Path) -> String {
    pfad.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn ist_dokument(pfad: &std::path::Path) -> bool {
    DOKUMENTE.contains(&endung(pfad).as_str())
}

pub fn ist_text(pfad: &std::path::Path) -> bool {
    TEXT.contains(&endung(pfad).as_str())
}

pub fn ist_bild(pfad: &std::path::Path) -> bool {
    BILDER.contains(&endung(pfad).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_stimmen_mit_sql_ueberein() {
        let liste = |sql: &str| -> Vec<String> {
            sql.trim_matches(|c| c == '(' || c == ')').split(',').map(|e| e.trim_matches('\'').to_string()).collect()
        };
        let mut lesbar: Vec<String> = DOKUMENTE.iter().chain(BILDER).map(|s| s.to_string()).collect();
        lesbar.insert(0, "pdf".into());
        assert_eq!(liste(lesbar_sql!()), lesbar);
        let mut ocr: Vec<String> = BILDER.iter().map(|s| s.to_string()).collect();
        ocr.insert(0, "pdf".into());
        assert_eq!(liste(ocr_sql!()), ocr);
        let vorschau: Vec<String> = ocr.iter().cloned().chain(TEXT.iter().map(|s| s.to_string())).collect();
        assert_eq!(liste(vorschau_sql!()), vorschau);
        assert!(TEXT.iter().all(|e| DOKUMENTE.contains(e)) && ist_text(std::path::Path::new("Notiz.TXT")));
        assert!(ist_bild(std::path::Path::new("a/B.JPG")) && ist_dokument(std::path::Path::new("x.Docx")));
    }

    #[test]
    fn klassen() {
        assert_eq!(klasse("pdf"), Klasse::Gelesen);
        assert_eq!(klasse("jpg"), Klasse::Gelesen);
        assert_eq!(klasse("zip"), Klasse::NurName);
        // Quelltext, Programme und Unbekanntes seit v0.1.5 mit Namen
        assert_eq!(klasse("rs"), Klasse::NurName);
        assert_eq!(klasse("exe"), Klasse::NurName);
        assert_eq!(klasse(""), Klasse::NurName);
    }
}
