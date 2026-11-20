import { useEffect, useState } from "react";
import { vorschauUrl } from "../api";

interface Props {
  datei: number;
  /** Seite ab 1. `null`: keine Vorschau, nur das Symbol */
  seite: number | null;
  /** Dateiendung für das Symbol, wenn es keine Vorschau gibt */
  endung: string;
}

/** Vorschau einer Seite mit weichem Einblenden. Ohne Vorschau ein Symbol mit dem Dateityp. */
export default function Vorschaubild({ datei, seite, endung }: Props) {
  const [geladen, setGeladen] = useState(false);
  const [fehlt, setFehlt] = useState(false);
  useEffect(() => {
    setGeladen(false);
    setFehlt(false);
  }, [datei, seite]);

  if (seite === null || fehlt) {
    return (
      <div className="vorschau__symbol" title="Keine Vorschau">
        <svg viewBox="0 0 24 24" aria-hidden>
          <path d="M6 3h8l4 4v14H6z" />
          <path d="M14 3v4h4" />
        </svg>
        <span className="vorschau__typ">{endung.toUpperCase() || "?"}</span>
      </div>
    );
  }
  return (
    <img
      className={`vorschau__bild ${geladen ? "geladen" : ""}`}
      src={vorschauUrl(datei, seite)}
      alt=""
      draggable={false}
      decoding="async"
      onLoad={() => setGeladen(true)}
      onError={() => setFehlt(true)}
    />
  );
}
