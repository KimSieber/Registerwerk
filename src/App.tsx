import { useCallback, useEffect, useState } from "react";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import { AppInfo, LaufErgebnis, RegisterStand, appInfo, dateibaumEinlesen, registerStand } from "./api";
import { alsDauer, alsGroesse, alsZahl, alsZeitpunkt } from "./format";

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [stand, setStand] = useState<RegisterStand | null>(null);
  const [lauf, setLauf] = useState<LaufErgebnis | null>(null);
  const [laeuft, setLaeuft] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);

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

  const bereit = info !== null && !info.konfig_fehler;

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
          </dl>
        )}

        {lauf && <LaufMeldung lauf={lauf} />}
      </section>

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

function StandAnzeige({ stand }: { stand: RegisterStand }) {
  const maxJahr = Math.max(1, ...stand.jahre.map((j) => j.anzahl));
  const anteilDatum = stand.dateien > 0 ? Math.round((stand.mit_datum / stand.dateien) * 100) : 0;
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
        <Kachel titel="Datum im Dateinamen" wert={`${anteilDatum} %`} zusatz={`${alsZahl(stand.mit_datum)} Dateien`} />
      </section>

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
          <p className="unterzeile">Nach dem Datum am Anfang des Dateinamens</p>
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
