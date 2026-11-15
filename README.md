<p align="center">
  <img src="docs/icons/png/registerwerk_128.png" alt="Registerwerk" width="128" height="128">
</p>

# Registerwerk

Registerwerk führt ein durchsuchbares Register über ein privates Dokumentenarchiv,
dessen Originale nur noch in der Cloud liegen. Es hält Volltext und lesbare
Vorschauen aller Seiten lokal vor, sodass die Suche ohne Download der Originale
funktioniert.

**Stand:** Konzept abgeschlossen (November 2026). Es gibt noch keinen
Programmcode. Die Umsetzung beginnt mit einem Piloten auf einem kleinen Teil
des Archivs.

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
   Qualitätsreduktion. Einzige Ausnahme: Fehlt einer PDF die Textebene, wird sie
   per OCR ergänzt und verlustfrei in die Datei eingebettet.
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

Ordner bekommen per Regel eine Behandlungsstufe. Die genaueste Regel gewinnt.

| Stufe | Was ins Register kommt | Beispiel |
|---|---|---|
| Voll | Pfad, Name, Text, Vorschau | das eigentliche Archiv |
| Nur Dokumente | wie Voll, aber nur pdf, md, txt | Entwicklungsprojekte |
| Nur Metadaten | Pfad, Name, Größe, Datum | eBooks, Musik |
| Ausgeschlossen | nichts | Bildordner, die Bilderwerk abdeckt |

Unabhängig vom Ordner gilt: Nicht lesbare oder gepackte Formate werden nur mit
Metadaten erfasst. Archive werden nicht entpackt.

Die Regeln stehen in einer lesbaren Textdatei neben dem Archiv, nicht nur in der
Datenbank.

## Technik

- **Anwendung:** Tauri (Rust) mit Web-Oberfläche, lauffähig auf macOS, Windows
  und Linux
- **Register:** SQLite mit Volltextindex, verschlüsselt
- **PDF:** PDFium für Text und Rendering
- **OCR:** Tesseract, nur für Seiten ohne oder mit schwachem Text
- **Vorschau:** jede Seite in 1024 px bezogen auf A4, AVIF mit Qualität 25

Alle Entscheidungen, Messwerte und offenen Punkte stehen im
[Konzept](docs/konzept.md).

## Aufbau des Repos

```
docs/           Dokumentation, darin konzept.md
docs/icons/     App-Icon in allen Größen (PNG, SVG, ICNS, ICO)
docs/intern/    interne Notizen, nicht versioniert
tools/          Analyse-Skripte der Konzeptphase (Python, nur lesend)
```

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
