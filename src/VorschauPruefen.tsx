// Ansicht zum Prüfen der Vorschauen: Datei über Teile des Pfads wählen, Seiten
// durchblättern. Die eigentliche Suche folgt in Stufe 2.
import { useEffect, useState } from "react";
import { DateiVorschau, dateienMitVorschau, vorschauUrl } from "./api";
import { alsZahl } from "./format";

export default function VorschauPruefen({ neuLaden }: { neuLaden: number }) {
  const [muster, setMuster] = useState("");
  const [liste, setListe] = useState<DateiVorschau[]>([]);
  const [gewaehlt, setGewaehlt] = useState<DateiVorschau | null>(null);
  const [gross, setGross] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);

  useEffect(() => {
    let gueltig = true;
    const warten = setTimeout(() => {
      dateienMitVorschau(muster)
        .then((l) => {
          if (!gueltig) return;
          setListe(l);
          setFehler(null);
          setGewaehlt((g) => l.find((d) => d.id === g?.id) ?? l[0] ?? null);
        })
        .catch((e) => gueltig && setFehler(String(e)));
    }, 200);
    return () => {
      gueltig = false;
      clearTimeout(warten);
    };
  }, [muster, neuLaden]);

  return (
    <section className="karte">
      <div>
        <h2>Vorschauen prüfen</h2>
        <p className="unterzeile">
          Datei über Teile des Pfads wählen, mehrere Wörter grenzen ein. Ein Klick auf eine Seite schaltet zwischen
          angepasster Breite und Originalgröße der Vorschau um.
        </p>
      </div>
      <input
        className="eingabe"
        type="search"
        placeholder="z. B. 2024 Rechnung"
        value={muster}
        onChange={(e) => setMuster(e.target.value)}
      />
      {fehler && <p className="achtung">{fehler}</p>}
      <div className="pruefen">
        <ul className="dateiliste">
          {liste.map((d) => (
            <li key={d.id}>
              <button className={d.id === gewaehlt?.id ? "eintrag aktiv" : "eintrag"} onClick={() => setGewaehlt(d)}>
                <span className="eintrag-name">{d.pfad.split("/").pop()}</span>
                <span className="eintrag-zusatz">
                  {d.datum ?? "ohne Datum"} · {alsZahl(d.seiten.length)} {d.seiten.length === 1 ? "Seite" : "Seiten"}
                </span>
              </button>
            </li>
          ))}
          {liste.length === 0 && <li className="stand-zeile">Keine Datei gefunden.</li>}
          {liste.length === 100 && <li className="stand-zeile">Die ersten 100 Treffer.</li>}
        </ul>
        <div className="blaettern">
          {gewaehlt && (
            <>
              <p className="pfad">{gewaehlt.pfad}</p>
              {gewaehlt.seiten.map((s) => (
                <figure key={s.nr} className="vorschau-seite">
                  <img
                    src={vorschauUrl(gewaehlt.id, s.nr)}
                    width={s.breite}
                    height={s.hoehe}
                    loading="lazy"
                    alt={`Seite ${s.nr}`}
                    className={gross ? "gross" : undefined}
                    onClick={() => setGross((g) => !g)}
                  />
                  <figcaption>
                    Seite {s.nr} von {gewaehlt.seiten.length} · {s.breite} × {s.hoehe} ·{" "}
                    {s.farbig ? "farbig" : "Graustufen"}
                  </figcaption>
                </figure>
              ))}
            </>
          )}
        </div>
      </div>
    </section>
  );
}
