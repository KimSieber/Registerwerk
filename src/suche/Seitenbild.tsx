import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { vorschauUrl } from "../api";
import Vorschaubild from "./Vorschaubild";

interface Props {
  datei: number;
  /** Seite ab 1, `null`: keine Vorschau */
  seite: number | null;
  endung: string;
  /** Klick auf das Bild (Detailbereich: Großansicht öffnen). Ohne: Klick vergrößert an dieser Stelle. */
  onKlick?: () => void;
  /** Tasten + − 0 für die Vergrößerung (Großansicht) */
  tasten?: boolean;
  /** dunkle Knöpfe auf dunklem Grund (Großansicht) */
  dunkel?: boolean;
}

const STUFEN = [0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];
const MIN = 0.25;
const MAX = 4;
/** Bewegung in Pixeln, ab der ein Klick als Ziehen gilt */
const ZIEHEN_AB = 4;

/** Punkt im Bild (0 bis 1) und seine Lage im Rahmen, der beim Vergrößern stehen bleibt */
type Anker = { rx: number; ry: number; ox: number; oy: number };

/**
 * Seite mit Vergrößerung: eingepasst oder 25 bis 400 %, bezogen auf die Pixel der Vorschau
 * (100 % = 1024 px Breite bei A4). Stufenlos mit Zwei-Finger-Zoom oder ⌘/Strg + Mausrad, der Punkt
 * unter dem Zeiger bleibt dabei stehen. Knöpfe und Tasten gehen in Stufen. Vergrößert lässt sich die
 * Seite mit Bildlauf oder durch Ziehen verschieben.
 */
export default function Seitenbild({ datei, seite, endung, onKlick, tasten, dunkel }: Props) {
  const [zoom, setZoom] = useState<number | null>(null); // null = eingepasst
  const [mass, setMass] = useState<{ b: number; h: number } | null>(null);
  const [fehlt, setFehlt] = useState(false);
  const rahmen = useRef<HTMLDivElement>(null);
  const zieht = useRef<{ x: number; y: number; l: number; t: number; bewegt: boolean } | null>(null);
  const anker = useRef<Anker | null>(null);
  const geste = useRef<number | null>(null); // Ausgangsgröße während einer Zwei-Finger-Geste
  // Zuletzt gesetzte Größe, auch wenn noch nicht gezeichnet (viele Ereignisse je Bild)
  const gewuenscht = useRef<number | null>(null);

  // Neue Seite: wieder einpassen
  useEffect(() => {
    setZoom(null);
    gewuenscht.current = null;
    setMass(null);
    setFehlt(false);
  }, [datei, seite]);

  /** Maßstab der eingepassten Seite */
  const eingepasst = (): number => {
    const r = rahmen.current;
    if (!r || !mass) return 1;
    return Math.min(r.clientWidth / mass.b, r.clientHeight / mass.h);
  };
  const aktuell = () => gewuenscht.current ?? eingepasst();

  /** Punkt im Bild unter (ox, oy) im Rahmen, für den gezeichneten Zustand */
  const ankerBei = (ox: number, oy: number): Anker | null => {
    const r = rahmen.current;
    if (!r || !mass) return null;
    const z = zoom ?? eingepasst();
    const b = mass.b * z;
    const h = mass.h * z;
    const randX = Math.max(0, (r.clientWidth - b) / 2);
    const randY = Math.max(0, (r.clientHeight - h) / 2);
    return {
      rx: Math.min(1, Math.max(0, (r.scrollLeft + ox - randX) / b)),
      ry: Math.min(1, Math.max(0, (r.scrollTop + oy - randY) / h)),
      ox,
      oy,
    };
  };

  /** Neue Größe setzen, der Punkt bei (ox, oy) bleibt stehen (ohne Angabe: die Mitte) */
  const setzen = (neu: number | null, ox?: number, oy?: number) => {
    const r = rahmen.current;
    if (neu !== null) {
      neu = Math.min(MAX, Math.max(MIN, neu));
      // Kleiner als eingepasst: einpassen
      if (neu <= eingepasst() * 1.001) neu = null;
    }
    if (r) anker.current = ankerBei(ox ?? r.clientWidth / 2, oy ?? r.clientHeight / 2);
    gewuenscht.current = neu;
    setZoom(neu);
  };
  const groesser = () => setzen(STUFEN.find((s) => s > aktuell() * 1.01) ?? MAX);
  const kleiner = () => {
    const z = aktuell();
    setzen([...STUFEN].reverse().find((s) => s < z * 0.99) ?? null);
  };

  // Ausschnitt nach dem Vergrößern so setzen, dass der Ankerpunkt stehen bleibt
  useLayoutEffect(() => {
    const r = rahmen.current;
    const a = anker.current;
    anker.current = null;
    if (!r || !a || zoom === null || !mass) return;
    const b = mass.b * zoom;
    const h = mass.h * zoom;
    const randX = Math.max(0, (r.clientWidth - b) / 2);
    const randY = Math.max(0, (r.clientHeight - h) / 2);
    r.scrollLeft = a.rx * b + randX - a.ox;
    r.scrollTop = a.ry * h + randY - a.oy;
  }, [zoom, mass]);

  // Stufenlos: ⌘/Strg + Mausrad und Zwei-Finger-Zoom (WebKit meldet ihn als Geste
  // oder als Mausrad mit ctrlKey; während einer Geste zählt nur die Geste)
  useEffect(() => {
    const r = rahmen.current;
    if (!r) return;
    const lage = (e: { clientX: number; clientY: number }) => {
      const b = r.getBoundingClientRect();
      return [e.clientX - b.left, e.clientY - b.top] as const;
    };
    const rad = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      if (geste.current !== null) return;
      const schritt = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
      const faktor = Math.exp(-Math.max(-50, Math.min(50, schritt)) * 0.008);
      setzen(aktuell() * faktor, ...lage(e));
    };
    type Geste = Event & { scale: number; clientX: number; clientY: number };
    const anfang = (e: Event) => {
      e.preventDefault();
      geste.current = aktuell();
    };
    const aenderung = (e: Event) => {
      e.preventDefault();
      const g = e as Geste;
      if (geste.current === null) return;
      setzen(geste.current * g.scale, ...lage(g));
    };
    const ende = (e: Event) => {
      e.preventDefault();
      geste.current = null;
    };
    r.addEventListener("wheel", rad, { passive: false });
    r.addEventListener("gesturestart", anfang);
    r.addEventListener("gesturechange", aenderung);
    r.addEventListener("gestureend", ende);
    return () => {
      r.removeEventListener("wheel", rad);
      r.removeEventListener("gesturestart", anfang);
      r.removeEventListener("gesturechange", aenderung);
      r.removeEventListener("gestureend", ende);
    };
  });

  // + − 0 in der Großansicht
  useEffect(() => {
    if (!tasten) return;
    const h = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "+" || e.key === "=") groesser();
      else if (e.key === "-") kleiner();
      else if (e.key === "0") setzen(null);
      else return;
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  });

  if (seite === null || fehlt) {
    return (
      <div className="seitenbild">
        <Vorschaubild datei={datei} seite={null} endung={endung} />
      </div>
    );
  }

  const vergroessert = zoom !== null && mass !== null;
  return (
    <div className={`seitenbild ${dunkel ? "seitenbild--dunkel" : ""}`} onClick={(e) => e.stopPropagation()}>
      <div
        ref={rahmen}
        className={`seitenbild__rahmen ${vergroessert ? "seitenbild__rahmen--zoom" : "seitenbild__rahmen--klick"}`}
        onDoubleClick={() => vergroessert && !onKlick && setzen(null)}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          const r = rahmen.current!;
          zieht.current = { x: e.clientX, y: e.clientY, l: r.scrollLeft, t: r.scrollTop, bewegt: false };
          if (vergroessert) e.currentTarget.setPointerCapture?.(e.pointerId);
        }}
        onPointerMove={(e) => {
          const z = zieht.current;
          if (!z) return;
          if (Math.abs(e.clientX - z.x) + Math.abs(e.clientY - z.y) > ZIEHEN_AB) z.bewegt = true;
          if (!vergroessert || !z.bewegt) return;
          const r = rahmen.current!;
          r.scrollLeft = z.l - (e.clientX - z.x);
          r.scrollTop = z.t - (e.clientY - z.y);
        }}
        onPointerUp={(e) => {
          const z = zieht.current;
          zieht.current = null;
          if (!z || z.bewegt) return;
          // Klick ohne Ziehen
          if (onKlick) onKlick();
          else if (!vergroessert) {
            const b = rahmen.current!.getBoundingClientRect();
            setzen(1, e.clientX - b.left, e.clientY - b.top);
          }
        }}
      >
        <img
          className="seitenbild__bild"
          src={vorschauUrl(datei, seite)}
          alt=""
          draggable={false}
          style={vergroessert ? { width: mass!.b * zoom!, height: mass!.h * zoom! } : undefined}
          onLoad={(e) => setMass({ b: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}
          onError={() => setFehlt(true)}
        />
      </div>
      <div className="seitenbild__zoom">
        <button onClick={kleiner} title="Verkleinern (−)">
          −
        </button>
        <button onClick={() => setzen(null)} title="Einpassen (0)">
          {zoom === null ? "Eingepasst" : `${Math.round(zoom * 100)} %`}
        </button>
        <button onClick={groesser} title="Vergrößern (+)">
          +
        </button>
      </div>
    </div>
  );
}
