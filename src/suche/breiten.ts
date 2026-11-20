// Breite des Detailbereichs: per Schieber änderbar, gemerkt, immer in sinnvollen Grenzen (wie Bilderwerk).

export const DETAIL_MIN = 300;
export const DETAIL_STANDARD = 420;
/** Mindestbreite der Liste, damit Name, Ordner und Datum lesbar bleiben */
export const LISTE_MIN = 460;
/** Kopfzeile (Regler, Raster/Liste, Details) muss auch bei kleinster Kachel ganz passen */
export const KOPF_MIN = 250;
/** Breite des Schiebers zwischen Ergebnis und Details */
export const TEILER = 14;

export type Ansicht = "raster" | "liste";

/** Platz, den das Ergebnis mindestens braucht: eine Kachel in der aktuellen Größe bzw. die Liste */
export function ergebnisMin(ansicht: Ansicht, kachel: number): number {
  return ansicht === "liste" ? LISTE_MIN : Math.max(KOPF_MIN, kachel + 2 * 6 + 16);
}

/** Gewünschte Breite auf [DETAIL_MIN, gesamt − Teiler − Ergebnis-Minimum] begrenzen */
export function begrenzen(wunsch: number, gesamt: number, ergebnis: number): number {
  const max = Math.max(DETAIL_MIN, gesamt - TEILER - ergebnis);
  return Math.round(Math.min(max, Math.max(DETAIL_MIN, wunsch)));
}

/** Kleine Vorlieben der Ansicht, Zugriff abgesichert. */
export function lese<T>(schluessel: string, standard: T): T {
  try {
    const v = localStorage.getItem(schluessel);
    return v === null ? standard : (JSON.parse(v) as T);
  } catch {
    return standard;
  }
}

export function schreibe(schluessel: string, wert: unknown) {
  try {
    localStorage.setItem(schluessel, JSON.stringify(wert));
  } catch {
    /* egal */
  }
}
