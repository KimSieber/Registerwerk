import { useEffect, useRef, useState } from "react";
import { imFinderZeigen, originalOeffnen, seitentexte, type DateiDetails, type SeitenText, type Treffer } from "../api";
import { alsGroesse, alsZahl, alsZeitpunkt } from "../format";
import { datumDe, ordnerVon, seitenVon, titelVon, vorschauSeite } from "./anzeige";
import { lese, schreibe } from "./breiten";
import Seitenbild from "./Seitenbild";

interface Props {
  treffer: Treffer;
  /** angezeigte Seite, `null`: keine Vorschau */
  seite: number | null;
  onSeite: (seite: number) => void;
  /** Angaben zur Datei, `null` solange sie laden */
  details: DateiDetails | null;
  fehler: string | null;
  /** Liste: ← → blättern Seiten, Raster: Dokumente */
  pfeileSeiten: boolean;
  /** Position in der Trefferliste (1-basiert) und Anzahl */
  position: [number, number];
  /** Breite in px (Schieber) */
  breite?: number;
  onGross: () => void;
}

const QUELLE: Record<string, string> = {
  dateiname: "aus dem Dateinamen",
  pdf: "aus den PDF-Angaben",
  geaendert: "Änderungsdatum der Datei",
};

/**
 * Detailbereich rechts: Vorschau der Seite mit Blättern, Angaben zur Datei, Original öffnen
 * oder im Finder zeigen. Aufbau wie in Bilderwerk.
 */
export default function Detail({ treffer, seite, onSeite, details: d, fehler, pfeileSeiten, position, breite, onGross }: Props) {
  const [meldung, setMeldung] = useState<{ text: string; fehler: boolean } | null>(null);
  const datei = treffer.datei;

  useEffect(() => setMeldung(null), [datei]);

  // Gespeicherter Text der Seiten, erst geladen, wenn der Abschnitt offen ist
  const [textOffen, setTextOffen] = useState<boolean>(() => lese("suche.text", false));
  const [texte, setTexte] = useState<{ datei: number; seiten: SeitenText[] } | null>(null);
  const [textFehler, setTextFehler] = useState<string | null>(null);
  const textAnfrage = useRef(0);
  useEffect(() => {
    if (!textOffen) return;
    const nr = ++textAnfrage.current;
    setTextFehler(null);
    seitentexte(datei)
      .then((t) => nr === textAnfrage.current && setTexte({ datei, seiten: t }))
      .catch((m) => nr === textAnfrage.current && setTextFehler(String(m)));
  }, [datei, textOffen]);
  const seitenText = texte?.datei === datei ? texte.seiten.find((t) => t.nr === seite) : undefined;

  const aktion = async (f: () => Promise<void>, text: string) => {
    setMeldung(null);
    try {
      await f();
      if (text) setMeldung({ text, fehler: false });
    } catch (m) {
      setMeldung({ text: String(m), fehler: true });
    }
  };
  const oeffnen = () =>
    aktion(() => originalOeffnen(datei), d?.nur_online ? "Liegt nur online, die Dropbox lädt die Datei …" : "");
  const zeigen = () => aktion(() => imFinderZeigen(datei), "");

  // ⌘O öffnen, ⌥⌘R im Finder
  const kuerzel = useRef<(e: KeyboardEvent) => void>(() => {});
  kuerzel.current = (e) => {
    if (!e.metaKey || e.ctrlKey || e.shiftKey) return;
    const k = e.key.toLowerCase();
    if (k === "o" && !e.altKey) {
      e.preventDefault();
      void oeffnen();
    } else if ((k === "r" || e.code === "KeyR") && e.altKey) {
      e.preventDefault();
      void zeigen();
    }
  };
  useEffect(() => {
    const h = (e: KeyboardEvent) => kuerzel.current(e);
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  const passend = d && d.id === datei ? d : null;
  const seiten = seitenVon(passend);
  const mitVorschau = passend ? passend.vorschau.length > 0 : vorschauSeite(treffer) !== null;
  const i = seite === null ? -1 : seiten.indexOf(seite);
  const treffseiten = treffer.trefferseiten;

  return (
    <aside className="detail" aria-label="Details" style={breite ? { flexBasis: breite } : undefined}>
      <div className="detail__bild" title="Klick: Großansicht · ⌘ + Mausrad oder zwei Finger: vergrößern">
        <Seitenbild datei={datei} seite={mitVorschau ? seite : null} endung={treffer.endung} onKlick={onGross} />
      </div>

      {seiten.length > 1 && (
        <div className="detail__seiten">
          <button className="knopf knopf--klein" disabled={i <= 0} onClick={() => onSeite(seiten[i - 1])} title="Vorige Seite">
            ‹
          </button>
          <span>
            Seite {seite} von {seiten.length}
            {seite !== null && treffseiten.includes(seite) && <span className="detail__marke">Treffer</span>}
          </span>
          <button
            className="knopf knopf--klein"
            disabled={i < 0 || i >= seiten.length - 1}
            onClick={() => onSeite(seiten[i + 1])}
            title="Nächste Seite"
          >
            ›
          </button>
        </div>
      )}

      <div className="detail__knoepfe">
        <button className="knopf knopf--klein" onClick={onGross} title="Großansicht (Leertaste)">
          Großansicht
        </button>
        <button className="knopf knopf--klein" onClick={oeffnen} title="Original in der Standard-App öffnen (⌘O)">
          Öffnen
        </button>
        <button className="knopf knopf--klein" onClick={zeigen} title="Im Finder zeigen (⌥⌘R)">
          Im Finder
        </button>
      </div>
      {meldung && <div className={`detail__meldung ${meldung.fehler ? "detail__meldung--fehler" : ""}`}>{meldung.text}</div>}
      {fehler && <div className="detail__meldung detail__meldung--fehler">{fehler}</div>}

      <h2 className="detail__titel">{titelVon(treffer.name)}</h2>

      {treffseiten.length > 0 && (
        <section className="detail__gruppe">
          <h3>Treffer auf {treffseiten.length === 1 ? "Seite" : `${alsZahl(treffseiten.length)} Seiten`}</h3>
          <div className="detail__chips">
            {treffseiten.slice(0, 40).map((s) => (
              <button
                key={s}
                className={`detail__chip ${s === seite ? "detail__chip--aktiv" : ""}`}
                onClick={() => onSeite(s)}
              >
                {s}
              </button>
            ))}
            {treffseiten.length > 40 && <span className="leise">…</span>}
          </div>
          {treffer.unscharf && <p className="detail__meldung">Teilweise über ähnliche Wörter gefunden.</p>}
        </section>
      )}

      <section className="detail__gruppe">
        <h3>Dokument</h3>
        <dl className="detail__liste">
          <Zeile label="Datum" wert={treffer.datum && `${datumDe(treffer.datum)}${passend?.datum_quelle ? ` · ${QUELLE[passend.datum_quelle] ?? ""}` : ""}`} />
          <Zeile label="Typ" wert={treffer.endung.toUpperCase()} />
          <Zeile label="Seiten" wert={treffer.seiten !== null ? alsZahl(treffer.seiten) : null} />
          <Zeile label="Größe" wert={passend?.groesse != null ? alsGroesse(passend.groesse) : null} />
          <Zeile label="Geändert" wert={passend ? alsZeitpunkt(passend.geaendert) : null} />
          <Zeile label="Zustand" wert={passend?.nur_online ? "nur online" : passend ? "lokal" : null} />
          <Zeile
            label="Vorschau"
            wert={(() => {
              const v = passend?.vorschau.find((x) => x.nr === seite);
              return v ? `${v.breite} × ${v.hoehe} px · ${v.farbig ? "Farbe" : "Graustufen"}` : null;
            })()}
          />
        </dl>
      </section>

      {(treffer.seiten ?? 0) > 0 && (
        <details
          className="detail__gruppe"
          open={textOffen}
          onToggle={(e) => {
            const offen = (e.currentTarget as HTMLDetailsElement).open;
            setTextOffen(offen);
            schreibe("suche.text", offen);
          }}
        >
          <summary>Gespeicherter Text{seite !== null ? ` · Seite ${seite}` : ""}</summary>
          {textFehler && <div className="detail__meldung detail__meldung--fehler">{textFehler}</div>}
          {textOffen && seitenText && <TextDerSeite t={seitenText} pdf={treffer.endung === "pdf"} />}
          {textOffen && texte?.datei === datei && !seitenText && <p className="detail__meldung">Kein Text zu dieser Seite.</p>}
        </details>
      )}

      <section className="detail__gruppe">
        <h3>Ablage</h3>
        <dl className="detail__liste">
          <Zeile label="Ordner" wert={ordnerVon(treffer.pfad)} />
          <Zeile label="Datei" wert={treffer.name} />
        </dl>
      </section>

      <div className="detail__fuss">
        <span>{pfeileSeiten ? "↑ ↓ Dokumente · ← → Seiten" : "Pfeiltasten Dokumente"} · Leertaste Großansicht</span>
        <span>
          {alsZahl(position[0])} / {alsZahl(position[1])}
        </span>
      </div>
    </aside>
  );
}

function Zeile({ label, wert }: { label: string; wert: string | null | undefined | false }) {
  if (!wert) return null;
  return (
    <div className="detail__zeile">
      <dt>{label}</dt>
      <dd>{wert}</dd>
    </div>
  );
}

/** Gültiger Text einer Seite und woher er stammt. Liegt daneben ein anderer Text, ist er aufklappbar. */
function TextDerSeite({ t, pdf }: { t: SeitenText; pdf: boolean }) {
  const ocr = t.uebernommen && t.ocr_text !== null;
  const text = ocr ? t.ocr_text! : t.pdf_text;
  const quelle = ocr ? "erkannt mit Tesseract" : !pdf ? "aus der Datei" : t.bildanteil >= 0.7 ? "eingebettet (Scan-App)" : "eingebettet";
  const anderer = ocr ? t.pdf_text : t.ocr_text;
  return (
    <>
      <p className="detail__meldung">
        {quelle} · {text.trim().length} Zeichen
      </p>
      {text.trim() ? <pre className="detail__text">{text}</pre> : <p className="detail__meldung">Diese Seite hat keinen Text.</p>}
      {anderer && anderer.trim() && (
        <details>
          <summary className="detail__meldung">{ocr ? "Eingebetteter Text" : "Erkannter Text (nicht übernommen)"}</summary>
          <pre className="detail__text">{anderer}</pre>
        </details>
      )}
    </>
  );
}
