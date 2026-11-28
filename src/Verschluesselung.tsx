import { useCallback, useEffect, useState } from "react";
import {
  Fortschritt,
  SchluesselStand,
  UmstellErgebnis,
  inhalteAbbrechen,
  registerVerschluesseln,
  schluesselEingeben,
  schluesselEntfernen,
  schluesselStand,
} from "./api";
import { alsDauer, alsGroesse, alsZahl } from "./format";

/** Zählt die Hex-Zeichen einer Eingabe, Leerzeichen und Bindestriche zählen nicht. */
function zeichen(text: string) {
  return text.replace(/[\s-]/g, "").length;
}

/**
 * Eingabefeld für den Schlüssel: verdeckt, ohne Autovervollständigung, mit Zähler der Zeichen.
 * Der Schlüssel selbst wird nie angezeigt.
 */
function SchluesselFeld(props: {
  wert: string;
  setWert: (w: string) => void;
  beiEnter: () => void;
  gesperrt: boolean;
  bezeichnung: string;
}) {
  const n = zeichen(props.wert);
  return (
    <label className="schluessel__feld">
      <span>{props.bezeichnung}</span>
      <input
        type="password"
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        value={props.wert}
        disabled={props.gesperrt}
        onChange={(e) => props.setWert(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && n === 64 && props.beiEnter()}
        placeholder="64 Zeichen 0–9, a–f"
      />
      <span className={`leise schluessel__zaehler ${n === 64 ? "ok" : ""}`}>{n} von 64 Zeichen</span>
    </label>
  );
}

/**
 * Schlüssel eingeben, wenn das Register verschlüsselt ist und auf diesem Gerät noch kein Schlüssel
 * liegt (v0.1.0). Auch als Sperre vor der ganzen Anwendung.
 */
export function SchluesselEingabe(props: { stand: SchluesselStand; fertig: () => void }) {
  const [wert, setWert] = useState("");
  const [arbeitet, setArbeitet] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);

  const speichern = async () => {
    setArbeitet(true);
    setFehler(null);
    try {
      await schluesselEingeben(wert);
      setWert("");
      props.fertig();
    } catch (e) {
      setFehler(String(e));
    } finally {
      setArbeitet(false);
    }
  };

  return (
    <div className="kasten schluessel">
      <h3>Register verschlüsselt</h3>
      <p>
        Auf diesem Gerät ist noch kein Schlüssel hinterlegt
        {props.stand.schluessel_fehler && props.stand.verschluesselung.schluessel_hinterlegt
          ? ", oder der hinterlegte passt nicht"
          : ""}
        . Bitte den Schlüssel aus dem Passwortsafe einfügen.{" "}
        {props.stand.schluessel_dauerhaft
          ? "Er wird geprüft und im Schlüsselbund dieses Geräts abgelegt, ohne Abgleich über die iCloud."
          : "Dieses System hat keinen angebundenen Schlüsselbund: Der Schlüssel gilt bis zum Beenden."}
      </p>
      <SchluesselFeld
        wert={wert}
        setWert={setWert}
        beiEnter={() => void speichern()}
        gesperrt={arbeitet}
        bezeichnung="Schlüssel"
      />
      {fehler && <p className="achtung">{fehler}</p>}
      <div className="registerinfo__knoepfe">
        <button className="knopf" disabled={arbeitet || zeichen(wert) !== 64} onClick={() => void speichern()}>
          {arbeitet && <span className="kreisel" />}
          Prüfen und speichern
        </button>
      </div>
    </div>
  );
}

/**
 * Sperre vor der ganzen Anwendung (v0.1.4): Auf diesem Gerät ist ein Schlüssel hinterlegt, aber
 * mindestens eine Datei des Registers ist unverschlüsselt. Bietet die Umstellung an und lädt nach
 * einem vollständigen Lauf neu.
 */
export function UnverschluesseltSperre(props: { stand: SchluesselStand }) {
  const n = props.stand.verschluesselung.unverschluesselt;
  return (
    <div className="kasten schluessel">
      <h3>Register nicht vollständig verschlüsselt</h3>
      <p>
        Auf diesem Gerät ist ein Schlüssel für das Register hinterlegt, aber{" "}
        {n === 1 ? "eine Datei ist" : `${alsZahl(n)} Dateien sind`} unverschlüsselt. Das kommt vor, wenn die
        Umstellung angehalten wurde oder eine ältere Fassung zurückgespielt wurde, etwa aus dem Versionsverlauf
        der Cloud. Registerwerk öffnet das Register so nicht, damit nichts unverschlüsselt weitergeschrieben wird.
      </p>
      <p>
        Ist die Liste der Dateien erwartet, die Umstellung abschließen. Wenn nicht, erst klären, woher die
        Dateien stammen.
      </p>
      <Verschluesselung laeuft={() => {}} fertig={() => window.location.reload()} />
    </div>
  );
}

/**
 * Kasten „Verschlüsselung“ im Datenbank-Symbol (v0.1.0): Zustand, Schlüssel für die Umstellung,
 * Umstellung mit Fortschritt und Anhalten, Schlüssel von diesem Gerät entfernen.
 * `laeuft` meldet dem Datenbank-Symbol, dass es offen bleiben soll. `fertig` nach einem
 * vollständigen Lauf (v0.1.4).
 */
export default function Verschluesselung(props: { laeuft: (l: boolean) => void; fertig?: () => void }) {
  // Eigener, schneller Stand: nur Dateiköpfe, unabhängig von den Kennzahlen des Registers
  const [stand, setStand] = useState<SchluesselStand | null>(null);
  const neuLaden = useCallback(() => {
    schluesselStand()
      .then(setStand)
      .catch((e) => setFehler(String(e)));
  }, []);
  useEffect(() => neuLaden(), [neuLaden]);
  const [wert, setWert] = useState("");
  const [arbeitet, setArbeitet] = useState(false);
  const [stellt, setStellt] = useState(false);
  const [haelt, setHaelt] = useState(false);
  const [fortschritt, setFortschritt] = useState<Fortschritt | null>(null);
  const [ergebnis, setErgebnis] = useState<UmstellErgebnis | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [entfernenFragen, setEntfernenFragen] = useState(false);

  if (!stand) return null;
  if (stand.schluessel_fehler) return <SchluesselEingabe stand={stand} fertig={() => window.location.reload()} />;
  const v = stand.verschluesselung;
  if (v.dateien === 0) return null;

  const eingeben = async () => {
    setArbeitet(true);
    setFehler(null);
    try {
      await schluesselEingeben(wert);
      setWert("");
      // Schlüssel hinterlegt, Register noch unverschlüsselt: Die Anwendung ist jetzt gesperrt und
      // zeigt nach dem Neuladen die Sperre mit Schritt 2 (v0.1.4)
      window.location.reload();
    } catch (e) {
      setFehler(String(e));
    } finally {
      setArbeitet(false);
    }
  };

  const verschluesseln = async () => {
    setStellt(true);
    props.laeuft(true);
    setHaelt(false);
    setFehler(null);
    setErgebnis(null);
    setFortschritt(null);
    try {
      const e = await registerVerschluesseln(wert, setFortschritt);
      setErgebnis(e);
      setWert("");
      if (!e.abgebrochen) props.fertig?.();
    } catch (e) {
      setFehler(String(e));
    } finally {
      setStellt(false);
      props.laeuft(false);
      neuLaden();
    }
  };

  const entfernen = async () => {
    setFehler(null);
    try {
      await schluesselEntfernen();
      setEntfernenFragen(false);
      // Alle Ansichten lesen neu und zeigen die Eingabe
      window.location.reload();
    } catch (e) {
      setFehler(String(e));
    }
  };

  // Nach einem vollständigen Lauf ist nichts mehr offen, auch bevor der neue Stand da ist
  const offen = ergebnis && !ergebnis.abgebrochen ? 0 : v.unverschluesselt;
  const fertig = v.dateien > 0 && offen === 0;

  return (
    <div className="registerinfo__frage verschluesselung">
      <h4>
        Verschlüsselung{" "}
        {fertig ? (
          <span className="marke marke--gut">verschlüsselt</span>
        ) : v.verschluesselt > 0 ? (
          <span className="marke marke--warnung">teilweise</span>
        ) : (
          <span className="marke marke--warnung">unverschlüsselt</span>
        )}
      </h4>

      {stellt ? (
        <>
          <p>
            <span className="kreisel" /> <b>Register wird verschlüsselt.</b>{" "}
            {fortschritt
              ? `Datei ${alsZahl(Math.min(fortschritt.erledigt + 1, fortschritt.gesamt))} von ${alsZahl(fortschritt.gesamt)}`
              : "Startet …"}
            {fortschritt && fortschritt.datei && <span className="leise"> · {fortschritt.datei}</span>}
          </p>
          <div className="fortschritt-balken">
            <span
              style={{
                width: `${fortschritt && fortschritt.gesamt > 0 ? (fortschritt.erledigt / fortschritt.gesamt) * 100 : 0}%`,
              }}
            />
          </div>
          <p className="leise">
            Angehalten wird zwischen zwei Dateien. Die Index-Datenbank kommt zuletzt und braucht am längsten.
          </p>
          <div className="registerinfo__knoepfe">
            <button
              className="knopf knopf--leise"
              disabled={haelt}
              onClick={() => {
                setHaelt(true);
                void inhalteAbbrechen();
              }}
            >
              {haelt ? "Hält nach dieser Datei an …" : "Anhalten"}
            </button>
          </div>
        </>
      ) : (
        <>
          <dl className="registerinfo__zahlen">
            <dt>Dateien</dt>
            <dd>
              {alsZahl(v.dateien - offen)} von {alsZahl(v.dateien)} verschlüsselt
              {offen > 0 && (
                <details className="verschluesselung__liste">
                  <summary>
                    {alsZahl(offen)} unverschlüsselt, {alsGroesse(v.unverschluesselt_bytes)}
                  </summary>
                  <ul className="pfad">
                    {v.unverschluesselt_liste.map((n) => (
                      <li key={n}>{n}</li>
                    ))}
                  </ul>
                </details>
              )}
            </dd>
            <dt>Schlüssel</dt>
            <dd>
              {v.schluessel_hinterlegt
                ? stand.schluessel_dauerhaft
                  ? "auf diesem Gerät im Schlüsselbund"
                  : "bis zum Beenden hinterlegt"
                : "auf diesem Gerät keiner"}
            </dd>
          </dl>

          {ergebnis && (
            <p>
              {ergebnis.abgebrochen ? "Angehalten" : "Fertig"} nach {alsDauer(ergebnis.dauer_ms)}:{" "}
              <b>
                {alsZahl(ergebnis.dateien)} {ergebnis.dateien === 1 ? "Datei" : "Dateien"}
              </b>{" "}
              ({alsGroesse(ergebnis.bytes)}) verschlüsselt.
            </p>
          )}

          {v.arbeitskopien.length > 0 && (
            <p className="achtung">
              Im Register-Ordner liegt die Arbeitskopie einer abgebrochenen Umstellung ({v.arbeitskopien.join(", ")}).
              Der nächste Lauf ersetzt sie. Das Original ist unberührt.
            </p>
          )}

          {offen > 0 && !v.schluessel_hinterlegt && (
            <>
              <h5 className="verschluesselung__schritt">Schritt 1 von 2: Schlüssel speichern</h5>
              <p>
                Den selbst erzeugten Schlüssel aus dem Passwortsafe einfügen. Er wird im Schlüsselbund dieses Geräts
                abgelegt. Das Register ändert sich dabei noch nicht.
              </p>
              <SchluesselFeld
                wert={wert}
                setWert={setWert}
                beiEnter={() => void eingeben()}
                gesperrt={arbeitet}
                bezeichnung="Schlüssel"
              />
              <div className="registerinfo__knoepfe">
                <button className="knopf" disabled={arbeitet || zeichen(wert) !== 64} onClick={() => void eingeben()}>
                  {arbeitet && <span className="kreisel" />}
                  Schlüssel speichern
                </button>
              </div>
            </>
          )}

          {offen > 0 && v.schluessel_hinterlegt && (
            <>
              <h5 className="verschluesselung__schritt">Schritt 2 von 2: Bestätigen und verschlüsseln</h5>
              <p>
                Zur Bestätigung den Schlüssel <b>noch einmal</b> aus dem Passwortsafe einfügen. So verschließt
                kein vertippter Schlüssel das Register. Das Verschlüsseln schreibt jede Datei neu, die Cloud lädt
                das Register einmal ganz neu hoch. Vorher: Cloud fertig abgeglichen, Registerwerk auf keinem
                anderen Gerät offen.
              </p>
              <SchluesselFeld
                wert={wert}
                setWert={setWert}
                beiEnter={() => void verschluesseln()}
                gesperrt={false}
                bezeichnung="Bestätigung"
              />
              <div className="registerinfo__knoepfe">
                <button className="knopf" disabled={zeichen(wert) !== 64} onClick={() => void verschluesseln()}>
                  {ergebnis?.abgebrochen || v.verschluesselt > 0 ? "Verschlüsseln fortsetzen" : "Register verschlüsseln"}
                </button>
              </div>
            </>
          )}

          {v.schluessel_hinterlegt && stand.schluessel_dauerhaft && !entfernenFragen && (
            <div className="registerinfo__knoepfe">
              <button className="knopf knopf--leise" onClick={() => setEntfernenFragen(true)}>
                Schlüssel von diesem Gerät entfernen …
              </button>
            </div>
          )}
          {entfernenFragen && (
            <>
              <p>
                Der Schlüssel wird aus dem Schlüsselbund dieses Geräts entfernt. Das Register bleibt unverändert,
                ist hier aber erst nach erneuter Eingabe wieder lesbar. Liegt der Schlüssel sicher im Passwortsafe?
              </p>
              <div className="registerinfo__knoepfe">
                <button className="knopf knopf--leise" onClick={() => setEntfernenFragen(false)}>
                  Abbrechen
                </button>
                <button className="knopf" onClick={() => void entfernen()}>
                  Entfernen
                </button>
              </div>
            </>
          )}
        </>
      )}
      {fehler && <p className="achtung">{fehler}</p>}
    </div>
  );
}
