import { useEffect, useMemo, useRef, useState } from "react";
import type { Zaehler } from "../api";
import { alsZahl } from "../format";

interface Props {
  /** alle Ordner mit Anzahl, nach Pfad sortiert (aus dem Kern) */
  ordner: Zaehler[];
  wert: string;
  onChange: (ordner: string) => void;
}

interface Knoten {
  pfad: string;
  name: string;
  anzahl: number;
  kinder: Knoten[];
}

/** Baut den Baum aus der sortierten Liste. Oberste Ebene: die kürzesten Pfade. */
function baum(liste: Zaehler[]): Knoten[] {
  const knoten = new Map<string, Knoten>();
  const wurzeln: Knoten[] = [];
  for (const z of liste) {
    const k: Knoten = { pfad: z.wert, name: z.wert.split("/").pop() ?? z.wert, anzahl: z.anzahl, kinder: [] };
    knoten.set(z.wert, k);
    const i = z.wert.lastIndexOf("/");
    const eltern = i > 0 ? knoten.get(z.wert.slice(0, i)) : undefined;
    if (eltern) eltern.kinder.push(k);
    else wurzeln.push(k);
  }
  return wurzeln;
}

/**
 * Ordnerwahl als aufklappbarer Baum: zuerst nur die oberste Ebene, Unterordner über den Pfeil.
 * Ein Klick auf den Namen wählt den Ordner samt Unterordnern.
 */
export default function OrdnerWahl({ ordner, wert, onChange }: Props) {
  const [offen, setOffen] = useState(false);
  const [auf, setAuf] = useState<Set<string>>(new Set());
  const bereich = useRef<HTMLDivElement>(null);
  const wurzeln = useMemo(() => baum(ordner), [ordner]);
  const oben = wurzeln.length ? wurzeln[0].pfad.split("/").length - 1 : 0;

  // Beim Öffnen den Weg zum gewählten Ordner aufklappen
  useEffect(() => {
    if (!offen || !wert) return;
    const teile = wert.split("/");
    setAuf((a) => {
      const neu = new Set(a);
      for (let i = oben + 1; i < teile.length; i++) neu.add(teile.slice(0, i).join("/"));
      return neu;
    });
  }, [offen, wert, oben]);

  // Esc und Klick außerhalb schließen
  useEffect(() => {
    if (!offen) return;
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOffen(false);
      }
    };
    const maus = (e: MouseEvent) => !bereich.current?.contains(e.target as Node) && setOffen(false);
    window.addEventListener("keydown", taste, true);
    window.addEventListener("mousedown", maus);
    return () => {
      window.removeEventListener("keydown", taste, true);
      window.removeEventListener("mousedown", maus);
    };
  }, [offen]);

  const umschalten = (pfad: string) =>
    setAuf((a) => {
      const neu = new Set(a);
      if (neu.has(pfad)) neu.delete(pfad);
      else neu.add(pfad);
      return neu;
    });
  const waehlen = (pfad: string) => {
    onChange(pfad);
    setOffen(false);
  };

  const anzeige = wert ? wert.split("/").slice(oben).join(" › ") : "alle Ordner";

  const zeilen = (liste: Knoten[], tiefe: number): JSX.Element[] =>
    liste.flatMap((k) => [
      <div key={k.pfad} className={`ordner__zeile ${k.pfad === wert ? "ordner__zeile--aktiv" : ""}`} style={{ paddingLeft: 6 + tiefe * 16 }}>
        {k.kinder.length > 0 ? (
          <button
            className={`ordner__pfeil ${auf.has(k.pfad) ? "ordner__pfeil--auf" : ""}`}
            onClick={() => umschalten(k.pfad)}
            aria-label={auf.has(k.pfad) ? "Zuklappen" : "Aufklappen"}
          >
            <svg viewBox="0 0 24 24" aria-hidden>
              <path d="M9 6l6 6-6 6" />
            </svg>
          </button>
        ) : (
          <span className="ordner__pfeil" />
        )}
        <button className="ordner__name" onClick={() => waehlen(k.pfad)} title={k.pfad}>
          {k.name}
        </button>
        <span className="ordner__anzahl">{alsZahl(k.anzahl)}</span>
      </div>,
      ...(auf.has(k.pfad) ? zeilen(k.kinder, tiefe + 1) : []),
    ]);

  return (
    <div className="ordner" ref={bereich}>
      <button className={`ordner__feld ${offen ? "ordner__feld--offen" : ""}`} onClick={() => setOffen(!offen)} title={wert || undefined}>
        <span className="ordner__text">{anzeige}</span>
        <svg viewBox="0 0 24 24" aria-hidden>
          <path d="M7 10l5 5 5-5" />
        </svg>
      </button>
      {offen && (
        <div className="ordner__liste" role="tree">
          <div className={`ordner__zeile ${!wert ? "ordner__zeile--aktiv" : ""}`} style={{ paddingLeft: 6 }}>
            <span className="ordner__pfeil" />
            <button className="ordner__name" onClick={() => waehlen("")}>
              alle Ordner
            </button>
          </div>
          {zeilen(wurzeln, 0)}
        </div>
      )}
    </div>
  );
}
