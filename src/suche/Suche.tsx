import { useCallback, useEffect, useRef, useState } from "react";
import {
  dateiDetails,
  sucheAngaben,
  sucheAufbauen,
  suchen,
  type DateiDetails,
  type Fortschritt,
  type SuchErgebnis,
  type SucheAngaben,
} from "../api";
import { alsZahl } from "../format";
import { seitenVon } from "./anzeige";
import Detail from "./Detail";
import Ergebnis from "./Ergebnis";
import Grossansicht from "./Grossansicht";
import Suchfeld, { istLeer, LEERER_FILTER, type SuchFilter } from "./Suchfeld";
import Teiler from "./Teiler";
import { begrenzen, DETAIL_STANDARD, ergebnisMin, lese, schreibe, type Ansicht } from "./breiten";

/** Höchstzahl der Treffer, die die Liste zeigt. */
const MAX = 2000;

/** Suchansicht (Stufe 2), aufgebaut wie Bilderwerk: Suchfeld, Raster oder Liste, Details rechts, Großansicht. */
export default function Suche({ sichtbar }: { sichtbar: boolean }) {
  const [filter, setFilter] = useState<SuchFilter>(LEERER_FILTER);
  const [ergebnis, setErgebnis] = useState<SuchErgebnis | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [angaben, setAngaben] = useState<SucheAngaben | null>(null);
  const [auswahl, setAuswahl] = useState<number | null>(null);
  const [seite, setSeite] = useState<number | null>(null);
  const [details, setDetails] = useState<DateiDetails | null>(null);
  const [detailFehler, setDetailFehler] = useState<string | null>(null);
  const [detailsAn, setDetailsAn] = useState<boolean>(() => lese("suche.details", true));
  const [ansicht, setAnsicht] = useState<Ansicht>(() => lese("suche.ansicht", "raster"));
  const [kachel, setKachel] = useState<number>(() => lese("suche.kachel", 160));
  const [detailBreite, setDetailBreite] = useState<number>(() => lese("suche.detailbreite", DETAIL_STANDARD));
  const [bereichBreite, setBereichBreite] = useState(0);
  const [grossOffen, setGrossOffen] = useState(false);
  const [stand, setStand] = useState(0);
  const [aufbau, setAufbau] = useState<Fortschritt | "laeuft" | null>(null);
  const bereich = useRef<HTMLDivElement | null>(null);
  const anfrage = useRef(0);
  const detailAnfrage = useRef(0);

  useEffect(() => schreibe("suche.details", detailsAn), [detailsAn]);
  useEffect(() => schreibe("suche.ansicht", ansicht), [ansicht]);
  useEffect(() => schreibe("suche.kachel", kachel), [kachel]);

  // Angaben für die Filter, jedes Mal, wenn die Ansicht sichtbar wird
  useEffect(() => {
    if (!sichtbar) return;
    sucheAngaben()
      .then(setAngaben)
      .catch((m) => setFehler(String(m)));
  }, [sichtbar, stand]);

  // Suche mit kurzer Verzögerung, nur die jüngste Antwort zählt
  useEffect(() => {
    if (!sichtbar) return;
    if (istLeer(filter)) {
      anfrage.current++;
      setErgebnis(null);
      setFehler(null);
      return;
    }
    const nr = ++anfrage.current;
    const t = window.setTimeout(() => {
      suchen({
        text: filter.text,
        von: filter.von.trim() || null,
        bis: filter.bis.trim() || null,
        endungen: filter.endungen,
        ordner: filter.ordner || null,
        nach_datum: filter.nachDatum,
        max: MAX,
      })
        .then((e) => {
          if (nr !== anfrage.current) return;
          setErgebnis(e);
          setFehler(null);
          setAuswahl((a) => (a !== null && e.treffer.some((t) => t.datei === a) ? a : null));
        })
        .catch((m) => nr === anfrage.current && setFehler(String(m)));
    }, 150);
    return () => window.clearTimeout(t);
  }, [filter, sichtbar, stand]);

  const index = ergebnis && auswahl !== null ? ergebnis.treffer.findIndex((t) => t.datei === auswahl) : -1;
  const treffer = index >= 0 ? ergebnis!.treffer[index] : null;
  const anzahl = ergebnis?.treffer.length ?? 0;

  // Angaben zur gewählten Datei; Seite: beste Trefferseite, sonst die erste mit Vorschau
  useEffect(() => {
    const nr = ++detailAnfrage.current;
    setDetailFehler(null);
    if (!treffer) {
      setDetails(null);
      setSeite(null);
      return;
    }
    setSeite(treffer.beste_seite ?? 1);
    dateiDetails(treffer.datei)
      .then((d) => {
        if (nr !== detailAnfrage.current) return;
        setDetails(d);
        const liste = seitenVon(d);
        setSeite((s) => (s !== null && liste.includes(s) ? s : (liste[0] ?? null)));
      })
      .catch((m) => nr === detailAnfrage.current && setDetailFehler(String(m)));
    // nur bei einem anderen Dokument neu laden
  }, [treffer?.datei, stand]);

  const passend = details && treffer && details.id === treffer.datei ? details : null;
  const seiten = seitenVon(passend);

  useEffect(() => {
    if (index < 0) setGrossOffen(false);
  }, [index]);

  const blaettern = useCallback(
    (schritt: number) => {
      if (!ergebnis || index < 0) return;
      const ziel = Math.max(0, Math.min(anzahl - 1, index + schritt));
      setAuswahl(ergebnis.treffer[ziel].datei);
    },
    [ergebnis, index, anzahl],
  );

  // Tasten außerhalb von Eingabefeldern: Leertaste Großansicht, Esc Auswahl aufheben,
  // in der Liste ← → die Seiten
  useEffect(() => {
    if (!sichtbar || index < 0 || grossOffen) return;
    const h = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const a = document.activeElement as HTMLElement | null;
      if (a && (a instanceof HTMLInputElement || a instanceof HTMLSelectElement || a.isContentEditable)) return;
      if (e.key === " " && !e.repeat) {
        e.preventDefault();
        setGrossOffen(true);
      } else if (e.key === "Escape") {
        setAuswahl(null);
      } else if (ansicht === "liste" && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        const i = seite === null ? -1 : seiten.indexOf(seite);
        const ziel = e.key === "ArrowLeft" ? i - 1 : i + 1;
        if (i >= 0 && ziel >= 0 && ziel < seiten.length) {
          e.preventDefault();
          setSeite(seiten[ziel]);
        }
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [sichtbar, index, grossOffen, ansicht, seite, seiten]);

  // Breite des Arbeitsbereichs beobachten (Fenstergröße), begrenzt den Schieber
  const bereichSetzen = useCallback((el: HTMLDivElement | null) => {
    bereich.current = el;
    if (el) setBereichBreite(el.clientWidth);
  }, []);
  useEffect(() => {
    const el = bereich.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBereichBreite(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ergebnis !== null]);

  async function indexAufbauen() {
    setAufbau("laeuft");
    try {
      await sucheAufbauen((f) => setAufbau(f));
    } catch (m) {
      setFehler(String(m));
    } finally {
      setAufbau(null);
      setStand((n) => n + 1);
    }
  }

  const minimum = ergebnisMin(ansicht, kachel);
  const breite = bereichBreite > 0 ? begrenzen(detailBreite, bereichBreite, minimum) : undefined;
  const hatSuche = ergebnis !== null || fehler !== null;

  return (
    <div className={`suchansicht ${hatSuche ? "suchansicht--suche" : ""}`} hidden={!sichtbar}>
      <Suchfeld filter={filter} onChange={setFilter} kompakt={hatSuche} angaben={angaben} />

      {angaben && angaben.ausstehend > 0 && (
        <div className="indexhinweis">
          {aufbau === null ? (
            <>
              <span>
                Suchindex unvollständig: {alsZahl(angaben.ausstehend)} von {alsZahl(angaben.dateien)} Dateien stehen aus.
              </span>
              <button className="knopf knopf--klein" onClick={indexAufbauen}>
                Jetzt aufbauen
              </button>
            </>
          ) : (
            <span>
              Suchindex wird aufgebaut …
              {aufbau !== "laeuft" && ` ${alsZahl(aufbau.erledigt)} von ${alsZahl(aufbau.gesamt)} Dateien`}
            </span>
          )}
        </div>
      )}

      {fehler && <div className="hinweis hinweis--fehler">{fehler}</div>}
      {ergebnis && !fehler && (
        <div className="arbeitsbereich" ref={bereichSetzen}>
          <Ergebnis
            ergebnis={ergebnis}
            auswahl={auswahl}
            onAuswahl={setAuswahl}
            onGross={() => setGrossOffen(true)}
            ansicht={ansicht}
            onAnsicht={setAnsicht}
            kachel={kachel}
            onKachel={setKachel}
            details={detailsAn}
            onDetails={setDetailsAn}
            nachDatum={filter.nachDatum}
            onNachDatum={(n) => setFilter((f) => ({ ...f, nachDatum: n }))}
          />
          {detailsAn && treffer && (
            <>
              <Teiler
                bereich={() => bereich.current}
                onZiehen={(w) => setDetailBreite(begrenzen(w, bereichBreite, minimum))}
                onFertig={() => schreibe("suche.detailbreite", begrenzen(detailBreite, bereichBreite, minimum))}
                onZuruecksetzen={() => {
                  setDetailBreite(DETAIL_STANDARD);
                  schreibe("suche.detailbreite", DETAIL_STANDARD);
                }}
              />
              <Detail
                treffer={treffer}
                seite={seite}
                onSeite={setSeite}
                details={passend}
                fehler={detailFehler}
                pfeileSeiten={ansicht === "liste"}
                position={[index + 1, ergebnis.gesamt]}
                breite={breite}
                onGross={() => setGrossOffen(true)}
              />
            </>
          )}
        </div>
      )}
      {grossOffen && treffer && ergebnis && (
        <Grossansicht
          treffer={treffer}
          seite={seite}
          seiten={seiten}
          position={[index + 1, ergebnis.gesamt]}
          mitVorschau={!!passend?.vorschau.length}
          onDokument={blaettern}
          onSeite={setSeite}
          onSchliessen={() => setGrossOffen(false)}
        />
      )}
    </div>
  );
}
