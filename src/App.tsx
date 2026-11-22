import { useCallback, useEffect, useState } from "react";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import Ordneransicht from "./ordner/Ordneransicht";
import Registerinfo from "./Registerinfo";
import Suche from "./suche/Suche";
import Ablauf from "./register/Ablauf";
import { lese, schreibe } from "./suche/breiten";

type Bereich = "suche" | "ordner" | "register";

/**
 * Rahmen der Anwendung: oben die Wahl zwischen Suche, Ordner (Regeln, welche Ordner ins Register
 * kommen) und Register (Einlesen, Vorschauen, OCR).
 * Beide Bereiche bleiben geladen, damit Suche und Auswahl beim Wechsel erhalten bleiben.
 */
export default function App() {
  const [bereich, setBereich] = useState<Bereich>(() => lese("app.bereich", "suche"));
  useEffect(() => schreibe("app.bereich", bereich), [bereich]);
  // Anstehende Dateien aus der Prüfung beim Start, als Zahl am Reiter „Register“
  const [anstehend, setAnstehend] = useState(0);
  const anstehendMelden = useCallback((n: number) => setAnstehend(n), []);

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
            {anstehend > 0 && <span className="reiter__zahl">{anstehend.toLocaleString("de-DE")}</span>}
          </button>
        </span>
        <Registerinfo />
      </nav>
      <div className="rahmen__inhalt">
        <Suche sichtbar={bereich === "suche"} />
        <Ordneransicht sichtbar={bereich === "ordner"} />
        <div className="rahmen__seite" hidden={bereich !== "register"}>
          <Ablauf sichtbar={bereich === "register"} onAnstehend={anstehendMelden} />
        </div>
      </div>
    </div>
  );
}
