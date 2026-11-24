<p align="center">
  <img src="docs/icons/png/registerwerk_128.png" alt="Registerwerk" width="128" height="128">
</p>

# Registerwerk

Registerwerk führt ein durchsuchbares Register über ein privates Dokumentenarchiv,
dessen Originale nur noch in der Cloud liegen. Es hält Volltext und lesbare
Vorschauen aller Seiten lokal vor, sodass die Suche ohne Download der Originale
funktioniert.

**Stand:** v0.0.9 (November 2026), im produktiven Einsatz. Registerwerk öffnet
mit der Suche und prüft im Hintergrund, was neu, geändert oder noch offen ist.
Steht etwas an, führt ein Knopf zum Ordnerbaum mit Dateilisten. Lesen und Suchen
ändern das Register nicht, so gleicht die Dropbox nur echte Änderungen ab. Ein
Klick auf „Verarbeiten“ lädt die gewählten Dateien bei Bedarf aus der Dropbox,
führt sie durch alle Schritte und stellt sie danach wieder auf „nur online“.
Im Reiter „Ordner“ wird gewählt, welche
Ordner des Geräts ins Register kommen und wie: voll, nur mit Namen oder gar
nicht. Aus den gewählten Ordnern werden der Dateibaum, der Text jeder PDF-Seite,
der Text von Office-, Pages-, RTF-, HTML- und Textdateien sowie Visitenkarten
(auch sehr alter Formate wie Word 2, Excel 4 oder Lotus 1-2-3) und eine Vorschau jeder PDF-Seite,
jedes Bildes und jeder Textdatei eingelesen. Seiten ohne Text und Bilder werden
per OCR erkannt, in Scans kommt der erkannte Text als unsichtbare Textebene in
die PDF. Die Suche ist fehlertolerant, mit Filtern nach Ordner, Datum und
Dateityp. Mehrere Geräte können nacheinander auf demselben Register arbeiten.

## Warum

Das Archiv ist über die Jahre größer geworden als die Festplatten der Endgeräte.
Dateien, die im Cloud-Ordner auf "nur online" stehen, sind lokal nur Platzhalter:
Die Systemsuche findet ihren Inhalt nicht, und eine Schnellansicht gibt es erst
nach dem Download. Die Suche in der Weboberfläche des Cloud-Anbieters ist langsam
und bindet an genau diesen Anbieter.

Registerwerk soll beides lösen: vollständige Suche über alle Dokumente bei
geringem lokalem Platzbedarf, und Unabhängigkeit vom Cloud-Anbieter, weil dessen
eigene Volltextsuche nicht mehr gebraucht wird.

## Grundsätze

1. **Die Dateien sind die einzige Wahrheit.** Ordnerstruktur und Dateinamen des
   Archivs bleiben unverändert. Registerwerk ist nur eine Schicht darüber.
2. **Die Datenbank enthält nur Kopien und Auszüge.** Sie muss sich jederzeit
   vollständig aus den Dateien neu aufbauen lassen. Eigene Informationen, die
   bei einem Systemwechsel verloren gingen, gibt es darin nicht.
3. **Originale werden nicht verschlechtert.** Keine Neukomprimierung, keine
   Qualitätsreduktion. Einzige Ausnahme: Fehlt einer Scan-Seite die Textebene,
   wird sie per OCR ergänzt und verlustfrei in die Datei eingebettet.
4. **Unabhängig von Betriebssystem und Cloud-Anbieter.** Der Zugriff läuft über
   das normale Dateisystem, nicht über die Schnittstelle eines Anbieters.
5. **Nur ein Schreiber.** Es indiziert immer genau ein Gerät, alle anderen lesen.

## Was das Programm tun soll

- **Indizieren:** Dateibaum durchlaufen, neue und geänderte Dateien erkennen,
  eingebetteten Text auslesen, Vorschauen aller Seiten erzeugen.
- **Suchen:** fehlertolerante Volltextsuche; Dateiname und Pfad zählen stärker
  als der Inhalt; Sortierung nach Anlage- bzw. Speicherdatum.
- **Anzeigen:** Vorschau jeder Seite in einer Auflösung, die normalen Brieftext
  lesbar macht. Für Kleingedrucktes wird das Original aus der Cloud geholt.
- **Original öffnen:** Datei im Cloud-Ordner öffnen; der Download geschieht dabei
  bei Bedarf.

Der Einsatz von KI ist nicht vorgesehen.

## Behandlung von Ordnern und Dateitypen

Ordner bekommen per Regel eine Behandlungsstufe. Ohne Regel wird nichts gelesen.
Eine Regel gilt auch für die Unterordner, die genaueste Regel gewinnt.

| Stufe | Was ins Register kommt | Beispiel |
|---|---|---|
| Voll | Pfad, Name, Text, Vorschau | das eigentliche Archiv |
| Nur Namen | Pfad, Name, Größe, Datum | eBooks, Musik |
| Aus | nichts | Bildordner, die Bilderwerk abdeckt |

In einem Ordner der Stufe Voll entscheidet die Endung: Dokumente und Bilder
werden gelesen, nicht lesbare oder gepackte Formate nur mit Namen erfasst
(Archive werden nicht entpackt), alles andere wird ignoriert, etwa Quelltext.
Bibliotheksordner wie `node_modules` werden übergangen.

Die Ordner können überall auf dem Gerät liegen. Jeder aufgenommene Ordner ist
ein „Ast“ mit einem Namen. Wo er liegt, steht nur in der Konfiguration des
Geräts. Das Register merkt sich, welches Gerät einen Ordner gesehen hat, und
entfernt nichts, nur weil ein Ordner auf einem anderen Gerät fehlt.

Die Regeln stehen in der lesbaren Textdatei `regeln.toml` neben dem Register,
nicht in der Datenbank.

## Unterstützte Cloud-Speicher

**Vollständig unterstützt ist nur die klassische Dropbox-App unter macOS mit
ihrem Ordner unter `~/Dropbox`.** Nur dort lädt Registerwerk „nur online“-Dateien
selbst und stellt sie danach wieder auf „nur online“.

| Cloud-Speicher | Einlesen | Laden durch Registerwerk | Zurück auf „nur online“ |
|---|---|---|---|
| Dropbox, klassische App unter `~/Dropbox` (macOS) | ja | ja | ja, mit [onlineonly](https://github.com/Zamua/onlineonly) |
| Google Drive, OneDrive, iCloud, Dropbox unter `~/Library/CloudStorage` | ja | nicht geprüft | nein |
| Ordner ohne Cloud | ja | nicht nötig | nicht nötig |

Bei den anderen Anbietern erkennt Registerwerk „nur online“-Dateien und lädt sie
nicht ungefragt. Sie lassen sich im Finder offline stellen und dann einlesen.

Für das Zurückstellen auf „nur online“ wird das Werkzeug `onlineonly` (MIT,
inoffiziell) gebraucht: `brew install zamua/tap/onlineonly`. Fehlt es, bleiben
die geladenen Dateien lokal.

## Technik

- **Anwendung:** Tauri (Rust) mit Web-Oberfläche, lauffähig auf macOS, Windows
  und Linux
- **Register:** SQLCipher (verschlüsseltes SQLite) im klassischen Journal,
  Suchindex mit FTS5 und Trigrammen. Der Schlüssel (256 Bit) wird selbst erzeugt
  und verwahrt und liegt je Gerät im Schlüsselbund
- **PDF:** PDFium für Text und Rendering
- **OCR:** Tesseract, nur für Seiten ohne Text
- **Weitere Formate:** Text aus Word, Excel, PowerPoint (auch doc, xls, ppt bis
  zurück zu Word 2, Excel 2 und Lotus 1-2-3), OpenDocument, Pages, Keynote (auch
  '09), RTF, HTML, csv, tsv, log, txt, md und Visitenkarten ohne fremdes
  Programm. Bilder (auch HEIC) bekommen Vorschau und OCR, Textdateien eine
  Vorschau. Was kein Leser versteht, wird grob gelesen und so gekennzeichnet
- **Vorschau:** jede Seite mit 1024 px an der kurzen Kante, AVIF mit Qualität 25,
  kodiert mit libavif und libaom, abgelegt in Vorschau-Scheiben (eigene
  SQLite-Dateien neben dem Register)

Alle Entscheidungen, Messwerte und offenen Punkte stehen im
[Konzept](docs/konzept.md). Stand, Vorgehen und Entscheidungen je Version stehen
in der Entwicklungsdokumentation unter `docs/`:
[v0.0.1](docs/v0.0.1%20Entwicklungsdokumentation.md),
[v0.0.2](docs/v0.0.2%20Entwicklungsdokumentation.md),
[v0.0.3](docs/v0.0.3%20Entwicklungsdokumentation.md),
[v0.0.4](docs/v0.0.4%20Entwicklungsdokumentation.md),
[v0.0.5](docs/v0.0.5%20Entwicklungsdokumentation.md),
[v0.0.6](docs/v0.0.6%20Entwicklungsdokumentation.md),
[v0.0.7](docs/v0.0.7%20Entwicklungsdokumentation.md),
[v0.0.8](docs/v0.0.8%20Entwicklungsdokumentation.md),
[v0.0.9](docs/v0.0.9%20Entwicklungsdokumentation.md),
[v0.1.0](docs/v0.1.0%20Entwicklungsdokumentation.md).

## Aufbau des Repos

```
crates/registerwerk-core/   Kern in Rust: Konfiguration, Ordnerregeln, Dateibaum, PDFs,
                            Office-Formate, Bilder, Vorschauen, OCR, Register, Suche
src-tauri/                  Tauri-Hülle: Fenster und Befehle für die Oberfläche
src/                        Oberfläche (React, TypeScript, Vite): Rahmen, Datenbank-Symbol
src/register/               Reiter „Register“: Abgleich, Ordnerbaum, Verarbeiten
src/suche/                  Suchansicht (Aufbau wie Bilderwerk)
src/ordner/                 Reiter „Ordner“: Baum, Stufen, Ordner-Regeln, Prüfung
config.beispiel.toml        Vorlage für die Konfiguration
docs/                       Dokumentation, darin konzept.md
docs/icons/                 App-Icon in allen Größen (PNG, SVG, ICNS, ICO)
docs/intern/                interne Notizen, nicht versioniert
tools/                      Hilfsskripte: Analyse der Konzeptphase (Python, nur
                            lesend), Dropbox-Ausschluss der Build-Ordner,
                            Laden von PDFium
tools/pdf_pruefen/          Prüfwerkzeug: PDFs mit dem Kern lesen, ohne Register,
                            Tempotest der Vorschauen, Messung der OCR, Probe der
                            Textebene, Messung der Suche an einer Registerkopie
crates/registerwerk-ocr/    Tesseract, statisch aus dem Quelltext gebaut
crates/registerwerk-macos/  macOS: Dateien über die Dateikoordination aus der Cloud anfordern
bibliotheken/               per Skript geladene Bibliotheken, nicht versioniert
```

Der Kern kennt keine Oberfläche. So lässt sich der lesende Teil später für eine
Web-Oberfläche wiederverwenden.

## Entwicklung

Voraussetzungen: Rust (über rustup), Node.js, CMake und unter macOS die Xcode
Command Line Tools. CMake braucht der erste Bau, weil libaom (AV1-Encoder für die
Vorschauen) sowie Tesseract und Leptonica (OCR) aus dem Quelltext übersetzt werden. Unter macOS: `brew install cmake`.
Auf Intel-Rechnern und unter Windows wird zusätzlich NASM gebraucht.

```
bash tools/dropbox_ausschluss.sh     # einmal je Gerät, siehe unten
bash tools/pdfium_holen.sh           # einmal je Gerät: PDFium laden
bash tools/tesseract_holen.sh        # einmal je Gerät: Quelltext und Sprachdaten von Tesseract
cp config.beispiel.toml config.local.toml   # wahlweise, die Anwendung legt sie sonst selbst an
npm install
npm run tauri dev                    # Anwendung im Entwicklungsmodus
npm run tauri dev -- --release       # dasselbe mit Release-Bau, volles Tempo
cargo test -p registerwerk-core -p registerwerk-ocr  # Tests des Kerns und der OCR
npm run tauri build                  # fertige Anwendung bauen
cargo run --release -p pdf_pruefen -- --still <Ordner>       # PDFs prüfen
cargo run --release -p pdf_pruefen -- --vorschau 150 <Ordner> # Tempotest Vorschauen
cargo run --release -p pdf_pruefen -- --ocr 100 <Ordner>     # Messung der OCR
cargo run --release -p pdf_pruefen -- --textebene 20 <Ordner> # Probe der Textebene, Kopien in target/
cargo run --release -p pdf_pruefen -- --suche                # Messung der Suche an einer Registerkopie in target/
cargo run --release -p pdf_pruefen -- --anfrage "Edeka Quittung"  # Suche in dieser Kopie
cargo run --release -p pdf_pruefen -- --aufteilen <Register-Ordner>  # Probe: Scheiben aufteilen an einer Kopie in target/
```

PDFium ist eine C++-Bibliothek (BSD-3-Clause) und liegt nicht im Repo.
`tools/pdfium_holen.sh` lädt einen fertigen Build mit fest eingetragener
Version samt Lizenztexten nach `bibliotheken/pdfium/`.

Liegt das Repo in einem Cloud-Ordner, nimmt `tools/dropbox_ausschluss.sh` die
Build-Ordner (`target`, `node_modules`, `dist`, `src-tauri/gen`) von der
Synchronisation aus. Das Skript sollte vor dem ersten `npm install` laufen.
Cargo baut in `target/cargo` (`.cargo/config.toml`). So löscht `cargo clean`
nur diesen Unterordner, und `target` bleibt mit seiner Ausnahme stehen.

Kern und Abhängigkeiten werden auch im Entwicklungsmodus optimiert übersetzt
(`Cargo.toml`, `[profile.dev.package]`). Sonst wäre das Erzeugen der Vorschauen
um ein Vielfaches langsamer. Für Messungen trotzdem den Release-Bau nehmen.

Das Register liegt ohne weitere Angabe im Datenordner der Anwendung im
Benutzerprofil. Über das Datenbank-Symbol rechts oben lässt sich ein anderer
Ordner wählen. Dort liegen die Index-Datenbank `register.db`, die
Vorschau-Scheiben `vorschau-0001.db` usw. (je rund 200 MB) und die
Ordnerregeln `regeln.toml`. Größere Scheiben älterer Versionen teilt der Knopf
„Scheiben aufteilen“ im Datenbank-Symbol.

Seit v0.1.0 ist das Register verschlüsselt. Den Schlüssel (64 Hex-Zeichen, etwa
aus `openssl rand -hex 32`) erzeugt man selbst und verwahrt ihn im Passwortsafe.
Im Datenbank-Symbol wird er einmal je Gerät eingegeben und im Schlüsselbund
abgelegt (unter macOS ohne iCloud). Ein unverschlüsseltes Register stellt der
Kasten „Verschlüsselung“ dort um. Ohne Schlüssel ist das Register nicht
lesbar, es lässt sich nur aus den Dateien neu aufbauen.

Zugangsdaten, lokale Pfade und andere Interna gehören nicht in versionierte
Dateien, sondern nach `docs/intern/` oder in eine `config.local`-Datei. Beides
ist in der `.gitignore` ausgeschlossen.

## Icon

Das Icon zeigt Karteikarten mit versetzten Registerreitern. Die farbige Karte in
der Mitte steht für den Treffer im Stapel.

| Ordner | Inhalt |
|---|---|
| `docs/icons/png` | 1024 bis 16 px mit Rand (macOS) |
| `docs/icons/png-randlos` | dieselben Größen ohne Rand (Windows, Web) |
| `docs/icons/tauri` | fertige Dateien für den Tauri-Icon-Ordner |
| `docs/icons/svg` | Vektor-Vorlagen |

Farben: Dunkelblau `#1E2B45`, Graublau `#8FA3C8`, Orange `#F2A93B`, Weiß `#FFFFFF`.

## Verwandte Projekte

Registerwerk gehört zur "…werk"-Reihe. Das Vorbild ist Bilderwerk, das dieselbe
Idee für die Fotosammlung umsetzt.
