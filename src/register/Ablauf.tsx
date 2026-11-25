import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AblaufErgebnis,
  DeltaAntwort,
  DeltaDatei,
  DeltaFilter,
  GEAENDERT,
  Meldung,
  OrdnerZahl,
  Schritt,
  deltaDateien,
  deltaListe,
  deltaPruefen,
  pruefungAnhalten,
  geaendertMelden,
  inhalteAbbrechen,
  ladenUeberspringen,
  verarbeiten,
  Grund,
} from "../api";
import { GRUENDE, VermisstListe } from "../Einlesen";
import { alsDauer, alsZahl, alsZeitpunkt } from "../format";
import Verwaltung from "../Verwaltung";
import { useTaetigkeit } from "../taetigkeit";

/**
 * Reiter „Register“ (v0.0.8): ein klarer Ablauf. Beim Öffnen der Anwendung prüft Registerwerk im
 * Hintergrund, was ansteht, ohne etwas zu ändern, und meldet Änderungen an den Rahmen (v0.0.9). Der Baum zeigt die Dateien nach Ordnern, alles ist angehakt.
 * „Verarbeiten“ nimmt auf, lädt „nur online“-Dateien und führt die Auswahl durch alle Schritte.
 * Seit v0.1.1 entfernt „Verarbeiten“ auch, was wegfällt, außer dem Abgewählten. Die bisherige Ansicht steht unter „Einzelheiten“.
 */
/** Stand der letzten Prüfung für den Knopf neben den Reitern (v0.0.9). */
export interface Anstehend {
  /** Es steht etwas an: neu, geändert, noch offen, zu entfernen oder ein vermisster Ordner */
  etwas: boolean;
  /** Darunter neue, geänderte oder zu entfernende Dateien oder vermisste Ordner */
  aenderungen: boolean;
  /** Die Prüfung läuft gerade (v0.1.1, für den Kasten im Reiter „Ordner“) */
  prueft: boolean;
  /** Zahlen der letzten Prüfung (v0.1.1) */
  neu: number;
  geaendert: number;
  offen: number;
  nurNamen: number;
  entfernen: number;
}

export const NICHTS_ANSTEHEND: Anstehend = {
  etwas: false,
  aenderungen: false,
  prueft: false,
  neu: 0,
  geaendert: 0,
  offen: 0,
  nurNamen: 0,
  entfernen: 0,
};

export default function Ablauf({
  onAenderungen,
}: {
  /** Ergebnis der letzten Prüfung: Gibt es etwas zu tun, und sind Änderungen dabei (neu, geändert, zu entfernen)? */
  onAenderungen: (stand: Partial<Anstehend>) => void;
}) {
  const [delta, setDelta] = useState<DeltaAntwort | null>(null);
  const [prueft, setPrueft] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const [aus, setAus] = useState<Set<string>>(new Set());
  /** Alle „nur online“-Dateien auf einen Klick ausschließen */
  const [ohneOnline, setOhneOnline] = useState(false);
  /** Geladene Dateien je Ordner, für Zahlen beim Abwählen einzelner Dateien */
  const dateienCache = useRef<Map<string, DeltaDatei>>(new Map());
  const [laeuft, setLaeuft] = useState(false);
  const [haelt, setHaelt] = useState(false);
  const [meldung, setMeldung] = useState<Meldung | null>(null);
  const [fertig, setFertig] = useState<Set<Schritt>>(new Set());
  const [ergebnis, setErgebnis] = useState<AblaufErgebnis | null>(null);
  /** Diesmal nichts aus dem Register entfernen (v0.1.1) */
  const [ohneEntfernen, setOhneEntfernen] = useState(false);
  /** Rückfrage vor dem Entfernen vieler Einträge (v0.1.1) */
  const [entfernenFrage, setEntfernenFrage] = useState(false);
  /** Fenster mit Dateinamen: Art und wahlweise Ordner */
  const [liste, setListe] = useState<{ filter: DeltaFilter; ordner: string | null; titel: string } | null>(null);
  // Eine Prüfung zur selben Zeit. Beim Start fragt React im Entwicklungsmodus doppelt.
  const pruefLauf = useRef(false);
  // Kommt während einer Prüfung ein neuer Anlass (etwa gespeicherte Regeln), wird danach noch
  // einmal geprüft. Sonst gälte das Ergebnis der alten Regeln (v0.1.1).
  const nochmal = useRef(false);
  const pruefenRef = useRef<() => Promise<void>>(async () => {});

  const pruefen = useCallback(async () => {
    if (pruefLauf.current) {
      nochmal.current = true;
      return;
    }
    pruefLauf.current = true;
    setPrueft(true);
    onAenderungen({ prueft: true });
    setFehler(null);
    try {
      const d = await deltaPruefen();
      setDelta(d);
      setAus(new Set());
      dateienCache.current = new Map();
      const aenderungen =
        d.uebersicht.neu +
          d.uebersicht.geaendert +
          d.nur_namen +
          d.lauf.entfernt +
          d.lauf.reduziert +
          d.lauf.vermisst.filter((o) => o.dateien > 0).length >
        0;
      onAenderungen({
        etwas: aenderungen || d.uebersicht.dateien > 0,
        aenderungen,
        neu: d.uebersicht.neu,
        geaendert: d.uebersicht.geaendert,
        offen: d.uebersicht.offen,
        nurNamen: d.nur_namen,
        entfernen: d.lauf.entfernt + d.lauf.reduziert,
      });
    } catch (e) {
      // Läuft gerade ein Vorgang (etwa unter „Einzelheiten“), bleibt der letzte Stand stehen
      const text = String(e);
      setFehler(
        text.includes("angehalten")
          ? "Prüfung angehalten. Mit „Erneut prüfen“ neu starten."
          : text.includes("läuft")
            ? "Nicht geprüft: Unter „Einzelheiten“ läuft gerade ein Schritt. Danach erneut prüfen."
            : text,
      );
    } finally {
      pruefLauf.current = false;
      setPrueft(false);
      onAenderungen({ prueft: false });
      if (nochmal.current) {
        nochmal.current = false;
        setTimeout(() => void pruefenRef.current(), 0);
      }
    }
  }, [onAenderungen]);
  pruefenRef.current = pruefen;

  // Beim Start der Anwendung
  useEffect(() => {
    void pruefen();
  }, [pruefen]);

  // Geprüft wird nur beim Start und mit „Erneut prüfen“ (v0.0.9, Kim): keine unerwarteten
  // Läufe während der Suche. Beim Wechsel in den Reiter steht der letzte Stand.

  // Nach Änderungen an anderer Stelle (Regeln, Einlesen unter „Einzelheiten“)
  useEffect(() => {
    const neu = () => {
      if (!laeuft) void pruefen();
    };
    window.addEventListener(GEAENDERT, neu);
    return () => window.removeEventListener(GEAENDERT, neu);
  }, [pruefen, laeuft]);

  const ordner = useMemo(() => {
    const m = new Map<string, OrdnerZahl>();
    delta?.uebersicht.ordner.forEach((o) => m.set(o.pfad, o));
    return m;
  }, [delta]);

  // Gewählte Dateien: alle außer den ausgeschlossenen (Einträge in `aus` überschneiden sich nicht)
  const gewaehlt = useMemo(() => {
    if (!delta) return { dateien: 0, online: 0, entfernen: 0 };
    let dateien = delta.uebersicht.dateien;
    let online = delta.uebersicht.nur_online;
    aus.forEach((p) => {
      const o = ordner.get(p);
      if (o) {
        dateien -= o.dateien;
        online -= o.nur_online;
      } else {
        dateien -= 1;
        online -= dateienCache.current.get(p)?.nur_online ? 1 : 0;
      }
    });
    if (ohneOnline) {
      dateien -= online;
      online = 0;
    }
    // Zu entfernen: alle außer den abgewählten (v0.1.1). Abgewählte Ordner zählen mit ihrem Bestand.
    let entfernen = delta.lauf.entfernt + delta.lauf.reduziert;
    aus.forEach((p) => {
      const o = ordner.get(p);
      if (o) entfernen -= o.entfernen;
      else if (dateienCache.current.get(p)?.art === "entfernen") entfernen -= 1;
    });
    if (ohneEntfernen) entfernen = 0;
    return { dateien, online, entfernen };
  }, [delta, aus, ordner, ohneOnline, ohneEntfernen]);

  const starten = async () => {
    setLaeuft(true);
    setHaelt(false);
    setFehler(null);
    setErgebnis(null);
    setMeldung(null);
    setFertig(new Set());
    let vorher: Schritt | null = null;
    try {
      const e = await verarbeiten([...aus], ohneOnline, !ohneEntfernen, (m) => {
        if (vorher && vorher !== m.schritt) {
          const v = vorher;
          setFertig((f) => new Set(f).add(v));
        }
        vorher = m.schritt;
        setMeldung(m);
      });
      setErgebnis(e);
    } catch (e) {
      setFehler(String(e));
    } finally {
      setLaeuft(false);
      setMeldung(null);
      geaendertMelden();
      await pruefen();
    }
  };

  const u = delta?.uebersicht;
  const lauf = delta?.lauf;
  const zuEntfernen = lauf ? lauf.entfernt + lauf.reduziert : 0;
  useTaetigkeit(prueft ? "Prüft den Dateibaum" : laeuft ? "Verarbeitet" : null);
  const nichtsZuTun = u && u.dateien === 0 && (delta?.nur_namen ?? 0) === 0 && zuEntfernen === 0;
  const etwasGewaehlt = gewaehlt.dateien > 0 || (delta?.nur_namen ?? 0) > 0 || gewaehlt.entfernen > 0;
  // Viele Einträge auf einmal: einmal nachfragen. Ein versehentlich auf „Aus“ gestellter Ordner
  // würde sonst mit einem Klick Text und Vorschauen entfernen (Kim, v0.1.1).
  const ENTFERNEN_FRAGEN_AB = 100;
  const verarbeitenKlick = () => {
    if (gewaehlt.entfernen > ENTFERNEN_FRAGEN_AB) setEntfernenFrage(true);
    else void starten();
  };

  return (
    <div className="seite ablauf">
      {entfernenFrage && (
        <EntfernenFrage
          anzahl={gewaehlt.entfernen}
          zeigen={() => setListe({ filter: "entfernen", ordner: null, titel: "Zu entfernen" })}
          onAbbrechen={() => setEntfernenFrage(false)}
          onBestaetigen={() => {
            setEntfernenFrage(false);
            void starten();
          }}
        />
      )}
      {liste && <DateiFenster {...liste} onSchliessen={() => setListe(null)} />}

      <section className="karte">
        <div className="karte-kopf">
          <div>
            <h2>Abgleich</h2>
            <p className="unterzeile">
              {prueft
                ? "Prüft den Dateibaum …"
                : delta
                  ? `Geprüft ${alsZeitpunkt(delta.geprueft)}. Es wurde nichts geändert.`
                  : "Noch nicht geprüft."}
            </p>
          </div>
          {prueft ? (
            <button className="knopf knopf--leise" onClick={() => void pruefungAnhalten()}>
              <span className="kreisel" /> Prüfung anhalten
            </button>
          ) : (
            <button className="knopf knopf--leise" onClick={() => void pruefen()} disabled={laeuft}>
              Erneut prüfen
            </button>
          )}
        </div>

        {fehler && <p className="achtung">{fehler}</p>}

        {u && (u.dateien > 0 || zuEntfernen > 0 || laeuft) && (
          <div className="ablauf__zahlen">
            <Zahl wert={u.neu} titel="neu" zeigen={() => setListe({ filter: "neu", ordner: null, titel: "Neu" })} />
            <Zahl
              wert={u.geaendert}
              titel="geändert"
              zeigen={() => setListe({ filter: "geaendert", ordner: null, titel: "Geändert" })}
            />
            <Zahl
              wert={u.offen}
              titel="noch offen"
              zeigen={() => setListe({ filter: "offen", ordner: null, titel: "Noch offen" })}
            />
            <Zahl
              wert={u.nur_online}
              titel="davon nur online"
              leise
              zeigen={() => setListe({ filter: "online", ordner: null, titel: "Nur online" })}
            />
            {(delta?.nur_namen ?? 0) > 0 && (
              <Zahl
                wert={delta!.nur_namen}
                titel="neu nur mit Namen"
                leise
                zeigen={() => setListe({ filter: "namen", ordner: null, titel: "Neu, nur mit Namen" })}
              />
            )}
            {zuEntfernen > 0 && (
              <Zahl
                wert={zuEntfernen}
                titel="zu entfernen"
                warnung
                zeigen={() => setListe({ filter: "entfernen", ordner: null, titel: "Zu entfernen" })}
              />
            )}
          </div>
        )}

        {nichtsZuTun && !laeuft && <p className="ablauf__ruhe">✓ Das Register ist aktuell.</p>}

        {u && (u.dateien > 0 || delta!.nur_namen > 0 || zuEntfernen > 0) && !laeuft && (
          <>
            {u.nur_online > 0 && (
              <div className="ablauf__online">
                <span>
                  ☁ {alsZahl(u.nur_online)} {u.nur_online === 1 ? "Datei liegt" : "Dateien liegen"} nur online
                </span>
                <span className="umschalter umschalter--text">
                  <button className={!ohneOnline ? "aktiv" : ""} onClick={() => setOhneOnline(false)}>
                    Laden und verarbeiten
                  </button>
                  <button className={ohneOnline ? "aktiv" : ""} onClick={() => setOhneOnline(true)}>
                    Diesmal auslassen
                  </button>
                </span>
              </div>
            )}
            {zuEntfernen > 0 && (
              <div className="ablauf__online ablauf__entfernen">
                <span>
                  {alsZahl(zuEntfernen)} {zuEntfernen === 1 ? "Eintrag fällt" : "Einträge fallen"} aus dem Register
                  oder verlieren Text und Vorschau. Die Originale bleiben unberührt.
                </span>
                <span className="umschalter umschalter--text">
                  <button className={!ohneEntfernen ? "aktiv" : ""} onClick={() => setOhneEntfernen(false)}>
                    Entfernen
                  </button>
                  <button className={ohneEntfernen ? "aktiv" : ""} onClick={() => setOhneEntfernen(true)}>
                    Diesmal behalten
                  </button>
                </span>
              </div>
            )}
            <Baum
              ohneOnline={ohneOnline}
              ohneEntfernen={ohneEntfernen}
              ordner={delta!.uebersicht.ordner}
              aus={aus}
              setAus={setAus}
              cache={dateienCache.current}
              zeigen={(filter, ordner, titel) => setListe({ filter, ordner, titel })}
            />
            <div className="ablauf__fuss">
              <span className="leise">
                {alsZahl(gewaehlt.dateien)} von {alsZahl(u.dateien)} Dateien gewählt
                {gewaehlt.online > 0 && <>, {alsZahl(gewaehlt.online)} werden aus der Cloud geladen</>}
                {delta!.nur_namen > 0 && <> · dazu {alsZahl(delta!.nur_namen)} neue nur mit Namen</>}
                {gewaehlt.entfernen > 0 && <> · {alsZahl(gewaehlt.entfernen)} werden entfernt</>}
                {" · Zahlen anklicken zeigt die Dateien"}
              </span>
              <button className="knopf" onClick={verarbeitenKlick} disabled={!etwasGewaehlt || prueft}>
                {gewaehlt.entfernen > 0 ? `Verarbeiten und ${alsZahl(gewaehlt.entfernen)} entfernen` : "Verarbeiten"}
              </button>
            </div>
          </>
        )}

        {(laeuft || ergebnis) && u && (
          <Schritte u={u} meldung={meldung} fertig={fertig} laeuft={laeuft} ergebnis={ergebnis} />
        )}
        {laeuft && (
          <div className="ablauf__fuss">
            <span className="leise">Angehalten wird nach der laufenden Datei. Ein neuer Lauf macht dort weiter.</span>
            <button
              className="knopf knopf--leise"
              disabled={haelt}
              onClick={() => {
                setHaelt(true);
                void inhalteAbbrechen();
              }}
            >
              {haelt ? "Hält an …" : "Anhalten"}
            </button>
          </div>
        )}
        {ergebnis && !laeuft && <ErgebnisZeile e={ergebnis} />}
      </section>

      {lauf && lauf.vermisst.length > 0 && !laeuft && (
        <section className="karte">
          <VermisstListe ordner={lauf.vermisst} />
        </section>
      )}

      <details className="ablauf__einzelheiten">
        <summary>Einzelheiten und einzelne Schritte</summary>
        <Verwaltung eingebettet />
      </details>
    </div>
  );
}

function Zahl({
  wert,
  titel,
  leise,
  warnung,
  zeigen,
}: {
  wert: number;
  titel: string;
  leise?: boolean;
  warnung?: boolean;
  zeigen?: () => void;
}) {
  const klasse = `ablauf__zahl ${leise ? "leise" : ""} ${warnung ? "warnung" : ""} ${wert === 0 ? "null" : ""}`;
  const inhalt = (
    <>
      <b>{alsZahl(wert)}</b>
      <span>{titel}</span>
    </>
  );
  return zeigen && wert > 0 ? (
    <button className={`${klasse} ablauf__zahl--knopf`} onClick={zeigen} title="Dateien zeigen">
      {inhalt}
    </button>
  ) : (
    <div className={klasse}>{inhalt}</div>
  );
}

/** Fenster mit den Dateinamen einer Zahl, nach Ordner gruppiert. */
function DateiFenster({
  filter,
  ordner,
  titel,
  onSchliessen,
}: {
  filter: DeltaFilter;
  ordner: string | null;
  titel: string;
  onSchliessen: () => void;
}) {
  const [daten, setDaten] = useState<[DeltaDatei[], number] | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  useEffect(() => {
    deltaListe(filter, ordner)
      .then(setDaten)
      .catch((e) => setFehler(String(e)));
  }, [filter, ordner]);
  useEffect(() => {
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onSchliessen();
      }
    };
    window.addEventListener("keydown", taste, true);
    return () => window.removeEventListener("keydown", taste, true);
  }, [onSchliessen]);

  const gruppen = useMemo(() => {
    const m = new Map<string, DeltaDatei[]>();
    daten?.[0].forEach((d) => {
      const o = eltern(d.pfad);
      m.set(o, [...(m.get(o) ?? []), d]);
    });
    return [...m.entries()];
  }, [daten]);

  return (
    <div className="dialog__grund" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && onSchliessen()}>
      <div className="dialog dateifenster" role="dialog" aria-modal="true" aria-labelledby="dateifenster-titel">
        <h2 id="dateifenster-titel">
          {titel}
          {daten && <span className="leise"> · {alsZahl(daten[1])} {daten[1] === 1 ? "Datei" : "Dateien"}</span>}
        </h2>
        {ordner && <p className="pfad leise">in {ordner}</p>}
        {fehler && <p className="achtung">{fehler}</p>}
        {!daten && !fehler && <p className="leise">lädt …</p>}
        <div className="dateifenster__liste">
          {gruppen.map(([o, dateien]) => (
            <div key={o} className="dateifenster__gruppe">
              <div className="pfad leise">{o}</div>
              {dateien.map((d) => (
                <div key={d.pfad} className="dateifenster__datei">
                  <span>{name(d.pfad)}</span>
                  <span className="baum__zahlen">
                    {filter !== "namen" && d.art === "neu" && <span className="baum__marke baum__marke--neu">neu</span>}
                    {d.art === "geaendert" && <span className="baum__marke">geändert</span>}
                    {d.art === "offen" && <span className="baum__marke baum__marke--leise">offen</span>}
                    {d.art === "entfernen" && <EntfernenMarke grund={d.grund} />}
                    {d.nur_online && <span className="baum__marke baum__marke--online">☁ nur online</span>}
                  </span>
                </div>
              ))}
            </div>
          ))}
        </div>
        {daten && daten[1] > daten[0].length && (
          <p className="leise">Die ersten {alsZahl(daten[0].length)} von {alsZahl(daten[1])} Dateien.</p>
        )}
        <div className="dialog__knoepfe">
          <button className="knopf" onClick={onSchliessen} autoFocus>
            Schließen
          </button>
        </div>
      </div>
    </div>
  );
}

/* ---------- Baum ---------- */

function eltern(p: string) {
  const i = p.lastIndexOf("/");
  return i < 0 ? "" : p.slice(0, i);
}

function name(p: string) {
  return p.slice(p.lastIndexOf("/") + 1);
}

type BaumProps = {
  ordner: OrdnerZahl[];
  aus: Set<string>;
  setAus: (s: Set<string>) => void;
  cache: Map<string, DeltaDatei>;
  zeigen: Zeigen;
  ohneOnline: boolean;
  ohneEntfernen: boolean;
};

type Zeigen = (filter: DeltaFilter, ordner: string | null, titel: string) => void;

function Baum({ ordner, aus, setAus, cache, zeigen, ohneOnline, ohneEntfernen }: BaumProps) {
  const kinder = useMemo(() => {
    const m = new Map<string, OrdnerZahl[]>();
    ordner.forEach((o) => {
      const e = eltern(o.pfad);
      m.set(e, [...(m.get(e) ?? []), o]);
    });
    return m;
  }, [ordner]);
  const [offen, setOffen] = useState<Set<string>>(() => {
    // Einzelne Wurzel und kleine Bäume gleich aufgeklappt
    const s = new Set<string>();
    let ebene = kinder.get("") ?? [];
    while (ebene.length === 1) {
      s.add(ebene[0].pfad);
      ebene = kinder.get(ebene[0].pfad) ?? [];
    }
    return s;
  });

  const umschalten = (pfad: string) => {
    const n = new Set(aus);
    if (n.has(pfad)) {
      n.delete(pfad);
    } else {
      // Ausgeschlossene darunter gehen im Ordner auf
      [...n].forEach((p) => p.startsWith(pfad + "/") && n.delete(p));
      n.add(pfad);
    }
    setAus(n);
  };

  return (
    <div className="baum" role="tree">
      {(kinder.get("") ?? []).map((o) => (
        <Knoten
          key={o.pfad}
          o={o}
          tiefe={0}
          kinder={kinder}
          offen={offen}
          setOffen={setOffen}
          aus={aus}
          umschalten={umschalten}
          vorfahrAus={false}
          cache={cache}
          zeigen={zeigen}
          ohneOnline={ohneOnline}
          ohneEntfernen={ohneEntfernen}
        />
      ))}
    </div>
  );
}

type KnotenProps = {
  o: OrdnerZahl;
  tiefe: number;
  kinder: Map<string, OrdnerZahl[]>;
  offen: Set<string>;
  setOffen: (s: Set<string>) => void;
  aus: Set<string>;
  umschalten: (p: string) => void;
  vorfahrAus: boolean;
  cache: Map<string, DeltaDatei>;
  zeigen: Zeigen;
  ohneOnline: boolean;
  ohneEntfernen: boolean;
};

function Knoten({
  o,
  tiefe,
  kinder,
  offen,
  setOffen,
  aus,
  umschalten,
  vorfahrAus,
  cache,
  zeigen,
  ohneOnline,
  ohneEntfernen,
}: KnotenProps) {
  const auf = offen.has(o.pfad);
  const [dateien, setDateien] = useState<DeltaDatei[] | null>(null);
  const selbstAus = aus.has(o.pfad);
  const gewaehlt = !vorfahrAus && !selbstAus;
  // Ordner nur mit Dateien nur mit Namen: kommen immer ins Register, nichts zu wählen (v0.1.1)
  const nurName = o.dateien === 0 && o.entfernen === 0;
  // Ohne „nur online“ ist ein Ordner nur zum Teil gewählt, ganz aus nur online gar nicht
  const alleOnline = ohneOnline && o.nur_online === o.dateien;
  const teils =
    gewaehlt && !alleOnline && ([...aus].some((p) => p.startsWith(o.pfad + "/")) || (ohneOnline && o.nur_online > 0));

  useEffect(() => {
    if (auf && dateien === null) {
      deltaDateien(o.pfad)
        .then((d) => {
          d.forEach((x) => cache.set(x.pfad, x));
          setDateien(d);
        })
        .catch(() => setDateien([]));
    }
  }, [auf, dateien, o.pfad, cache]);

  const klappen = () => {
    const n = new Set(offen);
    if (auf) n.delete(o.pfad);
    else n.add(o.pfad);
    setOffen(n);
  };

  return (
    <div role="treeitem" aria-expanded={auf}>
      <div className="baum__zeile" style={{ paddingLeft: 4 + tiefe * 20 }}>
        <button className="baum__pfeil" onClick={klappen} aria-label={auf ? "Zuklappen" : "Aufklappen"}>
          {auf ? "▾" : "▸"}
        </button>
        {nurName ? (
          <Haken an fest onClick={() => {}} />
        ) : (
          <Haken an={gewaehlt && !alleOnline} teils={teils} gesperrt={vorfahrAus} onClick={() => umschalten(o.pfad)} />
        )}
        <span className="baum__name" onClick={klappen}>
          {name(o.pfad)}
        </span>
        <span className="baum__zahlen">
          {o.neu > 0 && (
            <button className="baum__marke baum__marke--neu" onClick={() => zeigen("neu", o.pfad, "Neu")}>
              {alsZahl(o.neu)} neu
            </button>
          )}
          {o.geaendert > 0 && (
            <button className="baum__marke" onClick={() => zeigen("geaendert", o.pfad, "Geändert")}>
              {alsZahl(o.geaendert)} geändert
            </button>
          )}
          {o.nur_online > 0 && (
            <button
              className="baum__marke baum__marke--online"
              onClick={() => zeigen("online", o.pfad, "Nur online")}
              title="nur online"
            >
              ☁ {alsZahl(o.nur_online)}
            </button>
          )}
          {o.entfernen > 0 && (
            <button
              className="baum__marke baum__marke--entfernen"
              onClick={() => zeigen("entfernen", o.pfad, "Zu entfernen")}
              title="Fallen aus dem Register oder verlieren Text und Vorschau. Die Originale bleiben."
            >
              {alsZahl(o.entfernen)} entfernen
            </button>
          )}
          {o.nur_namen > 0 && (
            <button
              className="baum__marke baum__marke--name"
              onClick={() => zeigen("namen", o.pfad, "Neu, nur mit Namen")}
              title="Kommen nur mit Name, Größe und Datum ins Register, ohne Inhalt"
            >
              {alsZahl(o.nur_namen)} nur Name
            </button>
          )}
          <button className="baum__anzahl" onClick={() => zeigen("alle", o.pfad, "Alle Dateien")} title="Dateien zeigen">
            {o.dateien > 0 ? alsZahl(o.dateien) : ""}
          </button>
        </span>
      </div>
      {auf && (
        <>
          {(kinder.get(o.pfad) ?? []).map((k) => (
            <Knoten
              key={k.pfad}
              o={k}
              tiefe={tiefe + 1}
              kinder={kinder}
              offen={offen}
              setOffen={setOffen}
              aus={aus}
              umschalten={umschalten}
              vorfahrAus={vorfahrAus || selbstAus}
              cache={cache}
              zeigen={zeigen}
              ohneOnline={ohneOnline}
              ohneEntfernen={ohneEntfernen}
            />
          ))}
          {dateien === null && (
            <div className="baum__zeile leise" style={{ paddingLeft: 44 + tiefe * 20 }}>
              lädt …
            </div>
          )}
          {dateien?.map((d) => (
            <div key={d.pfad} className="baum__zeile baum__datei" style={{ paddingLeft: 26 + (tiefe + 1) * 20 }}>
              {d.art === "entfernen" ? (
                // Zu entfernen (v0.1.1): angehakt wird entfernt, abgewählt bleibt im Register
                <Haken
                  an={gewaehlt && !aus.has(d.pfad) && !ohneEntfernen}
                  gesperrt={!gewaehlt || ohneEntfernen}
                  onClick={() => umschalten(d.pfad)}
                />
              ) : d.schritte === 0 ? (
                // Nur mit Namen (v0.1.1): kommt immer ins Register, fester grauer Haken
                <Haken an fest onClick={() => {}} />
              ) : (
                <Haken
                  an={gewaehlt && !aus.has(d.pfad) && !(ohneOnline && d.nur_online)}
                  gesperrt={!gewaehlt || (ohneOnline && d.nur_online)}
                  onClick={() => umschalten(d.pfad)}
                />
              )}
              <span className="baum__name" title={d.pfad}>
                {name(d.pfad)}
              </span>
              <span className="baum__zahlen">
                {d.art === "entfernen" ? (
                  <EntfernenMarke grund={d.grund} />
                ) : d.schritte === 0 ? (
                  <span
                    className="baum__marke baum__marke--name"
                    title="Kommt nur mit Name, Größe und Datum ins Register, ohne Inhalt"
                  >
                    neu · nur Name
                  </span>
                ) : (
                  d.art === "neu" && <span className="baum__marke baum__marke--neu">neu</span>
                )}
                {d.art === "geaendert" && <span className="baum__marke">geändert</span>}
                {d.art === "offen" && <span className="baum__marke baum__marke--leise">offen</span>}
                {d.nur_online && d.schritte > 0 && (
                  <span className="baum__marke baum__marke--online">☁ nur online</span>
                )}
              </span>
            </div>
          ))}
        </>
      )}
    </div>
  );
}

function Haken({
  an,
  teils,
  gesperrt,
  fest,
  onClick,
}: {
  an: boolean;
  teils?: boolean;
  gesperrt?: boolean;
  /** Fester grauer Haken: kommt ins Register, nicht abwählbar (v0.1.1) */
  fest?: boolean;
  onClick: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = !!teils;
  }, [teils]);
  return (
    <input
      ref={ref}
      type="checkbox"
      className={`baum__haken${fest ? " baum__haken--fest" : ""}`}
      checked={an}
      disabled={gesperrt || fest}
      onChange={onClick}
      title={fest ? "Kommt nur mit Namen ins Register. Nichts zu wählen." : undefined}
    />
  );
}

/** Kurzer Grund für die Marke im Baum (v0.1.1), der lange steht im Tooltip. */
const KURZ: Record<Grund, string> = {
  fehlt: "gibt es nicht mehr",
  typ: "Typ ignoriert",
  bibliothek: "Bibliothek",
  ausgeschlossen: "Ordner aus",
  ohne_regel: "ohne Regel",
  nur_metadaten: "nur noch Name",
};

function EntfernenMarke({ grund }: { grund?: Grund }) {
  const lang = GRUENDE.find((g) => g.grund === grund)?.titel;
  return (
    <span className="baum__marke baum__marke--entfernen" title={lang}>
      entfernen{grund ? ` · ${KURZ[grund]}` : ""}
    </span>
  );
}

/** Rückfrage vor dem Entfernen vieler Einträge (v0.1.1). */
function EntfernenFrage({
  anzahl,
  zeigen,
  onAbbrechen,
  onBestaetigen,
}: {
  anzahl: number;
  zeigen: () => void;
  onAbbrechen: () => void;
  onBestaetigen: () => void;
}) {
  return (
    <div className="dialog__grund" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && onAbbrechen()}>
      <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="entfernen-frage-titel">
        <h2 id="entfernen-frage-titel">{alsZahl(anzahl)} Einträge aus dem Register entfernen?</h2>
        <p>
          Sie fallen aus dem Register oder verlieren Text und Vorschau. Die Originale bleiben unberührt. Ein
          Neuaufbau dauert bei vielen Dateien lange.
        </p>
        <p>
          <button className="knopf knopf--leise" onClick={zeigen}>
            Dateien zeigen
          </button>
        </p>
        <div className="dialog__knoepfe">
          <button className="knopf knopf--leise" onClick={onAbbrechen} autoFocus>
            Abbrechen
          </button>
          <button className="knopf knopf--gefahr" onClick={onBestaetigen}>
            Verarbeiten und {alsZahl(anzahl)} entfernen
          </button>
        </div>
      </div>
    </div>
  );
}

/* ---------- Schritte ---------- */

const SCHRITTE: { s: Schritt; titel: string }[] = [
  { s: "aufnehmen", titel: "Dateibaum aufnehmen und entfernen" },
  { s: "laden", titel: "Aus der Cloud laden" },
  { s: "inhalte", titel: "Inhalte lesen" },
  { s: "vorschauen", titel: "Vorschauen erzeugen" },
  { s: "erkennen", titel: "Text erkennen" },
  { s: "textebene", titel: "In PDFs schreiben" },
  { s: "auslagern", titel: "Wieder „nur online“" },
  { s: "suche", titel: "Suchindex nachführen" },
];

function Schritte({
  meldung,
  fertig,
  laeuft,
  ergebnis,
}: {
  u: DeltaAntwort["uebersicht"];
  meldung: Meldung | null;
  fertig: Set<Schritt>;
  laeuft: boolean;
  ergebnis: AblaufErgebnis | null;
}) {
  const zahl = (s: Schritt): string => {
    if (!ergebnis) return "";
    const e = ergebnis;
    switch (s) {
      case "aufnehmen":
        return (
          `${alsZahl(e.aufgenommen.neu)} neu, ${alsZahl(e.aufgenommen.geaendert)} geändert` +
          (e.aufgenommen.entfernt + e.aufgenommen.reduziert > 0
            ? `, ${alsZahl(e.aufgenommen.entfernt + e.aufgenommen.reduziert)} entfernt`
            : "") +
          (e.aufgenommen.behalten > 0 ? `, ${alsZahl(e.aufgenommen.behalten)} behalten` : "")
        );
      case "laden":
        return `${alsZahl(e.laden.geladen)} geladen` + (e.laden.nicht_geladen_anzahl ? `, ${alsZahl(e.laden.nicht_geladen_anzahl)} nicht` : "");
      case "inhalte":
        return `${alsZahl(e.inhalte.gelesen)} gelesen` + (e.inhalte.fehler ? `, ${alsZahl(e.inhalte.fehler)} Fehler` : "");
      case "vorschauen":
        return `${alsZahl(e.vorschauen.seiten)} Seiten` + (e.vorschauen.fehler ? `, ${alsZahl(e.vorschauen.fehler)} Fehler` : "");
      case "erkennen":
        return `${alsZahl(e.ocr.seiten)} Seiten` + (e.ocr.fehler ? `, ${alsZahl(e.ocr.fehler)} Fehler` : "");
      case "textebene":
        return `${alsZahl(e.textebene.geschrieben)} PDFs` + (e.textebene.fehler ? `, ${alsZahl(e.textebene.fehler)} Fehler` : "");
      case "auslagern":
        if (e.auslagern.werkzeug_fehlt) return "übersprungen, onlineonly fehlt";
        return (
          `${alsZahl(e.auslagern.ausgelagert)} ${e.auslagern.ausgelagert === 1 ? "Datei" : "Dateien"}` +
          (e.auslagern.nicht_anzahl ? `, ${alsZahl(e.auslagern.nicht_anzahl)} bleiben lokal` : "")
        );
      default:
        return "";
    }
  };
  return (
    <ol className="schritte">
      {SCHRITTE.map(({ s, titel }) => {
        const aktiv = laeuft && meldung?.schritt === s;
        const erledigt = fertig.has(s) || (!laeuft && !!ergebnis && !ergebnis.abgebrochen);
        const anteil = aktiv && meldung && meldung.gesamt > 0 ? (meldung.erledigt / meldung.gesamt) * 100 : 0;
        return (
          <li key={s} className={`schritte__zeile ${aktiv ? "aktiv" : ""} ${erledigt ? "fertig" : ""}`}>
            <span className="schritte__zeichen">{erledigt ? "✓" : aktiv ? "●" : "○"}</span>
            <span className="schritte__titel">{titel}</span>
            {aktiv && meldung ? (
              <span className="schritte__lauf">
                <span className="fortschritt-balken">
                  <span style={{ width: `${anteil}%` }} />
                </span>
                <span className="leise">
                  {s === "laden"
                    ? `${alsZahl(meldung.erledigt)} von ${alsZahl(meldung.gesamt)} da`
                    : meldung.gesamt > 1
                      ? `${alsZahl(meldung.erledigt)} von ${alsZahl(meldung.gesamt)}`
                      : "…"}
                </span>
                {meldung.datei && (
                  <span className="schritte__datei leise" title={meldung.datei}>
                    {s === "laden" ? "wartet auf die Cloud: " : ""}
                    {name(meldung.datei)}
                  </span>
                )}
                {s === "laden" && <NichtWarten />}
              </span>
            ) : (
              <span className="leise">{zahl(s)}</span>
            )}
          </li>
        );
      })}
    </ol>
  );
}

/** Während des Ladens: mit den vorhandenen Dateien weitermachen, die übrigen bleiben offen. */
function NichtWarten() {
  const [geklickt, setGeklickt] = useState(false);
  return (
    <button
      className="knopf knopf--leise schritte__knopf"
      disabled={geklickt}
      title="Mit den vorhandenen Dateien weitermachen. Die übrigen bleiben offen für den nächsten Lauf."
      onClick={() => {
        setGeklickt(true);
        void ladenUeberspringen();
      }}
    >
      {geklickt ? "Geht weiter …" : "Nicht warten, weiter"}
    </button>
  );
}

function ErgebnisZeile({ e }: { e: AblaufErgebnis }) {
  return (
    <div className="ablauf__ergebnis">
      <p>
        {e.abgebrochen ? "Angehalten" : "Fertig"} nach {alsDauer(e.dauer_ms)}: {alsZahl(e.ausgewaehlt)} Dateien
        verarbeitet.
      </p>
      {e.laden.nicht_geladen_anzahl > 0 && (
        <details>
          <summary className="achtung">
            {e.laden.nicht_geladen_anzahl === 1
              ? "Eine Datei kam nicht aus der Cloud"
              : `${alsZahl(e.laden.nicht_geladen_anzahl)} Dateien kamen nicht aus der Cloud`}{" "}
            {e.laden.uebersprungen
              ? "(nicht abgewartet)."
              : "(Warteschlange der Cloud, offline oder die Datei ist leer)."}{" "}
            {e.laden.nicht_geladen_anzahl === 1 ? "Sie bleibt" : "Sie bleiben"} offen für den nächsten Lauf.
          </summary>
          <ul className="pfad">
            {e.laden.nicht_geladen.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </details>
      )}
      {e.auslagern.werkzeug_fehlt && (
        <p className="achtung">
          {alsZahl(e.auslagern.nicht_anzahl)} geladene Dateien bleiben lokal. Zum Zurückstellen auf „nur online“
          das Werkzeug onlineonly installieren: <code>brew install zamua/tap/onlineonly</code>
        </p>
      )}
      {e.auslagern.meldung && <p className="achtung">{e.auslagern.meldung}</p>}
      {!e.auslagern.werkzeug_fehlt && e.auslagern.nicht_anzahl > 0 && (
        <details>
          <summary className="achtung">
            {alsZahl(e.auslagern.nicht_anzahl)} geladene Dateien bleiben lokal (die Dropbox hat sie nicht
            zurückgestellt)
          </summary>
          <ul className="pfad">
            {e.auslagern.nicht.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
