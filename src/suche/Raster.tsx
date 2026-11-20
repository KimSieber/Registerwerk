import { useCallback, useEffect, useRef, useState } from "react";
import { VirtuosoGrid, type VirtuosoGridHandle } from "react-virtuoso";
import type { Treffer } from "../api";
import { datumDe, titelVon, vorschauSeite } from "./anzeige";
import { useTastatur } from "./useTastatur";
import Vorschaubild from "./Vorschaubild";

interface Props {
  treffer: Treffer[];
  auswahl: number | null;
  onAuswahl: (datei: number) => void;
  onGross: () => void;
  /** Mindestbreite einer Kachel in px */
  kachel: number;
}

const ABSTAND = 14;
const RAND = 6;

export default function Raster({ treffer, auswahl, onAuswahl, onGross, kachel }: Props) {
  const raster = useRef<VirtuosoGridHandle>(null);
  const scrollEl = useRef<HTMLElement | null>(null);
  const bereich = useRef({ start: 0, ende: 0 });
  const [breite, setBreite] = useState(0);

  // Spaltenzahl wie im CSS: repeat(auto-fill, minmax(kachel, 1fr))
  const spalten = Math.max(1, Math.floor((breite - 2 * RAND + ABSTAND) / (kachel + ABSTAND)));

  useEffect(() => {
    const el = scrollEl.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBreite(el.clientWidth));
    ro.observe(el);
    setBreite(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const index = auswahl === null ? -1 : treffer.findIndex((t) => t.datei === auswahl);
  const waehle = useCallback((i: number) => onAuswahl(treffer[i].datei), [treffer, onAuswahl]);
  const scroller = useCallback(() => scrollEl.current, []);
  useTastatur({ anzahl: treffer.length, index, waehle, spalten, scroller });

  // Auswahl sichtbar halten, nur scrollen, wenn sie außerhalb liegt
  useEffect(() => {
    if (index < 0) return;
    const { start, ende } = bereich.current;
    if (index < start + spalten || index > ende - spalten) {
      raster.current?.scrollToIndex({ index, align: index < start + spalten ? "start" : "end" });
    }
  }, [index, spalten]);

  return (
    <VirtuosoGrid
      ref={raster}
      className="raster-scroller"
      scrollerRef={(el) => {
        scrollEl.current = el as HTMLElement | null;
        if (el) setBreite((el as HTMLElement).clientWidth);
      }}
      style={{ ["--kachel" as string]: `${kachel}px` }}
      totalCount={treffer.length}
      overscan={400}
      listClassName="raster"
      computeItemKey={(i) => treffer[i].datei}
      rangeChanged={(r) => (bereich.current = { start: r.startIndex, ende: r.endIndex })}
      itemContent={(i) => {
        const t = treffer[i];
        return (
          <div
            className={`kachel ${t.datei === auswahl ? "kachel--aktiv" : ""}`}
            onClick={() => onAuswahl(t.datei)}
            onDoubleClick={onGross}
            title={t.pfad}
          >
            <div className="kachel__bild">
              <Vorschaubild datei={t.datei} seite={vorschauSeite(t)} endung={t.endung} />
            </div>
            <div className="kachel__titel">{titelVon(t.name)}</div>
            <div className="kachel__datum">
              {datumDe(t.datum)}
              {t.seiten ? ` · ${t.seiten} S.` : ""}
            </div>
          </div>
        );
      }}
    />
  );
}
