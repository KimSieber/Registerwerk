//! Datum eines Dokuments (Konzept, Abschnitt 9).
//!
//! Reihenfolge der Quellen:
//! 1. Datum am Anfang des Dateinamens
//! 2. Erstellungsdatum in den PDF-Metadaten
//! 3. Änderungsdatum der Datei

/// Liest ein Datum vom Anfang eines Dateinamens.
///
/// Erkannt werden `JJJJ-MM-TT`, `JJJJ-MM`, `JJJJMMTT` und `JJJJ`. Das Ergebnis
/// ist so genau wie die Angabe: `2020-12-24`, `2020-12` oder `2020`. Die Werte
/// sortieren sich als Text richtig.
pub fn aus_dateiname(name: &str) -> Option<String> {
    let z: Vec<u8> = name.bytes().take(11).collect();
    let ziffern = |von: usize, n: usize| -> Option<u32> {
        let teil = z.get(von..von + n)?;
        if !teil.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(teil).ok()?.parse().ok()
    };
    let ist_ziffer = |i: usize| z.get(i).is_some_and(u8::is_ascii_digit);
    let jahr = ziffern(0, 4)?;
    if !(1900..=2100).contains(&jahr) {
        return None;
    }

    // JJJJ-MM-TT und JJJJ-MM
    if z.get(4) == Some(&b'-') {
        if let Some(monat) = ziffern(5, 2).filter(|m| (1..=12).contains(m)) {
            if z.get(7) == Some(&b'-') && !ist_ziffer(10) {
                if let Some(tag) = ziffern(8, 2).filter(|t| (1..=tage_im_monat(jahr, monat)).contains(t)) {
                    return Some(format!("{jahr:04}-{monat:02}-{tag:02}"));
                }
            }
            if !ist_ziffer(7) {
                return Some(format!("{jahr:04}-{monat:02}"));
            }
        }
        return Some(format!("{jahr:04}"));
    }

    // JJJJMMTT
    if let (Some(monat), Some(tag)) = (ziffern(4, 2), ziffern(6, 2)) {
        if !ist_ziffer(8) && (1..=12).contains(&monat) && (1..=tage_im_monat(jahr, monat)).contains(&tag) {
            return Some(format!("{jahr:04}-{monat:02}-{tag:02}"));
        }
        return None;
    }

    // JJJJ, danach darf keine weitere Ziffer folgen
    if ist_ziffer(4) {
        return None;
    }
    Some(format!("{jahr:04}"))
}

/// Quelle eines Dokumentdatums, wie sie im Register steht.
pub const QUELLE_DATEINAME: &str = "dateiname";
pub const QUELLE_PDF: &str = "pdf";
pub const QUELLE_GEAENDERT: &str = "geaendert";

/// Liest das Datum aus einem PDF-Datumswert wie `D:20261027135130Z00'00'`.
///
/// Übernommen wird nur der Kalenderteil, so genau wie angegeben. Die Uhrzeit
/// und die Zeitzone werden nicht ausgewertet.
pub fn aus_pdf_datum(wert: &str) -> Option<String> {
    let w = wert.trim();
    let w = w.strip_prefix("D:").unwrap_or(w);
    let z = w.as_bytes();
    let ziffern = |von: usize, n: usize| -> Option<u32> {
        let teil = z.get(von..von + n)?;
        teil.iter().all(u8::is_ascii_digit).then(|| std::str::from_utf8(teil).ok()?.parse().ok())?
    };
    let jahr = ziffern(0, 4).filter(|j| (1900..=2100).contains(j))?;
    let Some(monat) = ziffern(4, 2) else {
        return Some(format!("{jahr:04}"));
    };
    if !(1..=12).contains(&monat) {
        return Some(format!("{jahr:04}"));
    }
    match ziffern(6, 2) {
        Some(tag) if (1..=tage_im_monat(jahr, monat)).contains(&tag) => {
            Some(format!("{jahr:04}-{monat:02}-{tag:02}"))
        }
        _ => Some(format!("{jahr:04}-{monat:02}")),
    }
}

/// Kalendertag eines Zeitstempels (Sekunden seit 1970, UTC) als `JJJJ-MM-TT`.
pub fn aus_zeitstempel(sekunden: i64) -> String {
    // Umrechnung nach H. Hinnant, "civil_from_days"
    let tage = sekunden.div_euclid(86_400);
    let z = tage + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let tag = doy - (153 * mp + 2) / 5 + 1;
    let monat = if mp < 10 { mp + 3 } else { mp - 9 };
    let jahr = yoe + era * 400 + i64::from(monat <= 2);
    format!("{jahr:04}-{monat:02}-{tag:02}")
}

fn tage_im_monat(jahr: u32, monat: u32) -> u32 {
    match monat {
        4 | 6 | 9 | 11 => 30,
        2 if jahr % 4 == 0 && (jahr % 100 != 0 || jahr % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::aus_dateiname as d;

    #[test]
    fn erkennt_datumsformen() {
        assert_eq!(d("2020-12-24 Rechnung.pdf").as_deref(), Some("2020-12-24"));
        assert_eq!(d("2020-12 Hochzeit").as_deref(), Some("2020-12"));
        assert_eq!(d("2018 Bewerbungstraining").as_deref(), Some("2018"));
        assert_eq!(d("20191021_MRT.pdf").as_deref(), Some("2019-10-21"));
        assert_eq!(d("2024-02-29.pdf").as_deref(), Some("2024-02-29"));
        assert_eq!(d("2021.pdf").as_deref(), Some("2021"));
    }

    #[test]
    fn faellt_bei_unsinn_zurueck() {
        assert_eq!(d("2023-02-30 x.pdf").as_deref(), Some("2023-02"));
        assert_eq!(d("2023-13-01 x.pdf").as_deref(), Some("2023"));
        assert_eq!(d("2020-12-241.pdf").as_deref(), Some("2020-12"));
    }

    #[test]
    fn liest_pdf_datum() {
        use super::aus_pdf_datum as p;
        assert_eq!(p("D:20261027135130Z00'00'").as_deref(), Some("2026-10-27"));
        assert_eq!(p("D:20180626160814+02'00'").as_deref(), Some("2018-06-26"));
        assert_eq!(p("20220615").as_deref(), Some("2022-06-15"));
        assert_eq!(p("D:2019").as_deref(), Some("2019"));
        assert_eq!(p("D:201905").as_deref(), Some("2019-05"));
        assert_eq!(p("D:20190231").as_deref(), Some("2019-02"));
        assert_eq!(p("-"), None);
        assert_eq!(p("D:0000"), None);
        assert_eq!(p(""), None);
    }

    #[test]
    fn rechnet_zeitstempel_um() {
        use super::aus_zeitstempel as z;
        assert_eq!(z(0), "1970-01-01");
        assert_eq!(z(951_782_400), "2000-02-29");
        assert_eq!(z(1_794_857_940), "2026-11-16");
        assert_eq!(z(-86_400), "1969-12-31");
    }

    #[test]
    fn erkennt_kein_datum() {
        assert_eq!(d("Rechnung 2020.pdf"), None);
        assert_eq!(d("123456 Vertrag.pdf"), None);
        assert_eq!(d("20201345.pdf"), None);
        assert_eq!(d("1234 alt.pdf"), None);
        assert_eq!(d("Ärztebrief.pdf"), None);
        assert_eq!(d(""), None);
    }
}
