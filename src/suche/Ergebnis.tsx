import type { SuchErgebnis } from "../api";
import { alsZahl } from "../format";
import Liste from "./Liste";
import Raster from "./Raster";
import type { Ansicht } from "./breiten";

interface Props {
  ergebnis: SuchErgebnis;
  auswahl: number | null;
  onAuswahl: (datei: number) => void;
  onGross: () => void;
  ansicht: Ansicht;
  onAnsicht: (a: Ansicht) => void;
  kachel: number;
  onKachel: (px: number) => void;
  details: boolean;
  onDetails: (offen: boolean) => void;
  /** Reihenfolge: neueste zuerst statt nach Relevanz */
  nachDatum: boolean;
  onNachDatum: (neueste: boolean) => void;
}

/** Kopfzeile (Anzahl, Hinweise, Ansicht, Kachelgröße, Details) und Raster oder Liste. */
export default function Ergebnis(p: Props) {
  const { gesamt, treffer, dauer_ms, aehnliche, uebergangen } = p.ergebnis;
  const hinweise = [
    ...aehnliche.map(([w, v]) => `„${w}“ auch als ${v.slice(0, 3).join(", ")}${v.length > 3 ? " …" : ""}`),
    ...(uebergangen.length ? [`übergangen (zu kurz): ${uebergangen.join(", ")}`] : []),
  ];
  return (
    <section className="ergebnis">
      <div className="ergebnis__kopf">
        <span>
          {alsZahl(gesamt)} {gesamt === 1 ? "Dokument" : "Dokumente"}
          {treffer.length < gesamt && <> · die ersten {alsZahl(treffer.length)} angezeigt</>}
          <span className="leise"> · {Math.round(dauer_ms)} ms</span>
          {hinweise.length > 0 && <span className="leise"> · {hinweise.join(" · ")}</span>}
        </span>
        <span className="ergebnis__werkzeuge">
          <span className="umschalter umschalter--text umschalter--klein" title="Reihenfolge">
            <button className={!p.nachDatum ? "aktiv" : ""} onClick={() => p.onNachDatum(false)}>
              Relevanz
            </button>
            <button className={p.nachDatum ? "aktiv" : ""} onClick={() => p.onNachDatum(true)}>
              Neueste
            </button>
          </span>
          {p.ansicht === "raster" && (
            <input
              className="regler"
              type="range"
              min={110}
              max={340}
              step={10}
              value={p.kachel}
              onChange={(e) => p.onKachel(+e.target.value)}
              title="Kachelgröße"
            />
          )}
          <span className="umschalter">
            <button className={p.ansicht === "raster" ? "aktiv" : ""} onClick={() => p.onAnsicht("raster")} title="Raster">
              <svg viewBox="0 0 24 24" aria-hidden>
                <rect x="4" y="4" width="7" height="7" rx="1.5" />
                <rect x="13" y="4" width="7" height="7" rx="1.5" />
                <rect x="4" y="13" width="7" height="7" rx="1.5" />
                <rect x="13" y="13" width="7" height="7" rx="1.5" />
              </svg>
            </button>
            <button className={p.ansicht === "liste" ? "aktiv" : ""} onClick={() => p.onAnsicht("liste")} title="Liste">
              <svg viewBox="0 0 24 24" aria-hidden>
                <path d="M4 6h16M4 12h16M4 18h16" />
              </svg>
            </button>
          </span>
          <button
            className={`umschalter__einzeln ${p.details ? "aktiv" : ""}`}
            onClick={() => p.onDetails(!p.details)}
            title={p.details ? "Details ausblenden" : "Details einblenden"}
            aria-pressed={p.details}
          >
            <svg viewBox="0 0 24 24" aria-hidden>
              <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
              <path d="M14.5 4.5v15" />
            </svg>
          </button>
        </span>
      </div>
      {gesamt === 0 ? (
        <div className="hinweis">Keine Treffer.</div>
      ) : p.ansicht === "raster" ? (
        <Raster treffer={treffer} auswahl={p.auswahl} onAuswahl={p.onAuswahl} onGross={p.onGross} kachel={p.kachel} />
      ) : (
        <Liste treffer={treffer} auswahl={p.auswahl} onAuswahl={p.onAuswahl} onGross={p.onGross} />
      )}
    </section>
  );
}
