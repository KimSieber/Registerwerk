import { useCallback, useEffect, useState } from "react";
import { SchluesselStand, schluesselStand } from "./api";
import { SchluesselEingabe, UnverschluesseltSperre } from "./Verschluesselung";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import Ordneransicht from "./ordner/Ordneransicht";
import Registerinfo from "./Registerinfo";
import Suche from "./suche/Suche";
import Ablauf, { Anstehend, NICHTS_ANSTEHEND } from "./register/Ablauf";
import { useTaetigkeiten } from "./taetigkeit";

type Bereich = "suche" | "ordner" | "register";

/**
 * Rahmen der Anwendung: oben die Wahl zwischen Suche, Ordner (Regeln, welche Ordner ins Register
 * kommen) und Register (Einlesen, Vorschauen, OCR).
 * Beide Bereiche bleiben geladen, damit Suche und Auswahl beim Wechsel erhalten bleiben.
 *
 * Die Anwendung öffnet immer mit der Suche (v0.0.9). Der Abgleich läuft dabei im Hintergrund.
 * Steht etwas an (neu, geändert, noch offen, zu entfernen), erscheint neben den Reitern ein Knopf,
 * der in den Reiter „Register“ führt. Die Zahlen stehen dort. Geprüft wird beim Start und auf
 * Wunsch im Reiter „Register“.
 */
export default function App() {
  const [bereich, setBereich] = useState<Bereich>("suche");
  const [anstehend, setAnstehend] = useState<Anstehend>(NICHTS_ANSTEHEND);
  const anstehendMelden = useCallback((a: Partial<Anstehend>) => setAnstehend((alt) => ({ ...alt, ...a })), []);
  // Kreisel am Reiter „Register“, solange im Hintergrund am Register gearbeitet wird (v0.1.1)
  const taetigkeiten = useTaetigkeiten();
  const zumRegister = useCallback(() => setBereich("register"), []);
  // Verschlüsseltes Register ohne Schlüssel auf diesem Gerät (v0.1.0): erst den Schlüssel eingeben
  // `undefined`: noch nicht geprüft, dann bleibt der Inhalt leer, damit nichts vergeblich liest
  // Schnell: liest nur die Dateiköpfe und eine Seite der Index-Datenbank
  const [gesperrt, setGesperrt] = useState<SchluesselStand | null | undefined>(undefined);
  useEffect(() => {
    schluesselStand()
      .then((s) => setGesperrt(s.schluessel_fehler || s.verschluesselung.gesperrt ? s : null))
      .catch(() => setGesperrt(null));
  }, []);

  return (
    <div className="rahmen">
      <nav className="reiter">
        <img src={logo} alt="" width={22} height={22} />
        <span className="reiter__name">Registerwerk</span>
        <span className="umschalter umschalter--text">
          <button className={bereich === "suche" ? "aktiv" : ""} onClick={() => setBereich("suche")}>
            Suche
          </button>
          <button className={bereich === "ordner" ? "aktiv" : ""} onClick={() => setBereich("ordner")}>
            Ordner
          </button>
          <button
            className={`reiter__register${bereich === "register" ? " aktiv" : ""}`}
            onClick={() => setBereich("register")}
            title={taetigkeiten.length > 0 ? `Läuft: ${taetigkeiten.join(", ")}` : undefined}
          >
            Register
            {/* Platz bleibt immer frei, damit der Reiter beim Ein- und Ausblenden nicht springt */}
            <span
              className={`kreisel reiter__kreisel${taetigkeiten.length > 0 ? "" : " reiter__kreisel--ruhe"}`}
              aria-label={taetigkeiten.length > 0 ? "Läuft" : undefined}
            />
          </button>
        </span>
        {anstehend.etwas && bereich !== "register" && (
          <button
            className="reiter__aenderungen"
            onClick={() => setBereich("register")}
            title="Der Abgleich hat Dateien gefunden, die noch ins Register aufzunehmen oder zu verarbeiten sind"
          >
            {anstehend.aenderungen ? "Änderungen gefunden" : "Verarbeitung ausstehend"}
          </button>
        )}
        <Registerinfo />
      </nav>
      {gesperrt === undefined ? (
        <div className="rahmen__inhalt" />
      ) : gesperrt ? (
        <div className="rahmen__inhalt sperre">
          {gesperrt.schluessel_fehler ? (
            <SchluesselEingabe stand={gesperrt} fertig={() => window.location.reload()} />
          ) : (
            <UnverschluesseltSperre stand={gesperrt} />
          )}
        </div>
      ) : (
      <div className="rahmen__inhalt">
        <Suche sichtbar={bereich === "suche"} />
        <Ordneransicht sichtbar={bereich === "ordner"} stand={anstehend} zumRegister={zumRegister} />
        <div className="rahmen__seite" hidden={bereich !== "register"}>
          <Ablauf onAenderungen={anstehendMelden} />
        </div>
      </div>
      )}
    </div>
  );
}
