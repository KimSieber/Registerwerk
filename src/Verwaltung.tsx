import { useCallback, useEffect, useState } from "react";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import {
  AppInfo,
  Fortschritt,
  FehlerZeile,
  InhaltErgebnis,
  OcrErgebnis,
  RegisterStand,
  TextebeneErgebnis,
  VorschauErgebnis,
  GEAENDERT,
  OnlineOrdner,
  appInfo,
  nurOnlineOrdner,
  ordnerImFinder,
  inhalteAbbrechen,
  inhalteLesen,
  ocrAusfuehren,
  registerStand,
  textebeneSchreiben,
  vorschauenErzeugen,
} from "./api";
import { LaufMeldung, useEinlesen } from "./Einlesen";
import { NAMEN } from "./ordner/stufen";
import VorschauPruefen from "./VorschauPruefen";
import { alsAnteil, alsDauer, alsGroesse, alsZahl, alsZeitpunkt } from "./format";

/** `eingebettet`: unter „Einzelheiten“ im Reiter „Register“ (v0.0.8), ohne eigenen Kopf. */
export default function Verwaltung({ eingebettet = false }: { eingebettet?: boolean }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [stand, setStand] = useState<RegisterStand | null>(null);
  const { laeuft, lauf, fehler: einlesenFehler, einlesen, rueckfrage } = useEinlesen();
  const [fehler, setFehler] = useState<string | null>(null);
  // Laufender Vorgang, der ins Register schreibt. Es läuft höchstens einer.
  const [vorgang, setVorgang] = useState<"inhalte" | "vorschauen" | "ocr" | "probelauf" | "textebene" | null>(
    null,
  );
  const [fortschritt, setFortschritt] = useState<Fortschritt | null>(null);
  const [inhaltLauf, setInhaltLauf] = useState<InhaltErgebnis | null>(null);
  const [vorschauLauf, setVorschauLauf] = useState<VorschauErgebnis | null>(null);
  const [ocrLauf, setOcrLauf] = useState<OcrErgebnis | null>(null);
  const [textebeneLauf, setTextebeneLauf] = useState<TextebeneErgebnis | null>(null);
  // Das Schreiben in die Originale braucht einen zweiten Klick
  const [bestaetigen, setBestaetigen] = useState(false);
  const [haelt, setHaelt] = useState(false);
  // Erhöht sich nach jedem Vorschau-Lauf, damit die Prüfansicht neu lädt
  const [vorschauStand, setVorschauStand] = useState(0);

  const standLaden = useCallback(async () => {
    try {
      setStand(await registerStand());
    } catch (e) {
      setFehler(String(e));
    }
  }, []);

  // Beim Start und nach jedem Einlesen oder Speichern der Regeln (auch im Reiter „Ordner“)
  useEffect(() => {
    const laden = () =>
      appInfo()
        .then((i) => {
          setInfo(i);
          if (!i.konfig_fehler) void standLaden();
        })
        .catch((e) => setFehler(String(e)));
    void laden();
    window.addEventListener(GEAENDERT, laden);
    return () => window.removeEventListener(GEAENDERT, laden);
  }, [standLaden]);


  async function inhalteStarten() {
    setVorgang("inhalte");
    setHaelt(false);
    setFehler(null);
    setFortschritt(null);
    setInhaltLauf(null);
    try {
      setInhaltLauf(await inhalteLesen(setFortschritt));
    } catch (e) {
      setFehler(String(e));
    } finally {
      setVorgang(null);
      setFortschritt(null);
      await standLaden();
    }
  }

  async function vorschauenStarten() {
    setVorgang("vorschauen");
    setHaelt(false);
    setFehler(null);
    setFortschritt(null);
    setVorschauLauf(null);
    try {
      setVorschauLauf(await vorschauenErzeugen(setFortschritt));
    } catch (e) {
      setFehler(String(e));
    } finally {
      setVorgang(null);
      setFortschritt(null);
      setVorschauStand((n) => n + 1);
      await standLaden();
    }
  }

  async function ocrStarten() {
    setVorgang("ocr");
    setHaelt(false);
    setFehler(null);
    setFortschritt(null);
    setOcrLauf(null);
    try {
      setOcrLauf(await ocrAusfuehren(setFortschritt));
    } catch (e) {
      setFehler(String(e));
    } finally {
      setVorgang(null);
      setFortschritt(null);
      setVorschauStand((n) => n + 1);
      await standLaden();
    }
  }

  async function textebeneStarten(probelauf: boolean, eine = false) {
    setVorgang(probelauf ? "probelauf" : "textebene");
    setBestaetigen(false);
    setHaelt(false);
    setFehler(null);
    setFortschritt(null);
    setTextebeneLauf(null);
    try {
      setTextebeneLauf(await textebeneSchreiben(probelauf, eine, setFortschritt));
    } catch (e) {
      setFehler(String(e));
    } finally {
      setVorgang(null);
      setFortschritt(null);
      await standLaden();
    }
  }

  async function anhalten() {
    setHaelt(true);
    await inhalteAbbrechen();
  }

  const bereit = info !== null && !info.konfig_fehler;
  const liest = vorgang === "inhalte";
  const ausstehend = stand?.inhalt.ausstehend ?? 0;
  const lokalAusstehend = ausstehend - (stand?.inhalt.ausstehend_online ?? 0);
  const v = stand?.vorschau;
  const vAusstehend = v?.ausstehend ?? 0;
  const vLokalAusstehend = vAusstehend - (v?.ausstehend_online ?? 0);
  const o = stand?.ocr;
  const oAusstehend = o?.ausstehend ?? 0;
  const oLokalAusstehend = oAusstehend - (o?.ausstehend_online ?? 0);
  const t = stand?.textebene;
  const tLokalAusstehend = (t?.dateien_ausstehend ?? 0) - (t?.dateien_ausstehend_online ?? 0);
  const schreibt = vorgang === "probelauf" || vorgang === "textebene";

  return (
    <div className={eingebettet ? "seite seite--eingebettet" : "seite"}>
      {!eingebettet && <header className="kopf">
        <img src={logo} alt="" width={44} height={44} />
        <div>
          <h1>Registerwerk</h1>
          <p className="unterzeile">Register über das Dokumentenarchiv</p>
        </div>
        {info && <span className="version">v{info.version}</span>}
      </header>}

      {info?.konfig_fehler && (
        <section className="hinweis warnung">
          <strong>Konfiguration fehlt oder ist fehlerhaft</strong>
          <p>{info.konfig_fehler}</p>
          {info.konfig_pfad && <p className="pfad">{info.konfig_pfad}</p>}
        </section>
      )}
      {(fehler || einlesenFehler) && (
        <section className="hinweis fehler">
          <strong>Fehler</strong>
          <p>{fehler ?? einlesenFehler}</p>
        </section>
      )}
      {rueckfrage}

      <section className="karte">
        <div className="karte-kopf">
          <div>
            <h2>Dateibaum</h2>
            <p className="unterzeile">
              Liest nur Namen, Größe und Datum der gewählten Ordner. Es wird keine Datei geöffnet und nichts aus der
              Cloud geladen.
            </p>
          </div>
          <button onClick={einlesen} disabled={!bereit || laeuft}>
            {laeuft ? "Liest ein …" : "Dateibaum einlesen"}
          </button>
        </div>

        {bereit && (
          <dl className="angaben">
            <dt>Gerät</dt>
            <dd>{info.geraet}</dd>
            <dt>Äste</dt>
            <dd>
              {info.aeste.map((a) => (
                <span key={a.name} className="marke" title={a.ort}>
                  {a.name}
                  {a.fehlt && <small> · nicht gefunden</small>}
                </span>
              ))}
              {info.aeste.length === 0 && (
                <span className="leise">Noch kein Ordner gewählt: Reiter „Ordner“, „Ordner hinzufügen …“.</span>
              )}
            </dd>
            <dt>Ordner</dt>
            <dd>
              {info.regeln_fehler && <span className="achtung">{info.regeln_fehler}</span>}
              {info.regeln.map((r) => (
                <span key={r.ordner} className={`marke marke--${r.stufe}`} title={NAMEN[r.stufe]}>
                  {r.ordner}
                  {r.stufe !== "voll" && <small> · {NAMEN[r.stufe]}</small>}
                </span>
              ))}
              {!info.regeln_fehler && info.regeln.length === 0 && (
                <span className="leise">Noch kein Ordner gewählt. Die Auswahl steht im Reiter „Ordner“.</span>
              )}
            </dd>
            <dt>Register</dt>
            <dd className="pfad">{info.register_ordner}</dd>
            <dt>PDFium</dt>
            <dd className="pfad">{info.pdfium_ordner ?? "nicht gefunden"}</dd>
            <dt>Tesseract</dt>
            <dd className="pfad">
              {info.tesseract_version}, Sprachdaten {info.tessdata_ordner ?? "nicht gefunden"}
            </dd>
          </dl>
        )}

        {lauf && <LaufMeldung lauf={lauf} />}
      </section>

      {bereit && stand && (
        <section className="karte">
          <div className="karte-kopf">
            <div>
              <h2>Inhalte der Dateien</h2>
              <p className="unterzeile">
                Öffnet lokal vorhandene PDFs, Word-, Excel-, PowerPoint-, OpenDocument-, RTF-, HTML-, CSV- und Textdateien sowie Visitenkarten nur lesend und übernimmt ihren Text, bei PDFs je Seite, bei Excel je Blatt. „Nur online“-Dateien werden
                übergangen, damit nichts aus der Cloud geladen wird.
              </p>
            </div>
            {liest ? (
              <button onClick={anhalten} disabled={haelt}>
                {haelt ? "Hält an …" : "Anhalten"}
              </button>
            ) : (
              <button
                onClick={inhalteStarten}
                disabled={!info.pdfium_ordner || laeuft || vorgang !== null || lokalAusstehend === 0}
              >
                Inhalte lesen
              </button>
            )}
          </div>

          {!info.pdfium_ordner && (
            <p className="achtung">
              PDFium nicht gefunden. Bitte im Projektordner <code>bash tools/pdfium_holen.sh</code> ausführen.
            </p>
          )}

          {liest ? (
            <FortschrittAnzeige f={fortschritt} />
          ) : (
            <p className="stand-zeile">
              {ausstehend === 0
                ? "Alle Dateien sind gelesen."
                : `${alsZahl(lokalAusstehend)} Dateien stehen zum Lesen an` +
                  (stand.inhalt.ausstehend_online > 0
                    ? `, ${alsZahl(stand.inhalt.ausstehend_online)} weitere liegen nur online.`
                    : ".")}
            </p>
          )}

          {!liest && <NurOnlineHinweis lokal={lokalAusstehend} online={stand.inhalt.ausstehend_online} />}
          {inhaltLauf && <InhaltMeldung e={inhaltLauf} />}
          {stand.inhalt.fehler_liste.length > 0 && (
            <FehlerListe anzahl={stand.inhalt.fehler} liste={stand.inhalt.fehler_liste} />
          )}
          {stand.inhalt.grob_liste.length > 0 && (
            <FehlerListe
              anzahl={stand.inhalt.grob}
              liste={stand.inhalt.grob_liste}
              titel={(n) => `${alsZahl(n)} ${n === 1 ? "Datei" : "Dateien"} nur grob gelesen`}
            />
          )}
        </section>
      )}

      {bereit && stand && v && (
        <section className="karte">
          <div className="karte-kopf">
            <div>
              <h2>Vorschauen</h2>
              <p className="unterzeile">
                Rendert jede PDF-Seite und jedes Bild mit 1024 Pixeln an der kurzen Kante (lange Belege höchstens 3072 Pixel
                hoch) und speichert sie als AVIF in den Scheiben des Registers. Seiten ohne Farbe werden als Graustufen gespeichert. „Nur online“-Dateien werden übergangen.
              </p>
            </div>
            {vorgang === "vorschauen" ? (
              <button onClick={anhalten} disabled={haelt}>
                {haelt ? "Hält an …" : "Anhalten"}
              </button>
            ) : (
              <button
                onClick={vorschauenStarten}
                disabled={!info.pdfium_ordner || laeuft || vorgang !== null || vLokalAusstehend === 0}
              >
                Vorschauen erzeugen
              </button>
            )}
          </div>

          {vorgang === "vorschauen" ? (
            <FortschrittAnzeige f={fortschritt} />
          ) : (
            <p className="stand-zeile">
              {vAusstehend === 0
                ? "Für alle PDFs, Bilder und Textdateien liegen Vorschauen vor."
                : `Für ${alsZahl(vLokalAusstehend)} Dateien stehen Vorschauen an` +
                  (v.ausstehend_online > 0 ? `, ${alsZahl(v.ausstehend_online)} weitere liegen nur online.` : ".")}
            </p>
          )}

          {vorgang !== "vorschauen" && <NurOnlineHinweis lokal={vLokalAusstehend} online={v.ausstehend_online} vorschau />}
          {vorschauLauf && <VorschauMeldung e={vorschauLauf} />}

          {v.seiten > 0 && (
            <div className="kacheln">
              <Kachel
                titel="Seiten mit Vorschau"
                wert={alsZahl(v.seiten)}
                zusatz={`aus ${alsZahl(v.erzeugt - v.fehler)} von ${alsZahl(v.pdfs)} PDFs`}
              />
              <Kachel titel="Volumen" wert={alsGroesse(v.bytes)} zusatz={`${alsGroesse(v.bytes / v.seiten)} je Seite`} />
              <Kachel
                titel="Farbig"
                wert={alsAnteil(v.farbig, v.seiten)}
                zusatz={`${alsZahl(v.seiten - v.farbig)} Seiten in Graustufen`}
              />
              <Kachel
                titel="Scheiben"
                wert={alsZahl(v.scheiben.length)}
                zusatz={v.scheiben.map((z) => `${z.datei} ${alsGroesse(z.bytes)}`).join(", ")}
              />
            </div>
          )}

          {v.fehler_liste.length > 0 && <FehlerListe anzahl={v.fehler} liste={v.fehler_liste} />}
        </section>
      )}

      {bereit && stand && o && (
        <section className="karte">
          <div className="karte-kopf">
            <div>
              <h2>Texterkennung</h2>
              <p className="unterzeile">
                Erkennt Seiten ohne Text (höchstens 10 Zeichen) mit Tesseract, gerendert mit 300 dpi. Seiten mit
                schwachem Text behalten ihren eingebetteten Text. Der erkannte Text kommt mit der Lage der Wörter ins
                Register, die PDFs bleiben dabei unverändert.
              </p>
            </div>
            {vorgang === "ocr" ? (
              <button onClick={anhalten} disabled={haelt}>
                {haelt ? "Hält an …" : "Anhalten"}
              </button>
            ) : (
              <button
                onClick={ocrStarten}
                disabled={
                  !info.pdfium_ordner || !info.tessdata_ordner || laeuft || vorgang !== null || oLokalAusstehend === 0
                }
              >
                Text erkennen
              </button>
            )}
          </div>

          {!info.tessdata_ordner && (
            <p className="achtung">
              Sprachdaten nicht gefunden. Bitte im Projektordner <code>bash tools/tesseract_holen.sh</code> ausführen.
            </p>
          )}

          {vorgang === "ocr" ? (
            <FortschrittAnzeige f={fortschritt} />
          ) : (
            <p className="stand-zeile">
              {o.pdfs === 0
                ? "Keine Seite ohne Text."
                : oAusstehend === 0
                  ? "Alle betroffenen PDFs sind erkannt."
                  : `${alsZahl(oLokalAusstehend)} PDFs stehen zur Erkennung an` +
                    (o.ausstehend_online > 0 ? `, ${alsZahl(o.ausstehend_online)} weitere liegen nur online.` : ".")}
            </p>
          )}

          {ocrLauf && <OcrMeldung e={ocrLauf} />}

          {o.pdfs > 0 && (
            <div className="kacheln">
              <Kachel
                titel="Seiten ohne Text"
                wert={alsZahl(o.seiten_ohne_text)}
                zusatz={`schwacher Text bleibt: Scan ${alsZahl(o.seiten_scan_schwach)}, digital ${alsZahl(
                  o.seiten_digital_schwach,
                )}`}
              />
              <Kachel
                titel="Erkannt"
                wert={alsZahl(o.seiten_erkannt)}
                zusatz={
                  o.seiten_erkannt > 0
                    ? `${alsDauer(Math.round(o.erkenn_ms / o.seiten_erkannt))} je Seite und Faden`
                    : `aus ${alsZahl(o.pdfs)} PDFs`
                }
              />
              <Kachel
                titel="Übernommen"
                wert={alsAnteil(o.seiten_uebernommen, o.seiten_erkannt)}
                zusatz={`${alsZahl(o.seiten_uebernommen)} Seiten mit mehr als 10 Zeichen`}
              />
              <Kachel
                titel="Danach noch"
                wert={alsZahl(o.nachher_ohne_text + o.nachher_schwach)}
                zusatz={`${alsZahl(o.nachher_ohne_text)} ohne Text, ${alsZahl(o.nachher_schwach)} schwach`}
              />
            </div>
          )}

          {o.fehler_liste.length > 0 && <FehlerListe anzahl={o.fehler} liste={o.fehler_liste} />}
        </section>
      )}

      {bereit && stand && t && (t.dateien_ausstehend > 0 || t.dateien_geschrieben > 0 || t.fehler > 0) && (
        <section className="karte">
          <div className="karte-kopf">
            <div>
              <h2>Textebene in Scans</h2>
              <p className="unterzeile">
                Schreibt den erkannten Text unsichtbar in Scan-Seiten ohne Text, damit Markieren und Suchen in jedem
                PDF-Programm funktionieren. Das Original bleibt Byte für Byte erhalten, die Textebene wird angehängt.
                Digitale PDFs und vorhandener Text bleiben unberührt. Der Probelauf schreibt nur Kopien.
              </p>
            </div>
            {schreibt ? (
              <button onClick={anhalten} disabled={haelt}>
                {haelt ? "Hält an …" : "Anhalten"}
              </button>
            ) : (
              <div className="knoepfe">
                <button
                  onClick={() => textebeneStarten(true)}
                  disabled={!info.pdfium_ordner || laeuft || vorgang !== null || tLokalAusstehend === 0}
                >
                  Probelauf
                </button>
                {bestaetigen ? (
                  <>
                    <button className="gefahr" onClick={() => textebeneStarten(false, true)}>
                      Eine Datei
                    </button>
                    <button className="gefahr" onClick={() => textebeneStarten(false)}>
                      Alle
                    </button>
                    <button onClick={() => setBestaetigen(false)}>Abbrechen</button>
                  </>
                ) : (
                  <button
                    onClick={() => setBestaetigen(true)}
                    disabled={!info.pdfium_ordner || laeuft || vorgang !== null || tLokalAusstehend === 0}
                  >
                    In PDFs schreiben
                  </button>
                )}
              </div>
            )}
          </div>

          {schreibt ? (
            <FortschrittAnzeige f={fortschritt} />
          ) : (
            <p className="stand-zeile">
              {t.dateien_ausstehend === 0
                ? "Alle betroffenen Scans haben ihre Textebene."
                : `${alsZahl(tLokalAusstehend)} PDFs mit ${alsZahl(t.seiten_ausstehend)} Seiten stehen an` +
                  (t.dateien_ausstehend_online > 0
                    ? `, ${alsZahl(t.dateien_ausstehend_online)} weitere liegen nur online.`
                    : ".")}
              {bestaetigen && " Die Originale im Cloud-Ordner werden ersetzt. Erst eine Datei oder gleich alle?"}
            </p>
          )}

          {textebeneLauf && <TextebeneMeldung e={textebeneLauf} />}

          {(t.dateien_geschrieben > 0 || t.seiten_in_pdf > 0) && (
            <div className="kacheln">
              <Kachel titel="PDFs geändert" wert={alsZahl(t.dateien_geschrieben)} zusatz="mit Textebene von Registerwerk" />
              <Kachel titel="Seiten mit Textebene" wert={alsZahl(t.seiten_in_pdf)} zusatz="erkannter Text in der PDF" />
            </div>
          )}

          {t.fehler_liste.length > 0 && <FehlerListe anzahl={t.fehler} liste={t.fehler_liste} />}
        </section>
      )}

      {bereit && v && v.seiten > 0 && <VorschauPruefen neuLaden={vorschauStand} />}

      {stand ? (
        <StandAnzeige stand={stand} />
      ) : (
        bereit && <p className="leer">Das Register ist noch leer. Der erste Durchlauf legt es an.</p>
      )}
    </div>
  );
}

/**
 * Dateien, die ausstehen und nur online liegen: Registerwerk öffnet sie nicht, weil das den Download
 * auslösen würde. Der Hinweis nennt die Ordner, damit sie sich in der Cloud offline verfügbar machen
 * lassen, und zeigt sie auf Wunsch im Finder.
 */
function NurOnlineHinweis({ lokal, online, vorschau = false }: { lokal: number; online: number; vorschau?: boolean }) {
  const [ordner, setOrdner] = useState<OnlineOrdner[] | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  // Nach jedem Einlesen ist die Liste überholt
  useEffect(() => setOrdner(null), [online]);
  if (online === 0) return null;
  const laden = () =>
    nurOnlineOrdner(vorschau)
      .then(setOrdner)
      .catch((e) => setFehler(String(e)));
  return (
    <div className="nur-online">
      <p className="achtung">
        {lokal === 0 && <>Der Knopf ist inaktiv, weil alle ausstehenden Dateien nur online liegen. </>}
        {alsZahl(online)} Dateien liegen nur online. Geladen werden sie mit „Verarbeiten“ im Abgleich oben. Stand des
        Registers: Wechselt eine Datei zwischen lokal und „nur online“, steht das erst nach dem nächsten Verarbeiten
        oder Einlesen hier.{" "}
        {ordner === null && (
          <button className="knopf knopf--leise" onClick={laden}>
            Ordner zeigen
          </button>
        )}
      </p>
      {fehler && <p className="achtung">{fehler}</p>}
      {ordner?.length === 0 && <p className="leise">Das Register nennt dazu gerade keinen Ordner. Bitte den Dateibaum einlesen.</p>}
      {ordner && ordner.length > 0 && (
        <ul className="nur-online__liste">
          {ordner.map((o) => (
            <li key={o.ordner}>
              <span className="pfad">{o.ordner}</span>
              <span className="nur-online__zahl">{alsZahl(o.dateien)}</span>
              <button
                className="knopf knopf--leise"
                onClick={() => ordnerImFinder(o.ordner).catch((e) => setFehler(String(e)))}
              >
                Im Finder
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function FortschrittAnzeige({ f }: { f: Fortschritt | null }) {
  const anteil = f && f.gesamt > 0 ? (f.erledigt / f.gesamt) * 100 : 0;
  return (
    <div className="fortschritt">
      <div className="fortschritt-balken">
        <span style={{ width: `${anteil}%` }} />
      </div>
      <p className="stand-zeile">
        {f ? `${alsZahl(f.erledigt)} von ${alsZahl(f.gesamt)}` : "Startet …"}
        {f && <span className="pfad fortschritt-datei"> {f.datei}</span>}
      </p>
    </div>
  );
}

function InhaltMeldung({ e }: { e: InhaltErgebnis }) {
  return (
    <div className="lauf">
      <p>
        {e.abgebrochen ? "Angehalten" : "Fertig"} nach {alsDauer(e.dauer_ms)}: <b>{alsZahl(e.gelesen)} Dateien</b> mit{" "}
        {alsZahl(e.seiten)} Seiten gelesen
        {e.fehler > 0 && (
          <>
            , <b>{alsZahl(e.fehler)} mit Fehler</b>
          </>
        )}
        {e.nur_online > 0 && <>, {alsZahl(e.nur_online)} nur online übergangen</>}.
      </p>
      {e.abgebrochen && <p className="stand-zeile">Der nächste Lauf setzt hier fort.</p>}
    </div>
  );
}

function VorschauMeldung({ e }: { e: VorschauErgebnis }) {
  return (
    <div className="lauf">
      <p>
        {e.abgebrochen ? "Angehalten" : "Fertig"} nach {alsDauer(e.dauer_ms)}: <b>{alsZahl(e.erzeugt)} PDFs</b> mit{" "}
        {alsZahl(e.seiten)} Seiten, {alsGroesse(e.bytes)}
        {e.fehler > 0 && (
          <>
            , <b>{alsZahl(e.fehler)} mit Fehler</b>
          </>
        )}
        {e.nur_online > 0 && <>, {alsZahl(e.nur_online)} nur online übergangen</>}
        {e.aufgeraeumt > 0 && <>, {alsZahl(e.aufgeraeumt)} alte Vorschauen entfernt</>}.
      </p>
      {e.abgebrochen && <p className="stand-zeile">Der nächste Lauf setzt hier fort.</p>}
      {e.seiten > 0 && <MessungAnzeige e={e} />}
    </div>
  );
}

function TextebeneMeldung({ e }: { e: TextebeneErgebnis }) {
  const zuwachs = e.bytes_nachher - e.bytes_vorher;
  return (
    <div className="lauf">
      <p>
        {e.probelauf ? "Probelauf" : "Schreiben"} {e.abgebrochen ? "angehalten" : "fertig"} nach {alsDauer(e.dauer_ms)}
        : <b>{alsZahl(e.geschrieben)} PDFs</b> mit {alsZahl(e.seiten)} Seiten {e.probelauf ? "als Kopie " : ""}
        geschrieben
        {e.fehler > 0 && (
          <>
            , <b>{alsZahl(e.fehler)} mit Fehler</b>
          </>
        )}
        {e.nur_online > 0 && <>, {alsZahl(e.nur_online)} nur online übergangen</>}
        {e.veraendert > 0 && <>, {alsZahl(e.veraendert)} seit dem Lesen geändert</>}.
        {e.geschrieben > 0 && (
          <>
            {" "}
            Zuwachs {alsGroesse(zuwachs)} ({alsGroesse(zuwachs / Math.max(1, e.seiten))} je Seite).
          </>
        )}
      </p>
      {e.probelauf && e.ziel && (
        <p className="stand-zeile">
          Kopien in <code>{e.ziel}</code>. Die Originale und das Register sind unverändert.
        </p>
      )}
      {e.veraendert > 0 && (
        <p className="stand-zeile">Geänderte Dateien zuerst neu einlesen, ihre Inhalte lesen und erkennen.</p>
      )}
      {e.fehler_liste.length > 0 && <FehlerListe anzahl={e.fehler} liste={e.fehler_liste} />}
    </div>
  );
}

function OcrMeldung({ e }: { e: OcrErgebnis }) {
  const m = e.messung;
  const seiten = Math.max(1, e.seiten);
  const dauer = Math.max(1, e.dauer_ms);
  const ms = (x: number) => `${Math.round(x).toLocaleString("de-DE")} ms`;
  const anteil = (x: number) => `${Math.round(x * 100)} %`;
  return (
    <div className="lauf">
      <p>
        {e.abgebrochen ? "Angehalten" : "Fertig"} nach {alsDauer(e.dauer_ms)}: <b>{alsZahl(e.erkannt)} PDFs</b> mit{" "}
        {alsZahl(e.seiten)} Seiten erkannt, <b>{alsZahl(e.uebernommen)} übernommen</b>
        {e.fehler > 0 && (
          <>
            , <b>{alsZahl(e.fehler)} mit Fehler</b>
          </>
        )}
        {e.nur_online > 0 && <>, {alsZahl(e.nur_online)} nur online übergangen</>}
        {e.veraendert > 0 && <>, {alsZahl(e.veraendert)} seit dem Lesen geändert</>}.
      </p>
      {e.abgebrochen && <p className="stand-zeile">Der nächste Lauf setzt hier fort.</p>}
      {e.veraendert > 0 && (
        <p className="stand-zeile">Geänderte Dateien zuerst neu einlesen und ihre Inhalte lesen.</p>
      )}
      {e.seiten > 0 && (
        <table className="messung">
          <tbody>
            <tr>
              <td>Tempo</td>
              <td className="r">
                {((seiten * 60000) / dauer).toLocaleString("de-DE", { maximumFractionDigits: 0 })} Seiten/min
              </td>
              <td>
                {ms(dauer / seiten)} je Seite,{" "}
                {(m.pixel / seiten / 1e6).toLocaleString("de-DE", { maximumFractionDigits: 1 })} Megapixel je Seite
              </td>
            </tr>
            <tr>
              <td>Rendern (1 Faden)</td>
              <td className="r">{ms(m.render_ms / seiten)} je Seite</td>
              <td>
                beschäftigt {anteil(m.render_ms / dauer)} der Laufzeit, wartet {anteil(m.render_wartet_ms / dauer)}{" "}
                auf freie Fäden
              </td>
            </tr>
            <tr>
              <td>Erkennen ({m.faeden} Fäden)</td>
              <td className="r">{ms(m.erkenn_ms / seiten)} je Seite</td>
              <td>
                ausgelastet {anteil(m.erkenn_ms / (m.faeden * dauer))}, leer{" "}
                {anteil(m.erkenn_leer_ms / (m.faeden * dauer))}
              </td>
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}

/** Wo die Zeit bleibt: Rendern läuft in einem Faden, Kodieren in mehreren. */
function MessungAnzeige({ e }: { e: VorschauErgebnis }) {
  const m = e.messung;
  const seiten = Math.max(1, e.seiten);
  const dauer = Math.max(1, e.dauer_ms);
  const ms = (x: number) => `${Math.round(x).toLocaleString("de-DE")} ms`;
  const anteil = (x: number) => `${Math.round(x * 100)} %`;
  return (
    <table className="messung">
      <tbody>
        <tr>
          <td>Tempo</td>
          <td className="r">{((seiten * 1000) / dauer).toLocaleString("de-DE", { maximumFractionDigits: 1 })} Seiten/s</td>
          <td>{ms(dauer / seiten)} je Seite, {(m.pixel / seiten / 1e6).toLocaleString("de-DE", { maximumFractionDigits: 2 })} Megapixel je Seite</td>
        </tr>
        <tr>
          <td>Rendern (1 Faden)</td>
          <td className="r">{ms(m.render_ms / seiten)} je Seite</td>
          <td>
            beschäftigt {anteil(m.render_ms / dauer)} der Laufzeit, wartet {anteil(m.render_wartet_ms / dauer)} auf freie
            Kodier-Fäden
          </td>
        </tr>
        <tr>
          <td>Kodieren ({m.faeden} Fäden)</td>
          <td className="r">{ms(m.kodier_ms / seiten)} je Seite</td>
          <td>
            ausgelastet {anteil(m.kodier_ms / (m.faeden * dauer))}, leer {anteil(m.kodier_leer_ms / (m.faeden * dauer))}
          </td>
        </tr>
        <tr>
          <td>Schreiben</td>
          <td className="r">{ms(m.schreib_ms / Math.max(1, e.erzeugt + e.fehler))} je Datei</td>
          <td>{anteil(m.schreib_ms / dauer)} der Laufzeit</td>
        </tr>
      </tbody>
    </table>
  );
}

function FehlerListe({
  anzahl,
  liste,
  titel,
}: {
  anzahl: number;
  liste: FehlerZeile[];
  /** Überschrift, sonst „… ließen sich nicht lesen“ */
  titel?: (n: number) => string;
}) {
  return (
    <details className="fehlerliste">
      <summary>
        {titel ? titel(anzahl) : `${alsZahl(anzahl)} ${anzahl === 1 ? "Datei ließ" : "Dateien ließen"} sich nicht lesen`}
      </summary>
      <table>
        <tbody>
          {liste.map((f) => (
            <tr key={f.pfad}>
              <td className="pfad">{f.pfad}</td>
              <td>{f.meldung}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="stand-zeile">Die Dateien werden erneut gelesen, sobald sie sich ändern.</p>
    </details>
  );
}

function StandAnzeige({ stand }: { stand: RegisterStand }) {
  const maxJahr = Math.max(1, ...stand.jahre.map((j) => j.anzahl));
  const i = stand.inhalt;
  const mitText = i.seiten - i.seiten_ohne_text;
  const pdfsMitText = i.gelesen - i.fehler - i.pdfs_ohne_text;
  return (
    <>
      <section className="kacheln">
        <Kachel titel="Dateien" wert={alsZahl(stand.dateien)} />
        <Kachel
          titel="Bekanntes Volumen"
          wert={alsGroesse(stand.bytes)}
          zusatz={stand.groesse_unbekannt > 0 ? `${alsZahl(stand.groesse_unbekannt)} ohne bekannte Größe` : undefined}
        />
        <Kachel titel="Nur online oder leer" wert={alsZahl(stand.nur_online)} />
        <Kachel
          titel="Datum im Dateinamen"
          wert={alsAnteil(stand.mit_datum, stand.dateien)}
          zusatz={`sonst PDF ${alsZahl(stand.datum_pdf)}, Änderung ${alsZahl(stand.datum_geaendert)}`}
        />
      </section>

      {i.seiten > 0 && (
        <section className="kacheln">
          <Kachel
            titel="Seiten im Register"
            wert={alsZahl(i.seiten)}
            zusatz={`aus ${alsZahl(i.gelesen - i.fehler)} von ${alsZahl(i.pdfs)} Dateien`}
          />
          <Kachel
            titel="Seiten mit Text"
            wert={alsAnteil(mitText, i.seiten)}
            zusatz={`${alsZahl(i.seiten_ohne_text)} Seiten ohne Text, ${alsZahl(i.pdfs_ohne_text)} PDFs ganz ohne`}
          />
          <Kachel
            titel="PDFs mit schwachem Text"
            wert={alsAnteil(i.pdfs_schwach, pdfsMitText)}
            zusatz={`${alsZahl(i.pdfs_schwach)} PDFs, ${alsZahl(i.seiten_schwach)} Seiten unter 60 % Güte`}
          />
          <Kachel
            titel="Scan-Seiten"
            wert={alsAnteil(i.seiten_scan, i.seiten)}
            zusatz={`${alsZahl(i.seiten - i.seiten_scan)} Seiten digital erzeugt`}
          />
        </section>
      )}

      {stand.letzter_lauf && (
        <p className="stand-zeile">Letzter Durchlauf: {alsZeitpunkt(stand.letzter_lauf.beendet)}</p>
      )}

      <div className="spalten">
        <section className="karte">
          <h2>Dateitypen</h2>
          <table>
            <thead>
              <tr>
                <th>Typ</th>
                <th className="r">Anzahl</th>
                <th className="r">Volumen</th>
                <th className="r">nur online</th>
              </tr>
            </thead>
            <tbody>
              {stand.typen.map((t) => (
                <tr key={t.endung}>
                  <td>{t.endung ? `.${t.endung}` : "(ohne Endung)"}</td>
                  <td className="r">{alsZahl(t.anzahl)}</td>
                  <td className="r">{alsGroesse(t.bytes)}</td>
                  <td className="r">{t.nur_online > 0 ? alsZahl(t.nur_online) : "–"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <section className="karte">
          <h2>Dokumente nach Jahr</h2>
          <p className="unterzeile">Datum im Dateinamen, sonst PDF-Erstellungsdatum, sonst Änderungsdatum</p>
          <ul className="jahre">
            {stand.jahre.map((j) => (
              <li key={j.jahr}>
                <span className="jahr">{j.jahr}</span>
                <span className="balken">
                  <span style={{ width: `${(j.anzahl / maxJahr) * 100}%` }} />
                </span>
                <span className="r">{alsZahl(j.anzahl)}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </>
  );
}

function Kachel({ titel, wert, zusatz }: { titel: string; wert: string; zusatz?: string }) {
  return (
    <div className="kachel">
      <span className="kachel-titel">{titel}</span>
      <span className="kachel-wert">{wert}</span>
      {zusatz && <span className="kachel-zusatz">{zusatz}</span>}
    </div>
  );
}
