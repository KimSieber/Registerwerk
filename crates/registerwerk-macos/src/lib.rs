//! Helfer für macOS (v0.0.8).
//!
//! Die Dropbox (klassischer Client unter `~/Dropbox`) lädt eine "nur
//! online"-Datei nicht, wenn ein Programm sie einfach öffnet und liest. Sie
//! lädt sie, wenn das Programm die Datei über die Dateikoordination von macOS
//! (`NSFileCoordinator`) anfordert, wie es Vorschau und andere Apps tun.
//! Gemessen am 22.11.2026 auf Kims MacBook Air: Lesen mit `head` lud nichts,
//! die Dateikoordination lud die Datei ohne sichtbares Fenster.
//!
//! Das ist ein Weg des Betriebssystems, keine Schnittstelle des Cloud-Anbieters
//! (Konzept, Grundsatz 4).

use std::path::Path;

/// Fordert die Datei über die Dateikoordination an und liest ein Byte, sobald
/// sie freigegeben ist. Blockiert, bis der Cloud-Client sie geliefert hat oder
/// die Koordination scheitert. Unter anderen Systemen geschieht nichts.
#[cfg(target_os = "macos")]
pub fn anfordern(pfad: &Path) -> Result<(), String> {
    use std::io::Read;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2_foundation::{NSError, NSFileCoordinator, NSFileCoordinatorReadingOptions, NSString, NSURL};

    let Some(text) = pfad.to_str() else { return Err("Pfad ist kein gültiges UTF-8".into()) };
    let url = NSURL::fileURLWithPath(&NSString::from_str(text));
    // Ohne eigenen Datei-Beobachter, wie `NSFileCoordinator(filePresenter: nil)`
    let koordinator = NSFileCoordinator::new();
    let mut fehler: Option<objc2::rc::Retained<NSError>> = None;
    let leser = RcBlock::new(|freigegeben: NonNull<NSURL>| {
        // Die Datei ist jetzt lokal. Ein Byte genügt.
        let url = unsafe { freigegeben.as_ref() };
        if let Some(p) = url.path() {
            if let Ok(mut f) = std::fs::File::open(p.to_string()) {
                let mut b = [0u8; 1];
                let _ = f.read(&mut b);
            }
        }
    });
    koordinator.coordinateReadingItemAtURL_options_error_byAccessor(
        &url,
        NSFileCoordinatorReadingOptions::empty(),
        Some(&mut fehler),
        &leser,
    );
    match fehler {
        Some(f) => Err(f.localizedDescription().to_string()),
        None => Ok(()),
    }
}

/// Unter anderen Systemen: nichts zu tun.
#[cfg(not(target_os = "macos"))]
pub fn anfordern(_pfad: &Path) -> Result<(), String> {
    Ok(())
}
