//! Baut Leptonica und Tesseract statisch aus dem Quelltext.
//!
//! Der Quelltext liegt nicht im Repo. `tools/tesseract_holen.sh` lädt ihn nach
//! `bibliotheken/tesseract/quelle/`. Ein anderer Ort lässt sich über die
//! Umgebungsvariable `REGISTERWERK_TESSERACT_QUELLE` angeben.
//!
//! Gebaut wird nur, was Registerwerk braucht: keine Bildformate in Leptonica
//! (die Seiten kommen als Rohdaten), keine Trainingswerkzeuge, kein Netz.
//! Das alte Erkennungsverfahren bleibt eingebaut, weil Tesseract 5.5.2 ohne es
//! nicht übersetzt. Benutzt wird nur LSTM. Alles landet im Build-Ordner von
//! cargo, nichts außerhalb des Projekts.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=REGISTERWERK_TESSERACT_QUELLE");
    let quelle = match std::env::var_os("REGISTERWERK_TESSERACT_QUELLE") {
        Some(o) => PathBuf::from(o),
        None => PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../bibliotheken/tesseract/quelle"),
    };
    let tesseract = quelle.join("tesseract");
    let leptonica = quelle.join("leptonica");
    if !tesseract.join("CMakeLists.txt").is_file() || !leptonica.join("CMakeLists.txt").is_file() {
        panic!(
            "\n\nQuelltext von Tesseract nicht gefunden unter {}.\n\
             Bitte im Projektordner ausführen: bash tools/tesseract_holen.sh\n\n",
            quelle.display()
        );
    }
    // Neu bauen, wenn eine andere Version geladen wurde
    println!("cargo:rerun-if-changed={}", tesseract.join("VERSION").display());
    println!("cargo:rerun-if-changed={}", leptonica.join("CMakeLists.txt").display());

    let leptonica_ziel = cmake::Config::new(&leptonica)
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("BUILD_PROG", "OFF")
        .define("SW_BUILD", "OFF")
        .define("ENABLE_ZLIB", "OFF")
        .define("ENABLE_PNG", "OFF")
        .define("ENABLE_GIF", "OFF")
        .define("ENABLE_JPEG", "OFF")
        .define("ENABLE_TIFF", "OFF")
        .define("ENABLE_WEBP", "OFF")
        .define("ENABLE_OPENJPEG", "OFF")
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        .build();

    let tesseract_ziel = cmake::Config::new(&tesseract)
        .profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("SW_BUILD", "OFF")
        .define("CMAKE_PREFIX_PATH", &leptonica_ziel)
        .define("BUILD_TRAINING_TOOLS", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("INSTALL_CONFIGS", "OFF")
        .define("GRAPHICS_DISABLED", "ON")
        .define("OPENMP_BUILD", "OFF")
        .define("DISABLE_ARCHIVE", "ON")
        .define("DISABLE_CURL", "ON")
        .define("DISABLE_TIFF", "ON")
        .define("ENABLE_CCACHE", "OFF")
        .define("ENABLE_PRECOMPILED_HEADERS", "OFF")
        .define("ENABLE_LTO", "OFF")
        .build();

    // Tesseract vor Leptonica, die Reihenfolge zählt beim statischen Binden
    binden(&tesseract_ziel, "tesseract");
    binden(&leptonica_ziel, "leptonica");

    let ziel = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    match ziel.as_str() {
        "macos" | "ios" => println!("cargo:rustc-link-lib=c++"),
        "windows" => {}
        _ => {
            println!("cargo:rustc-link-lib=stdc++");
            println!("cargo:rustc-link-lib=pthread");
            println!("cargo:rustc-link-lib=m");
        }
    }
}

/// Sucht die statische Bibliothek, deren Name mit `name` beginnt
/// (unter Windows z.B. tesseract55.lib), und bindet sie.
fn binden(ziel: &Path, name: &str) {
    for unterordner in ["lib", "lib64"] {
        let ordner = ziel.join(unterordner);
        let Ok(eintraege) = std::fs::read_dir(&ordner) else { continue };
        for eintrag in eintraege.flatten() {
            let datei = eintrag.file_name().to_string_lossy().to_string();
            let stamm = datei
                .strip_suffix(".a")
                .map(|s| s.strip_prefix("lib").unwrap_or(s))
                .or_else(|| datei.strip_suffix(".lib"));
            if let Some(stamm) = stamm {
                if stamm.starts_with(name) {
                    println!("cargo:rustc-link-search=native={}", ordner.display());
                    println!("cargo:rustc-link-lib=static={stamm}");
                    return;
                }
            }
        }
    }
    panic!("Statische Bibliothek {name} nach dem Bau nicht gefunden in {}", ziel.display());
}
