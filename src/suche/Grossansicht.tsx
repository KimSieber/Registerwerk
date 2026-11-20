import { useEffect, useRef } from "react";
import type { Treffer } from "../api";
import { alsZahl } from "../format";
import { titelVon } from "./anzeige";
import Seitenbild from "./Seitenbild";

interface Props {
  treffer: Treffer;
  seite: number | null;
  seiten: number[];
  /** Die Seiten haben Vorschauen (nicht bei Word, Excel) */
  mitVorschau: boolean;
  /** Position in der Trefferliste (1-basiert) und Anzahl */
  position: [number, number];
  /** −1 / +1: voriges / nächstes Dokument */
  onDokument: (schritt: number) => void;
  onSeite: (seite: number) => void;
  onSchliessen: () => void;
}

/**
 * Großansicht über dem ganzen Fenster, wie in Bilderwerk.
 * Tasten: ←/→ Seiten, ↑/↓ Dokumente, Esc oder Leertaste schließen. Sie gehen nur an die Großansicht.
 */
export default function Grossansicht({ treffer, seite, seiten, mitVorschau, position, onDokument, onSeite, onSchliessen }: Props) {
  const i = seite === null ? -1 : seiten.indexOf(seite);
  const aktionen = useRef({ onDokument, onSeite, onSchliessen, i, seiten });
  aktionen.current = { onDokument, onSeite, onSchliessen, i, seiten };

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const a = aktionen.current;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "ArrowLeft") {
        if (a.i > 0) a.onSeite(a.seiten[a.i - 1]);
      } else if (e.key === "ArrowRight") {
        if (a.i >= 0 && a.i < a.seiten.length - 1) a.onSeite(a.seiten[a.i + 1]);
      } else if (e.key === "ArrowUp") a.onDokument(-1);
      else if (e.key === "ArrowDown") a.onDokument(1);
      else if (e.key === "Escape" || e.key === " ") a.onSchliessen();
      else return;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, []);

  return (
    <div className="grossansicht" role="dialog" aria-label="Großansicht" onClick={onSchliessen}>
      <div className="grossansicht__kopf" onClick={(e) => e.stopPropagation()}>
        <span className="grossansicht__titel">{titelVon(treffer.name)}</span>
        {seiten.length > 0 && (
          <span className="grossansicht__position">
            Seite {seite} von {seiten.length}
          </span>
        )}
        <span className="grossansicht__position">
          Dokument {alsZahl(position[0])} / {alsZahl(position[1])}
        </span>
        <button className="grossansicht__zu" onClick={onSchliessen} aria-label="Schließen" title="Schließen (Esc)">
          <svg viewBox="0 0 24 24" aria-hidden>
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>
      <div className="grossansicht__bild">
        <Seitenbild datei={treffer.datei} seite={mitVorschau ? seite : null} endung={treffer.endung} tasten dunkel />
      </div>
      <div className="grossansicht__fuss">
        <span>{treffer.pfad}</span>
        <span>← → Seiten · ↑ ↓ Dokumente · + − 0 Größe · Esc schließen</span>
      </div>
    </div>
  );
}
