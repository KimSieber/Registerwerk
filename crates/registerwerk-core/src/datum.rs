//! Datum eines Dokuments (Konzept, Abschnitt 9).
//!
//! In dieser Version wird nur die erste Quelle ausgewertet: das Datum am Anfang
//! des Dateinamens. PDF-Erstellungsdatum und Änderungsdatum folgen mit der
//! Verarbeitung der PDFs.

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
    fn erkennt_kein_datum() {
        assert_eq!(d("Rechnung 2020.pdf"), None);
        assert_eq!(d("123456 Vertrag.pdf"), None);
        assert_eq!(d("20201345.pdf"), None);
        assert_eq!(d("1234 alt.pdf"), None);
        assert_eq!(d("Ärztebrief.pdf"), None);
        assert_eq!(d(""), None);
    }
}
