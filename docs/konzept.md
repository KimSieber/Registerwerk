# Registerwerk: Konzept

Stand: 19.11.2026 (ergänzt um die Festlegungen aus v0.0.1 bis v0.0.5). Dieses Dokument hält alle Entscheidungen der Konzeptphase fest und ist die Grundlage für die Umsetzung. Konkrete Pfade, Ordnernamen und Zugangsdaten stehen nicht hier, sondern in `docs/intern/` (nicht versioniert).

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
5. **Nur ein Schreiber.** Es indiziert immer genau ein Gerät. Alle anderen lesen.
6. **Kein dauerhaftes Backup-Volumen.** Das Programm legt keine Sicherungskopien an, die später von Hand gesucht und entfernt werden müssten.

## 3. Umfang

### Enthalten

- Indizieren aller Dateien der konfigurierten Verzeichnisse nach Behandlungsstufe
- Volltext aller Seiten, nicht nur der ersten
- Vorschau aller Seiten
- Fehlertolerante Suche mit Gewichtung von Dateiname und Pfad
- Öffnen des Originals aus dem Cloud-Ordner
- OCR für Seiten ohne Text
- Anwendung mit eigener Oberfläche für Indizierung, Suche und Anzeige

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
- **Laufender Betrieb:** Prüfung und Delta-Befüllung überwiegend auf dem Notebook. Der stationäre Rechner läuft nicht dauerhaft und hält keine dauerhafte Vollkopie.
- **Zuwachs:** rund 1.000 bis 2.000 Dokumente pro Jahr, etwa 3 Promille des Bestands.
- **Ein Schreiber:** Organisatorisch ist sichergestellt, dass das Programm nur auf einem Gerät schreibend geöffnet ist. Technisch sichert eine Sperre mit Stand-Zähler den Gerätewechsel ab (Abschnitt 10).

### Scan-Prozess

Der bestehende Scan-Prozess bleibt unverändert: Scan per Smartphone-App mit Bildoptimierung und OCR, Benennung, Ablage als PDF direkt im Cloud-Ordner. Die Datei erscheint danach als Platzhalter oder lokale Datei auf den Rechnern und wird beim nächsten Delta-Lauf ins Register aufgenommen.

## 5. Zugriff auf die Dateien

### Erkennen neuer und geänderter Dateien

- Der Indexer durchläuft den Dateibaum und vergleicht ihn mit dem Register.
- Neu ist jeder Pfad, den das Register nicht kennt. Ein Datum wird dafür nicht gebraucht.
- Geändert ist eine Datei, deren Größe oder Änderungsdatum abweicht. Ausnahme ist der Wechsel zwischen lokal und "nur online", siehe unten.
- Entfernt ist ein Pfad, der im Register steht und im Dateibaum fehlt. Der Eintrag und seine Vorschauen werden aus dem Register entfernt. **Annahme**
- Entfernt wird nur in Verzeichnissen, die der Durchlauf vollständig gelesen hat. Fehlt ein konfiguriertes Verzeichnis auf dem Gerät oder gab es darin Lesefehler, bleibt sein Bestand im Register unangetastet. Ein nicht eingehängter Cloud-Ordner leert so nicht das Register. Umgesetzt in v0.0.1.
- Verschobene und umbenannte Dateien sollen über einen Inhalts-Hash erkannt werden, damit Text und Vorschauen nicht neu erzeugt werden müssen. **Annahme**, wird mit dem Delta-Lauf umgesetzt.

### Platzhalter ("nur online")

Gemessen am 15.11.2026 auf dem Notebook:

- Platzhalter melden 0 Byte, 0 belegte Blöcke und keine besondere Markierung. Sie sind für ein Programm von einer leeren Datei nicht zu unterscheiden.
- Name, Pfad und Änderungsdatum sind lesbar, die Größe nicht.
- Das Auflisten lädt nichts herunter. Das Öffnen einer Datei löst den Download aus.

Folgen:

- Eine Datei mit 0 Byte gilt als "nur online oder leer".
- Zum Indizieren öffnet der Indexer die Datei. Der Cloud-Client lädt sie dabei automatisch.
- Bis zum Delta-Lauf (Stufe 4) öffnet der Indexer nur lokal vorhandene Dateien. "Nur online"-Dateien bleiben ausstehend. Für die Erstindizierung werden die Ordner von Hand offline verfügbar gemacht. Umgesetzt in v0.0.2.
- Das Gesamtvolumen von "nur online"-Dateien lässt sich über das Dateisystem nicht ermitteln.
- Wechselt eine Datei bei gleichem Änderungsdatum zwischen lokal und "nur online", gilt sie als unverändert. Das Register behält die letzte bekannte Größe und merkt sich nur den Zustand. Erst ein anderes Änderungsdatum bedeutet einen neuen Inhalt. Umgesetzt in v0.0.1. **Annahme**: Der Cloud-Client lässt das Änderungsdatum beim Auslagern und Laden unverändert. Das wird im Pilot beobachtet.
- Eine Datei, die bisher nur als Platzhalter gesehen wurde, hat im Register keine Größe.

### Zurück auf "nur online"

Ob sich eine Datei nach dem Indizieren per Programm wieder auf "nur online" setzen lässt, ist zu testen. Es ist eine Komfortfunktion. Als Rückfallweg wird das Archiv-Verzeichnis etwa einmal im Jahr von Hand zurückgesetzt, was bei dem geringen Zuwachs genügt.

### Original öffnen

Die Anwendung öffnet die Datei über ihren Pfad im Cloud-Ordner. Steht sie auf "nur online", lädt der Cloud-Client sie beim Öffnen. Das entspricht dem Vorgehen in Bilderwerk.

## 6. Behandlung von Ordnern und Dateitypen

### Behandlungsstufen

| Stufe | Was ins Register kommt | Gedacht für |
|---|---|---|
| Voll | Pfad, Name, Metadaten, Text, Vorschau | das eigentliche Archiv |
| Nur Dokumente | wie Voll, aber nur pdf, md und txt. Programmdateien und Bibliotheken werden übergangen | Entwicklungsprojekte |
| Nur Metadaten | Pfad, Name, Größe, Datum. Kein Inhalt, keine Vorschau | eBooks, Musik |
| Ausgeschlossen | nichts | Bildordner, die Bilderwerk abdeckt |

Begründung für "Nur Metadaten" bei eBooks: Ihr Inhalt würde sonst bei fast jeder Suche Treffer liefern.

### Regeln

- Jedes Verzeichnis kann einzeln genannt und einzeln ausgeschlossen werden.
- Die genaueste Regel gewinnt. Ein ausgeschlossener Unterordner bleibt ausgeschlossen, auch wenn der Oberordner auf Voll steht.
- Die Regeln stehen in einer lesbaren Textdatei neben dem Register, nicht nur in der Datenbank. Sie überleben damit einen Neuaufbau.
- Die konkrete Zuordnung der Ordner steht in `docs/intern/umgebung.md`.
- Bis die Behandlungsstufen umgesetzt sind (Stufe 3), nennt eine Konfigurationsdatei nur den Cloud-Ordner und die zu lesenden Verzeichnisse. Sie heißt `config.local.toml` und wird nicht versioniert, eine Vorlage ohne echte Pfade liegt im Repo.
- Verwaltungsdateien von Cloud-Client und Betriebssystem werden immer übergangen.

### Dateitypen

- **PDF** ist der Regelfall und wird vollständig verarbeitet.
- **Nicht lesbare oder gepackte Formate** werden in jeder Stufe nur mit Metadaten erfasst. Archive werden nicht entpackt. Beispiele: zip, Notizbuch-Formate, Tracking-Dateien.
- **Weitere Dokumenttypen** wie Word, Pages, Tabellen und einzelne Bilder im Archiv starten mit Metadaten. Ihre Textauslese ist eine spätere Ausbaustufe.

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
| Umfang | jede Seite jedes voll verarbeiteten Dokuments |
| Auflösung | 1024 px Breite bezogen auf A4, das sind rund 124 dpi. Schmalere Seiten wie Kassenbelege werden im selben Maßstab gerendert |
| Format | AVIF, Qualität 25, kodiert mit libavif und libaom (Stufe 8) |
| Farbe | farbige Seiten in 4:2:0, Seiten ohne sichtbare Farbe als echte Graustufen (4:0:0) |
| Farberkennung | Ein Pixel gilt als farbig, wenn seine Farbkanäle um mehr als 48 auseinanderliegen. Eine Seite gilt als farbig ab 0,1 % farbiger Pixel. **Annahme**, im Pilot stichprobenartig bestätigt |
| Übergrößen | längste Kante höchstens 2048 px, etwa bei Plänen. **Annahme** |

Begründung:

- 640 px sind lesbar, aber leicht unscharf. Das ermüdet beim Suchen und Lesen.
- 1600 px bringen nur beim extremen Vergrößern einen Unterschied.
- AVIF Q25 war im direkten Vergleich mit WebP Q50 bei normaler Ansicht nicht zu unterscheiden, auch bei Farbseiten. Die Dateien sind gut ein Drittel so groß.
- Kleine Vorschauen halten das Blättern flüssig, auch falls das Register später auf einem Netzlaufwerk oder im Web liegt.
- Die Vorschau muss normalen Brieftext lesbar machen. Für Kleingedrucktes wird das Original geladen.

Die Vorschau ist eine abgeleitete Kopie. Ihre starke Kompression berührt die Originale nicht.

Die Einstellungen entsprechen dem Vergleich der Konzeptphase, der ebenfalls libavif nutzte (über Pillow). Im Pilot (v0.0.3) ergaben sich 27,2 KB je Seite, genau der Wert aus der Stichprobe. Beim Durchsehen war kein Unterschied zum Original zu erkennen.

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
| Vorschau-Datenbanken ("Scheiben") | die AVIF-Bilder als Datensätze, wie bei Bilderwerk | je etwa 1 bis 2 GB |

Die Vorschauen liegen also in der Datenbank, nicht als einzelne Bilddateien. Sie sind nur auf mehrere Dateien verteilt. Gründe gegen eine einzige große Datei:

- Jeder Delta-Lauf würde aus Sicht der Cloud die ganze Datei ändern. Manche Anbieter laden sie dann vollständig neu hoch.
- Die Arbeitskopie beim Schreiben bräuchte zeitweise den doppelten Platz.
- Ein Schaden bei der Übertragung träfe das gesamte Register statt einer Scheibe.

Alte Scheiben bleiben unverändert. Ein Delta-Lauf fasst nur die Index-Datenbank und die jüngste Scheibe an. Umgesetzt in v0.0.3:

- Die Scheiben werden nach **Erfassungsreihenfolge** gefüllt. Neue Vorschauen kommen in die jüngste Scheibe, bis diese rund 1,5 GB erreicht (**Annahme**). Dann beginnt eine neue.
- Die Dateien heißen `vorschau-0001.db`, `vorschau-0002.db` usw. und liegen neben der Index-Datenbank.
- Die Index-Datenbank hält je Seite einen Verweis (Scheibe und Schlüssel), Maße und Farbe.
- Ändert sich eine Datei oder verschwindet sie, wandern ihre alten Verweise in eine Liste. Beim nächsten Lauf werden die Bilder aus ihrer Scheibe gelöscht. Nur dabei ändert sich eine ältere Scheibe.
- Geschrieben wird erst die Scheibe, dann der Verweis. Ein Abbruch dazwischen hinterlässt höchstens ein Bild ohne Verweis, das der nächste Neuaufbau beseitigt.

**Pfade im Register** sind relativ zum Cloud-Ordner gespeichert, mit `/` als Trenner und einheitlicher Schreibweise der Umlaute (Unicode NFC). Wo der Cloud-Ordner auf einem Gerät liegt, steht nur in dessen Konfiguration. Dasselbe Register gilt damit auf jedem Gerät und Betriebssystem. Die einheitliche Schreibweise ist nötig, weil macOS Umlaute zerlegt liefert und dieselbe Datei sonst je nach Gerät ein anderer Eintrag wäre. Umgesetzt in v0.0.1.

Die Replikation großer Dateien ist kein Engpass: Leitungen und Laufzeit reichen aus, notfalls läuft ein Rechner eine Nacht durch.

### Ablage und Veröffentlichen

- Das Register liegt im Cloud-Ordner und wird auf die Endgeräte repliziert. Der Ort steht in `docs/intern/umgebung.md`. Der Register-Ordner ist von der Indizierung ausgeschlossen.
- Der Indexer schreibt in eine Arbeitskopie außerhalb des Cloud-Ordners und legt erst den fertigen Stand hinein. Lesende Geräte sehen so keine halb geschriebenen Zustände. **Annahme**. Seit v0.0.1 liegt die Arbeitskopie ohne weitere Angabe im Datenordner der Anwendung im Benutzerprofil. Das Veröffentlichen in den Cloud-Ordner folgt in Stufe 3.
- Eine Sperre mit Stand-Zähler hält fest, welches Gerät zuletzt geschrieben hat. Beim Start prüft das Programm, ob die lokale Kopie diesen Stand hat. Wenn nicht, öffnet es nur lesend und meldet das. **Annahme**

### Verschlüsselung

Das Register wird verschlüsselt, zusätzlich zur Verschlüsselung der Festplatte. Es enthält den Volltext und die Vorschauen aller Unterlagen.

- Bei der Einrichtung wird ein Kennwort vergeben. Es wird im eigenen Passwortsafe aufbewahrt. Kein Cloud-Schlüsselbund eines Herstellers.
- Die Datenbanken sind mit einem Zufallsschlüssel verschlüsselt, den das Kennwort schützt. Das Kennwort lässt sich so ändern, ohne das Register neu zu verschlüsseln. **Annahme**
- Pro Gerät kann der lokale Schlüsselbund des Betriebssystems das Kennwort speichern, ohne Cloud-Abgleich. Das ist abschaltbar, dann fragt das Programm bei jedem Start. **Annahme**
- Ein verlorenes Kennwort kostet keine Daten. Das Register wird aus den Dateien neu aufgebaut.

Die Verschlüsselung schützt das Register. Die Originale im Cloud-Ordner schützt sie nicht.

### Neuaufbau

Ein vollständiger Neuaufbau aus den Dateien ist jederzeit möglich. Im Pilot am 18.11.2026 ausgeführt (v0.0.3): Register-Ordner beiseitegelegt, Dateibaum, Inhalte und Vorschauen neu erzeugt. Ergebnis identisch mit dem vorherigen Stand: 3.384 PDFs, 11.131 Seiten, 295 MB Vorschauen. Text in 17 s, Vorschauen in 3 min 16 s.

## 11. Suche und Anzeige

- Fehlertolerante Volltextsuche, die Tippfehler und OCR-Fehler verkraftet
- Dateiname und Pfad zählen stärker als der Inhalt. Vorbild ist das Verhalten der bisherigen Systemsuche.
- Priorisierung nach dem Dokumentdatum aus Abschnitt 9
- Deutsche Besonderheiten: Umlaute, zusammengesetzte Wörter
- Trefferliste mit Vorschaubild, Seitenansicht zum Blättern, Öffnen des Originals
- Einträge der Stufe "Nur Metadaten" erscheinen mit Pfad und Name
- Oberfläche in Deutsch, hell und dunkel. **Annahme**

Gewichtung, Filter nach Ordner und Jahr und die Darstellung der Treffer werden am laufenden Piloten eingestellt.

## 12. Technik

| Bereich | Festlegung | Stand |
|---|---|---|
| Anwendung | Tauri 2 mit Rust, lauffähig auf macOS, Windows und Linux | entschieden, im Einsatz seit v0.0.1 |
| Oberfläche | React mit TypeScript und Vite | entschieden, im Einsatz seit v0.0.1 |
| Aufbau | Kern als eigene Rust-Bibliothek ohne Oberfläche, die Tauri-Anwendung ist nur eine Hülle darüber | entschieden. Hält den lesenden Teil für eine Web-Oberfläche wiederverwendbar |
| Datenbank | SQLite mit Volltextindex, verschlüsselt | entschieden. Bis Stufe 3 unverschlüsselt, der Wechsel auf die verschlüsselnde Variante ist eine Einstellung der Bibliothek |
| PDF | PDFium für Textauslese und Rendering, über `pdfium-render`. Die Bibliothek wird zur Laufzeit geladen und kommt aus den fertigen Builds von bblanchon/pdfium-binaries | entschieden, Textauslese seit v0.0.2 |
| OCR | Tesseract 5.5.2, Sprachen Deutsch und Englisch, Sprachdaten `tessdata`. Statisch aus dem Quelltext gebaut (mit Leptonica, über CMake), eigene schmale Anbindung an die C-Schnittstelle. Keine Installation auf dem Gerät | entschieden, im Einsatz seit v0.0.4 |
| Vorschau | AVIF Q25, 1024 px bezogen auf A4. Kodiert mit libavif und libaom über `libavif-sys`, beim Bau aus dem Quelltext übersetzt (braucht CMake, unter x86 auch NASM) | entschieden, im Einsatz seit v0.0.3 |
| Zugangsdaten | lokaler Schlüsselbund des Betriebssystems, wie in Stellwerk | Annahme |

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

Hochgerechnet auf 1,5 Mio. Seiten: rund 8 Stunden und rund 40 GB. Mehrere Render-Prozesse könnten das Tempo noch einmal etwa verdoppeln. Das lohnt sich bei diesen Werten nicht.

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

Hochgerechnet auf 1,5 Mio. Seiten: rund 160.000 Seiten ohne Text, etwa 9 bis 10 Stunden auf dem Notebook.

### Mengengerüst (Schätzung)

| Größe | Wert |
|---|---|
| Dateien in der Cloud insgesamt | über 600.000, rund 0,9 TB einschließlich Fotos |
| Dokumente ohne Fotos | rund 500.000, rund 500 GB |
| Seiten | rund 1,5 Mio. bei 3 Seiten je Dokument |
| Vorschauen in AVIF Q25 | rund 39 GB |
| Volltext und Suchindex | einige GB, nicht gemessen |

Die Hochrechnung nimmt an, dass das übrige Archiv den beiden geprüften Verzeichnissen ähnelt. Das ist nicht geprüft. Die Zahl von 500.000 Dokumenten ist eine Schätzung, eine Inventur der übrigen Verzeichnisse steht aus.

## 14. Pilot und Stufen

### Pilot

Der Pilot umfasst nur die zwei Hauptverzeichnisse des aktuellen Archivs mit rund 3.600 Dateien. Er ist schnell neu aufgebaut und deckt alle Fälle ab: Farb-, Graustufen- und 1-Bit-Scans, digital erzeugte PDFs, schwache OCR, Platzhalter und andere Dateitypen.

### Vorschlag für die Stufen

| Stufe | Inhalt | Ergebnis |
|---|---|---|
| 1 | Indexer für PDFs: Dateibaum, Textauslese, Vorschauen, Register | Pilot-Register aus den Dateien aufgebaut, Neuaufbau getestet |
| 2 | Anwendung: Suche, Trefferliste, Seitenvorschau, Original öffnen | tägliche Nutzung auf dem Pilotbestand |
| 3 | Verschlüsselung, Ordnerregeln, Behandlungsstufen | Register im Cloud-Ordner, zweites Gerät liest |
| 4 | Delta-Lauf, Sperre und Stand-Zähler, Erkennen verschobener Dateien | laufender Betrieb auf dem Notebook |
| 5 | OCR für Seiten ohne Text, Zurückschreiben in Scans | Suchqualität auf dem Pilotbestand geprüft |
| 6 | Erstindizierung des gesamten Archivs in Tranchen | vollständiges Register |

Die Reihenfolge ist ein Vorschlag. **Annahme**. Stufe 5 wurde vorgezogen (v0.0.4 und v0.0.5), damit die Suche am endgültigen Text eingestellt wird.

### Stand der Umsetzung

| Version | Inhalt | Stufe |
|---|---|---|
| v0.0.1 | Gerüst der Anwendung, Einlesen des Dateibaums ins Register mit Abgleich (neu, geändert, entfernt), Datum aus dem Dateinamen, Übersicht in der Oberfläche | Beginn von Stufe 1, noch ohne PDF-Inhalte und Vorschauen |
| v0.0.2 | PDFium angebunden, Text je Seite im Register, Gütemaß und Seitenart, Datum aus PDF und Änderungsdatum, Fortsetzen nach Abbruch | Stufe 1 ohne Vorschauen |
| v0.0.3 | Vorschauen aller Seiten als AVIF in Scheiben, Seitenansicht zum Prüfen, Neuaufbau des Pilot-Registers getestet | Stufe 1 abgeschlossen |
| v0.0.4 | Tesseract statisch eingebaut, OCR der Seiten ohne Text ins Register, gültiger Text je Seite, Textvergleich in der Prüfansicht | erster Teil von Stufe 5 |
| v0.0.5 | Textebene in Scans zurückgeschrieben, Lage der Wörter aus Tesseract, Grenze "ohne Text" bei 10 Zeichen, Lesen gedrehter Seiten korrigiert | Stufe 5 abgeschlossen |

Einzelheiten je Version stehen in der Entwicklungsdokumentation unter `docs/`.

## 15. Offene Punkte

| Punkt | Wann |
|---|---|
| Inventur der Verzeichnisse außerhalb des aktuellen Archivs. Bestimmt die Endgröße, nicht den Bau | vor Stufe 6 |
| Bau von Tesseract und Leptonica unter Windows | vor dem ersten Windows-Paket |
| Sprachdaten ins Programmpaket legen (Ressource `tessdata/standard` mit Lizenztexten) | mit dem ersten Paket |
| Anzeige von AVIF unter Windows und Linux. macOS ist geprüft | Stufe 2 |
| Bau von libaom unter Windows | vor dem ersten Windows-Paket |
| Zurücksetzen auf "nur online" per Programm | Stufe 4 |
| Textauslese für Word, Pages, Tabellen und Bilder | nach Stufe 6 |
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
| Vorschau | alle Seiten, 1024 px, AVIF Q25 | lesbar, klein, flüssig beim Blättern |
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
| Ablage der Vorschauen | in der Datenbank, verteilt auf Scheiben von 1 bis 2 GB | nur Geändertes wird repliziert, Anbieterwechsel bleibt offen |
| Sicherungskopien | keine | sollen kein Volumen erzeugen, das von Hand bereinigt werden muss |
| Datum | Dateiname, dann PDF, dann Datei | Dateinamen tragen fast immer ein Datum |
| Verschlüsselung | Register verschlüsselt, Kennwort im eigenen Passwortsafe | hochsensible Inhalte, keine Bindung an einen Hersteller |
| Betrieb | Erstlauf stationär, danach Delta auf dem Notebook | kein Dauerbetrieb, keine dauerhafte Vollkopie |
| Systemsuche | nicht angebunden | gesucht wird in Registerwerk |
| KI | nicht vorgesehen | kein Bedarf |
| Pilot | zwei Hauptverzeichnisse, rund 3.600 Dateien | klein, vollständig in den Fällen, schnell neu aufgebaut |
| Oberfläche | React mit TypeScript und Vite | bewährt in Stellwerk, überzeugt in Gestaltung und Möglichkeiten |
| Pfade im Register | relativ zum Cloud-Ordner, einheitliche Schreibweise | Register gilt auf jedem Gerät und Betriebssystem |
| Wechsel auf "nur online" | keine Änderung, solange das Änderungsdatum gleich bleibt | sonst würde jedes Auslagern Text und Vorschauen neu erzeugen |
| Anbindung von PDFium | `pdfium-render`, Bibliothek per Skript geladen, nicht im Repo | fertige Builds für alle Zielsysteme, freizügige Lizenzen, keine Binärdateien im Repo |
| "Nur online" bis Stufe 4 | nicht öffnen, bleiben ausstehend | Öffnen löst den Download aus. Erstindizierung mit von Hand offline gestellten Ordnern |
| Verarbeitungsstand | Änderungsdatum und Größe beim Lesen, dazu eine Verfahrensversion | geänderte Dateien und Verfahren werden gezielt neu gelesen, ein Abbruch kostet nichts |
| Suchindex | erst mit der Suche | die Wahl hängt an der fehlertoleranten Suche, die Texttabelle je Seite ist die Grundlage |
| Entfernen aus dem Register | nur in vollständig gelesenen Verzeichnissen | ein fehlender Cloud-Ordner darf das Register nicht leeren |
| AVIF-Encoder | libavif mit libaom, Stufe 8 | bei gleicher Qualität und Größe knapp 3-mal so schnell wie rav1e. Kostet einen C-Bau mit CMake |
| Einteilung der Scheiben | nach Erfassungsreihenfolge, neue Scheibe ab rund 1,5 GB | Delta-Läufe ändern nur die jüngste Scheibe |
| Graustufen | echte Graustufen (4:0:0) für Seiten ohne Farbe | kleiner, ohne sichtbaren Verlust |
| Entwicklungsmodus | Kern und Abhängigkeiten auch im Entwicklungsmodus optimiert | ohne Optimierung ist der Encoder um ein Vielfaches langsamer, auch weil seine generischen Teile erst im Kern übersetzt werden |
| Zurückschreiben in die PDF | inkrementell mit `lopdf`, Schrift `glyphless.ttf` aus Tesseract im Repo | Original bleibt Byte für Byte erhalten. Gleich breite Zeichen lassen jedes Wort genau strecken |
| Änderungsdatum nach dem Zurückschreiben | alter Wert | Sortierung kann darauf aufbauen. Das Register führt Größe und Datum selbst nach |
| Ablage des Projekts | im Cloud-Ordner, Build-Ordner bleiben im Projekt und sind von der Synchronisation ausgenommen | sichert auch die nicht versionierten Inhalte. Eine zentrale Ablage je Projekt, keine Fragmente an anderen Orten |

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

## 18. Werkzeuge

- `archiv_inventur.py`: Inventur und PDF-Stichprobe der Konzeptphase, nur lesend
- `vorschau_vergleich.py`: Vorschauformate nebeneinander, Konzeptphase, nur lesend
- `dropbox_ausschluss.sh`: nimmt die Build-Ordner des Projekts von der Synchronisation des Cloud-Ordners aus. Einmal je Gerät auszuführen
- `pdfium_holen.sh`: lädt die PDFium-Bibliothek samt Lizenztexten nach `bibliotheken/pdfium/`. Einmal je Gerät auszuführen
- `tesseract_holen.sh`: lädt Quelltext von Tesseract und Leptonica sowie die Sprachdaten `standard` nach `bibliotheken/tesseract/`. Einmal je Gerät auszuführen. Mit `--best` zusätzlich `tessdata_best` für Vergleiche. Der Quelltext muss liegen bleiben, cargo braucht ihn für jeden neuen Bau
- `pdf_pruefen`: liest PDFs mit dem Kern, ohne das Register anzufassen. Für Prüfungen nach einem PDFium-Update, auf einem neuen Gerät oder bei einzelnen Problemdateien. Mit `--vorschau N` ein Tempotest der Vorschauen, mit `--render N` unkomprimierte Seiten als Vorlage für Encoder-Vergleiche, mit `--ocr N` eine Messung der OCR mit den vorhandenen Sätzen der Sprachdaten, mit `--textebene N` eine Probe der Textebene an Kopien. Die Ausgaben enthalten Dokumentinhalte und landen in `target/`

Die Skripte liegen im Repo unter `tools/`. Die Ausgaben der beiden Analyse-Skripte enthalten Dokumentinhalte und Dateinamen und gehören nicht ins Repo.
