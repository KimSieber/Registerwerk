import { useCallback, useEffect, useRef } from "react";
import { Virtuoso, type VirtuosoHandle } from "react-virtuoso";
import type { Treffer } from "../api";
import { datumDe, ordnerVon, titelVon, vorschauSeite } from "./anzeige";
import { useTastatur } from "./useTastatur";
import Vorschaubild from "./Vorschaubild";

interface Props {
  treffer: Treffer[];
  auswahl: number | null;
  onAuswahl: (datei: number) => void;
  onGross: () => void;
}

/** Listenansicht, virtualisiert, mit kleinem Vorschaubild je Zeile. */
export default function Liste({ treffer, auswahl, onAuswahl, onGross }: Props) {
  const liste = useRef<VirtuosoHandle>(null);
  const scrollEl = useRef<HTMLElement | null>(null);

  const index = auswahl === null ? -1 : treffer.findIndex((t) => t.datei === auswahl);
  const waehle = useCallback((i: number) => onAuswahl(treffer[i].datei), [treffer, onAuswahl]);
  const scroller = useCallback(() => scrollEl.current, []);
  useTastatur({ anzahl: treffer.length, index, waehle, spalten: 1, scroller });

  useEffect(() => {
    if (index >= 0) liste.current?.scrollIntoView({ index, behavior: "auto" });
  }, [index]);

  return (
    <Virtuoso
      ref={liste}
      className="liste-scroller"
      scrollerRef={(el) => (scrollEl.current = el as HTMLElement | null)}
      data={treffer}
      overscan={300}
      computeItemKey={(_, t) => t.datei}
      itemContent={(_, t) => (
        <div
          className={`zeile ${t.datei === auswahl ? "zeile--aktiv" : ""}`}
          onClick={() => onAuswahl(t.datei)}
          onDoubleClick={onGross}
        >
          <span className="zeile__vorschau">
            <Vorschaubild datei={t.datei} seite={vorschauSeite(t)} endung={t.endung} />
          </span>
          <span className="zeile__titel" title={t.name}>
            {titelVon(t.name)}
          </span>
          <span className="zeile__ablage leise" title={t.pfad}>
            {ordnerVon(t.pfad)}
          </span>
          <span className="zeile__datum">{datumDe(t.datum)}</span>
          <span className="zeile__typ">{t.endung.toUpperCase()}</span>
        </div>
      )}
    />
  );
}
