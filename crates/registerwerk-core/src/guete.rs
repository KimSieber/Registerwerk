//! Gütemaß für eingebetteten Text (Konzept, Abschnitt 7).
//!
//! Die Güte ist der Anteil wortartiger Tokens an allen Tokens. Wortartig sind
//! Wörter aus mindestens zwei Buchstaben sowie Zahlen- und Betragsmuster. Das
//! Maß ist grob, trennt aber brauchbaren von unbrauchbarem Text.

/// Unter dieser Güte gilt ein Text als schwach.
pub const SCHWELLE: f64 = 0.6;

/// Ab so vielen sichtbaren Zeichen hat eine Seite Text. Mit höchstens 10
/// Zeichen gilt sie als ohne Text (Kim, 19.11.2026, vorher unter 20).
pub const MIN_ZEICHEN: usize = 11;

/// Zählung für einen Text. Gespeichert werden die Zähler, nicht der Quotient,
/// damit sich Seiten zu einem Dokument zusammenfassen lassen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Zaehlung {
    /// Sichtbare Zeichen (ohne Leerraum).
    pub zeichen: usize,
    pub tokens: usize,
    pub wortartig: usize,
}

impl Zaehlung {
    /// Güte zwischen 0 und 1, `None` wenn der Text keine Tokens hat.
    pub fn guete(&self) -> Option<f64> {
        (self.tokens > 0).then(|| self.wortartig as f64 / self.tokens as f64)
    }

    pub fn ohne_text(&self) -> bool {
        self.zeichen < MIN_ZEICHEN
    }
}

pub fn zaehlen(text: &str) -> Zaehlung {
    let mut z = Zaehlung::default();
    for token in text.split_whitespace() {
        z.zeichen += token.chars().count();
        z.tokens += 1;
        if wortartig(token) {
            z.wortartig += 1;
        }
    }
    z
}

/// Satzzeichen am Rand eines Tokens zählen nicht zum Token.
fn rand(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '!' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '«' | '»'
            | '„' | '“' | '”' | '‚' | '‘' | '’' | '*' | '…'
    )
}

fn wortartig(token: &str) -> bool {
    let t = token.trim_matches(rand);
    if t.is_empty() {
        return false;
    }
    wort(t) || zahl(t)
}

/// Wort: nur Buchstaben, Teile höchstens durch Bindestrich oder Apostroph
/// verbunden, insgesamt mindestens zwei Buchstaben.
fn wort(t: &str) -> bool {
    let mut buchstaben = 0;
    for teil in t.split(['-', '\'', '’']) {
        if teil.is_empty() || !teil.chars().all(char::is_alphabetic) {
            return false;
        }
        buchstaben += teil.chars().count();
    }
    buchstaben >= 2
}

/// Zahl, Datum oder Betrag: Ziffern mit Trennzeichen, optional Vorzeichen,
/// Prozent- oder Währungszeichen. Beispiele: 4711, 12,50, -2.250,00, 01.03.2024,
/// 11:28, 19,0%, 1.249,00€.
fn zahl(t: &str) -> bool {
    let t = t.strip_prefix(['+', '-', '€', '$']).unwrap_or(t);
    let t = t.strip_suffix(['%', '€', '-', '+']).unwrap_or(t);
    let mut ziffern = 0;
    let mut vorher_trenner = true;
    for c in t.chars() {
        if c.is_ascii_digit() {
            ziffern += 1;
            vorher_trenner = false;
        } else if matches!(c, '.' | ',' | '/' | ':' | '-') {
            // keine zwei Trenner hintereinander, nicht am Anfang
            if vorher_trenner {
                return false;
            }
            vorher_trenner = true;
        } else {
            return false;
        }
    }
    ziffern > 0 && !vorher_trenner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erkennt_woerter_und_zahlen() {
        for t in [
            "Rechnung", "Müller", "Straße", "Müller-Lüdenscheid", "(Seite", "Euro.", "„Hallo“", "ab",
            "4711", "12,50", "-2.250,00", "01.03.2024", "11:28", "19,0%", "1.249,00€", "2017-05-04",
        ] {
            assert!(wortartig(t), "{t} sollte wortartig sein");
        }
        for t in ["a", "x7", "§", "--", "a1b2", "1..2", "DE28", "/:", "ÿ¥"] {
            assert!(!wortartig(t), "{t} sollte nicht wortartig sein");
        }
    }

    #[test]
    fn guete_trennt_text_von_zeichensalat() {
        let gut = zaehlen("Sehr geehrter Herr Sieber, anbei die Rechnung Nr. 4711 über 123,45 EUR.");
        assert_eq!(gut.tokens, 12);
        assert!(gut.guete().unwrap() > 0.9);
        assert!(!gut.ohne_text());

        let salat = zaehlen("ÿ§ 3a %$ Ü1 ~~ ¶ q ® 7x ±± ab");
        assert!(salat.guete().unwrap() < SCHWELLE);
    }

    #[test]
    fn leere_seite() {
        let z = zaehlen("  \n 12 ");
        assert!(z.ohne_text());
        assert_eq!(zaehlen("").guete(), None);
    }
}
