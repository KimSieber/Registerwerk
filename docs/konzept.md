# Registerwerk: Konzept

Stand: 25.11.2026 (ergänzt um die Festlegungen aus v0.0.1 bis v0.1.1). Dieses Dokument hält alle Entscheidungen der Konzeptphase fest und ist die Grundlage für die Umsetzung. Konkrete Pfade, Ordnernamen und Zugangsdaten stehen nicht hier, sondern in `docs/intern/` (nicht versioniert).

Punkte, die noch nicht ausdrücklich entschieden sind, tragen den Vermerk **Annahme**. Sie gelten, bis sie in der Umsetzung bestätigt oder geändert werden.

## 1. Zweck

Registerwerk führt ein durchsuchbares Register über ein privates Dokumentenarchiv, dessen Originale nur noch in der Cloud liegen. Es hält den Volltext und eine lesbare Vorschau jeder Seite lokal vor. Suchen, Blättern und Lesen funktionieren damit ohne Download der Originale.

### Ausgangslage

- Das Archiv wird seit 2002 digital geführt. Papierunterlagen werden gescannt und als PDF mit eingebettetem OCR-Text abgelegt.
- Alle Daten liegen in einem Cloud-Ordner, der auf die Endgeräte gespiegelt wird.
- Das Archiv ist größer als die Festplatte des Notebooks. Viele Dateien stehen deshalb auf "nur online".
- Für "nur online"-Dateien findet die Systemsuche keine Inhalte mehr, und eine Schnellansicht gibt es erst nach dem Download. Die Suche in der Weboberfläche des Cloud-Anbieters ist langsam und weniger treffsicher.
- Die Volltextsuche des Anbieters ist bisher der Grund, nicht zu einem europäischen Anbieter wechseln zu können.

### Ziele

1. Vollständige Suche über alle Dokumente, auch die nur online gespeicherten
2. Schnelle Vorschau jeder Seite ohne Download
3. Geringer lokaler Platzbedarf
4. Unabhängigkeit vom Cloud-Anbieter, vom Betriebssystem und von künftigen Archivverfahren

## 2. Grundsätze

1. **Die Dateien sind die einzige Wahrheit.** Ordnerstruktur und Dateinamen des Archivs bleiben unverändert. Registerwerk ist nur eine Schicht darüber. Hintergrund: Mehrere frühere Archivsysteme mussten aufgegeben werden, jede Migration kostete Aufwand und Daten.
2. **Das Register enthält nur Kopien und Auszüge.** Es muss sich jederzeit vollständig aus den Dateien neu aufbauen lassen. Die einzige eigene Information des Programms sind die Ordnerregeln, und die liegen als lesbare Textdatei vor.
3. **Originale werden nicht verschlechtert.** Keine Neukomprimierung, keine Qualitätsreduktion. Einzige zulässige Änderung an einer Datei ist das verlustfreie Ergänzen der Textebene bei Scan-Seiten ohne Text (Abschnitt 7).
4. **Unabhängig von Betriebssystem und Cloud-Anbieter.** Der Zugriff läuft über das normale Dateisystem. Kein Swift, keine Apple-Dienste, keine Schnittstelle eines Cloud-Anbieters im laufenden Betrieb.
5. **Nur ein Schreiber zur selben Zeit.** Jedes Gerät darf einlesen und ergänzen, aber nie zwei gleichzeitig. Seit v0.0.7 arbeiten mehrere Geräte nacheinander auf demselben Register.
6. **Kein dauerhaftes Backup-Volumen.** Das Programm legt keine Sicherungskopien an, die später von Hand gesucht und entfernt werden müssten.

## 3. Umfang

### Enthalten

- Indizieren der Ordner, für die eine Regel gilt, nach Behandlungsstufe. Die Ordner können überall auf dem Gerät liegen, nicht nur im Cloud-Ordner (seit v0.0.7)
- Volltext aller Seiten, nicht nur der ersten
- Text aus Office-, OpenDocument-, Pages-, Keynote- und Textdateien (seit v0.0.6), aus doc, xls und ppt (seit v0.0.7), aus RTF, HTML, csv, tsv, log und Visitenkarten sowie aus Word 2 bis 95, Excel 2 bis 95, Lotus 1-2-3 und Pages '09 (seit v0.0.9)
- Vorschau aller Seiten, auch für einzelne Bilder im Archiv (seit v0.0.6) und für Textdateien (seit v0.0.7, weitere Textformate seit v0.0.9)
- Fehlertolerante Suche mit Gewichtung von Dateiname und Pfad
- Öffnen des Originals aus dem Cloud-Ordner
- OCR für Seiten ohne Text und für einzelne Bilder im Archiv
- Anwendung mit eigener Oberfläche für Indizierung, Suche und Anzeige
- Delta-Lauf: Prüfung beim Öffnen, Auswahl im Ordnerbaum, Verarbeitung durch alle Schritte, Laden von "nur online"-Dateien und Zurückstellen auf "nur online" (seit v0.0.8)
- Lesen, Suchen und Prüfen ändern das Register nicht. Geschrieben wird nur, wenn eine Datei neu, geändert oder entfernt ist (seit v0.0.9)
- Verschlüsseltes Register mit einem Schlüssel, den Kim selbst erzeugt und verwahrt, je Gerät im Schlüsselbund (seit v0.1.0)
- Entfernen aus dem Register als Schritt des Delta-Laufs, abwählbar (seit v0.1.1)

### Unterstützte Cloud-Speicher

Festgelegt in v0.0.8: **Vollständig unterstützt ist nur die klassische Dropbox-App mit ihrem Ordner unter `~/Dropbox` auf macOS.** Nur dort lädt Registerwerk "nur online"-Dateien selbst und stellt sie danach wieder auf "nur online".

| Cloud-Speicher | Einlesen | Laden durch Registerwerk | Zurück auf "nur online" |
|---|---|---|---|
| Dropbox, klassische App unter `~/Dropbox` (macOS) | ja | ja, über die Dateikoordination von macOS, gemessen | ja, mit dem Werkzeug `onlineonly` |
| Anbieter über den File Provider von macOS: Google Drive, OneDrive, iCloud, Dropbox unter `~/Library/CloudStorage` | ja, Platzhalter werden erkannt | **Annahme**: Lesen löst den Download aus, nicht gemessen | nein, macOS gibt Platz bei Bedarf selbst frei |
| Ordner ohne Cloud | ja | nicht nötig | nicht nötig |

Das muss in README und auf der späteren Webseite deutlich stehen. Sonst erwarten Anwender mit anderen Anbietern etwas, das Registerwerk nicht leistet.

### Nicht enthalten

- KI-Auswertung der Inhalte. Anders als bei Bilderwerk ist sie nicht vorgesehen.
- Anbindung an die Systemsuche. Gesucht wird in Registerwerk.
- Fotos. Die Bildordner deckt Bilderwerk ab, sie sind ausgeschlossen.
- Lösung für mobile Geräte. Sie würde Register und Funktionen online voraussetzen. Wegen der hochsensiblen Daten wird das gesondert betrachtet und ist kein Teil dieses Konzepts.

## 4. Architektur

### Bausteine

| Baustein | Aufgabe |
|---|---|
| Indexer | Durchläuft den Dateibaum, erkennt neue und geänderte Dateien, liest Text aus, führt bei Bedarf OCR aus, erzeugt Vorschauen, schreibt das Register |
| Register | Verschlüsselte SQLite-Datenbanken mit Metadaten, Volltext und Vorschauen |
| Anwendung | Oberfläche für Suche, Trefferliste, Seitenvorschau, Öffnen des Originals und Steuerung des Indexers |

Indexer und Anwendung sind Teile desselben Programms. Der lesende Teil soll so gebaut sein, dass er sich später für eine Web-Oberfläche wiederverwenden lässt.

### Betrieb

- **Erstindizierung:** auf dem stationären Rechner, in Tranchen, weil das gesamte Archiv dort nicht gleichzeitig lokal liegen kann. Ab Februar 2027 ist dort mehr Platz frei.
- **Laufender Betrieb:** Prüfung und Delta-Befüllung überwiegend auf dem Notebook. Der stationäre Rechner läuft nicht dauerhaft und hält keine dauerhafte Vollkopie. Seit v0.0.8 prüft Registerwerk beim Öffnen von selbst, was ansteht, und verarbeitet es nach Kims Bestätigung (Abschnitt 5, Delta-Lauf).
- **Zuwachs:** rund 1.000 bis 2.000 Dokumente pro Jahr, etwa 3 Promille des Bestands.
- **Ein Schreiber zur selben Zeit:** Organisatorisch ist sichergestellt, dass das Programm nur auf einem Gerät geöffnet ist. Welches Gerät das ist, wechselt: Wer länger an einem Gerät arbeitet, liest dort ein. Eine Sperre mit Stand-Zähler soll den Gerätewechsel technisch absichern (Abschnitt 10), sie ist noch nicht gebaut.

### Scan-Prozess

Der bestehende Scan-Prozess bleibt unverändert: Scan per Smartphone-App mit Bildoptimierung und OCR, Benennung, Ablage als PDF direkt im Cloud-Ordner. Die Datei erscheint danach als Platzhalter oder lokale Datei auf den Rechnern und wird beim nächsten Delta-Lauf ins Register aufgenommen.

## 5. Zugriff auf die Dateien

### Erkennen neuer und geänderter Dateien

- Der Indexer durchläuft den Dateibaum und vergleicht ihn mit dem Register.
- Neu ist jeder Pfad, den das Register nicht kennt. Ein Datum wird dafür nicht gebraucht.
- Geändert ist eine Datei, deren Größe oder Änderungsdatum abweicht. Ausnahme ist der Wechsel zwischen lokal und "nur online", siehe unten.
- Entfernt wird nur mit Grund (seit v0.0.7): Für den Ordner gilt keine Regel mehr oder die Stufe „Aus“, der Dateityp wird nicht geführt, die Datei liegt in einem Bibliotheksordner, oder ihr Ordner ist auf diesem Gerät vorhanden und vollständig gelesen und die Datei fehlt darin. **Fehlen allein löscht nie.**
- Fehlt ein ganzer Ordner oder gab es darin Lesefehler, bleibt sein Bestand im Register unangetastet. Ein nicht eingehängter Cloud-Ordner leert so nicht das Register. Hat das Gerät den Ordner früher gesehen, meldet der Lauf ihn als „vermisst“, sonst als Ordner eines anderen Geräts (Abschnitt 6, Äste und Geräte).
- Vor dem Entfernen zeigt die Oberfläche die betroffenen Dateien mit dem Grund. Dazu läuft der Abgleich erst als Probe, die nichts ändert. Seit v0.1.1 stehen sie angehakt im Baum des Delta-Laufs und lassen sich abwählen. Abgewähltes bleibt im Register und steht bei der nächsten Prüfung wieder an. Ab mehr als 100 Einträgen fragt die Oberfläche vor dem Verarbeiten einmal nach.
- Verschobene und umbenannte Dateien sollen über einen Inhalts-Hash erkannt werden, damit Text und Vorschauen nicht neu erzeugt werden müssen. **Annahme**, noch nicht umgesetzt (Stufe 4).

### Platzhalter ("nur online")

Gemessen am 15.11.2026 auf dem Notebook:

- Platzhalter melden 0 Byte, 0 belegte Blöcke und keine Dateiflags.
- **Ergänzung 22.11.2026 (v0.0.9):** Die klassische Dropbox hängt an jeden Platzhalter das erweiterte Attribut `com.dropbox.placeholder`. Eine lokale leere Datei trägt es nicht. Daran unterscheidet Registerwerk beides. Das Attribut ist undokumentiert. **Annahme**, dass es bleibt.
- Name, Pfad und Änderungsdatum sind lesbar, die Größe nicht.
- Das Auflisten lädt nichts herunter.
- **Korrektur 22.11.2026 (v0.0.8):** Das bloße Öffnen und Lesen durch ein Programm lädt nichts. Ein Lesen mit `head` kam nach 4 ms mit 0 Byte zurück. Geladen wird, wenn die Datei über das System geöffnet wird (`open`, Doppelklick) oder ein Programm sie über die Dateikoordination von macOS (`NSFileCoordinator`) anfordert. Die Dateikoordination lud eine Datei ohne sichtbares Fenster. Vorher stand hier, dass Öffnen den Download auslöst.

Folgen:

- Eine Datei mit 0 Byte gilt unter macOS nur mit dem Attribut der Dropbox als "nur online", sonst als lokal und leer. Eine leere Datei gilt als gelesen, ohne Seiten und ohne Fehler (v0.0.9). Auf anderen Systemen gilt weiter: 0 Byte heißt "nur online oder leer".
- Inhalte, Vorschauen und OCR öffnen nur lokal vorhandene Dateien. "Nur online"-Dateien bleiben ausstehend. Umgesetzt in v0.0.2.
- Seit v0.0.8 lädt der Delta-Lauf die ausgewählten "nur online"-Dateien selbst über die Dateikoordination (Abschnitt Delta-Lauf).
- **File Provider:** Platzhalter von Google Drive, OneDrive, iCloud und der Dropbox unter `~/Library/CloudStorage` melden die echte Größe. Erkennbar sind sie am Kennzeichen `SF_DATALESS` in den Dateiflags. Seit v0.0.8 gelten sie wie Dateien mit 0 Byte als "nur online", sonst würden sie beim Lesen still geladen. Ob dort ein einfaches Lesen den Download auslöst, ist nicht gemessen. **Annahme**, vor der Aufnahme solcher Äste zu prüfen.
- Das Gesamtvolumen von "nur online"-Dateien lässt sich über das Dateisystem nicht ermitteln.
- Wechselt eine Datei bei gleichem Änderungsdatum zwischen lokal und "nur online", gilt sie als unverändert. Das Register behält die letzte bekannte Größe und merkt sich nur den Zustand. Erst ein anderes Änderungsdatum bedeutet einen neuen Inhalt. Umgesetzt in v0.0.1. **Annahme**: Der Cloud-Client lässt das Änderungsdatum beim Auslagern und Laden unverändert. Das wird im Pilot beobachtet.
- Eine Datei, die bisher nur als Platzhalter gesehen wurde, hat im Register keine Größe.

### Delta-Lauf

Umgesetzt in v0.0.8 (Stufe 4, ohne Sperre und Inhalts-Hash).

1. **Prüfen:** Beim Öffnen der Anwendung läuft der Abgleich des Dateibaums als Probe, im Hintergrund, während die Suche schon bedienbar ist. Seit v0.0.9 rechnet die Probe auf Kopien der Tabellen im Arbeitsspeicher, über eine nur lesend geöffnete Verbindung. Das Register bleibt unverändert, keine Datei im Register-Ordner wird angefasst. Steht etwas an, erscheint neben den Reitern ein gelber Knopf („Änderungen gefunden“ oder „Verarbeitung ausstehend“), der in den Reiter „Register“ führt. Geprüft wird nur beim Start, mit „Erneut prüfen“ und nach eigenen Aktionen, nicht nach Zeit (Kim: keine unerwarteten Läufe während der Suche). Dateien in vermissten Ordnern stehen nicht im Ablauf.
2. **Anzeigen:** Zahlen für neu, geändert, noch offen, nur online, neu nur mit Namen und zu entfernen. Jede Zahl öffnet die Liste der Dateien. Darunter ein Ordnerbaum mit Anzahl je Ordner, aufklappbar bis zu den Dateien. Seit v0.1.1 stehen auch die Dateien nur mit Namen (fester grauer Haken, Marke „nur Name“) und die zu entfernenden (rote Marke mit Grund) im Baum.
3. **Auswählen:** Alles ist angehakt, auch "nur online" und zu Entfernendes. Ordner und Dateien lassen sich herausnehmen, alle "nur online"-Dateien und alles zu Entfernende je mit einem Schalter. Dateien nur mit Namen kommen immer ins Register.
4. **Verarbeiten:** neue und geänderte Dateien aufnehmen und das Ausgewählte entfernen (seit v0.1.1) → aus der Cloud laden → Inhalte lesen → Vorschauen erzeugen → Text erkennen → in PDFs schreiben → wieder "nur online" → Suchindex nachführen. Jeder Schritt nur für die Auswahl.
5. **Entfernen** war bis v0.1.0 ein eigener Schritt. Seit v0.1.1 gehört es zu „Verarbeiten“, der Knopf nennt die Zahl („Verarbeiten und 12 entfernen“).

Läuft im Hintergrund eine Tätigkeit am Register (Prüfung, Verarbeiten, Aufteilen, Verschlüsseln), zeigt der Reiter „Register“ einen Kreisel, der Tooltip nennt sie (seit v0.1.1).

Das Schreiben in die PDFs braucht im Delta-Lauf keinen zweiten Klick. Der Klick auf „Verarbeiten“ gilt als Freigabe (Kim, 22.11.2026). Die Textebene wird nur angehängt, das Risiko ist gering.

**Laden:** Die ausgewählten "nur online"-Dateien werden in vier Fäden über die Dateikoordination angefordert. Gewartet wird gemeinsam, angekommene Dateien kommen sofort ins Register. Das Warten endet, wenn alle da sind, nach 2 Minuten ohne neue Datei oder mit „Nicht warten, weiter“. Was nicht ankam, bleibt offen für den nächsten Lauf. Die Anzeige zeigt laufend, wie viele Dateien da sind und auf welche gewartet wird. Grund: Die Warteschlange der Cloud kann voll sein, oder das Gerät ist offline.

### Zurück auf "nur online"

Seit v0.0.8 stellt der Delta-Lauf die Dateien, die er selbst geladen hat, danach wieder auf "nur online". Dateien, die Kim bewusst lokal hält, bleiben lokal. Dafür nutzt Registerwerk das Werkzeug `onlineonly` (MIT, inoffiziell). Es spricht mit der Dropbox-App über dieselbe Schnittstelle wie ihr Finder-Menü und wartet, bis die Dropbox den neuen Zustand meldet. Pfade, die die Dropbox noch abgleicht, hält es zurück und fordert sie erneut an. Getestet am 22.11.2026: aus `local` wurde `online-only` mit 0 Byte.

- Es arbeitet nur mit der klassischen Dropbox unter `~/Dropbox`.
- Die Schnittstelle ist undokumentiert. Ein Update der Dropbox kann sie brechen. Dann bleiben die Dateien lokal, und die Anzeige meldet das.
- Fehlt das Werkzeug, wird der Schritt übersprungen, und die Oberfläche nennt den Befehl zur Installation (`brew install zamua/tap/onlineonly`).
- Beim ersten Programmpaket soll es mit Lizenztext als Beigabe ins Paket. **Annahme**
- Rückfallweg bleibt das Zurücksetzen von Hand.

Das ist die einzige Stelle, an der Registerwerk mit einem Cloud-Anbieter spricht, und sie ist freiwillig. Grundsatz 4 gilt für Einlesen, Suchen und Anzeigen weiter.

### Original öffnen

Die Anwendung öffnet die Datei über ihren Pfad im Cloud-Ordner. Steht sie auf "nur online", lädt der Cloud-Client sie beim Öffnen. Das entspricht dem Vorgehen in Bilderwerk.

## 6. Behandlung von Ordnern und Dateitypen

Umgesetzt in v0.0.7.

### Behandlungsstufen

| Stufe | In der Oberfläche | Was ins Register kommt | Gedacht für |
|---|---|---|---|
| Voll | Voll | Pfad, Name, Metadaten, Text, Vorschau | das eigentliche Archiv, Dokumente, Projekte |
| Nur Metadaten | Nur Namen | Pfad, Name, Größe, Datum. Kein Inhalt, keine Vorschau | eBooks, Musik |
| Ausgeschlossen | Aus | nichts | Bildordner, die Bilderwerk abdeckt |

Begründung für "Nur Metadaten" bei eBooks: Ihr Inhalt würde sonst bei fast jeder Suche Treffer liefern.

Die frühere Stufe "Nur Dokumente" für Entwicklungsprojekte ist entfallen. Die Einteilung der Dateitypen (unten) gilt in jedem Ordner der Stufe Voll und lässt Quelltext und Bibliotheken von selbst aus.

### Regeln

- **Ohne Regel wird nichts gelesen.** Ordner werden bewusst eingeschlossen.
- Eine Regel gilt für den Ordner und alle Unterordner. Die genaueste Regel gewinnt: Ein ausgeschlossener Unterordner bleibt ausgeschlossen, auch wenn der Oberordner auf Voll steht, und umgekehrt.
- Die Regeln stehen in der lesbaren Textdatei `regeln.toml` neben dem Register, nicht in der Datenbank. Sie überleben damit einen Neuaufbau und gelten für alle Geräte.
- Gewählt werden sie im Reiter "Ordner": ein aufklappbarer Baum mit der Stufe je Ordner. "Ordner prüfen" zählt vorab, was ein Ordner enthält, ohne etwas ins Register zu schreiben.
- Ein Wechsel der Stufe wirkt beim nächsten Verarbeiten. "Aus" entfernt die Einträge, "Nur Namen" entfernt Text, Vorschauen und OCR. Beides abwählbar (Abschnitt 5). Seit v0.1.1 hat der Reiter "Ordner" kein eigenes Einlesen mehr: Nach dem Speichern prüft Registerwerk, ein Kasten zeigt die Zahlen und führt in den Reiter "Register".
- Die konkrete Zuordnung der Ordner steht in `docs/intern/umgebung.md`.
- Verwaltungsdateien von Cloud-Client und Betriebssystem werden immer übergangen.
- **Bibliotheksordner** werden übergangen: immer `node_modules`, `__pycache__`, `site-packages`, `venv`, `Pods`, `DerivedData`, `bower_components` und Ordner mit einem Punkt am Anfang. `target`, `build`, `dist` und `vendor` nur neben einer Projektdatei, weil die Namen mehrdeutig sind. Eine eigene Regel auf einem solchen Ordner schließt ihn ein.

### Äste und Geräte

- Ein **Ast** ist ein Ordner eines Geräts, der einen Namen bekommt: der Cloud-Ordner, ein zweiter Cloud-Speicher, Dokumente, ein Wechseldatenträger. Aufgenommen wird er im Reiter "Ordner".
- Jeder Pfad im Register und in den Regeln beginnt mit dem Namen des Astes. Wo der Ast auf einem Gerät liegt, steht nur in dessen Konfiguration.
- Die Konfiguration je Gerät (`config.local.toml`, nicht versioniert) enthält den Gerätenamen, wahlweise den Ordner des Registers und die Äste mit ihrem Ort. Die Anwendung schreibt sie selbst. Eine Vorlage ohne echte Pfade liegt im Repo.
- Das Register merkt sich je Ordner, welches Gerät ihn zuletzt gesehen hat. Fehlt ein Ordner, unterscheidet das zwei Fälle:

  | Fall | Bedeutung | Folge |
  |---|---|---|
  | dieses Gerät hat ihn nie gesehen | Ordner eines anderen Geräts | wird übersprungen, Bestand bleibt |
  | dieses Gerät hat ihn früher gesehen | verschoben, gelöscht oder nicht eingehängt | wird als vermisst gemeldet, Bestand bleibt, bis Kim ihn entfernt oder dem Ast einen neuen Ort zuweist |

### Dateitypen

In einem Ordner der Stufe Voll entscheidet die Endung. Die Listen sind fest (`formate.rs`).

| Klasse | Was geschieht |
|---|---|
| gelesen | Text, je nach Format Vorschau und OCR |
| nur Name | Name, Ordner, Größe, Datum. Archive werden nicht entpackt |
| ignoriert | kommt nicht ins Register: Quelltext, Programme, Daten und alles, was in keiner Liste steht |

**Gelesen:**

- **PDF** ist der Regelfall und wird vollständig verarbeitet.
- **Office, OpenDocument, Pages, Keynote und Text**, ohne fremdes Programm. Vorschauen gibt es nur für txt und md, die übrigen bräuchten LibreOffice oder die Vorschau des Betriebssystems.

  | Format | Seiten im Register |
  |---|---|
  | Word (docx, docm, dotx, dotm, doc, dot), OpenDocument (odt, ods, odp), Pages und Keynote ab 2013 | eine Seite mit dem ganzen Text |
  | Excel (xlsx, xlsm, xltx, xltm, xls, xlt) | eine Seite je Tabellenblatt |
  | PowerPoint (pptx, pptm) | eine Seite je Folie |
  | PowerPoint alt (ppt, pps) | eine Seite mit dem ganzen Text |
  | txt, md | eine Seite, UTF-8, sonst Latin-1. Die Vorschau zeigt den Anfang des Textes in fester Schrift |

- **Einzelne Bilder** (jpg, jpeg, png, tif, tiff, bmp, gif, webp, seit v0.0.7 heic und heif) bekommen eine Vorschau und OCR. Der erkannte Text kommt nur ins Register. Meist sind es versehentlich als Bild gespeicherte Belege. Bildordner bleiben ausgeschlossen, sie deckt Bilderwerk ab. Von mehrseitigen TIFF wird die erste Seite gelesen, Fax-TIFF (CCITT Gruppe 3) seit v0.0.7.

- **Weitere Textformate** (seit v0.0.9) mit Vorschau wie txt: rtf (eigener Leser), csv, tsv, log, htm und html (Tags entfernt), vcf (je Eigenschaft eine Zeile, ohne Verwaltungsangaben und Fotos).
- **Alte Office-Dateien** (seit v0.0.9): Hinter den Endungen doc, xls und ppt steckt im Archiv vieles. Registerwerk erkennt am Inhalt, was es ist: Verbunddatei (eigener toleranter Leser, verzeiht kleine Fehler), neues Format, RTF, HTML, Text, Word für Windows 2, Excel 2 bis 95, Lotus 1-2-3. Word 6 und 95 mit beschädigtem Verzeichnis werden über die Suche des Dokumentkopfs gelesen. Pages und Keynote '09 liefern ihren Text aus `index.xml`.
- **Grober Rückfall** (seit v0.0.9): Scheitert jeder Leser an einer alten Office- oder iWork-Datei, kommen lesbare Zeichenfolgen der Datei ins Register, mit dem Vermerk „Text nur grob gelesen“. Die Datei ist so über die Suche auffindbar. Verschlüsselte Dateien sind ausgenommen.

**Nur Name:** Archive und Abbilder (zip, rar, 7z, dmg …), Numbers, E-Mail (eml, msg, mbox), Termine (ics), eBooks (epub, mobi …), Grafik- und Rohformate, Audio, Video, Notizbuch- und Tracking-Dateien. E-Mail und Termine bleiben bewusst ohne Inhalt (Kim, v0.0.9).

**Nicht gelesen** werden weiter Numbers, verschlüsselte Office-Dateien (auch Excel mit dem Standardkennwort von Excel), schnell gespeicherte Word-2-Dokumente und Word 1.

## 7. Verarbeitung von PDFs

### Regelfall

Alle Scans liegen als PDF vor: Das Scanbild ist als Bild gespeichert, der per OCR erkannte Text ist eingebettet. PDFs mit echtem, digital erzeugtem Text sind die Ausnahme. Der eingebettete Text wird übernommen. OCR läuft nur, wo es nötig ist.

### Gütemaß

Die Güte eines Textes ist der Anteil wortartiger Tokens: Wörter aus mindestens zwei Buchstaben sowie Zahlen- und Betragsmuster, bezogen auf alle Tokens. Das Maß ist grob, trennt aber brauchbaren von unbrauchbarem Text. Die Schwelle liegt bei **60 %**.

Eine Seite gilt als Scan, wenn Bilder mindestens 70 % ihrer Fläche abdecken. Gütemaß und Seitenart werden je Seite im Register gespeichert. Umgesetzt in v0.0.2.

### Welche Seiten erkannt werden

| Fall | Erkennung | Behandlung |
|---|---|---|
| Seite ohne Text | höchstens 10 Zeichen eingebetteter Text | OCR ausführen. Der Text gilt ab 11 Zeichen. Text ins Register, bei Scans auch in die PDF einbetten |
| Scan mit schwachem Text | Güte unter 60 % | keine OCR, der eingebettete Text bleibt |
| Digital erzeugte PDF mit schwachem Text | kein Scanbild, Güte unter 60 % | keine OCR, der eingebettete Text bleibt |

Umgesetzt in v0.0.4 für das Register, in v0.0.5 das Einbetten in die PDF.

**Eingebetteter Text wird nie überschrieben.** Die Scan-App erkennt den Text schon beim Scannen, und diese Erkennung ist besser als Tesseract. OCR läuft deshalb nur für Seiten, auf denen noch kein Text hinterlegt ist. Ein vorhandener Text bleibt in der PDF unverändert und bleibt auch im Register erhalten. Der erkannte Text liegt dort daneben, nicht an seiner Stelle. Das gilt für neue Dateien genauso wie für den Bestand.

Erläuterungen:

- Ursprünglich sollte auch schwacher Text neu erkannt und bei messbarer Verbesserung ersetzt werden. Im Pilot (v0.0.4, erster Lauf) war der eingebettete Text aber nicht schlechter, oft besser als das Ergebnis von Tesseract. Das Gütemaß zeigte trotzdem eine Verbesserung, es ist für diese Entscheidung zu grob. Schwacher Text bleibt deshalb unverändert.
- Digital erzeugte PDFs ohne Text, etwa ein Kontoauszug von 2017, dessen Schrift beim Drucken in ein PDF zu Kurven wurde, bekommen den erkannten Text **nur ins Register**. Die Textebene lässt sich dort nicht einbetten, ohne die Seite in ein Bild umzuwandeln. Das wäre ein Qualitätsverlust und ist ausgeschlossen. Der OCR-Text im Register ist ein Auszug und lässt sich jederzeit neu erzeugen, der Grundsatz 2 bleibt gewahrt.
- Handschrift, auch in Druckbuchstaben, und Text in Bildern erkennt Tesseract nicht brauchbar. Im Pilot blieben 307 von 1.185 Seiten ohne Text.
- Die Grenze von 10 Zeichen ist eine **Annahme** (v0.0.5, vorher unter 20). Im Pilot haben 67 Seiten 11 bis 19 Zeichen. Sie gelten als Seiten mit Text und werden nicht erkannt.
- Erkannt wird mit 300 dpi in Graustufen, Sprachen Deutsch und Englisch, Sprachdaten `tessdata` (Standard). `tessdata_best` brachte im Pilot dasselbe Ergebnis und war gut 30 % langsamer.
- Der erkannte Text liegt in einer eigenen Tabelle neben dem eingebetteten Text. Welcher gilt, zeigt eine Sicht im Register. Sie ist die Grundlage der Suche.

### Zurückschreiben in die PDF

Gilt nur für Scans mit Seiten ohne Text (Abschnitt "Welche Seiten erkannt werden"). Der Text wird in die Datei eingebettet, damit der Komfort der durchsuchbaren PDF auch außerhalb von Registerwerk erhalten bleibt. Der Ablauf kommt ohne Sicherungskopien aus:

1. Die neue Fassung entsteht als temporäre Datei.
2. Das Programm prüft sie: gleiche Seitenzahl, Scanbilder unverändert, Text vorhanden, Datei lässt sich öffnen.
3. Besteht sie die Prüfung, ersetzt sie das Original in einem Schritt. Besteht sie nicht, wird sie verworfen und das Original bleibt unberührt.
4. Das Änderungsdatum der Datei wird auf den vorherigen Wert zurückgesetzt, weil die Sortierung darauf aufbauen kann.
5. Ein Protokoll hält Pfad und Zeitpunkt jeder geänderten Datei fest.

Als zusätzliches Netz ohne eigenes Volumen dient der Versionsverlauf des Cloud-Anbieters, solange er zur Verfügung steht.

Umgesetzt in v0.0.5:

- Die Textebene besteht aus unsichtbarem Text (`3 Tr`) je Wort, an der von Tesseract erkannten Stelle und auf die Breite des Worts gestreckt, in einer eingebetteten Schrift ohne sichtbare Zeichen aus Tesseract. Markieren und Suchen funktionieren in jedem PDF-Programm.
- Geschrieben wird als inkrementelle Änderung: Die Bytes des Originals bleiben unverändert, die Textebene wird angehängt. Die Scanbilder können sich nicht verändern.
- Geprüft wird zusätzlich, dass die neue Fassung mit dem Original beginnt und dass die betroffenen Seiten gerendert genau gleich aussehen.
- Danach führt das Register Größe und Änderungsdatum nach. Die Datei gilt nicht als geändert, Vorschauen und OCR bleiben gültig.
- Das Protokoll `textebene.log` liegt neben dem Register. **Annahme**
- Im Pilot: 148 PDFs mit 806 Seiten in 30 s, Zuwachs 4,4 KB je Seite. Änderungsdatum in Finder und Dropbox blieb erhalten, der Versionsverlauf der Dropbox legte eine neue Version mit dem Tag des Schreibens an.

## 8. Vorschauen

| Merkmal | Festlegung |
|---|---|
| Umfang | jede Seite jedes voll verarbeiteten PDFs, jedes einzelne Bild |
| Auflösung | kurze Kante 1024 px, unabhängig von der Seitengröße in der PDF. A4 wird 1024 × 1448 px, das sind rund 124 dpi (seit v0.0.6) |
| Format | AVIF, Qualität 25, kodiert mit libavif und libaom (Stufe 8) |
| Farbe | farbige Seiten in 4:2:0, Seiten ohne sichtbare Farbe als echte Graustufen (4:0:0) |
| Farberkennung | Ein Pixel gilt als farbig, wenn seine Farbkanäle um mehr als 48 auseinanderliegen. Eine Seite gilt als farbig ab 0,1 % farbiger Pixel. **Annahme**, im Pilot stichprobenartig bestätigt |
| Lange Seiten | lange Kante höchstens 3072 px. Ein Kassenbon 1:6 wird 512 px breit. **Annahme** |
| Bilder | gedreht nach den EXIF-Angaben, eingepasst wie Seiten, nie vergrößert |

Begründung:

- 640 px sind lesbar, aber leicht unscharf. Das ermüdet beim Suchen und Lesen.
- Bis v0.0.5 galt ein fester Maßstab bezogen auf A4, schmale Seiten wurden schmaler. Die Seitengröße einer PDF ist aber nicht verlässlich: Im Pilot liegt ein A4-Scan mit 300 dpi auf einer Seite von 57 × 81 mm, er wurde 279 px breit und unlesbar. Kassenbons waren rund 280 px breit und unscharf. Seit v0.0.6 zählt deshalb die kurze Kante.
- 640 px wurden am 19.11.2026 (v0.0.6) noch einmal an 10 Pilotseiten mit den Einstellungen der Anwendung geprüft: halber Speicher (rund 15 statt 30 KB je Seite, Archiv rund 20 statt 40 GB), aber normale Schrift kaum noch lesbar. Verworfen, es bleibt bei 1024 px.
- 1600 px bringen nur beim extremen Vergrößern einen Unterschied.
- AVIF Q25 war im direkten Vergleich mit WebP Q50 bei normaler Ansicht nicht zu unterscheiden, auch bei Farbseiten. Die Dateien sind gut ein Drittel so groß.
- Kleine Vorschauen halten das Blättern flüssig, auch falls das Register später auf einem Netzlaufwerk oder im Web liegt.
- Die Vorschau muss normalen Brieftext lesbar machen. Für Kleingedrucktes wird das Original geladen.

Die Vorschau ist eine abgeleitete Kopie. Ihre starke Kompression berührt die Originale nicht.

Die Einstellungen entsprechen dem Vergleich der Konzeptphase, der ebenfalls libavif nutzte (über Pillow). Im Pilot (v0.0.3) ergaben sich 27,2 KB je Seite, genau der Wert aus der Stichprobe. Beim Durchsehen war kein Unterschied zum Original zu erkennen. Mit der Regel nach der kurzen Kante (v0.0.6) sind es 361 MB statt 295 MB, rund 32 KB je Seite. Der Zuwachs kommt aus den Belegen.

In Detailbereich und Großansicht lässt sich die Vorschau von 25 bis 400 % vergrößern. Über 100 % wird sie weicher, für Kleingedrucktes bleibt das Original.

## 9. Datum eines Dokuments

Für Sortierung und Priorisierung gilt diese Reihenfolge:

1. Datum am Anfang des Dateinamens
2. Erstellungsdatum in den PDF-Metadaten
3. Änderungsdatum der Datei

Erkannt werden am Anfang des Dateinamens die Formen `JJJJ-MM-TT`, `JJJJ-MM`, `JJJJMMTT` und `JJJJ`. Das Datum wird so genau gespeichert, wie es angegeben ist. Seit v0.0.2 sind alle drei Quellen umgesetzt. Jede Datei hat damit ein Dokumentdatum, die Quelle wird mitgespeichert. Im Pilotbestand tragen 93 % der Dateien ein Datum im Namen.

Begründung: Fast alle Dateinamen beginnen mit einem Datum. Ein PDF-Erstellungsdatum tragen nur 76 % der PDFs. Das Anlagedatum eines Platzhalters ist nicht verlässlich. Ein Änderungsdatum im Sinne von "zuletzt bearbeitet" wird nicht gebraucht.

## 10. Register

### Aufbau

Das Register besteht aus mehreren SQLite-Dateien, die das Programm gemeinsam öffnet und wie eine Datenbank behandelt:

| Datei | Inhalt | Größe |
|---|---|---|
| Index-Datenbank | Pfade, Metadaten, Volltext mit Suchindex, Verweis auf die Vorschau | einige GB |
| Vorschau-Datenbanken ("Scheiben") | die AVIF-Bilder als Datensätze, wie bei Bilderwerk | je rund 200 MB (seit v0.0.8, vorher bis 1,5 GB) |

Die Vorschauen liegen also in der Datenbank, nicht als einzelne Bilddateien. Sie sind nur auf mehrere Dateien verteilt. Gründe gegen eine einzige große Datei:

- Jeder Delta-Lauf würde aus Sicht der Cloud die ganze Datei ändern. Manche Anbieter laden sie dann vollständig neu hoch.
- Die Arbeitskopie beim Schreiben bräuchte zeitweise den doppelten Platz.
- Ein Schaden bei der Übertragung träfe das gesamte Register statt einer Scheibe.

Alte Scheiben bleiben unverändert. Ein Delta-Lauf fasst nur die Index-Datenbank und die jüngste Scheibe an. Umgesetzt in v0.0.3:

- Die Scheiben werden nach **Erfassungsreihenfolge** gefüllt. Neue Vorschauen kommen in die jüngste Scheibe, bis diese rund 200 MB erreicht. Dann beginnt eine neue. Bis v0.0.7 lag die Grenze bei 1,5 GB. Mit dem Register im Cloud-Ordner hat sich das als zu groß gezeigt, weil die wachsende Scheibe nach jedem Lauf neu abgeglichen wird.
- Größere Scheiben aus der Zeit davor teilt der Knopf „Scheiben aufteilen“ im Datenbank-Symbol (v0.0.8). Er erscheint nur, wenn eine Scheibe über der Grenze liegt. Die ältesten Vorschauen bleiben, die übrigen wandern in neue Scheiben, die Vorschauen einer Datei bleiben zusammen. Danach wird die alte Datei verkleinert. Keine Kopie, kein Löschen einer Datei, abbruchsicher. Bei Kim am 22.11.2026 ausgeführt: aus einer Scheibe von 1,3 GB wurden mehrere zu rund 200 MB.
- Die Dateien heißen `vorschau-0001.db`, `vorschau-0002.db` usw. und liegen neben der Index-Datenbank.
- Die Index-Datenbank hält je Seite einen Verweis (Scheibe und Schlüssel), Maße und Farbe.
- Ändert sich eine Datei oder verschwindet sie, wandern ihre alten Verweise in eine Liste. Beim nächsten Lauf werden die Bilder aus ihrer Scheibe gelöscht. Nur dabei ändert sich eine ältere Scheibe.
- Geschrieben wird erst die Scheibe, dann der Verweis. Ein Abbruch dazwischen hinterlässt höchstens ein Bild ohne Verweis, das der nächste Neuaufbau beseitigt.

**Index-Datenbank im Cloud-Ordner:** `register.db` ist bei Kim 1,2 GB groß (22.11.2026, während der Erstindizierung). Ein Aufteilen hilft wenig, weil jeder neue Beleg Text und Suchindex ändert. Soweit bekannt, überträgt der Dropbox-Client nur geänderte Blöcke von 4 MB, sodass ein Beleg einige MB statt der ganzen Datei hochlädt. **Annahme**, im Alltag nach der Erstindizierung zu beobachten.

**Pfade im Register** beginnen mit dem Namen des Astes (Abschnitt 6, seit v0.0.7, vorher relativ zum Cloud-Ordner), mit `/` als Trenner und einheitlicher Schreibweise der Umlaute (Unicode NFC). Wo ein Ast auf einem Gerät liegt, steht nur in dessen Konfiguration. Dasselbe Register gilt damit auf jedem Gerät und Betriebssystem. Die einheitliche Schreibweise ist nötig, weil macOS Umlaute zerlegt liefert und dieselbe Datei sonst je nach Gerät ein anderer Eintrag wäre. Umgesetzt in v0.0.1.

Die Replikation großer Dateien ist kein Engpass: Leitungen und Laufzeit reichen aus, notfalls läuft ein Rechner eine Nacht durch.

### Ablage und Veröffentlichen

- Der Ordner des Registers ist je Gerät wählbar (seit v0.0.7), über das Datenbank-Symbol der Anwendung. Ohne Angabe liegt es im Datenordner der Anwendung im Benutzerprofil. Der Ort steht in `docs/intern/umgebung.md`.
- Im Ordner liegen die Index-Datenbank `register.db`, die Scheiben, die Regeln `regeln.toml` und das Protokoll der Textebene. Verschoben wird der ganze Inhalt bei beendetem Programm, danach wird der neue Ordner gewählt.
- Kim legt das Register direkt in den Cloud-Ordner, damit alle Geräte dasselbe sehen. Voraussetzungen: Das Programm ist nur auf einem Gerät geöffnet, und die Cloud hat vor dem Wechsel fertig abgeglichen. Seit v0.0.9 arbeitet SQLite im klassischen Journal (`DELETE`) statt im WAL-Journal. Lesen ändert damit keine Datei. Beim Schreiben entsteht kurz `register.db-journal`. Geschrieben wird nur, was sich ändert: kein Vermerk „zuletzt gesehen“ je Datei, keine Zeile je Durchlauf ohne Änderung, keine Zeitstempel je Ordner. Grund: Jede Änderung löst einen Upload der Dropbox aus, der andere Abgleiche behindert (Kim).
- Die ursprünglich vorgesehene Arbeitskopie außerhalb des Cloud-Ordners mit Veröffentlichen des fertigen Standes ist damit zurückgestellt. Sie bleibt der Weg, falls der direkte Betrieb Konflikte erzeugt. **Annahme**
- Eine Sperre mit Stand-Zähler soll festhalten, welches Gerät zuletzt geschrieben hat. Beim Start prüft das Programm, ob die lokale Kopie diesen Stand hat. Wenn nicht, öffnet es nur lesend und meldet das. **Annahme**, noch nicht gebaut. Das Register hält seit v0.0.7 je Durchlauf das Gerät fest.

### Verschlüsselung

Umgesetzt in v0.1.0. Das Register ist verschlüsselt, zusätzlich zur Verschlüsselung der Festplatte. Seit v0.0.7 lag es mit Volltext und Vorschauen unverschlüsselt im Cloud-Ordner.

- **Verfahren:** SQLCipher (AES-256). Jede Seite der Datenbank ist für sich verschlüsselt, mit eigenem Zufallswert und Prüfsumme. Suche, Journal und nur lesendes Öffnen bleiben, wie sie sind. Ein neuer Beleg ändert weiter nur wenige Seiten, die Cloud gleicht nur geänderte Blöcke ab.
- **Schlüssel:** ein fertiger Schlüssel mit 256 Bit (64 Hex-Zeichen). Kim erzeugt ihn selbst auf einem anderen Rechner und verwahrt ihn im eigenen Passwortsafe. Kein Kennwort, keine Schlüsselableitung, kein Salt und keine Schlüsseldatei in Registerwerk (Kim, 24.11.2026).
- **Je Gerät:** Der Schlüssel liegt im lokalen Schlüsselbund des Betriebssystems, ohne Cloud-Abgleich, ein Eintrag je Register-Ordner. Registerwerk liest ihn beim Start, ohne Abfrage. Eingegeben wird er einmal je Gerät. Fehlt er oder passt er nicht, zeigt die Anwendung nur die Eingabe. Schutzbedarf ist der Verlust der Daten aus der Cloud. Das Gerät selbst schützen Anmeldung und Festplattenverschlüsselung (Kim).
- **Umstellung** eines unverschlüsselten Registers per Knopf im Datenbank-Symbol, nach erneuter Eingabe des Schlüssels zur Bestätigung. Datei für Datei: verschlüsselte Arbeitskopie außerhalb des Cloud-Ordners, Prüfung (Schlüssel, Integrität, Zeilen je Tabelle), dann Ersetzen in einem Schritt. Die Index-Datenbank zuletzt. Anhaltbar, abbruchsicher, ohne Sicherungskopie. Die Dateinamen bleiben gleich.
- **Gemischter Zustand:** Ob eine Datei verschlüsselt ist, zeigt ihr Kopf. Unverschlüsselte und verschlüsselte Dateien können nebeneinander liegen. Neue Dateien entstehen verschlüsselt, sobald ein Schlüssel hinterlegt ist.
- **Ein verlorener Schlüssel** kostet keine Daten. Das Register wird aus den Dateien neu aufgebaut, was bei diesem Bestand viele Stunden dauert.
- Unverschlüsselt bleiben `regeln.toml` (Ordnernamen) und `textebene.log` (Pfade geänderter PDFs). **Annahme**, dass das genügt.

Die Verschlüsselung schützt das Register. Die Originale im Cloud-Ordner schützt sie nicht.

### Neuaufbau

Ein vollständiger Neuaufbau aus den Dateien ist jederzeit möglich. Im Pilot am 18.11.2026 ausgeführt (v0.0.3): Register-Ordner beiseitegelegt, Dateibaum, Inhalte und Vorschauen neu erzeugt. Ergebnis identisch mit dem vorherigen Stand: 3.384 PDFs, 11.131 Seiten, 295 MB Vorschauen. Text in 17 s, Vorschauen in 3 min 16 s.

## 11. Suche und Anzeige

Umgesetzt in v0.0.6 (Stufe 2). Einzelheiten in der Entwicklungsdokumentation v0.0.6.

### Suchindex

- SQLite FTS5 in der Index-Datenbank, ohne eigene Kopie des Textes. Er lässt sich jederzeit aus `seite` und `ocr` neu aufbauen.
- Trigramme über Name und Ordner jeder Datei und über den gültigen Text je Seite **ohne Leerraum und Bindestriche**. So findet die Suche Teile von Wörtern („Bewässerung“ in „Bewässerungssystem“), zusammengesetzte Wörter und Wörter, die die OCR auseinandergerissen hat („Rech nung“). Groß- und Kleinschreibung und Akzente zählen nicht.
- Eine Wortliste (Wörter je Seite, `fts5vocab`) für Tipp- und OCR-Fehler. Findet ein Suchwort weniger als 3 Dokumente, kommen ähnliche Wörter dazu (Abstand 1 bis 7 Zeichen, ab 8 Zeichen Abstand 2). **Annahme**
- Der Index wird nach jedem Einlesen, jedem Lesen der Inhalte und jeder OCR nachgeführt. Auslöser im Register merken, welche Dateien neu indiziert werden müssen.

### Treffer

- Jedes Suchwort muss vorkommen, im Dateinamen, im Ordner oder auf einer Seite. Wörter unter 3 Zeichen werden übergangen.
- Treffer je Dokument. Gewichtung: Dateiname 8, Ordner 3, Text 1. Steht ein Wort nur über eine Wortgrenze hinweg im Text, zählt es 0,7, über ein ähnliches Wort die Hälfte. **Annahme**
- Sortierung nach Relevanz oder nach dem Dokumentdatum (Abschnitt 9), neueste zuerst.
- Filter: Ordner samt Unterordnern, Jahr, Datum von–bis (ein unscharfes Datum wie „2019“ passt bei Überschneidung), Dateityp in Gruppen oder einzeln.
- Ohne Suchwort, nur mit Filter, zeigt die Suche alle passenden Dokumente, neueste zuerst. Ordner geben oft das Thema vor, das im Dokument selbst nicht steht.

### Anzeige

Aufbau und Bedienung wie Bilderwerk:
- Suchfeld, Suche beim Tippen.
- Kriterien mit Ordnerbaum.
- Raster oder Liste mit Vorschau.
- Detailbereich mit Seiten, Angaben und gespeichertem Text.
- Großansicht mit Vergrößern.
- Original öffnen oder im Finder zeigen.
- Kein Textausschnitt, keine Hervorhebung: Gesucht werden Dokumente im Ganzen.
- Oberfläche in Deutsch, hell und dunkel nach System.

## 12. Technik

| Bereich | Festlegung | Stand |
|---|---|---|
| Anwendung | Tauri 2 mit Rust, lauffähig auf macOS, Windows und Linux | entschieden, im Einsatz seit v0.0.1 |
| Oberfläche | React mit TypeScript und Vite, Listen mit `react-virtuoso` | entschieden, im Einsatz seit v0.0.1 |
| Aufbau | Kern als eigene Rust-Bibliothek ohne Oberfläche, die Tauri-Anwendung ist nur eine Hülle darüber | entschieden. Hält den lesenden Teil für eine Web-Oberfläche wiederverwendbar |
| Datenbank | SQLCipher über `rusqlite` (`bundled-sqlcipher`), unter macOS mit CommonCrypto, sonst mit beim Bau übersetztem OpenSSL | entschieden, im Einsatz seit v0.1.0 |
| Suchindex | SQLite FTS5 mit Trigrammen im Register, Wortliste als Automat (`fst`) | entschieden, im Einsatz seit v0.0.6 |
| Office, OpenDocument | `zip` und `quick-xml`, eigene Auslese | entschieden, im Einsatz seit v0.0.6 |
| Pages, Keynote | eigener Leser für die Textspeicher der IWA-Archive, entpackt mit `snap` | entschieden, im Einsatz seit v0.0.6 |
| Bilder | `image` (jpeg, png, tiff, bmp, gif, webp), `zune-heic` für HEIC, `tiff` und `fax` für Fax-TIFF, eigener Leser für TIFF mit Farbpalette (`weezl` für LZW) | entschieden, im Einsatz seit v0.0.6, HEIC und Fax seit v0.0.7, Palette seit v0.0.8 |
| Altes Office | eigener toleranter Leser für OLE-Verbunddateien (`ole.rs`, seit v0.0.9, vorher `cfb`) mit eigenem Leser für doc und ppt, `calamine` für xls ab Excel 97, eigener Leser für Excel 2 bis 95 und Lotus 1-2-3 (`biff.rs`, seit v0.0.9) | entschieden, im Einsatz seit v0.0.7 |
| RTF, HTML, Visitenkarten | eigene Leser (`textformate.rs`) | entschieden, seit v0.0.9 |
| Journal von SQLite | klassisch (`DELETE`), lesende Zugriffe mit `SQLITE_OPEN_READ_ONLY` | entschieden, seit v0.0.9 |
| Vorschau für Text | `ab_glyph` mit DejaVu Sans Mono, Schrift im Repo | entschieden, im Einsatz seit v0.0.7 |
| Regeln und Konfiguration | TOML: `regeln.toml` beim Register, `config.local.toml` je Gerät | entschieden, im Einsatz seit v0.0.7 |
| PDF | PDFium für Textauslese und Rendering, über `pdfium-render`. Die Bibliothek wird zur Laufzeit geladen und kommt aus den fertigen Builds von bblanchon/pdfium-binaries | entschieden, Textauslese seit v0.0.2 |
| OCR | Tesseract 5.5.2, Sprachen Deutsch und Englisch, Sprachdaten `tessdata`. Statisch aus dem Quelltext gebaut (mit Leptonica, über CMake), eigene schmale Anbindung an die C-Schnittstelle. Keine Installation auf dem Gerät | entschieden, im Einsatz seit v0.0.4 |
| Vorschau | AVIF Q25, kurze Kante 1024 px. Kodiert mit libavif und libaom über `libavif-sys`, beim Bau aus dem Quelltext übersetzt (braucht CMake, unter x86 auch NASM) | entschieden, im Einsatz seit v0.0.3, Größenregel seit v0.0.6 |
| Laden aus der Cloud | Dateikoordination von macOS (`NSFileCoordinator`) über `objc2-foundation`, eigenes Paket `registerwerk-macos` | entschieden, im Einsatz seit v0.0.8 |
| Zurück auf "nur online" | Werkzeug `onlineonly` (MIT), wird aufgerufen, wenn installiert | entschieden, im Einsatz seit v0.0.8. Beigabe im Programmpaket offen |
| Platzhalter des File Providers | Kennzeichen `SF_DATALESS` in den Dateiflags | entschieden, seit v0.0.8 |
| Platzhalter der Dropbox | erweitertes Attribut `com.dropbox.placeholder` (`getxattr` in `registerwerk-macos`) | entschieden, seit v0.0.9 |
| Build-Ordner | `target/cargo` über `.cargo/config.toml`, damit `cargo clean` den von der Dropbox ausgenommenen Ordner `target` stehen lässt | entschieden, seit v0.0.8 |
| Schlüssel des Registers | lokaler Schlüsselbund, unter macOS über `security-framework` ohne iCloud. Windows und Linux offen | entschieden, im Einsatz seit v0.1.0 |

Die Oberfläche muss AVIF auf allen Zielsystemen anzeigen. Die Bilder kommen über ein eigenes Protokoll (`vorschau://`) aus dem Register. Unter macOS geprüft in v0.0.3. Windows und Linux sind offen. Wo die eingebettete Browser-Komponente AVIF nicht kann, dekodiert der Rust-Teil das Bild und reicht es weiter.

## 13. Messwerte

Grundlage: Inventur mit PDF-Stichprobe vom 15.11.2026 über die zwei Hauptverzeichnisse des aktuellen Archivs. 3.595 Dateien, davon 3.384 PDFs. Stichprobe: 500 PDFs mit 1.730 Seiten, 1.193 Seiten als Vorschau gerendert.

### PDFs nach Art

| Art | Anteil der PDFs | KB je Seite | Anteil am Volumen |
|---|---|---|---|
| Scan, Farbe (24 Bit, JPEG) | 70 % | 374 | 91 % |
| Scan, Graustufen 8 Bit | 2 % | 77 | 5 % |
| Scan, 1 Bit | 12 % | 65 | 2 % |
| digital erzeugt | 16 % | 33 | 2 % |

- 85 % der Scan-Seiten sind als Farbbild gespeichert, auch wenn sie schwarz-weiß aussehen. Die Scan-App entfernt im Schwarz-Weiß-Modus Schatten, behält für das Schriftbild aber Farb- oder Grauwerte. Die Einstellung bleibt, weil das Schriftbild gefällt.
- Seiten je PDF: Mittel 3,5, Median 2, Maximum 224
- Original im Mittel 255 KB je Seite
- 95 % der Seiten haben eine Textebene. 1,6 % der PDFs haben keinen Text.
- 15 % der PDFs liegen unter 60 % Güte. Vier Fünftel davon stammen von einer Scan-App der Jahre 2018 bis 2020, vor allem Kassenbelege.

### Vorschaugröße je Seite

| Variante bei 1024 px | KB Mittel | KB Median |
|---|---|---|
| AVIF Q25 | 27 | 23 |
| WebP Q25 | 50 | 43 |
| WebP Q50 | 67 | 57 |

### Vorschauen im Pilot (v0.0.3)

Gemessen am 18.11.2026 auf dem Notebook beim Neuaufbau, Anwendung im Release-Modus:

| Größe | Wert |
|---|---|
| Seiten | 11.131 aus 3.384 PDFs |
| Volumen | 295 MB, 27,2 KB je Seite |
| farbig | 34 % der Seiten |
| Dauer | 196 s, 18 ms je Seite (57 Seiten je Sekunde) |
| Rendern | 17 ms je Seite, ein Faden, zu 96 % ausgelastet. Der Engpass |
| Kodieren | 65 ms je Seite und Faden, 8 Fäden zu 46 % ausgelastet |

Hochgerechnet auf 1,5 Mio. Seiten (Schätzung bis v0.0.5): rund 8 Stunden und rund 40 GB. Für die rund 400.000 Seiten des neuen Mengengerüsts: gut 2 Stunden. Mehrere Render-Prozesse könnten das Tempo noch einmal etwa verdoppeln. Das lohnt sich bei diesen Werten nicht.

### OCR im Pilot (v0.0.4)

Gemessen am 18.11.2026 auf dem Notebook, Anwendung im Release-Modus:

| Größe | Wert |
|---|---|
| Seiten ohne Text | 1.185, gut 10 % aller Seiten |
| Text übernommen | 878 Seiten (74 %) |
| weiter ohne Text | 307 Seiten: Fotos, Karten, Handschrift, leere Seiten |
| Dauer | 242 s, 284 Seiten je Minute |
| Rendern mit 300 dpi | 58 ms je Seite, ein Faden |
| Erkennen | 1,7 s je Seite und Faden, 8 Fäden zu 99 % ausgelastet. Der Engpass |

Hochgerechnet auf 1,5 Mio. Seiten (Schätzung bis v0.0.5): rund 160.000 Seiten ohne Text, etwa 9 bis 10 Stunden auf dem Notebook. Für rund 400.000 Seiten: rund 43.000 Seiten ohne Text, etwa 2,5 Stunden.

### Suche im Pilot (v0.0.6)

Gemessen mit `pdf_pruefen --suche` auf einer Kopie des Pilot-Registers (10.972 Seiten, 19,7 MB Text):

| Größe | Wert |
|---|---|
| Suchindex | 68 MB, 3,4-mal der Text, rund 6 KB je Seite |
| Aufbau | rund 8 bis 9 s |
| Wortliste | 255.000 Wörter, 136 ms bis sie im Speicher liegt |
| Antwortzeit | Median 1 bis 3 ms, höchstens 10 ms (Notebook) |

Ein zweiter Trigramm-Index über den Text mit Leerraum hätte den Index verdoppelt und fand nichts zusätzlich. Er entfiel.

### Mengengerüst (Schätzung)

| Größe | Wert |
|---|---|
| Daten in der Cloud insgesamt | rund 0,9 TB |
| davon nicht im Register | Sicherungen 203 GB, Kamera-Uploads 35 GB, Bilder im Archiv knapp 400 GB |
| nur mit Name und Datum im Register | Musik 192 GB, eBooks 11 GB |
| Archivmaterial | höchstens rund 100 GB (Größen der Dropbox, November 2026) |
| Seiten | rund 400.000 bei 255 KB je Seite im Original |
| Vorschauen | rund 13 GB (32 KB je Seite) |
| Text, OCR, Metadaten | rund 1,5 GB (4 KB je Seite) |
| Suchindex | rund 2,5 GB (6 KB je Seite) |
| Register zusammen | **rund 17 GB**, realistisch 10 bis 20 GB |

Die Hochrechnung nimmt an, dass das übrige Archiv den beiden geprüften Verzeichnissen ähnelt. Das ist nicht geprüft. Die tatsächliche Größe zeigt die Erstindizierung (Stufe 6). Bis v0.0.5 war von 1,5 Mio. Seiten und rund 40 GB die Rede, das schloss die Bilder im Archiv mit ein.

## 14. Pilot und Stufen

### Pilot

Der Pilot umfasste bis v0.0.6 nur die zwei Hauptverzeichnisse des aktuellen Archivs mit rund 3.600 Dateien. Er war schnell neu aufgebaut und deckte alle Fälle ab: Farb-, Graustufen- und 1-Bit-Scans, digital erzeugte PDFs, schwache OCR, Platzhalter und andere Dateitypen. Seit v0.0.7 bestimmen die Ordnerregeln den Umfang, einen festen Pilotumfang gibt es nicht mehr.

### Vorschlag für die Stufen

| Stufe | Inhalt | Ergebnis |
|---|---|---|
| 1 | Indexer für PDFs: Dateibaum, Textauslese, Vorschauen, Register | Pilot-Register aus den Dateien aufgebaut, Neuaufbau getestet |
| 2 | Anwendung: Suche, Trefferliste, Seitenvorschau, Original öffnen | tägliche Nutzung auf dem Pilotbestand |
| 3 | Verschlüsselung, Ordnerregeln, Behandlungsstufen | Register im Cloud-Ordner, zweites Gerät liest |
| 4 | Delta-Lauf, Sperre und Stand-Zähler, Erkennen verschobener Dateien | laufender Betrieb auf dem Notebook |
| 5 | OCR für Seiten ohne Text, Zurückschreiben in Scans | Suchqualität auf dem Pilotbestand geprüft |
| 6 | Erstindizierung des gesamten Archivs in Tranchen | vollständiges Register |

Die Reihenfolge ist ein Vorschlag. **Annahme**. Stufe 5 wurde vorgezogen (v0.0.4 und v0.0.5), damit die Suche am endgültigen Text eingestellt wird. Stufe 2 folgte in v0.0.6, die Ordnerregeln aus Stufe 3 in v0.0.7, der Delta-Lauf aus Stufe 4 in v0.0.8. Von Stufe 4 fehlen noch die Sperre mit Stand-Zähler und das Erkennen verschobener Dateien. Stufe 3 ist mit der Verschlüsselung in v0.1.0 abgeschlossen.

### Stand der Umsetzung

| Version | Inhalt | Stufe |
|---|---|---|
| v0.0.1 | Gerüst der Anwendung, Einlesen des Dateibaums ins Register mit Abgleich (neu, geändert, entfernt), Datum aus dem Dateinamen, Übersicht in der Oberfläche | Beginn von Stufe 1, noch ohne PDF-Inhalte und Vorschauen |
| v0.0.2 | PDFium angebunden, Text je Seite im Register, Gütemaß und Seitenart, Datum aus PDF und Änderungsdatum, Fortsetzen nach Abbruch | Stufe 1 ohne Vorschauen |
| v0.0.3 | Vorschauen aller Seiten als AVIF in Scheiben, Seitenansicht zum Prüfen, Neuaufbau des Pilot-Registers getestet | Stufe 1 abgeschlossen |
| v0.0.4 | Tesseract statisch eingebaut, OCR der Seiten ohne Text ins Register, gültiger Text je Seite, Textvergleich in der Prüfansicht | erster Teil von Stufe 5 |
| v0.0.5 | Textebene in Scans zurückgeschrieben, Lage der Wörter aus Tesseract, Grenze "ohne Text" bei 10 Zeichen, Lesen gedrehter Seiten korrigiert | Stufe 5 abgeschlossen |
| v0.0.6 | Suchindex und Suche mit Filtern, Suchansicht wie Bilderwerk, Text aus Office, OpenDocument, Pages und Text, Vorschau und OCR für Bilder, Vorschauen nach der kurzen Kante | Stufe 2 abgeschlossen, Teile von Stufe 3 (Dateitypen) |
| v0.0.7 | Ordnerregeln mit Stufen, Reiter "Ordner", feste Einteilung der Dateitypen, Äste und Geräte, Entfernen nur mit Grund und Rückfrage, altes Office, HEIC, Vorschau für Text, Fax-TIFF, Register-Ort wählbar | Stufe 3 ohne Verschlüsselung |
| v0.0.8 | Reiter "Register" als Delta-Lauf: Prüfung beim Öffnen, Ordnerbaum mit Auswahl, Dateilisten je Zahl, Verarbeiten durch alle Schritte, Laden aus der Dropbox über die Dateikoordination, zurück auf "nur online" mit `onlineonly`. Scheiben 200 MB mit Aufteilen, TIFF mit Farbpalette, Platzhalter des File Providers, Build-Ordner `target/cargo` | Stufe 4 ohne Sperre und Inhalts-Hash |
| v0.0.9 | Fehlerrelease: Lesen und Prüfen ändern das Register nicht mehr (klassisches Journal, nur lesend geöffnet, Schreiben nur bei Änderung), "nur online" am Attribut der Dropbox, Start mit der Suche und gelber Knopf, toleranter Leser für alte Office-Formate, Excel 2 bis 95, Lotus, Word 2, Pages '09, grober Rückfall, RTF, HTML, csv, tsv, log, vcf. Seit v0.0.9 nutzt Kim Registerwerk produktiv, die nächste Version heißt v0.1.0 | Korrekturen in Stufe 3 und 4 |
| v0.1.0 | Verschlüsselung des Registers mit SQLCipher, eigener Schlüssel mit 256 Bit im Passwortsafe und je Gerät im Schlüsselbund, Umstellung per Knopf Datei für Datei, Sperre ohne Schlüssel, Prüfung des Dateibaums anhaltbar. Erste Version nach Beginn des produktiven Einsatzes | Stufe 3 abgeschlossen |
| v0.1.1 | Reiter "Ordner" ohne eigenes Einlesen, Kreisel bei Tätigkeit im Hintergrund, Dateien nur mit Namen im Baum, Entfernen als abwählbarer Schritt von "Verarbeiten", `npm test` und `npm run pruefen` mit Zusammenfassung und Abdeckung | Abschluss des Ablaufs aus Stufe 4 |

Einzelheiten je Version stehen in der Entwicklungsdokumentation unter `docs/`.

## 15. Offene Punkte

| Punkt | Wann |
|---|---|
| Inventur der Verzeichnisse außerhalb des aktuellen Archivs. Bestimmt die Endgröße, nicht den Bau | vor Stufe 6 |
| Bau von Tesseract und Leptonica unter Windows | vor dem ersten Windows-Paket |
| Sprachdaten ins Programmpaket legen (Ressource `tessdata/standard` mit Lizenztexten) | mit dem ersten Paket |
| Anzeige von AVIF unter Windows und Linux. macOS ist geprüft | Stufe 2 |
| Bau von libaom unter Windows | vor dem ersten Windows-Paket |
| `onlineonly` als Beigabe ins Programmpaket legen, mit Lizenztext | mit dem ersten Paket |
| Testabdeckung: 90 % der Zeilen je Datei, 90 % der Funktionen in der Summe, Ausnahmen mit Grund. Ziel danach 90 % für jede Datei und jede Funktion | v0.1.2 |
| Ohne „zuletzt gesehen“ je Datei: Unterscheidung „anderes Gerät“ gegenüber „gelöscht“ auf dem Mac mini prüfen | mit dem zweiten Gerät |
| Laden bei Anbietern über den File Provider (Google Drive, OneDrive): löst Lesen den Download aus? | vor der Aufnahme solcher Äste |
| Hinweis „nur klassische Dropbox vollständig unterstützt“ auf der Webseite | mit der Webseite |
| Schlüsselbund unter Windows und Linux, SQLCipher unter Windows bauen | vor dem ersten Windows-Paket |
| `pdf_pruefen` (`--suche`, `--aufteilen`) für ein verschlüsseltes Register | bei Bedarf |
| `regeln.toml` und `textebene.log` verschlüsseln? | bei Bedarf |
| Sperre mit Stand-Zähler für den Gerätewechsel | Stufe 4 |
| Text aus Numbers. Mehrseitige TIFF. Excel-Mappen mit dem Standardkennwort von Excel (Kim speichert sie bisher als .xlsx neu) | später |
| Antwortzeiten der Suche im ganzen Archiv | Stufe 6 |
| Vergrößern über 100 %: Original bei Bedarf hoch aufgelöst rendern | später |
| Verhalten eines europäischen Cloud-Anbieters bei der Replikation des Registers | vor einem Anbieterwechsel |
| Lösung für mobile Geräte | gesondert |

## 16. Entscheidungen im Überblick

| Thema | Entscheidung | Grund |
|---|---|---|
| Name | Registerwerk | Fortsetzung der "…werk"-Reihe. Beschreibt das Prinzip: ein Register über Dateien, die woanders liegen |
| Icon | Karteikarten mit versetzten Registerreitern, mittlere Karte farbig | zeigt wörtlich ein Register, passt zur Formensprache der Reihe |
| Dateien als einzige Wahrheit | ja | Erfahrung aus mehreren verlorenen Archivsystemen |
| Archiv verkleinern | nein | Risiko von Informationsverlust, nicht von Hand prüfbar |
| Auf den Cloud-Anbieter warten | nein | seit Jahren ohne Ergebnis |
| Programmiersprache | kein Swift | bindet an Apple und schließt Wiederverwendung im Web aus |
| Zugriff auf Dateien | Dateisystem, keine Anbieter-Schnittstelle | Unabhängigkeit vom Anbieter |
| Vorschau | alle Seiten, kurze Kante 1024 px, lange höchstens 3072 px, AVIF Q25 | lesbar, klein, flüssig beim Blättern. Die Seitengröße in PDFs ist unzuverlässig (v0.0.6) |
| Vorschau 640 px | nein | normale Schrift kaum lesbar, zweimal geprüft |
| Vorschau nur der ersten Seite | nein | alle Seiten müssen schnell erreichbar sein |
| OCR-Werkzeug | Tesseract | schon im Einsatz, unabhängig von Apple |
| OCR wofür | nur für Seiten ohne Text (höchstens 10 Zeichen), der erkannte Text gilt ab 11 Zeichen | bei schwachem Text war der eingebettete Text im Pilot nicht schlechter, oft besser als Tesseract |
| Eingebetteter Text | wird nie überschrieben, weder in der PDF noch im Register | die OCR der Scan-App ist besser als Tesseract |
| OCR-Text bei Scans | ins Register und in die PDF | durchsuchbare PDF bleibt auch außerhalb von Registerwerk erhalten |
| OCR-Text bei digitalen PDFs ohne Text | nur ins Register | Einbetten würde das Original verschlechtern |
| Einbindung von Tesseract | Bibliothek, statisch aus dem Quelltext, eigene Anbindung | keine Installation und Pflege, auch bei Weitergabe. `tesseract-rs` legt Bau-Cache im Benutzerprofil ab und lädt beim Bau aus dem Netz |
| Auflösung für OCR | 300 dpi | deutlich besser als 200 dpi |
| Sprachdaten | `tessdata` (Standard), deu+eng | gleiches Ergebnis wie `tessdata_best`, gut 30 % schneller |
| Reihenfolge der Stufen | OCR (Stufe 5) vor der Suche (Stufe 2) | die Suche wird am endgültigen Text eingestellt |
| Ablage der Vorschauen | in der Datenbank, verteilt auf Scheiben von rund 200 MB (bis v0.0.7: 1 bis 2 GB) | nur Geändertes wird repliziert, Anbieterwechsel bleibt offen |
| Sicherungskopien | keine | sollen kein Volumen erzeugen, das von Hand bereinigt werden muss |
| Datum | Dateiname, dann PDF, dann Datei | Dateinamen tragen fast immer ein Datum |
| Verschlüsselung | Register mit SQLCipher verschlüsselt, fertiger Schlüssel mit 256 Bit von Kim im eigenen Passwortsafe, je Gerät im lokalen Schlüsselbund (v0.1.0) | hochsensible Inhalte, keine Bindung an einen Hersteller. Stärkster Schutz gegen Durchprobieren, keine Schlüsseldatei, die verloren gehen kann |
| Umstellung auf Verschlüsselung | per Knopf, Datei für Datei mit Prüfung, nach zweiter Eingabe des Schlüssels, gleiche Dateinamen. Erst an einer Kopie geprüft (v0.1.0) | abbruchsicher, kein vertippter Schlüssel. Die Originale liegen ohnehin unverschlüsselt daneben |
| Prüfung beim Start | lässt sich anhalten, beim Wählen eines Registers und beim Verschlüsseln automatisch (v0.1.0) | blockierte bei 290.000 Dateien das Wählen (Kim) |
| Betrieb | Erstlauf stationär, danach Delta auf dem Notebook | kein Dauerbetrieb, keine dauerhafte Vollkopie |
| Systemsuche | nicht angebunden | gesucht wird in Registerwerk |
| KI | nicht vorgesehen | kein Bedarf |
| Pilot | zwei Hauptverzeichnisse, rund 3.600 Dateien, bis v0.0.6 | klein, vollständig in den Fällen, schnell neu aufgebaut |
| Oberfläche | React mit TypeScript und Vite | bewährt in Stellwerk, überzeugt in Gestaltung und Möglichkeiten |
| Pfade im Register | beginnen mit dem Ast-Namen, einheitliche Schreibweise | Register gilt auf jedem Gerät und Betriebssystem, für beliebige Ordner (v0.0.7) |
| Wechsel auf "nur online" | keine Änderung, solange das Änderungsdatum gleich bleibt | sonst würde jedes Auslagern Text und Vorschauen neu erzeugen |
| Anbindung von PDFium | `pdfium-render`, Bibliothek per Skript geladen, nicht im Repo | fertige Builds für alle Zielsysteme, freizügige Lizenzen, keine Binärdateien im Repo |
| "Nur online" in den Schritten | nicht öffnen, bleiben ausstehend. Geladen wird nur im Delta-Lauf, nach Auswahl | Kim behält die Kontrolle, was geladen wird |
| Verarbeitungsstand | Änderungsdatum und Größe beim Lesen, dazu eine Verfahrensversion | geänderte Dateien und Verfahren werden gezielt neu gelesen, ein Abbruch kostet nichts |
| Suchindex | SQLite FTS5 mit Trigrammen im Register, Text ohne Leerraum, Wortliste für ähnliche Wörter | bleibt im Register (Verschlüsselung, Neuaufbau). Findet Teilwörter und auseinandergerissene Wörter (v0.0.6) |
| Treffer | je Dokument, Name vor Ordner vor Text, ohne Textausschnitt | gesucht werden Dokumente, nicht Stellen |
| Ordnerwahl in der Suche | aufklappbarer Baum | die Bedeutung eines Ordners ergibt sich aus dem übergeordneten |
| Office, Pages | Text ja, Vorschau nein | Text ohne fremdes Programm. Vorschau bräuchte LibreOffice |
| Bilder im Archiv | Vorschau und OCR, Text nur ins Register | meist versehentlich als Bild gespeicherte Belege |
| Oberfläche der Suche | wie Bilderwerk | Wiedererkennung in der Reihe |
| Entfernen aus dem Register | nur mit Grund und nach Rückfrage mit Dateiliste. Ein fehlender Ordner löscht nie | ein fehlender Cloud-Ordner darf das Register nicht leeren (v0.0.1, v0.0.7) |
| Ordnerregeln | ohne Regel wird nichts gelesen, Vererbung, genaueste Regel gewinnt, drei Stufen | Ordner werden bewusst eingeschlossen (v0.0.7) |
| Dateitypen | feste Listen: gelesen, nur Name, ignoriert | Quelltext und Programmdateien sollen nie ins Register. Ersetzt die Stufe "Nur Dokumente" |
| Ordner außerhalb des Cloud-Ordners | Äste mit Namen, Ort je Gerät | mehrere Cloud-Speicher, Dokumente, Downloads |
| Mehrere Geräte | ein Register und gemeinsame Regeln, jedes Gerät darf einlesen, das Register merkt sich, wer einen Ordner gesehen hat | Ergänzen von jedem Gerät. Unterscheidet "nicht auf diesem Gerät" von "gelöscht" |
| Ort des Registers | je Gerät wählbar, bei Kim direkt im Cloud-Ordner | ein Register für alle Geräte ohne eigenen Schritt zum Veröffentlichen |
| HEIC | `zune-heic` | reines Rust, im Vergleich an 11 Fotos praktisch gleich mit libheif |
| Altes Office | eigener Leser, kein fremdes Programm | wie bei den neuen Formaten |
| AVIF-Encoder | libavif mit libaom, Stufe 8 | bei gleicher Qualität und Größe knapp 3-mal so schnell wie rav1e. Kostet einen C-Bau mit CMake |
| Einteilung der Scheiben | nach Erfassungsreihenfolge, neue Scheibe ab rund 200 MB (v0.0.8) | Delta-Läufe ändern nur die jüngste Scheibe, die Cloud gleicht wenig neu ab |
| Aufteilen alter Scheiben | Knopf im Datenbank-Symbol, nicht still beim Start | der Lauf lädt einmal alles neu hoch, Kim wählt den Zeitpunkt |
| Graustufen | echte Graustufen (4:0:0) für Seiten ohne Farbe | kleiner, ohne sichtbaren Verlust |
| Entwicklungsmodus | Kern und Abhängigkeiten auch im Entwicklungsmodus optimiert | ohne Optimierung ist der Encoder um ein Vielfaches langsamer, auch weil seine generischen Teile erst im Kern übersetzt werden |
| Zurückschreiben in die PDF | inkrementell mit `lopdf`, Schrift `glyphless.ttf` aus Tesseract im Repo | Original bleibt Byte für Byte erhalten. Gleich breite Zeichen lassen jedes Wort genau strecken |
| Änderungsdatum nach dem Zurückschreiben | alter Wert | Sortierung kann darauf aufbauen. Das Register führt Größe und Datum selbst nach |
| Ablage des Projekts | im Cloud-Ordner, Build-Ordner bleiben im Projekt und sind von der Synchronisation ausgenommen | sichert auch die nicht versionierten Inhalte. Eine zentrale Ablage je Projekt, keine Fragmente an anderen Orten |
| Build-Ordner von cargo | `target/cargo` | `cargo clean` löscht den ganzen Build-Ordner samt Ausnahme von der Dropbox. So bleibt `target` stehen (v0.0.8) |
| Ablauf im Reiter "Register" | Prüfung beim Öffnen, Ordnerbaum mit Haken, ein Knopf „Verarbeiten“ für alle Schritte. Bisherige Ansicht unter „Einzelheiten“ | aufgeräumt statt verstreuter Hinweise (Kim, v0.0.8) |
| Vorauswahl im Delta-Lauf | alles angehakt, auch "nur online" | neue Belege kommen täglich und liegen meist nur online |
| Dateien ansehen | Klick auf jede Zahl öffnet die Liste der Dateien | Kim will sehen, was gefunden wurde, bevor geladen wird |
| Verarbeiten entfernt | seit v0.1.1 das Ausgewählte, abwählbar, Rückfrage ab mehr als 100 Einträgen. Bis v0.1.0 nichts | Kim: ein Schritt wie die anderen. Die Rückfrage schützt vor einem versehentlich auf „Aus“ gestellten Ordner |
| Laden aus der Dropbox | Dateikoordination von macOS, vier Dateien gleichzeitig, gemeinsames Warten, „Nicht warten, weiter“ | einfaches Lesen lädt nicht, `open` öffnet Fenster. Warteschlange oder offline dürfen den Lauf nicht aufhalten |
| Zurück auf "nur online" | `onlineonly`, nur für die in diesem Lauf geladenen Dateien | keine Handarbeit danach. Bewusst lokale Dateien bleiben lokal |
| Unterstützte Cloud-Speicher | vollständig nur die klassische Dropbox unter `~/Dropbox` | Laden und Auslagern sind nur dort gemessen und möglich. Muss auf der Webseite deutlich stehen |
| Schreiben in die PDFs im Delta-Lauf | ohne zweiten Klick | die Textebene wird nur angehängt, im Pilot ohne Probleme (Kim) |
| Platzhalter des File Providers | gelten wie 0 Byte als "nur online" | sonst würden sie beim Lesen still geladen |
| TIFF mit Farbpalette | eigener Leser | `tiff` liest sie nicht. Kommt im Altarchiv vor (Scans von 2003) |
| Schreiben ins Register | nur bei neu, geändert oder entfernt. Keine Protokolle, keine zweite Datenbank (v0.0.9) | jede Änderung löst einen Upload der Dropbox aus, der andere Abgleiche behindert |
| Journal von SQLite | klassisch (`DELETE`) statt WAL (v0.0.9) | WAL ändert schon beim Lesen die Datei `-shm` |
| Erkennen von "nur online" | bei 0 Byte am Attribut `com.dropbox.placeholder` (v0.0.9) | einzige gemessene Unterscheidung von lokal leeren Dateien |
| Startseite | Suche, Abgleich im Hintergrund, gelber Knopf neben den Reitern (v0.0.9) | Suchen ist der Alltag, Verarbeiten die Ausnahme |
| Zeitpunkt der Prüfung | beim Start, auf Wunsch und nach eigenen Aktionen, nie nach Zeit (v0.0.9) | keine unerwarteten Läufe während der Suche |
| Unlesbare alte Office-Dateien | eigene tolerante Leser, sonst grober Rückfall mit Vermerk (v0.0.9) | fast alle ließen sich in den Originalprogrammen öffnen |
| Weitere Textformate | rtf, csv, tsv, log, htm, html, vcf lesen. eml, mbox, ics nur mit Namen (v0.0.9) | Kim |
| Reiter "Ordner" nach dem Speichern | Kasten mit Zahlen und Knopf in den Reiter "Register", kein automatischer Wechsel (v0.1.1) | Kim |
| Dateien nur mit Namen im Baum | fester grauer Haken, Marke „nur Name“ (v0.1.1) | ein leerer Haken hieße „wird nicht verarbeitet“ (Kim) |
| Tätigkeit im Hintergrund | Kreisel am Reiter "Register" (v0.1.1) | Kim will sehen, was läuft |
| Tests | Zusammenfassung wie in Stellwerk, Abdeckung mit `cargo llvm-cov`. Ziel 90 % je Datei und Funktion, 100 % ist kein Ziel (v0.1.1) | Kim |

## 17. Verworfene Alternativen

| Alternative | Warum verworfen |
|---|---|
| Archiv durch Neukomprimierung verkleinern | Risiko für Lesbarkeit und Reproduzierbarkeit |
| Fertiges Archivsystem mit eigener Ablage | übernimmt die Dateien in eine eigene Struktur und widerspricht Grundsatz 1 |
| Warten auf Volltextsuche für "nur online"-Dateien | nicht absehbar, löst die Bindung an den Anbieter nicht |
| Native Apple-Anwendung | Bindung an ein Betriebssystem |
| Schnittstelle des Cloud-Anbieters für die Änderungserkennung | Bindung an den Anbieter. Das Dateisystem genügt |
| Stationärer Rechner als dauerhafte Vollkopie | kein Dauerbetrieb gewünscht, Platz wird anderweitig gebraucht |
| Build-Ordner außerhalb des Projekts ablegen | verteilt die Projekte auf mehrere Orte |
| AVIF-Encoder rav1e (über ravif), reines Rust | im Pilot 92 ms je Seite statt 19 ms. Ein reiner Rust-Encoder mit dem Tempo von libaom existiert nicht |
| AVIF-Encoder SVT-AV1 | in der geprüften Version 1.7 für Einzelbilder nicht verlässlich, die Geschwindigkeitsstufe wirkte nicht |
| OCR auch für schwachen Text (Güte unter 60 %) | im Pilot nicht besser als der eingebettete Text, oft schlechter. Das Gütemaß zeigte trotzdem Verbesserungen |
| `tessdata_best` | gleiches Ergebnis wie `tessdata`, gut 30 % langsamer |
| Tesseract als Programm aufrufen und mitliefern | braucht eine eigene Installation oder Bündelung, ist schwerer zu pflegen |
| Scheiben nach Jahr des Dokumentdatums | ein nachträglich gescanntes altes Dokument hätte eine alte Scheibe geändert |
| Eine einzige große Datenbankdatei | jede Änderung beträfe die ganze Datei, die Arbeitskopie bräuchte doppelten Platz, ein Schaden träfe alles |
| Suchindex mit Tantivy | eigene Indexdateien außerhalb von SQLite, Fremdkörper bei Verschlüsselung und Replikation |
| Kennwort, aus dem der Schlüssel abgeleitet wird | Stärke hängt am Kennwort, SQLCipher leitet je Datei neu ab, bei vielen Scheiben spürbar beim Start (v0.1.0) |
| Zufallsschlüssel in einer Datei neben dem Register, durch ein Kennwort geschützt | eine Datei mehr, deren Verlust das Register verschließt. Der billige Kennwortwechsel ist nur scheinbar sicher (v0.1.0) |
| Neue Dateinamen bei der Umstellung, damit die unverschlüsselten Stände endgültig aus der Dropbox gelöscht werden können | die Originale liegen ohnehin unverschlüsselt in derselben Dropbox (Kim, v0.1.0) |
| Schlüssel bei jedem Start eingeben | unpraktisch, und das laufende Herauslesen aus dem Passwortsafe wäre selbst ein Risiko (Kim, v0.1.0) |
| Zweiter Trigramm-Index über den Text mit Leerraum | doppelte Größe, fand nichts zusätzlich (Messung v0.0.6) |
| Ordnerwahl über die Suche nach Ordnernamen | der Name allein trägt die Bedeutung oft nicht |
| Vorschauen im festen Maßstab bezogen auf A4 | falsche Seitengrößen in PDFs machten Belege unlesbar |
| Stufe "Nur Dokumente" für Entwicklungsprojekte | die feste Einteilung der Dateitypen leistet dasselbe in jedem Ordner |
| Eigene Regeln je Gerät | Kim will ein Register und eine Auswahl für alle Geräte |
| libheif für HEIC | C-Bibliothek, ein reiner Rust-Decoder liefert dasselbe Bild |
| Entfernen, sobald eine Datei im Dateibaum fehlt | ein nicht eingehängter oder verschobener Ordner hätte seinen Bestand gelöscht |
| Laden durch Öffnen und Lesen eines Bytes | die klassische Dropbox lädt dabei nichts (Messung 22.11.2026) |
| Laden über `open` (Launch Services) | lädt, öffnet aber die Datei sichtbar in Vorschau |
| Laden über Quick Look (`qlmanage`) | blieb im Test hängen |
| Datei für Datei 2 Minuten warten | eine volle Warteschlange hielt den ganzen Lauf auf |
| Scheiben still beim ersten Start aufteilen | einmaliger großer Upload zu einem Zeitpunkt, den Kim nicht wählt |
| Suchindex der Index-Datenbank in eine eigene Datei | jeder Beleg ändert Text und Index, es würden beide Dateien abgeglichen |
| WAL-Journal für das Register im Cloud-Ordner | schon Lesen ändert `-shm` und `-wal`, die Dropbox gleicht bei jedem Zugriff ab (v0.0.9) |
| Vermerk „zuletzt gesehen“ je Datei | änderte bei jedem Durchlauf jede Zeile des Registers (v0.0.9) |
| Prüfung alle 15 Minuten im Hintergrund | unerwartete Läufe stören die Suche (Kim, v0.0.9) |
| `cfb` für alte Office-Dateien | lehnt Dateien mit kleinen Fehlern ab, die Word und Excel öffnen (v0.0.9) |
| Entschlüsseln von Excel mit dem Standardkennwort | Aufwand für 9 Dateien zu hoch, Kim speichert sie neu (v0.0.9) |

## 18. Werkzeuge

- `archiv_inventur.py`: Inventur und PDF-Stichprobe der Konzeptphase, nur lesend
- `vorschau_vergleich.py`: Vorschauformate nebeneinander, Konzeptphase, nur lesend
- `dropbox_ausschluss.sh`: nimmt die Build-Ordner des Projekts von der Synchronisation des Cloud-Ordners aus. Einmal je Gerät auszuführen
- `pdfium_holen.sh`: lädt die PDFium-Bibliothek samt Lizenztexten nach `bibliotheken/pdfium/`. Einmal je Gerät auszuführen
- `tesseract_holen.sh`: lädt Quelltext von Tesseract und Leptonica sowie die Sprachdaten `standard` nach `bibliotheken/tesseract/`. Einmal je Gerät auszuführen. Mit `--best` zusätzlich `tessdata_best` für Vergleiche. Der Quelltext muss liegen bleiben, cargo braucht ihn für jeden neuen Bau
- `pdf_pruefen`: liest PDFs mit dem Kern, ohne das Register anzufassen. Mit `--suche` baut es den Suchindex in einer Kopie des Registers unter `target/suche_messung/` auf und misst Größe und Antwortzeiten, mit `--anfrage "…"` sucht es in dieser Kopie. Für Prüfungen nach einem PDFium-Update, auf einem neuen Gerät oder bei einzelnen Problemdateien. Mit `--vorschau N` ein Tempotest der Vorschauen, mit `--render N` unkomprimierte Seiten als Vorlage für Encoder-Vergleiche, mit `--ocr N` eine Messung der OCR mit den vorhandenen Sätzen der Sprachdaten, mit `--textebene N` eine Probe der Textebene an Kopien, mit `--aufteilen <Ordner>` eine Probe des Aufteilens der Scheiben an einer Kopie des Registers unter `target/aufteilen_probe/` samt Vergleich jeder Vorschau (v0.0.8). `--suche` und `--aufteilen` arbeiten seit v0.1.0 nur mit einem unverschlüsselten Register. Die Ausgaben enthalten Dokumentinhalte und landen in `target/`
- `scripts/pruefen.mjs` und `scripts/auswertung.mjs` (v0.1.1): `npm test` und `npm run pruefen` mit Zusammenfassung, Abdeckung gesamt und je Paket. Ausgaben in `target/pruefen/`
- `onlineonly` (fremd, MIT, über Homebrew): stellt Dateien der klassischen Dropbox auf "nur online". Registerwerk ruft es im Delta-Lauf auf, wenn es installiert ist

Die Skripte liegen im Repo unter `tools/`. Die Ausgaben der beiden Analyse-Skripte enthalten Dokumentinhalte und Dateinamen und gehören nicht ins Repo.
