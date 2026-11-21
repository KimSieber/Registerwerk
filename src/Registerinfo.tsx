import { useCallback, useEffect, useRef, useState } from "react";
import { GEAENDERT, RegisterInfo, ordnerWaehlen, registerImFinder, registerInfo, registerWaehlen } from "./api";
import { alsGroesse, alsZahl, alsZeitpunkt } from "./format";

/**
 * Datenbank-Symbol rechts oben (v0.0.7), wie in Bilderwerk: zeigt, wo das Register liegt und wie groß
 * es ist, und lässt ein anderes wählen, etwa nachdem der Ordner verschoben wurde.
 */
export default function Registerinfo() {
  const [offen, setOffen] = useState(false);
  const [info, setInfo] = useState<RegisterInfo | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  /** Gewählter Ordner ohne Register: erst nachfragen */
  const [leer, setLeer] = useState<RegisterInfo | null>(null);
  const feld = useRef<HTMLDivElement>(null);

  const laden = useCallback(() => {
    registerInfo()
      .then((i) => {
        setInfo(i);
        setFehler(null);
      })
      .catch((e) => setFehler(String(e)));
  }, []);

  useEffect(() => {
    laden();
    window.addEventListener(GEAENDERT, laden);
    return () => window.removeEventListener(GEAENDERT, laden);
  }, [laden]);

  // Klick daneben und Esc schließen
  useEffect(() => {
    if (!offen) return;
    laden();
    const klick = (e: MouseEvent) => {
      if (feld.current && !feld.current.contains(e.target as Node)) setOffen(false);
    };
    const taste = (e: KeyboardEvent) => e.key === "Escape" && setOffen(false);
    window.addEventListener("mousedown", klick);
    window.addEventListener("keydown", taste);
    return () => {
      window.removeEventListener("mousedown", klick);
      window.removeEventListener("keydown", taste);
    };
  }, [offen, laden]);

  const uebernehmen = async (ordner: string, bestaetigt: boolean) => {
    setFehler(null);
    try {
      const [neu, gilt] = await registerWaehlen(ordner, bestaetigt);
      if (gilt) {
        // Alle Ansichten lesen das neue Register
        window.location.reload();
      } else {
        setLeer(neu);
      }
    } catch (e) {
      setFehler(String(e));
    }
  };

  const waehlen = async () => {
    try {
      const ordner = await ordnerWaehlen();
      if (ordner !== null) await uebernehmen(ordner, false);
    } catch (e) {
      setFehler(String(e));
    }
  };

  return (
    <div className="registerinfo" ref={feld}>
      <button
        className={`registerinfo__knopf ${offen ? "aktiv" : ""}`}
        onClick={() => setOffen((o) => !o)}
        title="Register: Ort, Größe, anderes Register wählen"
        aria-label="Register-Datenbank"
        aria-expanded={offen}
      >
        <svg viewBox="0 0 24 24" aria-hidden>
          <ellipse cx="12" cy="6" rx="7.5" ry="3" />
          <path d="M4.5 6v6c0 1.7 3.4 3 7.5 3s7.5-1.3 7.5-3V6" />
          <path d="M4.5 12v6c0 1.7 3.4 3 7.5 3s7.5-1.3 7.5-3v-6" />
        </svg>
        {info && <span>{alsZahl(info.dateien)}</span>}
      </button>
      {offen && (
        <div className="registerinfo__feld" role="dialog" aria-label="Register-Datenbank">
          <h3>Register</h3>
          {fehler && <p className="achtung">{fehler}</p>}
          {info && (
            <>
              <p className="pfad registerinfo__ort">{info.ordner}</p>
              {!info.gewaehlt && <p className="leise">Standard-Ort im Benutzerprofil dieses Geräts.</p>}
              {!info.vorhanden && <p className="achtung">In diesem Ordner liegt noch kein Register.</p>}
              <dl className="registerinfo__zahlen">
                <dt>Dokumente</dt>
                <dd>{alsZahl(info.dateien)}</dd>
                <dt>Seiten mit Text</dt>
                <dd>{alsZahl(info.seiten)}</dd>
                <dt>Vorschauen</dt>
                <dd>{alsZahl(info.vorschauen)}</dd>
                <dt>Größe</dt>
                <dd>
                  <b>{alsGroesse(info.index_bytes + info.scheiben_bytes)}</b>
                  <span className="leise">
                    {" "}
                    · Index {alsGroesse(info.index_bytes)} · Vorschauen {alsGroesse(info.scheiben_bytes)} in{" "}
                    {alsZahl(info.scheiben)} {info.scheiben === 1 ? "Scheibe" : "Scheiben"}
                  </span>
                </dd>
                <dt>Ordner-Regeln</dt>
                <dd>{alsZahl(info.regeln)}</dd>
                <dt>Geräte</dt>
                <dd>
                  {info.geraete.length === 0 && <span className="leise">noch kein Durchlauf</span>}
                  {info.geraete.map(([name, zuletzt]) => (
                    <div key={name}>
                      {name}
                      {name === info.geraet && <span className="leise"> (dieses Gerät)</span>}
                      {zuletzt !== null && <span className="leise"> · zuletzt {alsZeitpunkt(zuletzt)}</span>}
                    </div>
                  ))}
                </dd>
              </dl>
            </>
          )}
          {leer ? (
            <div className="registerinfo__frage">
              <p>
                In <span className="pfad">{leer.ordner}</span> liegt kein Register. Soll dort ein neues, leeres
                entstehen? Das bisherige bleibt an seinem Ort unverändert.
              </p>
              <div className="registerinfo__knoepfe">
                <button className="knopf knopf--leise" onClick={() => setLeer(null)}>
                  Abbrechen
                </button>
                <button className="knopf" onClick={() => void uebernehmen(leer.ordner, true)}>
                  Neues Register dort anlegen
                </button>
              </div>
            </div>
          ) : (
            <div className="registerinfo__knoepfe">
              <button className="knopf knopf--leise" onClick={() => registerImFinder().catch((e) => setFehler(String(e)))}>
                Im Finder zeigen
              </button>
              <button className="knopf" onClick={() => void waehlen()}>
                Anderes Register wählen …
              </button>
            </div>
          )}
          <p className="leise registerinfo__hinweis">
            Verschieben: Registerwerk beenden, den ganzen Inhalt des Ordners im Finder verschieben (register.db, alle
            vorschau-….db, regeln.toml, textebene.log), dann hier den neuen Ordner wählen. Liegt das Register in
            einem Cloud-Ordner, darf es immer nur auf einem Gerät geöffnet sein, und die Cloud muss vor dem Wechsel
            fertig synchronisiert haben.
          </p>
        </div>
      )}
    </div>
  );
}
