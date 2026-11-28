//! Toleranter Leser für OLE-Verbunddateien (v0.0.9).
//!
//! Word, Excel und PowerPoint 97 bis 2003 (und Word 6/95) speichern ihre
//! Inhalte in einer Verbunddatei: ein kleines Dateisystem aus Sektoren mit
//! Belegungstabelle (FAT), Verzeichnis und Datenströmen. Die Bibliothek `cfb`
//! prüft diese Struktur streng und lehnt Dateien mit kleinen Fehlern ab, die
//! Word und Excel ohne Murren öffnen (im Archiv 24 Dateien: „Malformed
//! directory entry“, „Malformed MiniFAT“).
//!
//! Dieser Leser liest nur und verzeiht: Ketten enden an der ersten unmöglichen
//! Stelle, ein beschädigter Verzeichnisbaum wird übergangen und der Strom über
//! seinen Namen in der flachen Liste aller Einträge gesucht.

/// Kennung am Anfang jeder Verbunddatei.
pub const KENNUNG: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const FREI: u32 = 0xFFFF_FFFF;
const ENDE: u32 = 0xFFFF_FFFE;
// Eine Kette hat höchstens so viele Glieder, wie Sektoren in der Datei
// stehen (v0.1.5, vorher fest 4 Mio.). Eine Schleife in der FAT liefert so
// nie mehr Daten, als die Datei groß ist.

#[derive(Debug, Clone)]
struct Eintrag {
    name: String,
    /// 1 Speicher, 2 Strom, 5 Wurzel
    typ: u8,
    links: u32,
    rechts: u32,
    kind: u32,
    start: u32,
    groesse: u64,
}

/// Eine geöffnete Verbunddatei im Speicher.
pub struct Verbund {
    daten: Vec<u8>,
    sektor: usize,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    ministrom: Vec<u8>,
    mini_grenze: u64,
    eintraege: Vec<Eintrag>,
}

fn u16_bei(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(i..i + 2)?.try_into().ok()?))
}

fn u32_bei(d: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(i..i + 4)?.try_into().ok()?))
}

impl Verbund {
    /// Liest die Struktur. Fehler nur, wenn es keine Verbunddatei ist oder
    /// der Kopf unbrauchbar ist.
    pub fn oeffnen(daten: Vec<u8>) -> Result<Self, String> {
        if daten.get(..8) != Some(&KENNUNG[..]) {
            return Err("keine OLE-Verbunddatei".into());
        }
        let kaputt = || "OLE-Verbunddatei: Kopf beschädigt".to_string();
        let shift = u16_bei(&daten, 0x1E).ok_or_else(kaputt)?;
        if !(7..=16).contains(&shift) {
            return Err(kaputt());
        }
        let sektor = 1usize << shift;
        let mut v = Verbund {
            daten,
            sektor,
            fat: Vec::new(),
            minifat: Vec::new(),
            ministrom: Vec::new(),
            mini_grenze: 4096,
            eintraege: Vec::new(),
        };
        let d = &v.daten;
        v.mini_grenze = u32_bei(d, 0x38).map(u64::from).filter(|&g| g > 0).unwrap_or(4096);
        let verzeichnis_start = u32_bei(d, 0x30).ok_or_else(kaputt)?;
        let minifat_start = u32_bei(d, 0x3C).ok_or_else(kaputt)?;
        let mut difat_sektor = u32_bei(d, 0x44).ok_or_else(kaputt)?;

        // Sektoren der FAT: 109 im Kopf, weitere in der DIFAT-Kette
        let mut fat_sektoren: Vec<u32> = (0..109).filter_map(|i| u32_bei(d, 0x4C + i * 4)).filter(|&s| s < ENDE).collect();
        let je_sektor = sektor / 4;
        let mut gesehen = 0;
        while difat_sektor < ENDE && gesehen < 10_000 {
            let Some(block) = v.sektor_daten(difat_sektor) else { break };
            fat_sektoren.extend((0..je_sektor - 1).filter_map(|i| u32_bei(block, i * 4)).filter(|&s| s < ENDE));
            difat_sektor = u32_bei(block, (je_sektor - 1) * 4).unwrap_or(ENDE);
            gesehen += 1;
        }
        let mut fat = Vec::with_capacity(fat_sektoren.len() * je_sektor);
        for s in fat_sektoren {
            let Some(block) = v.sektor_daten(s) else { break };
            fat.extend((0..je_sektor).filter_map(|i| u32_bei(block, i * 4)));
        }
        v.fat = fat;

        // Verzeichnis: Einträge zu je 128 Byte
        let verzeichnis = v.kette(verzeichnis_start);
        v.eintraege = verzeichnis
            .chunks_exact(128)
            .map(|e| {
                let laenge = (u16_bei(e, 0x40).unwrap_or(0) as usize).min(64);
                let name: String = char::decode_utf16(
                    e[..laenge.saturating_sub(2)].chunks_exact(2).map(|z| u16::from_le_bytes([z[0], z[1]])),
                )
                .map(|z| z.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
                Eintrag {
                    name,
                    typ: e[0x42],
                    links: u32_bei(e, 0x44).unwrap_or(FREI),
                    rechts: u32_bei(e, 0x48).unwrap_or(FREI),
                    kind: u32_bei(e, 0x4C).unwrap_or(FREI),
                    start: u32_bei(e, 0x74).unwrap_or(ENDE),
                    // Version 3 nutzt nur die unteren 32 Bit
                    groesse: if shift == 9 {
                        u32_bei(e, 0x78).unwrap_or(0) as u64
                    } else {
                        u32_bei(e, 0x78).unwrap_or(0) as u64 | (u32_bei(e, 0x7C).unwrap_or(0) as u64) << 32
                    },
                }
            })
            .collect();

        // Kleine Ströme liegen im Ministrom der Wurzel, verkettet über die MiniFAT
        let minifat = v.kette(minifat_start);
        v.minifat = minifat.chunks_exact(4).map(|z| u32::from_le_bytes([z[0], z[1], z[2], z[3]])).collect();
        if let Some(wurzel) = v.eintraege.iter().find(|e| e.typ == 5).cloned() {
            let mut ministrom = v.kette(wurzel.start);
            ministrom.truncate(wurzel.groesse as usize);
            v.ministrom = ministrom;
        }
        Ok(v)
    }

    fn sektor_daten(&self, nr: u32) -> Option<&[u8]> {
        let von = (nr as usize).checked_add(1)?.checked_mul(self.sektor)?;
        // Der letzte Sektor darf kürzer sein
        let bis = (von + self.sektor).min(self.daten.len());
        self.daten.get(von..bis).filter(|b| !b.is_empty())
    }

    /// Daten einer Kette über die FAT. Endet an der ersten unmöglichen Stelle.
    fn kette(&self, start: u32) -> Vec<u8> {
        let mut aus = Vec::new();
        let mut s = start;
        let mut schritte = 0;
        // Der Kopf zählt mit, die Datei hat also höchstens so viele Sektoren
        let max = self.daten.len() / self.sektor.max(1);
        while s < ENDE && schritte < max {
            let Some(block) = self.sektor_daten(s) else { break };
            aus.extend_from_slice(block);
            s = self.fat.get(s as usize).copied().unwrap_or(ENDE);
            schritte += 1;
        }
        aus
    }

    fn mini_kette(&self, start: u32, groesse: u64) -> Vec<u8> {
        let mut aus = Vec::new();
        let mut s = start;
        let mut schritte = 0;
        let max = self.ministrom.len() / 64;
        while s < ENDE && schritte < max && (aus.len() as u64) < groesse {
            let von = s as usize * 64;
            let Some(block) = self.ministrom.get(von..(von + 64).min(self.ministrom.len())).filter(|b| !b.is_empty())
            else {
                break;
            };
            aus.extend_from_slice(block);
            s = self.minifat.get(s as usize).copied().unwrap_or(ENDE);
            schritte += 1;
        }
        aus
    }

    /// Einträge direkt unter der Wurzel, über den Verzeichnisbaum. Leer, wenn
    /// der Baum beschädigt ist.
    fn unter_wurzel(&self) -> Vec<usize> {
        let mut aus = Vec::new();
        let Some(wurzel) = self.eintraege.iter().find(|e| e.typ == 5) else { return aus };
        let mut offen = vec![wurzel.kind];
        while let Some(i) = offen.pop() {
            let Some(e) = self.eintraege.get(i as usize) else { continue };
            if aus.contains(&(i as usize)) || aus.len() > self.eintraege.len() {
                break;
            }
            aus.push(i as usize);
            offen.push(e.links);
            offen.push(e.rechts);
        }
        aus
    }

    fn finden(&self, name: &str) -> Option<&Eintrag> {
        let gleich = |e: &Eintrag| e.typ == 2 && e.name.eq_ignore_ascii_case(name);
        // Zuerst unter der Wurzel, sonst irgendwo (eingebettete Objekte
        // haben eigene Ströme gleichen Namens, beschädigte Bäume keine Wurzel)
        self.unter_wurzel()
            .into_iter()
            .filter_map(|i| self.eintraege.get(i))
            .find(|e| gleich(e))
            .or_else(|| self.eintraege.iter().find(|e| gleich(e)))
    }

    pub fn hat(&self, name: &str) -> bool {
        self.finden(name).is_some()
    }

    /// Inhalt eines Stroms. Bei beschädigter Kette so viel wie lesbar.
    pub fn strom(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.finden(name)?;
        let mut daten =
            if e.groesse < self.mini_grenze { self.mini_kette(e.start, e.groesse) } else { self.kette(e.start) };
        daten.truncate(e.groesse as usize);
        Some(daten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn liest_was_cfb_schreibt() {
        let tmp = tempfile::tempdir().unwrap();
        let pfad = tmp.path().join("v.ole");
        let gross: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        {
            let mut ole = cfb::create(&pfad).unwrap();
            ole.create_stream("Klein").unwrap().write_all(b"kleiner Strom").unwrap();
            ole.create_stream("Gross").unwrap().write_all(&gross).unwrap();
            ole.create_storage("Objekt").unwrap();
            ole.create_stream("Objekt/Klein").unwrap().write_all(b"anderer").unwrap();
            ole.flush().unwrap();
        }
        let v = Verbund::oeffnen(std::fs::read(&pfad).unwrap()).unwrap();
        assert_eq!(v.strom("Klein").unwrap(), b"kleiner Strom");
        assert_eq!(v.strom("gross").unwrap(), gross);
        assert!(!v.hat("Objekt") && v.strom("fehlt").is_none());
    }

    #[test]
    fn verzeiht_einen_kaputten_verzeichnisbaum() {
        let tmp = tempfile::tempdir().unwrap();
        let pfad = tmp.path().join("v.ole");
        {
            let mut ole = cfb::create(&pfad).unwrap();
            ole.create_stream("WordDocument").unwrap().write_all(b"Text des Dokuments").unwrap();
            ole.flush().unwrap();
        }
        let mut d = std::fs::read(&pfad).unwrap();
        // Kind-Verweis der Wurzel ins Leere zeigen lassen
        let sektor = 512;
        let verzeichnis = u32::from_le_bytes(d[0x30..0x34].try_into().unwrap()) as usize;
        let wurzel = (verzeichnis + 1) * sektor;
        d[wurzel + 0x4C..wurzel + 0x50].copy_from_slice(&0x7777u32.to_le_bytes());
        let v = Verbund::oeffnen(d).unwrap();
        assert_eq!(v.strom("WordDocument").unwrap(), b"Text des Dokuments");
    }
    #[test]
    fn schleife_in_der_fat_endet_an_der_dateigroesse() {
        // Sektor 1 zeigt auf sich selbst: Die Kette darf nicht mehr Daten
        // liefern, als die Datei Sektoren hat (v0.1.5)
        let daten = vec![0u8; 512 * 9];
        let v = Verbund {
            daten,
            sektor: 512,
            fat: vec![1, 1],
            minifat: vec![0],
            ministrom: vec![0u8; 64 * 3],
            mini_grenze: 4096,
            eintraege: Vec::new(),
        };
        assert_eq!(v.kette(0).len(), 512 * 9);
        assert_eq!(v.mini_kette(0, u64::MAX).len(), 64 * 3);
    }
}
