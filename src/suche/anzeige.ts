import type { DateiDetails, Treffer } from "../api";

/** Dokumentdatum in deutscher Schreibweise, so genau wie gespeichert. */
export function datumDe(d: string | null): string {
  if (!d) return "";
  const [j, m, t] = d.split("-");
  if (t) return `${t}.${m}.${j}`;
  if (m) return `${m}/${j}`;
  return j;
}

/** Ordner eines Pfads im Register (ohne Dateinamen). */
export function ordnerVon(pfad: string): string {
  const i = pfad.lastIndexOf("/");
  return i < 0 ? "" : pfad.slice(0, i);
}

/** Seite für das Vorschaubild eines Treffers: die beste Trefferseite, sonst Seite 1 (PDFs und Bilder). */
export function vorschauSeite(t: Treffer): number | null {
  if (BILDER.includes(t.endung)) return 1;
  if (t.endung !== "pdf" || t.seiten === 0) return null;
  return t.beste_seite ?? 1;
}

/** Bildformate mit Vorschau (wie formate.rs im Kern) */
export const BILDER = ["jpg", "jpeg", "png", "tif", "tiff", "bmp", "gif", "webp", "heic", "heif"];

/** Seiten zum Blättern: die mit Vorschau, sonst alle Seiten mit Text (Word, Excel je Blatt). */
export function seitenVon(d: DateiDetails | null): number[] {
  if (!d) return [];
  if (d.vorschau.length) return d.vorschau.map((s) => s.nr);
  return Array.from({ length: d.seiten ?? 0 }, (_, i) => i + 1);
}

/** Name ohne Endung, für Titel. */
export function titelVon(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(0, i).trim() : name;
}
