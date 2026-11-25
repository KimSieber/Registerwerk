import { useEffect, useId, useSyncExternalStore } from "react";

/**
 * Hintergrundtätigkeiten am Register (v0.1.1, Kim: Transparenz). Jede Stelle, die das Register
 * prüft, liest oder schreibt, meldet sich mit `useTaetigkeit`. Der Rahmen zeigt dann am Reiter
 * „Register“ einen Kreisel, der Tooltip nennt, was läuft.
 *
 * Bewusst ein Speicher auf Modulebene statt eines Ereignisses: Die Prüfung beim Start meldet sich,
 * bevor der Rahmen seinen Empfänger angemeldet hätte.
 */
const laufend = new Map<string, string>();
const hoerer = new Set<() => void>();
let stand: string[] = [];

function verteilen() {
  stand = [...laufend.values()];
  hoerer.forEach((h) => h());
}

/** Meldet eine Tätigkeit, solange `text` gesetzt ist. */
export function useTaetigkeit(text: string | null) {
  const id = useId();
  useEffect(() => {
    if (!text) return;
    laufend.set(id, text);
    verteilen();
    return () => {
      laufend.delete(id);
      verteilen();
    };
  }, [id, text]);
}

/** Alle laufenden Tätigkeiten. */
export function useTaetigkeiten(): string[] {
  return useSyncExternalStore(
    (h) => {
      hoerer.add(h);
      return () => hoerer.delete(h);
    },
    () => stand,
  );
}
