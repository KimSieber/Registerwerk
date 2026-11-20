//! Welche Dateitypen gelesen werden und welche eine Vorschau bekommen (v0.0.6).
//!
//! - PDFs: Text je Seite, Vorschau je Seite, OCR, Textebene
//! - Office und Text: nur der Text (`dokumente.rs`), keine Vorschau
//! - Bilder: eine Seite ohne Text, Vorschau und OCR (`bild.rs`), der erkannte
//!   Text kommt nur ins Register
//!
//! Die Listen stehen als Makros, damit sie in SQL-Bedingungen als Text
//! eingesetzt werden können.

/// Endungen, deren Text gelesen wird, als SQL-Liste.
#[macro_export]
macro_rules! lesbar_sql {
    () => {
        "('pdf','docx','docm','dotx','dotm','xlsx','xlsm','xltx','xltm','pptx','pptm','odt','ods','odp','txt','md','pages','key','jpg','jpeg','png','tif','tiff','bmp','gif','webp')"
    };
}

/// Endungen, die eine Vorschau bekommen und erkannt werden (OCR), als SQL-Liste.
#[macro_export]
macro_rules! vorschau_sql {
    () => {
        "('pdf','jpg','jpeg','png','tif','tiff','bmp','gif','webp')"
    };
}

/// Office- und Textformate, deren Text gelesen wird (ohne PDF).
pub const DOKUMENTE: &[&str] = &[
    "docx", "docm", "dotx", "dotm", "xlsx", "xlsm", "xltx", "xltm", "pptx", "pptm", "odt", "ods", "odp", "txt", "md",
    "pages", "key",
];

/// Bildformate, die eine Vorschau bekommen.
pub const BILDER: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "bmp", "gif", "webp"];

fn endung(pfad: &std::path::Path) -> String {
    pfad.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn ist_dokument(pfad: &std::path::Path) -> bool {
    DOKUMENTE.contains(&endung(pfad).as_str())
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
        let mut vorschau: Vec<String> = BILDER.iter().map(|s| s.to_string()).collect();
        vorschau.insert(0, "pdf".into());
        assert_eq!(liste(vorschau_sql!()), vorschau);
        assert!(ist_bild(std::path::Path::new("a/B.JPG")) && ist_dokument(std::path::Path::new("x.Docx")));
    }
}
