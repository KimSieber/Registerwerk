// Ansicht zum Prüfen der Vorschauen und der Texte: Datei über Teile des Pfads
// wählen, Seiten durchblättern, eingebetteten und erkannten Text vergleichen.
// Die eigentliche Suche folgt in Stufe 2.
import { useEffect, useState } from "react";
import { DateiVorschau, Fall, SeitenText, dateienMitVorschau, seitentexte, vorschauUrl } from "./api";
import { alsZahl } from "./format";

const FALL_TEXT: Record<Fall, string> = {
  ohne_text: "ohne Text",
  scan_schwach: "Scan, schwacher Text",
  digital_schwach: "digital, schwacher Text",
};

const prozent = (g: number | null) => (g === null ? "–" : `${Math.round(g * 100)} %`);

export default function VorschauPruefen({ neuLaden }: { neuLaden: number }) {
  const [muster, setMuster] = useState("");
  const [nurOcr, setNurOcr] = useState(false);
  const [textZeigen, setTextZeigen] = useState(false);
  const [liste, setListe] = useState<DateiVorschau[]>([]);
  const [gewaehlt, setGewaehlt] = useState<DateiVorschau | null>(null);
  const [texte, setTexte] = useState<SeitenText[]>([]);
  const [gross, setGross] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);

  useEffect(() => {
    let gueltig = true;
    const warten = setTimeout(() => {
      dateienMitVorschau(muster, nurOcr)
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
  }, [muster, nurOcr, neuLaden]);

  const dateiId = gewaehlt?.id;
  useEffect(() => {
    let gueltig = true;
    setTexte([]);
    if (dateiId === undefined || !textZeigen) return;
    seitentexte(dateiId)
      .then((t) => gueltig && setTexte(t))
      .catch((e) => gueltig && setFehler(String(e)));
    return () => {
      gueltig = false;
    };
  }, [dateiId, textZeigen, neuLaden]);

  return (
    <section className="karte">
      <div>
        <h2>Vorschauen und Texte prüfen</h2>
        <p className="unterzeile">
          Datei über Teile des Pfads wählen, mehrere Wörter grenzen ein. Ein Klick auf eine Seite schaltet zwischen
          angepasster Breite und Originalgröße der Vorschau um.
        </p>
      </div>
      <div className="filter">
        <input
          className="eingabe"
          type="search"
          placeholder="z. B. 2024 Rechnung"
          value={muster}
          onChange={(e) => setMuster(e.target.value)}
        />
        <label className="schalter">
          <input type="checkbox" checked={nurOcr} onChange={(e) => setNurOcr(e.target.checked)} />
          nur mit Texterkennung
        </label>
        <label className="schalter">
          <input type="checkbox" checked={textZeigen} onChange={(e) => setTextZeigen(e.target.checked)} />
          Text zeigen
        </label>
      </div>
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
              {gewaehlt.seiten.map((s) => {
                const t = texte.find((x) => x.nr === s.nr);
                return (
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
                      {t?.fall && <> · {FALL_TEXT[t.fall]}</>}
                    </figcaption>
                    {textZeigen && t && <TextVergleich t={t} />}
                  </figure>
                );
              })}
            </>
          )}
        </div>
      </div>
    </section>
  );
}

function TextVergleich({ t }: { t: SeitenText }) {
  const pdfGilt = !t.uebernommen;
  return (
    <div className={t.ocr_text === null ? "texte einzeln" : "texte"}>
      <div className={pdfGilt ? "text gilt" : "text"}>
        <div className="text-kopf">
          <b>Eingebettet</b>
          <span>
            Güte {prozent(t.pdf_guete)} · {alsZahl(t.pdf_zeichen)} Zeichen
          </span>
          {pdfGilt && t.ocr_text !== null && <span className="marke">gilt</span>}
        </div>
        <pre>{t.pdf_text.trim() || "(kein Text)"}</pre>
      </div>
      {t.ocr_text !== null && (
        <div className={t.uebernommen ? "text gilt" : "text"}>
          <div className="text-kopf">
            <b>Erkannt</b>
            <span>
              Güte {prozent(t.ocr_guete)} · {alsZahl(t.ocr_zeichen ?? 0)} Zeichen · Konfidenz {t.ocr_konfidenz} ·{" "}
              {alsZahl(t.ocr_ms ?? 0)} ms
            </span>
            {t.uebernommen && <span className="marke">gilt</span>}
          </div>
          <pre>{t.ocr_text || "(kein Text)"}</pre>
        </div>
      )}
    </div>
  );
}
