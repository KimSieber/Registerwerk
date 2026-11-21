import type { RegelZeile, Stufe } from "../api";

/** Regeln je Ordner im Entwurf. Schlüssel: Pfad relativ zum Cloud-Ordner. */
export type Regeln = ReadonlyMap<string, Stufe>;

export const NAMEN: Record<Stufe, string> = {
  voll: "Voll",
  metadaten: "Nur Namen",
  ausgeschlossen: "Aus",
};

export const ERKLAERUNG: Record<Stufe, string> = {
  voll: "Dokumente und Bilder mit Text, Vorschau und OCR. Quelltext und Programme werden ignoriert.",
  metadaten: "Alle Dateien nur mit Name, Ordner und Datum. Kein Inhalt, keine Vorschau.",
  ausgeschlossen: "Nichts aus diesem Ordner kommt ins Register.",
};

export const ausZeilen = (zeilen: RegelZeile[]): Map<string, Stufe> => new Map(zeilen.map((z) => [z.ordner, z.stufe]));

export const alsZeilen = (regeln: Regeln): RegelZeile[] =>
  [...regeln].map(([ordner, stufe]) => ({ ordner, stufe })).sort((a, b) => a.ordner.localeCompare(b.ordner));

const eltern = (pfad: string) => (pfad.includes("/") ? pfad.slice(0, pfad.lastIndexOf("/")) : "");

/** Die Regel, die ein Ordner von seinen Oberordnern erbt, mit dem Ordner, aus dem sie stammt. */
export function geerbt(regeln: Regeln, pfad: string): { stufe: Stufe; von: string } | null {
  let p = pfad;
  while (p !== "") {
    p = eltern(p);
    const s = regeln.get(p);
    if (s) return { stufe: s, von: p };
  }
  return null;
}

/** Was für einen Ordner gilt. Ohne Regel wird er nicht gelesen. */
export function wirksam(regeln: Regeln, pfad: string): { stufe: Stufe; eigene: boolean; von: string | null } {
  const eigene = regeln.get(pfad);
  if (eigene) return { stufe: eigene, eigene: true, von: pfad };
  const g = geerbt(regeln, pfad);
  return { stufe: g?.stufe ?? "ausgeschlossen", eigene: false, von: g?.von ?? null };
}

/**
 * Setzt die Stufe eines Ordners. Eine Regel, die nur wiederholt, was ohnehin gilt
 * (gleich der geerbten, oder „Aus“ ohne geerbte Regel), wird nicht angelegt.
 */
export function setzen(regeln: Regeln, pfad: string, stufe: Stufe): Map<string, Stufe> {
  const neu = new Map(regeln);
  const g = geerbt(regeln, pfad)?.stufe ?? "ausgeschlossen";
  if (stufe === g) neu.delete(pfad);
  else neu.set(pfad, stufe);
  return neu;
}

/** Gibt es unterhalb dieses Ordners eigene Regeln? */
export function hatRegelnUnter(regeln: Regeln, pfad: string): boolean {
  for (const p of regeln.keys()) if (p.startsWith(pfad + "/")) return true;
  return false;
}

export function gleich(a: Regeln, b: Regeln): boolean {
  if (a.size !== b.size) return false;
  for (const [p, s] of a) if (b.get(p) !== s) return false;
  return true;
}

/** Alle Oberordner eines Pfads, vom obersten an. */
export function oberordner(pfad: string): string[] {
  const teile = pfad.split("/");
  return teile.slice(0, -1).map((_, i) => teile.slice(0, i + 1).join("/"));
}
