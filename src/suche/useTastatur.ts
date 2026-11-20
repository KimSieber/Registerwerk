import { useEffect } from "react";

interface Optionen {
  anzahl: number;
  index: number; // -1 = keine Auswahl
  waehle: (index: number) => void;
  /** Spalten im Raster (Liste: 1), ↑/↓ springen eine Zeile */
  spalten: number;
  /** Scroll-Element für Bild↑/↓, Pos1/Ende */
  scroller: () => HTMLElement | null;
}

/**
 * Tastatur für Liste und Raster (wie Bilderwerk):
 * ↑/↓ Auswahl um eine Zeile · ←/→ um ein Dokument (nur Raster) · Bild↑/↓ blättern · Pos1/Ende.
 * ↑/↓ funktionieren auch aus dem Suchfeld heraus, danach verlässt der Fokus das Feld.
 */
export function useTastatur({ anzahl, index, waehle, spalten, scroller }: Optionen) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || anzahl === 0 || e.defaultPrevented) return;
      const a = document.activeElement;
      if (a instanceof HTMLSelectElement || a instanceof HTMLTextAreaElement) return;
      const imFeld = a instanceof HTMLInputElement;
      const setze = (i: number) => waehle(Math.max(0, Math.min(anzahl - 1, i)));
      const el = scroller();

      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          if (imFeld) (a as HTMLElement).blur();
          setze(index < 0 ? 0 : index + spalten);
          break;
        case "ArrowUp":
          e.preventDefault();
          if (imFeld) (a as HTMLElement).blur();
          setze(index < 0 ? 0 : index - spalten);
          break;
        case "ArrowRight":
          if (imFeld || spalten === 1) return;
          e.preventDefault();
          setze(index + 1);
          break;
        case "ArrowLeft":
          if (imFeld || spalten === 1) return;
          e.preventDefault();
          setze(index < 0 ? 0 : index - 1);
          break;
        case "PageDown":
        case "PageUp":
          if (!el) return;
          e.preventDefault();
          el.scrollBy({ top: (e.key === "PageDown" ? 1 : -1) * el.clientHeight * 0.9, behavior: "smooth" });
          break;
        case "Home":
        case "End":
          if (imFeld || !el) return;
          e.preventDefault();
          el.scrollTo({ top: e.key === "Home" ? 0 : el.scrollHeight, behavior: "smooth" });
          break;
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [anzahl, index, waehle, spalten, scroller]);
}
