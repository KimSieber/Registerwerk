import { useCallback, useState } from "react";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import Ordneransicht from "./ordner/Ordneransicht";
import Registerinfo from "./Registerinfo";
import Suche from "./suche/Suche";
import Ablauf, { Anstehend } from "./register/Ablauf";

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
  const [anstehend, setAnstehend] = useState<Anstehend>({ etwas: false, aenderungen: false });
  const anstehendMelden = useCallback((a: Anstehend) => setAnstehend(a), []);

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
          <button className={bereich === "register" ? "aktiv" : ""} onClick={() => setBereich("register")}>
            Register
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
      <div className="rahmen__inhalt">
        <Suche sichtbar={bereich === "suche"} />
        <Ordneransicht sichtbar={bereich === "ordner"} />
        <div className="rahmen__seite" hidden={bereich !== "register"}>
          <Ablauf onAenderungen={anstehendMelden} />
        </div>
      </div>
    </div>
  );
}
