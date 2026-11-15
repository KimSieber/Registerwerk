# Registerwerk: Konzept

Stand: 15.11.2026. Dieses Dokument hält alle Entscheidungen der Konzeptphase fest und ist die Grundlage für die Umsetzung. Konkrete Pfade, Ordnernamen und Zugangsdaten stehen nicht hier, sondern in `docs/intern/` (nicht versioniert).

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
3. **Originale werden nicht verschlechtert.** Keine Neukomprimierung, keine Qualitätsreduktion. Einzige zulässige Änderung an einer Datei ist das verlustfreie Ergänzen oder Ersetzen der Textebene bei Scans (Abschnitt 7).
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
- OCR für Seiten ohne oder mit schwachem Text
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
- Geändert ist eine Datei, deren Größe oder Änderungsdatum abweicht.
- Entfernt ist ein Pfad, der im Register steht und im Dateibaum fehlt. Der Eintrag und seine Vorschauen werden aus dem Register entfernt. **Annahme**
- Verschobene und umbenannte Dateien sollen über einen Inhalts-Hash erkannt werden, damit Text und Vorschauen nicht neu erzeugt werden müssen. **Annahme**, wird mit dem Delta-Lauf umgesetzt.

### Platzhalter ("nur online")

Gemessen am 15.11.2026 auf dem Notebook:

- Platzhalter melden 0 Byte, 0 belegte Blöcke und keine besondere Markierung. Sie sind für ein Programm von einer leeren Datei nicht zu unterscheiden.
- Name, Pfad und Änderungsdatum sind lesbar, die Größe nicht.
- Das Auflisten lädt nichts herunter. Das Öffnen einer Datei löst den Download aus.

Folgen:

- Eine Datei mit 0 Byte gilt als "nur online oder leer".
- Zum Indizieren öffnet der Indexer die Datei. Der Cloud-Client lädt sie dabei automatisch.
- Das Gesamtvolumen von "nur online"-Dateien lässt sich über das Dateisystem nicht ermitteln.

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

### Dateitypen

- **PDF** ist der Regelfall und wird vollständig verarbeitet.
- **Nicht lesbare oder gepackte Formate** werden in jeder Stufe nur mit Metadaten erfasst. Archive werden nicht entpackt. Beispiele: zip, Notizbuch-Formate, Tracking-Dateien.
- **Weitere Dokumenttypen** wie Word, Pages, Tabellen und einzelne Bilder im Archiv starten mit Metadaten. Ihre Textauslese ist eine spätere Ausbaustufe.

## 7. Verarbeitung von PDFs

### Regelfall

Alle Scans liegen als PDF vor: Das Scanbild ist als Bild gespeichert, der per OCR erkannte Text ist eingebettet. PDFs mit echtem, digital erzeugtem Text sind die Ausnahme. Der eingebettete Text wird übernommen. OCR läuft nur, wo es nötig ist.

### Gütemaß

Die Güte eines Textes ist der Anteil wortartiger Tokens: Wörter aus mindestens zwei Buchstaben sowie Zahlen- und Betragsmuster, bezogen auf alle Tokens. Das Maß ist grob, trennt aber brauchbaren von unbrauchbarem Text. Die Schwelle liegt bei **60 %**.

### Drei Fälle

| Fall | Erkennung | Behandlung |
|---|---|---|
| Seite ohne Text | weniger als 20 Zeichen eingebetteter Text | OCR ausführen, Text ins Register, Text in die PDF einbetten |
| Scan mit schwachem Text | Güte unter 60 % | OCR neu ausführen. Der neue Text wird nur übernommen, wenn er messbar besser ist. Dann Text ins Register und Textebene in der PDF ersetzen |
| Digital erzeugte PDF mit unlesbarem Text | kein Scanbild, Güte unter 60 % | OCR auf die gerenderte Seite, Text **nur ins Register**. Die Datei bleibt unverändert |

Erläuterungen:

- Der dritte Fall betrifft zum Beispiel Kontoauszüge, deren Text sich wegen der Schriftkodierung nicht auslesen lässt. Die Textebene lässt sich dort nicht ersetzen, ohne die Seite in ein Bild umzuwandeln. Das wäre ein Qualitätsverlust und ist ausgeschlossen. Der OCR-Text im Register ist ein Auszug und lässt sich jederzeit neu erzeugen, der Grundsatz 2 bleibt gewahrt. Ob diese PDFs tatsächlich unlesbaren Text liefern, wird im Pilotlauf an einer Datei geprüft.
- "Messbar besser" heißt: Die Güte des neuen Textes liegt mindestens 10 Prozentpunkte über der des alten. **Annahme**
- Es wird nicht das ganze Archiv neu erkannt. In der Stichprobe lagen 15 % der PDFs unter der Schwelle, 1,6 % hatten gar keinen Text.

### Zurückschreiben in die PDF

Der Text wird in die Datei eingebettet, damit der Komfort der durchsuchbaren PDF auch außerhalb von Registerwerk erhalten bleibt. Der Ablauf kommt ohne Sicherungskopien aus:

1. Die neue Fassung entsteht als temporäre Datei.
2. Das Programm prüft sie: gleiche Seitenzahl, Scanbilder unverändert, Text vorhanden, Datei lässt sich öffnen.
3. Besteht sie die Prüfung, ersetzt sie das Original in einem Schritt. Besteht sie nicht, wird sie verworfen und das Original bleibt unberührt.
4. Das Änderungsdatum der Datei wird auf den vorherigen Wert zurückgesetzt, weil die Sortierung darauf aufbauen kann.
5. Ein Protokoll hält Pfad und Zeitpunkt jeder geänderten Datei fest.

Als zusätzliches Netz ohne eigenes Volumen dient der Versionsverlauf des Cloud-Anbieters, solange er zur Verfügung steht.

## 8. Vorschauen

| Merkmal | Festlegung |
|---|---|
| Umfang | jede Seite jedes voll verarbeiteten Dokuments |
| Auflösung | 1024 px Breite bezogen auf A4, das sind rund 124 dpi. Schmalere Seiten wie Kassenbelege werden im selben Maßstab gerendert |
| Format | AVIF, Qualität 25 |
| Farbe | Seiten ohne sichtbare Farbe werden als Graustufen gespeichert |

Begründung:

- 640 px sind lesbar, aber leicht unscharf. Das ermüdet beim Suchen und Lesen.
- 1600 px bringen nur beim extremen Vergrößern einen Unterschied.
- AVIF Q25 war im direkten Vergleich mit WebP Q50 bei normaler Ansicht nicht zu unterscheiden, auch bei Farbseiten. Die Dateien sind gut ein Drittel so groß.
- Kleine Vorschauen halten das Blättern flüssig, auch falls das Register später auf einem Netzlaufwerk oder im Web liegt.
- Die Vorschau muss normalen Brieftext lesbar machen. Für Kleingedrucktes wird das Original geladen.

Die Vorschau ist eine abgeleitete Kopie. Ihre starke Kompression berührt die Originale nicht.

## 9. Datum eines Dokuments

Für Sortierung und Priorisierung gilt diese Reihenfolge:

1. Datum am Anfang des Dateinamens
2. Erstellungsdatum in den PDF-Metadaten
3. Änderungsdatum der Datei

Begründung: Fast alle Dateinamen beginnen mit einem Datum. Ein PDF-Erstellungsdatum tragen nur 76 % der PDFs. Das Anlagedatum eines Platzhalters ist nicht verlässlich. Ein Änderungsdatum im Sinne von "zuletzt bearbeitet" wird nicht gebraucht.

## 10. Register

### Aufbau

- SQLite als Datenbank, mit Volltextindex
- Eine kleine **Index-Datenbank** mit Metadaten und Volltext
- **Vorschau-Datenbanken in Scheiben** von je etwa 1 bis 2 GB. Nach einem Delta-Lauf müssen so nur die Index-Datenbank und die Scheibe mit den neuen Vorschauen repliziert werden. **Annahme**: Die Vorschauen liegen wie bei Bilderwerk in der Datenbank. Die Aufteilung in Scheiben ist ein Vorschlag, die Einteilung wird in der Umsetzung festgelegt.

Die Replikation großer Dateien ist kein Engpass: Leitungen und Laufzeit reichen aus, notfalls läuft ein Rechner eine Nacht durch.

### Ablage und Veröffentlichen

- Das Register liegt im Cloud-Ordner und wird auf die Endgeräte repliziert. Der Ort steht in `docs/intern/umgebung.md`. Der Register-Ordner ist von der Indizierung ausgeschlossen.
- Der Indexer schreibt in eine Arbeitskopie außerhalb des Cloud-Ordners und legt erst den fertigen Stand hinein. Lesende Geräte sehen so keine halb geschriebenen Zustände. **Annahme**
- Eine Sperre mit Stand-Zähler hält fest, welches Gerät zuletzt geschrieben hat. Beim Start prüft das Programm, ob die lokale Kopie diesen Stand hat. Wenn nicht, öffnet es nur lesend und meldet das. **Annahme**

### Verschlüsselung

Das Register wird verschlüsselt, zusätzlich zur Verschlüsselung der Festplatte. Es enthält den Volltext und die Vorschauen aller Unterlagen.

- Bei der Einrichtung wird ein Kennwort vergeben. Es wird im eigenen Passwortsafe aufbewahrt. Kein Cloud-Schlüsselbund eines Herstellers.
- Die Datenbanken sind mit einem Zufallsschlüssel verschlüsselt, den das Kennwort schützt. Das Kennwort lässt sich so ändern, ohne das Register neu zu verschlüsseln. **Annahme**
- Pro Gerät kann der lokale Schlüsselbund des Betriebssystems das Kennwort speichern, ohne Cloud-Abgleich. Das ist abschaltbar, dann fragt das Programm bei jedem Start. **Annahme**
- Ein verlorenes Kennwort kostet keine Daten. Das Register wird aus den Dateien neu aufgebaut.

Die Verschlüsselung schützt das Register. Die Originale im Cloud-Ordner schützt sie nicht.

### Neuaufbau

Ein vollständiger Neuaufbau aus den Dateien ist jederzeit möglich und wird im Pilot als Test ausgeführt.

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
| Anwendung | Tauri mit Rust und Web-Oberfläche, lauffähig auf macOS, Windows und Linux | geplant |
| Datenbank | SQLite mit Volltextindex, verschlüsselt | entschieden |
| PDF | PDFium für Textauslese und Rendering | geplant |
| OCR | Tesseract, Sprachen Deutsch und Englisch. Bereits in PaperTrail Compare im Einsatz, unabhängig von Apple | entschieden |
| Vorschau | AVIF Q25, 1024 px bezogen auf A4 | entschieden |
| Zugangsdaten | lokaler Schlüsselbund des Betriebssystems, wie in Stellwerk | Annahme |

Zu prüfen beim Start: Die Oberfläche muss AVIF auf allen Zielsystemen anzeigen. Wo die eingebettete Browser-Komponente das nicht kann, dekodiert der Rust-Teil das Bild und reicht es weiter.

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
| 5 | OCR: fehlender Text, schwacher Text, unlesbare digitale PDFs, Zurückschreiben | Suchqualität auf dem Pilotbestand geprüft |
| 6 | Erstindizierung des gesamten Archivs in Tranchen | vollständiges Register |

Die Reihenfolge ist ein Vorschlag. **Annahme**

## 15. Offene Punkte

| Punkt | Wann |
|---|---|
| Inventur der Verzeichnisse außerhalb des aktuellen Archivs. Bestimmt die Endgröße, nicht den Bau | vor Stufe 6 |
| Prüfung, ob die betroffenen Kontoauszüge wirklich unlesbaren Text liefern | Pilot |
| Anzeige von AVIF in der Oberfläche auf allen Zielsystemen | Stufe 2 |
| Einteilung der Vorschau-Scheiben | Stufe 1 |
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
| OCR wiederholen | nur unter 60 % Güte und nur bei messbarer Verbesserung | kein Durcharbeiten des ganzen Archivs für wenig Gewinn |
| OCR-Text bei Scans | ins Register und in die PDF | durchsuchbare PDF bleibt auch außerhalb von Registerwerk erhalten |
| OCR-Text bei unlesbaren digitalen PDFs | nur ins Register | Einbetten würde das Original verschlechtern |
| Sicherungskopien | keine | sollen kein Volumen erzeugen, das von Hand bereinigt werden muss |
| Datum | Dateiname, dann PDF, dann Datei | Dateinamen tragen fast immer ein Datum |
| Verschlüsselung | Register verschlüsselt, Kennwort im eigenen Passwortsafe | hochsensible Inhalte, keine Bindung an einen Hersteller |
| Betrieb | Erstlauf stationär, danach Delta auf dem Notebook | kein Dauerbetrieb, keine dauerhafte Vollkopie |
| Systemsuche | nicht angebunden | gesucht wird in Registerwerk |
| KI | nicht vorgesehen | kein Bedarf |
| Pilot | zwei Hauptverzeichnisse, rund 3.600 Dateien | klein, vollständig in den Fällen, schnell neu aufgebaut |

## 17. Verworfene Alternativen

| Alternative | Warum verworfen |
|---|---|
| Archiv durch Neukomprimierung verkleinern | Risiko für Lesbarkeit und Reproduzierbarkeit |
| Fertiges Archivsystem mit eigener Ablage | übernimmt die Dateien in eine eigene Struktur und widerspricht Grundsatz 1 |
| Warten auf Volltextsuche für "nur online"-Dateien | nicht absehbar, löst die Bindung an den Anbieter nicht |
| Native Apple-Anwendung | Bindung an ein Betriebssystem |
| Schnittstelle des Cloud-Anbieters für die Änderungserkennung | Bindung an den Anbieter. Das Dateisystem genügt |
| Stationärer Rechner als dauerhafte Vollkopie | kein Dauerbetrieb gewünscht, Platz wird anderweitig gebraucht |
| Eine einzige große Datenbankdatei | jede Änderung beträfe die ganze Datei. Scheiben kosten kaum Aufwand |

## 18. Werkzeuge der Konzeptphase

- `archiv_inventur.py`: Inventur und PDF-Stichprobe, nur lesend
- `vorschau_vergleich.py`: Vorschauformate nebeneinander, nur lesend

Beide Skripte liegen im Repo unter `tools/`. Ihre Ausgaben enthalten Dokumentinhalte und Dateinamen und gehören nicht ins Repo.
