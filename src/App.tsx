import { useCallback, useEffect, useState } from "react";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import {
  AppInfo,
  Fortschritt,
  InhaltErgebnis,
  InhaltStand,
  LaufErgebnis,
  RegisterStand,
  appInfo,
  dateibaumEinlesen,
  inhalteAbbrechen,
  inhalteLesen,
  registerStand,
} from "./api";
import { alsAnteil, alsDauer, alsGroesse, alsZahl, alsZeitpunkt } from "./format";

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [stand, setStand] = useState<RegisterStand | null>(null);
  const [lauf, setLauf] = useState<LaufErgebnis | null>(null);
  const [laeuft, setLaeuft] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [liest, setLiest] = useState(false);
  const [fortschritt, setFortschritt] = useState<Fortschritt | null>(null);
  const [inhaltLauf, setInhaltLauf] = useState<InhaltErgebnis | null>(null);
  const [haelt, setHaelt] = useState(false);

  const standLaden = useCallback(async () => {
    try {
      setStand(await registerStand());
    } catch (e) {
      setFehler(String(e));
    }
  }, []);

  useEffect(() => {
    appInfo()
      .then((i) => {
        setInfo(i);
        if (!i.konfig_fehler) void standLaden();
      })
      .catch((e) => setFehler(String(e)));
  }, [standLaden]);

  async function einlesen() {
    setLaeuft(true);
    setFehler(null);
    try {
      setLauf(await dateibaumEinlesen());
      await standLaden();
    } catch (e) {
      setFehler(String(e));
    } finally {
      setLaeuft(false);
    }
  }

  async function inhalteStarten() {
    setLiest(true);
    setHaelt(false);
    setFehler(null);
    setFortschritt(null);
    setInhaltLauf(null);
    try {
      setInhaltLauf(await inhalteLesen(setFortschritt));
    } catch (e) {
      setFehler(String(e));
    } finally {
      setLiest(false);
      setFortschritt(null);
      await standLaden();
    }
  }

  async function anhalten() {
    setHaelt(true);
    await inhalteAbbrechen();
  }

  const bereit = info !== null && !info.konfig_fehler;
  const ausstehend = stand?.inhalt.ausstehend ?? 0;
  const lokalAusstehend = ausstehend - (stand?.inhalt.ausstehend_online ?? 0);

  return (
    <div className="seite">
      <header className="kopf">
        <img src={logo} alt="" width={44} height={44} />
        <div>
          <h1>Registerwerk</h1>
          <p className="unterzeile">Register über das Dokumentenarchiv</p>
        </div>
        {info && <span className="version">v{info.version}</span>}
      </header>

      {info?.konfig_fehler && (
        <section className="hinweis warnung">
          <strong>Konfiguration fehlt oder ist fehlerhaft</strong>
          <p>{info.konfig_fehler}</p>
          {info.konfig_pfad && <p className="pfad">{info.konfig_pfad}</p>}
        </section>
      )}
      {fehler && (
        <section className="hinweis fehler">
          <strong>Fehler</strong>
          <p>{fehler}</p>
        </section>
      )}

      <section className="karte">
        <div className="karte-kopf">
          <div>
            <h2>Dateibaum</h2>
            <p className="unterzeile">
              Liest nur Namen, Größe und Datum. Es wird keine Datei geöffnet und nichts aus der Cloud geladen.
            </p>
          </div>
          <button onClick={einlesen} disabled={!bereit || laeuft}>
            {laeuft ? "Liest ein …" : "Dateibaum einlesen"}
          </button>
        </div>

        {bereit && (
          <dl className="angaben">
            <dt>Cloud-Ordner</dt>
            <dd className="pfad">{info.cloud_ordner}</dd>
            <dt>Verzeichnisse</dt>
            <dd>
              {info.verzeichnisse.map((v) => (
                <span key={v} className="marke">
                  {v}
                </span>
              ))}
            </dd>
            <dt>Register</dt>
            <dd className="pfad">{info.register_ordner}</dd>
            <dt>PDFium</dt>
            <dd className="pfad">{info.pdfium_ordner ?? "nicht gefunden"}</dd>
          </dl>
        )}

        {lauf && <LaufMeldung lauf={lauf} />}
      </section>

      {bereit && stand && (
        <section className="karte">
          <div className="karte-kopf">
            <div>
              <h2>Inhalte der PDFs</h2>
              <p className="unterzeile">
                Öffnet lokal vorhandene PDFs nur lesend und übernimmt den Text jeder Seite. „Nur online“-Dateien werden
                übergangen, damit nichts aus der Cloud geladen wird.
              </p>
            </div>
            {liest ? (
              <button onClick={anhalten} disabled={haelt}>
                {haelt ? "Hält an …" : "Anhalten"}
              </button>
            ) : (
              <button onClick={inhalteStarten} disabled={!info.pdfium_ordner || laeuft || lokalAusstehend === 0}>
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
                ? "Alle PDFs sind gelesen."
                : `${alsZahl(lokalAusstehend)} PDFs stehen zum Lesen an` +
                  (stand.inhalt.ausstehend_online > 0
                    ? `, ${alsZahl(stand.inhalt.ausstehend_online)} weitere liegen nur online.`
                    : ".")}
            </p>
          )}

          {inhaltLauf && <InhaltMeldung e={inhaltLauf} />}
          {stand.inhalt.fehler_liste.length > 0 && <FehlerListe inhalt={stand.inhalt} />}
        </section>
      )}

      {stand ? (
        <StandAnzeige stand={stand} />
      ) : (
        bereit && <p className="leer">Das Register ist noch leer. Der erste Durchlauf legt es an.</p>
      )}
    </div>
  );
}

function LaufMeldung({ lauf }: { lauf: LaufErgebnis }) {
  return (
    <div className="lauf">
      <p>
        Durchlauf {lauf.lauf} in {alsDauer(lauf.dauer_ms)}: {alsZahl(lauf.gesehen)} Dateien gesehen,{" "}
        <b>{alsZahl(lauf.neu)} neu</b>, <b>{alsZahl(lauf.geaendert)} geändert</b>,{" "}
        <b>{alsZahl(lauf.entfernt)} entfernt</b>, {alsZahl(lauf.unveraendert)} unverändert.
      </p>
      {lauf.fehlende_verzeichnisse.length > 0 && (
        <p className="achtung">
          Nicht gefunden: {lauf.fehlende_verzeichnisse.join(", ")}. Für diese Verzeichnisse wurde nichts entfernt.
        </p>
      )}
      {lauf.nicht_lesbar > 0 && (
        <p className="achtung">
          {alsZahl(lauf.nicht_lesbar)} Einträge waren nicht lesbar ({lauf.unvollstaendige_verzeichnisse.join(", ")}).
          Für diese Verzeichnisse wurde nichts entfernt.
        </p>
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
        {e.abgebrochen ? "Angehalten" : "Fertig"} nach {alsDauer(e.dauer_ms)}: <b>{alsZahl(e.gelesen)} PDFs</b> mit{" "}
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

function FehlerListe({ inhalt }: { inhalt: InhaltStand }) {
  return (
    <details className="fehlerliste">
      <summary>
        {alsZahl(inhalt.fehler)} {inhalt.fehler === 1 ? "PDF ließ" : "PDFs ließen"} sich nicht lesen
      </summary>
      <table>
        <tbody>
          {inhalt.fehler_liste.map((f) => (
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
            zusatz={`aus ${alsZahl(i.gelesen - i.fehler)} von ${alsZahl(i.pdfs)} PDFs`}
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
