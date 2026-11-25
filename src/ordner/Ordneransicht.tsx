import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  BaumEintrag,
  GEAENDERT,
  OrdnerPruefung,
  Stufe,
  appInfo,
  astHinzufuegen,
  astLoesen,
  geaendertMelden,
  ordnerListe,
  ordnerPruefen,
  ordnerPruefenAbbrechen,
  ordnerWaehlen,
  regelnLesen,
  regelnSpeichern,
} from "../api";
import type { Anstehend } from "../register/Ablauf";
import { alsZahl } from "../format";
import Pruefung from "./Pruefung";
import { ERKLAERUNG, NAMEN, alsZeilen, ausZeilen, gleich, hatRegelnUnter, oberordner, setzen, wirksam } from "./stufen";

const STUFEN: Stufe[] = ["voll", "metadaten", "ausgeschlossen"];

/**
 * Ordnerregeln (v0.0.7): Baum der Äste mit der Stufe je Ordner. Ein Ast ist ein beliebiger Ordner
 * dieses Geräts, aufgenommen über „Ordner hinzufügen“. Ohne Regel wird ein Ordner nicht gelesen. Eine Regel gilt auch für die Unterordner, dort lässt sie sich wieder ändern.
 * Rechts die Prüfung des gewählten Ordners: wie viele Dateien welchen Typs darin liegen.
 */
export default function Ordneransicht({
  sichtbar,
  stand,
  zumRegister,
}: {
  sichtbar: boolean;
  /** Ergebnis der Prüfung im Reiter „Register“ (v0.1.1) */
  stand: Anstehend;
  zumRegister: () => void;
}) {
  const [geraet, setGeraet] = useState<string | null>(null);
  /** Der Name eines neuen Astes ist schon vergeben: nach einem anderen fragen */
  const [nameFrage, setNameFrage] = useState<{ ort: string; name: string; hinweis: string } | null>(null);
  const [gespeichert, setGespeichert] = useState<Map<string, Stufe>>(new Map());
  const [entwurf, setEntwurf] = useState<Map<string, Stufe>>(new Map());
  /** Unterordner je Ordner, sobald geladen. "" liefert die Äste. */
  const [kinder, setKinder] = useState<Map<string, BaumEintrag[]>>(new Map());
  const [auf, setAuf] = useState<Set<string>>(new Set());
  const [gewaehlt, setGewaehlt] = useState<string | null>(null);
  const [pruefungen, setPruefungen] = useState<Map<string, OrdnerPruefung>>(new Map());
  const [prueft, setPrueft] = useState<string | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  /** Fehler beim Laden des Baums. Verschwindet, sobald eine Ebene wieder geladen ist (v0.0.9). */
  const [baumFehler, setBaumFehler] = useState<string | null>(null);
  const [speichert, setSpeichert] = useState(false);
  /**
   * Nach dem Speichern (v0.1.1): Die Regeln wirken erst beim Verarbeiten im Reiter „Register“.
   * Das eigene „Dateibaum einlesen“ ist entfallen, das Speichern stößt die Prüfung an.
   */
  const [offenesEinlesen, setOffenesEinlesen] = useState(false);
  const geladen = useRef(false);
  const zeilenRef = useRef<Map<string, HTMLDivElement>>(new Map());
  const kinderRef = useRef(kinder);
  kinderRef.current = kinder;

  const geaendert = !gleich(gespeichert, entwurf);

  const liste = useCallback(async (ordner: string, neu = false) => {
    if (!neu && kinderRef.current.has(ordner)) return;
    try {
      const eintraege = await ordnerListe(ordner);
      setKinder((k) => new Map(k).set(ordner, eintraege));
      setBaumFehler(null);
    } catch (e) {
      setBaumFehler(String(e));
      throw e;
    }
  }, []);

  /** Klappt den Weg zu einem Ordner auf und lädt die nötigen Ebenen. */
  const wegOeffnen = useCallback(
    async (pfad: string) => {
      const weg = oberordner(pfad);
      for (const o of ["", ...weg]) await liste(o).catch(() => undefined);
      setAuf((a) => new Set([...a, ...weg]));
    },
    [liste],
  );

  // Erstes Öffnen: Regeln und oberste Ebene laden, den Weg zu jeder Regel aufklappen
  useEffect(() => {
    if (!sichtbar || geladen.current) return;
    geladen.current = true;
    (async () => {
      const info = await appInfo();
      if (info.konfig_fehler) throw new Error(info.konfig_fehler);
      setGeraet(info.geraet);
      const regeln = ausZeilen(await regelnLesen());
      setGespeichert(regeln);
      setEntwurf(regeln);
      await liste("");
      for (const pfad of regeln.keys()) await wegOeffnen(pfad);
    })().catch((e) => {
      geladen.current = false;
      setFehler(String(e));
    });
  }, [sichtbar, liste, wegOeffnen]);

  // Nach einem Einlesen (auch im Reiter „Register“): Zahlen im Baum neu laden
  useEffect(() => {
    const neuLaden = () => {
      for (const o of kinderRef.current.keys()) void liste(o, true).catch(() => undefined);
    };
    window.addEventListener(GEAENDERT, neuLaden);
    return () => window.removeEventListener(GEAENDERT, neuLaden);
  }, [liste]);

  useEffect(() => {
    if (gewaehlt) zeilenRef.current.get(gewaehlt)?.scrollIntoView({ block: "nearest" });
  }, [gewaehlt, kinder]);

  const umschalten = (pfad: string) => {
    if (auf.has(pfad)) {
      setAuf((a) => {
        const neu = new Set(a);
        neu.delete(pfad);
        return neu;
      });
    } else {
      setAuf((a) => new Set(a).add(pfad));
      liste(pfad).catch(() => undefined);
    }
  };

  const pruefen = useCallback(async (pfad: string) => {
    setPrueft(pfad);
    setFehler(null);
    try {
      const p = await ordnerPruefen(pfad);
      setPruefungen((alt) => new Map(alt).set(pfad, p));
    } catch (e) {
      setFehler(String(e));
    } finally {
      setPrueft(null);
    }
  }, []);

  /** Nimmt einen Ordner des Geräts als Ast auf. Ist der Name vergeben, wird nach einem anderen gefragt. */
  const astAufnehmen = async (ort: string, name: string | null) => {
    setFehler(null);
    try {
      const ast = await astHinzufuegen(ort, name, false);
      setNameFrage(null);
      await liste("", true);
      setGewaehlt(ast);
      geaendertMelden();
    } catch (e) {
      const text = String(e);
      if (text.includes("gibt es schon") || text.includes("kein gültiger Name")) {
        setNameFrage({ ort, name: name ?? ort.split(/[\\/]/).filter(Boolean).pop() ?? "", hinweis: text });
      } else {
        setFehler(text);
      }
    }
  };

  const hinzufuegen = async () => {
    try {
      const ort = await ordnerWaehlen();
      if (ort !== null) await astAufnehmen(ort, null);
    } catch (e) {
      setFehler(String(e));
    }
  };

  /** Weist einem Ast, den es auf diesem Gerät nicht gibt, hier einen Ort zu. */
  const ortZuweisen = async (ast: string) => {
    try {
      const ort = await ordnerWaehlen();
      if (ort === null) return;
      await astHinzufuegen(ort, ast, true);
      await liste("", true);
      geaendertMelden();
    } catch (e) {
      setFehler(String(e));
    }
  };

  const loesen = (ast: string) =>
    astLoesen(ast)
      .then(() => liste("", true))
      .then(() => setGewaehlt((g) => (g === ast ? null : g)))
      .catch((e) => setFehler(String(e)));

  /** Aus der Prüfung in einen Unterordner eintauchen: zeigen, wählen, prüfen. */
  const eintauchen = async (pfad: string) => {
    await wegOeffnen(pfad);
    setGewaehlt(pfad);
    if (!pruefungen.has(pfad) && !prueft) void pruefen(pfad);
  };

  async function speichern() {
    setSpeichert(true);
    setFehler(null);
    try {
      const regeln = ausZeilen(await regelnSpeichern(alsZeilen(entwurf)));
      setGespeichert(regeln);
      setEntwurf(regeln);
      setOffenesEinlesen(true);
      geaendertMelden();
    } catch (e) {
      setFehler(String(e));
    } finally {
      setSpeichert(false);
    }
  }

  const regelListe = useMemo(() => alsZeilen(entwurf), [entwurf]);

  const zeilen = (ordner: string, tiefe: number): JSX.Element[] =>
    (kinder.get(ordner) ?? []).flatMap((k) => {
      const w = wirksam(entwurf, k.pfad);
      const offen = auf.has(k.pfad);
      const drin = w.stufe !== "ausgeschlossen";
      return [
        <div
          key={k.pfad}
          ref={(el) => {
            if (el) zeilenRef.current.set(k.pfad, el);
            else zeilenRef.current.delete(k.pfad);
          }}
          className={`baum__zeile ${k.pfad === gewaehlt ? "baum__zeile--gewaehlt" : ""} ${drin ? "" : "baum__zeile--aus"}`}
          style={{ paddingLeft: 8 + tiefe * 18 }}
        >
          {k.hat_unterordner ? (
            <button
              className={`ordner__pfeil ${offen ? "ordner__pfeil--auf" : ""}`}
              onClick={() => umschalten(k.pfad)}
              aria-label={offen ? "Zuklappen" : "Aufklappen"}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <path d="M9 6l6 6-6 6" />
              </svg>
            </button>
          ) : (
            <span className="ordner__pfeil" />
          )}
          <button
            className={`baum__name ${tiefe === 0 ? "baum__name--ast" : ""}`}
            onClick={() => !k.fehlt && setGewaehlt(k.pfad)}
            title={k.ort ?? k.pfad}
          >
            {k.name}
            {tiefe === 0 && k.ort && !k.fehlt && <span className="baum__ort">{k.ort}</span>}
          </button>
          {k.fehlt && (
            <>
              <span className="baum__marke" title={k.ort ? `Nicht gefunden: ${k.ort}` : "Diesem Ast ist auf diesem Gerät kein Ort zugewiesen."}>
                {k.geraete.length > 0 ? `liegt auf ${k.geraete.join(", ")}` : "auf diesem Gerät nicht vorhanden"}
              </span>
              <button className="knopf knopf--leise baum__knopf" onClick={() => void ortZuweisen(k.name)}>
                Ort zuweisen …
              </button>
            </>
          )}
          {tiefe === 0 && k.ort !== null && k.im_register === 0 && !entwurf.has(k.pfad) && !hatRegelnUnter(entwurf, k.pfad) && (
            <button
              className="regelliste__weg"
              onClick={() => void loesen(k.name)}
              title="Ast wieder lösen: Er hat keine Regeln und nichts im Register."
              aria-label={`Ast ${k.name} lösen`}
            >
              ×
            </button>
          )}
          {k.bibliothek && !w.eigene && drin && (
            <span className="baum__marke" title="Ordner fremder Bibliotheken werden übergangen. Eine eigene Regel schließt ihn ein.">
              übergangen
            </span>
          )}
          {!offen && hatRegelnUnter(entwurf, k.pfad) && (
            <span className="baum__punkt" title="Unterordner mit eigener Regel" aria-label="Unterordner mit eigener Regel" />
          )}
          {k.nur_online > 0 && (
            <span className="baum__online" title="Dateien, deren Inhalt aussteht und die nur online liegen">
              {alsZahl(k.nur_online)} nur online
            </span>
          )}
          {k.im_register > 0 && (
            <span className="baum__anzahl" title="Dateien im Register">
              {alsZahl(k.im_register)}
            </span>
          )}
          <span className="stufenwahl" role="group" aria-label={`Stufe für ${k.name}`}>
            {STUFEN.map((s) => (
              <button
                key={s}
                className={`stufenwahl__knopf stufenwahl__knopf--${s} ${
                  w.stufe === s ? (w.eigene ? "stufenwahl__knopf--eigene" : "stufenwahl__knopf--geerbt") : ""
                }`}
                aria-pressed={w.stufe === s}
                title={
                  w.stufe === s
                    ? w.eigene
                      ? `Eigene Regel: ${ERKLAERUNG[s]}`
                      : w.von !== null
                        ? `Geerbt von „${w.von}“: ${ERKLAERUNG[s]}`
                        : "Ohne Regel wird dieser Ordner nicht gelesen."
                    : ERKLAERUNG[s]
                }
                onClick={() => setEntwurf((e) => setzen(e, k.pfad, s))}
              >
                {NAMEN[s]}
              </button>
            ))}
          </span>
        </div>,
        ...(offen ? zeilen(k.pfad, tiefe + 1) : []),
      ];
    });

  const oben = kinder.get("");

  return (
    <div className="regelansicht" hidden={!sichtbar}>
      <header className="regel__kopf">
        <div className="regel__titel">
          <h2>Ordner</h2>
          <p className="leise">
            Welche Ordner ins Register kommen. Eine Regel gilt auch für die Unterordner und lässt sich dort wieder
            ändern. {geraet && <>Dieses Gerät: <b>{geraet}</b></>}
          </p>
        </div>
        <div className="regel__knoepfe">
          <button className="knopf knopf--leise" onClick={() => void hinzufuegen()}>
            Ordner hinzufügen …
          </button>
          {geaendert ? (
            <>
              <span className="achtung">Ungespeicherte Änderungen</span>
              <button className="knopf knopf--leise" onClick={() => setEntwurf(gespeichert)} disabled={speichert}>
                Verwerfen
              </button>
              <button className="knopf" onClick={speichern} disabled={speichert}>
                Speichern
              </button>
            </>
          ) : null}
        </div>
      </header>

      {offenesEinlesen && (
        <section className="regel__meldung regel__wirkung">
          <div>
            <strong>Regeln gespeichert. Jetzt im Register verarbeiten.</strong>
            {stand.prueft ? (
              <p className="arbeitet">
                <span className="kreisel" /> Prüft, was die Regeln am Register ändern …
              </p>
            ) : stand.etwas ? (
              <p>
                Im Register steht an: <b>{wirkung(stand)}</b>. Übernommen wird das erst mit „Verarbeiten“ im Reiter
                „Register“.
              </p>
            ) : (
              <p>Im Register steht nichts an. Die Regeln ändern am Register nichts.</p>
            )}
          </div>
          {stand.etwas || stand.prueft ? (
            <button
              className="knopf regel__wirkung-knopf"
              onClick={() => {
                setOffenesEinlesen(false);
                zumRegister();
              }}
            >
              Im Register verarbeiten →
            </button>
          ) : (
            <button className="knopf knopf--leise" onClick={() => setOffenesEinlesen(false)}>
              Schließen
            </button>
          )}
        </section>
      )}
      {(fehler || baumFehler) && (
        <section className="hinweis fehler regel__meldung regel__fehler">
          <div>
            <strong>Fehler</strong>
            <p>{fehler ?? baumFehler}</p>
          </div>
          {fehler && (
            <button className="regel__fehler-zu" onClick={() => setFehler(null)} aria-label="Meldung schließen" title="Schließen">
              ×
            </button>
          )}
        </section>
      )}
      {nameFrage && (
        <section className="regel__meldung regel__lauf regel__name">
          <p>
            {nameFrage.hinweis} Unter welchem Namen soll <span className="pfad">{nameFrage.ort}</span> im Register
            stehen?
          </p>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void astAufnehmen(nameFrage.ort, nameFrage.name);
            }}
          >
            <input
              value={nameFrage.name}
              onChange={(e) => setNameFrage({ ...nameFrage, name: e.target.value })}
              autoFocus
              aria-label="Name des Astes"
            />
            <button className="knopf" type="submit">
              Aufnehmen
            </button>
            <button className="knopf knopf--leise" type="button" onClick={() => setNameFrage(null)}>
              Abbrechen
            </button>
          </form>
        </section>
      )}

      <div className="regel__koerper">
        <div className="baum" role="tree">
          {oben === undefined && !fehler && !baumFehler && <p className="leise baum__leer">Lädt …</p>}
          {oben?.length === 0 && (
            <p className="leise baum__leer">
              Noch kein Ordner aufgenommen. „Ordner hinzufügen …“ öffnet die Auswahl: jeder Ordner dieses Geräts ist
              möglich, etwa ein Cloud-Speicher, Dokumente oder ein externes Laufwerk.
            </p>
          )}
          {zeilen("", 0)}
        </div>

        <aside className="regel__seite">
          <section className="regel__karte">
            <h3>Stufen</h3>
            <dl className="legende">
              {STUFEN.map((s) => (
                <div key={s}>
                  <dt>
                    <span className={`stufenwahl__knopf stufenwahl__knopf--${s} stufenwahl__knopf--eigene`}>{NAMEN[s]}</span>
                  </dt>
                  <dd>{ERKLAERUNG[s]}</dd>
                </div>
              ))}
            </dl>
            <p className="leise">Kräftig: eigene Regel des Ordners. Blass: vom Oberordner geerbt.</p>
          </section>

          <section className="regel__karte">
            <h3>Ordner-Regeln ({regelListe.length})</h3>
            {regelListe.length === 0 && <p className="leise">Noch keine Regel. Ohne Regel wird nichts gelesen.</p>}
            <ul className="regelliste">
              {regelListe.map((r) => (
                <li key={r.ordner}>
                  <span className={`stufenwahl__knopf stufenwahl__knopf--${r.stufe} stufenwahl__knopf--eigene`}>
                    {NAMEN[r.stufe]}
                  </span>
                  <button className="regelliste__pfad" onClick={() => void eintauchenOhnePruefung(r.ordner)} title="Im Baum zeigen">
                    {r.ordner}
                  </button>
                  <button
                    className="regelliste__weg"
                    onClick={() =>
                      setEntwurf((e) => {
                        const neu = new Map(e);
                        neu.delete(r.ordner);
                        return neu;
                      })
                    }
                    aria-label={`Regel für ${r.ordner} entfernen`}
                    title="Regel entfernen"
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          </section>

          {gewaehlt !== null ? (
            <Pruefung
              ordner={gewaehlt}
              wirksam={wirksam(entwurf, gewaehlt)}
              pruefung={pruefungen.get(gewaehlt) ?? null}
              laeuft={prueft === gewaehlt}
              gesperrt={prueft !== null}
              onPruefen={() => void pruefen(gewaehlt)}
              onAbbrechen={() => void ordnerPruefenAbbrechen()}
              onUnterordner={(name) => void eintauchen(`${gewaehlt}/${name}`)}
            />
          ) : (
            <section className="regel__karte">
              <h3>Ordner prüfen</h3>
              <p className="leise">
                Ein Klick auf einen Ordnernamen zeigt hier, wie viele Dateien welchen Typs darin liegen. Die Prüfung
                liest nur die Verzeichnisse, lädt nichts herunter und ändert nichts.
              </p>
            </section>
          )}
        </aside>
      </div>
    </div>
  );

  async function eintauchenOhnePruefung(pfad: string) {
    await wegOeffnen(pfad);
    setGewaehlt(pfad);
  }
}

/** Zahlen der Prüfung als kurzer Text, nur was nicht null ist (v0.1.1). */
function wirkung(s: Anstehend): string {
  return [
    [s.neu, "neu"],
    [s.geaendert, "geändert"],
    [s.offen, "noch offen"],
    [s.nurNamen, "neu nur mit Namen"],
    [s.entfernen, "zu entfernen"],
  ]
    .filter(([n]) => (n as number) > 0)
    .map(([n, t]) => `${alsZahl(n as number)} ${t}`)
    .join(" · ");
}
