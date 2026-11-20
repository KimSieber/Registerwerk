import { useEffect, useState } from "react";
import logo from "../docs/icons/png-randlos/registerwerk_128.png";
import Suche from "./suche/Suche";
import Verwaltung from "./Verwaltung";
import { lese, schreibe } from "./suche/breiten";

type Bereich = "suche" | "register";

/**
 * Rahmen der Anwendung: oben die Wahl zwischen Suche und Register (Einlesen, Vorschauen, OCR).
 * Beide Bereiche bleiben geladen, damit Suche und Auswahl beim Wechsel erhalten bleiben.
 */
export default function App() {
  const [bereich, setBereich] = useState<Bereich>(() => lese("app.bereich", "suche"));
  useEffect(() => schreibe("app.bereich", bereich), [bereich]);

  return (
    <div className="rahmen">
      <nav className="reiter">
        <img src={logo} alt="" width={22} height={22} />
        <span className="reiter__name">Registerwerk</span>
        <span className="umschalter umschalter--text">
          <button className={bereich === "suche" ? "aktiv" : ""} onClick={() => setBereich("suche")}>
            Suche
          </button>
          <button className={bereich === "register" ? "aktiv" : ""} onClick={() => setBereich("register")}>
            Register
          </button>
        </span>
      </nav>
      <div className="rahmen__inhalt">
        <Suche sichtbar={bereich === "suche"} />
        <div className="rahmen__seite" hidden={bereich !== "register"}>
          <Verwaltung />
        </div>
      </div>
    </div>
  );
}
