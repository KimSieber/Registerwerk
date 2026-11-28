import { useEffect, useRef, useState } from "react";
import type { SucheAngaben } from "../api";
import { alsZahl } from "../format";
import OrdnerWahl from "./OrdnerWahl";

export interface SuchFilter {
  text: string;
  von: string;
  bis: string;
  endungen: string[];
  ordner: string;
  nachDatum: boolean;
}

export const LEERER_FILTER: SuchFilter = { text: "", von: "", bis: "", endungen: [], ordner: "", nachDatum: false };

/** true, wenn weder Suchwort noch Filter gesetzt sind */
export function istLeer(f: SuchFilter) {
  return !f.text.trim() && !f.von.trim() && !f.bis.trim() && f.endungen.length === 0 && !f.ordner;
}

interface Props {
  filter: SuchFilter;
  onChange: (f: SuchFilter) => void;
  /** true, sobald Ergebnisse da sind: Suchfeld wandert nach oben */
  kompakt: boolean;
  angaben: SucheAngaben | null;
}

/** Dateitypen in Gruppen. Was in keine Gruppe passt, fällt unter „Sonstige“. */
export const TYPGRUPPEN: { name: string; endungen: string[] }[] = [
  { name: "PDF", endungen: ["pdf"] },
  { name: "Word", endungen: ["doc", "docx", "docm", "dot", "dotx", "dotm", "rtf", "odt", "pages"] },
  { name: "Excel", endungen: ["xls", "xlsx", "xlsm", "xlsb", "xlt", "xltx", "xltm", "ods", "numbers", "csv"] },
  { name: "PowerPoint", endungen: ["ppt", "pptx", "pptm", "pps", "ppsx", "odp", "key"] },
  { name: "Bilder", endungen: ["jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "gif", "bmp", "webp"] },
  { name: "Text", endungen: ["txt", "md"] },
];

export default function Suchfeld({ filter, onChange, kompakt, angaben }: Props) {
  const [mehr, setMehr] = useState(false);
  /** Gruppe, deren einzelne Endungen aufgeklappt sind */
  const [offen, setOffen] = useState<string | null>(null);
  /** Suche in den einzelnen Endungen einer langen Gruppe (v0.1.5) */
  const [typSuche, setTypSuche] = useState("");
  const eingabe = useRef<HTMLInputElement>(null);

  useEffect(() => eingabe.current?.focus(), []);

  // ⌘F fokussiert das Suchfeld
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        eingabe.current?.focus();
        eingabe.current?.select();
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  const setze = <K extends keyof SuchFilter>(k: K, v: SuchFilter[K]) => onChange({ ...filter, [k]: v });

  // Gruppen mit den Endungen, die im Register vorkommen, und ihrer Anzahl
  const vorhanden = angaben?.endungen ?? [];
  const bekannt = new Set(TYPGRUPPEN.flatMap((g) => g.endungen));
  const gruppen = [
    ...TYPGRUPPEN.map((g) => ({ name: g.name, endungen: g.endungen.filter((e) => vorhanden.some((v) => v.wert === e)) })),
    { name: "Sonstige", endungen: vorhanden.filter((v) => !bekannt.has(v.wert)).map((v) => v.wert) },
  ]
    .map((g) => ({ ...g, anzahl: vorhanden.filter((v) => g.endungen.includes(v.wert)).reduce((n, v) => n + v.anzahl, 0) }))
    .filter((g) => g.anzahl > 0);
  const typAktiv = (endungen: string[]) => endungen.every((e) => filter.endungen.includes(e));
  const typTeil = (endungen: string[]) => !typAktiv(endungen) && endungen.some((e) => filter.endungen.includes(e));
  const anzahlVon = (e: string) => vorhanden.find((v) => v.wert === e)?.anzahl ?? 0;
  const aufgeklappt = gruppen.find((g) => g.name === offen && g.endungen.length > 1);
  // Einzelne Endungen nach Anzahl, häufigste zuerst. Lange Gruppen wie
  // „Sonstige“ bekommen ein Suchfeld (v0.1.5)
  const MIT_SUCHE = 12;
  const einzeln = aufgeklappt
    ? [...aufgeklappt.endungen]
        .sort((a, b) => anzahlVon(b) - anzahlVon(a) || a.localeCompare(b))
        .filter((e) => e.includes(typSuche.trim().toLowerCase()))
    : [];
  const typUmschalten = (endungen: string[]) =>
    setze(
      "endungen",
      typAktiv(endungen) ? filter.endungen.filter((e) => !endungen.includes(e)) : [...filter.endungen, ...endungen],
    );

  const anzahlKriterien =
    [filter.von, filter.bis, filter.ordner].filter((s) => s.trim()).length +
    (filter.endungen.length ? 1 : 0);

  const alles = () => {
    onChange({ ...LEERER_FILTER, nachDatum: filter.nachDatum });
    setMehr(false);
    eingabe.current?.focus();
  };

  const jahr = filter.von && filter.von === filter.bis && /^\d{4}$/.test(filter.von) ? filter.von : "";

  return (
    <section className={`suche ${kompakt ? "suche--kompakt" : ""}`}>
      <div className="suchfeld">
        <svg className="suchfeld__lupe" viewBox="0 0 24 24" aria-hidden>
          <circle cx="11" cy="11" r="7" />
          <path d="M20 20l-3.5-3.5" />
        </svg>
        <input
          ref={eingabe}
          type="text"
          placeholder="Dokumente suchen …"
          value={filter.text}
          spellCheck={false}
          autoCorrect="off"
          onChange={(e) => setze("text", e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && alles()}
        />
        {!istLeer(filter) && (
          <button className="suchfeld__leeren" onClick={alles} title="Suche leeren (Esc)">
            <svg viewBox="0 0 24 24" aria-hidden>
              <path d="M7 7l10 10M17 7L7 17" />
            </svg>
          </button>
        )}
        <button className={`suchfeld__mehr ${mehr ? "aktiv" : ""}`} onClick={() => setMehr(!mehr)} title="Weitere Kriterien">
          Kriterien{anzahlKriterien > 0 && <span className="zaehler">{anzahlKriterien}</span>}
        </button>
      </div>

      <div className={`kriterien ${mehr ? "kriterien--offen" : ""}`}>
        <div className="kriterien__innen">
          <div className="feld feld--ordner">
            <span>Ordner</span>
            <OrdnerWahl ordner={angaben?.ordner ?? []} wert={filter.ordner} onChange={(o) => setze("ordner", o)} />
          </div>
          <label className="feld feld--jahr">
            <span>Jahr</span>
            <select value={jahr} onChange={(e) => onChange({ ...filter, von: e.target.value, bis: e.target.value })}>
              <option value="">–</option>
              {(angaben?.jahre ?? []).map((j) => (
                <option key={j.wert} value={j.wert}>
                  {j.wert} ({alsZahl(j.anzahl)})
                </option>
              ))}
            </select>
          </label>
          <div className="feld feld--zeitraum">
            <span>Datum von – bis</span>
            <div className="zeitraum">
              <input
                value={filter.von}
                placeholder="JJJJ[-MM[-TT]]"
                aria-label="Datum von"
                onChange={(e) => setze("von", e.target.value)}
              />
              <span className="leise">–</span>
              <input
                value={filter.bis}
                placeholder="JJJJ[-MM[-TT]]"
                aria-label="Datum bis"
                onChange={(e) => setze("bis", e.target.value)}
              />
            </div>
          </div>
          <div className="feld feld--dateityp">
            <span>Dateityp</span>
            <div className="chips">
              {gruppen.map((g) => (
                <span key={g.name} className="chipgruppe">
                  <button
                    className={`chip chip--klein ${typAktiv(g.endungen) ? "chip--aktiv" : typTeil(g.endungen) ? "chip--teil" : ""} ${g.endungen.length > 1 ? "chip--links" : ""}`}
                    onClick={() => typUmschalten(g.endungen)}
                    title={g.endungen.map((e) => e.toUpperCase()).join(", ")}
                  >
                    {g.name}
                    <span className="chip__anzahl">{alsZahl(g.anzahl)}</span>
                  </button>
                  {g.endungen.length > 1 && (
                    <button
                      className={`chip chip--klein chip--rechts ${offen === g.name ? "chip--offen" : ""}`}
                      onClick={() => {
                        setOffen(offen === g.name ? null : g.name);
                        setTypSuche("");
                      }}
                      title="Einzelne Dateitypen"
                      aria-expanded={offen === g.name}
                    >
                      <svg viewBox="0 0 24 24" aria-hidden>
                        <path d="M7 10l5 5 5-5" />
                      </svg>
                    </button>
                  )}
                </span>
              ))}
            </div>
            {aufgeklappt && (
              <div className="einzeln">
                {aufgeklappt.endungen.length > MIT_SUCHE && (
                  <input
                    className="einzeln__suche"
                    type="text"
                    value={typSuche}
                    placeholder={`${alsZahl(aufgeklappt.endungen.length)} Dateitypen durchsuchen …`}
                    spellCheck={false}
                    autoCorrect="off"
                    onChange={(e) => setTypSuche(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Escape") {
                        e.stopPropagation();
                        setTypSuche("");
                      }
                    }}
                  />
                )}
                <div className="chips chips--einzeln">
                  {einzeln.map((e) => (
                    <button
                      key={e}
                      className={`chip chip--klein ${filter.endungen.includes(e) ? "chip--aktiv" : ""}`}
                      onClick={() => typUmschalten([e])}
                    >
                      {e.toUpperCase()}
                      <span className="chip__anzahl">{alsZahl(anzahlVon(e))}</span>
                    </button>
                  ))}
                  {einzeln.length === 0 && <span className="leise">Kein Dateityp passt.</span>}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}
