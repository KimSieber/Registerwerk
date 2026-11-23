import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Betroffen,
  Grund,
  LaufErgebnis,
  OrdnerBestand,
  astHinzufuegen,
  dateibaumEinlesen,
  geaendertMelden,
  ordnerEntfernen,
  ordnerWaehlen,
} from "./api";
import { alsDauer, alsZahl, alsZeitpunkt } from "./format";

/**
 * Dateibaum einlesen, mit Rückfrage (v0.0.7): Würde der Abgleich Einträge entfernen, ändert der
 * erste Lauf nichts und meldet nur, was wegfiele. Erst nach der Bestätigung läuft er wirklich.
 * Fehlen allein löscht nie.
 */
export function useEinlesen() {
  const [laeuft, setLaeuft] = useState(false);
  const [lauf, setLauf] = useState<LaufErgebnis | null>(null);
  const [frage, setFrage] = useState<LaufErgebnis | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);

  const starten = useCallback(async (bestaetigt: boolean) => {
    setLaeuft(true);
    setFehler(null);
    setFrage(null);
    try {
      const e = await dateibaumEinlesen(bestaetigt);
      if (e.probe) {
        setFrage(e);
      } else {
        setLauf(e);
        geaendertMelden();
      }
    } catch (e) {
      setFehler(String(e));
    } finally {
      setLaeuft(false);
    }
  }, []);

  const einlesen = useCallback(() => starten(false), [starten]);
  const rueckfrage = frage && (
    <Rueckfrage lauf={frage} onAbbrechen={() => setFrage(null)} onBestaetigen={() => void starten(true)} />
  );
  return { laeuft, lauf, fehler, einlesen, rueckfrage };
}

function Rueckfrage({
  lauf,
  onAbbrechen,
  onBestaetigen,
}: {
  lauf: LaufErgebnis;
  onAbbrechen: () => void;
  onBestaetigen: () => void;
}) {
  // Esc bricht ab
  useEffect(() => {
    const taste = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onAbbrechen();
      }
    };
    window.addEventListener("keydown", taste, true);
    return () => window.removeEventListener("keydown", taste, true);
  }, [onAbbrechen]);

  const geloescht = lauf.entfernt - lauf.entfernt_regel;
  return (
    <div className="dialog__grund" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && onAbbrechen()}>
      <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="rueckfrage-titel">
        <h2 id="rueckfrage-titel">Einträge aus dem Register entfernen?</h2>
        <p>Der Abgleich mit dem Dateibaum würde das Register so ändern:</p>
        <ul className="dialog__liste">
          {geloescht > 0 && (
            <li>
              <b>{alsZahl(geloescht)} Dateien</b> gibt es in ihrem Ordner nicht mehr. Sie werden entfernt.
            </li>
          )}
          {lauf.entfernt_regel > 0 && (
            <li>
              <b>{alsZahl(lauf.entfernt_regel)} Dateien</b> gehören nach den Ordner-Regeln nicht mehr ins Register
              {lauf.entfernt_regel_seiten > 0 && <>, mit Text und Vorschau von {alsZahl(lauf.entfernt_regel_seiten)} Seiten</>}
              .
            </li>
          )}
          {lauf.reduziert > 0 && (
            <li>
              <b>{alsZahl(lauf.reduziert)} Dateien</b> bleiben nur mit Namen
              {lauf.reduziert_seiten > 0 && <>, Text und Vorschau von {alsZahl(lauf.reduziert_seiten)} Seiten entfallen</>}
              .
            </li>
          )}
        </ul>
        <BetroffenListe dateien={lauf.betroffen} gesamt={lauf.entfernt + lauf.reduziert} />
        <p className="leise">
          Die Originale bleiben unberührt. Text, Vorschauen und OCR entstehen erst wieder, wenn die Dateien erneut
          ins Register kommen und gelesen werden.
        </p>
        <p className="leise">
          Außerdem: {alsZahl(lauf.neu)} neu, {alsZahl(lauf.geaendert)} geändert.
        </p>
        <div className="dialog__knoepfe">
          <button className="knopf knopf--leise" onClick={onAbbrechen} autoFocus>
            Abbrechen
          </button>
          <button className="knopf knopf--gefahr" onClick={onBestaetigen}>
            Entfernen und einlesen
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Ordner, die dieses Gerät früher gelesen hat und jetzt nicht findet: verschoben, gelöscht oder nicht
 * eingehängt. Ihr Bestand bleibt im Register, bis hier entschieden wird.
 */
export function VermisstListe({ ordner }: { ordner: OrdnerBestand[] }) {
  const [erledigt, setErledigt] = useState<Map<string, string>>(new Map());
  const [frage, setFrage] = useState<string | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  /** Ordner, der gerade entfernt wird. Solange sind alle Knöpfe gesperrt (v0.0.9). */
  const [arbeitet, setArbeitet] = useState<string | null>(null);
  if (ordner.length === 0) return null;

  const fertig = (o: string, text: string) => {
    setErledigt((e) => new Map(e).set(o, text));
    setFrage(null);
    geaendertMelden();
  };
  const entfernen = async (o: OrdnerBestand) => {
    setArbeitet(o.ordner);
    setFehler(null);
    try {
      const n = await ordnerEntfernen(o.ordner);
      fertig(o.ordner, `${alsZahl(n)} Einträge entfernt, Regel gelöscht.`);
    } catch (e) {
      setFehler(String(e));
    } finally {
      setArbeitet(null);
    }
  };
  const zuweisen = async (o: OrdnerBestand) => {
    setFehler(null);
    try {
      const ort = await ordnerWaehlen();
      if (ort === null) return;
      await astHinzufuegen(ort, o.ordner, true);
      fertig(o.ordner, "Neuer Ort zugewiesen. Bitte den Dateibaum erneut einlesen.");
    } catch (e) {
      setFehler(String(e));
    }
  };

  return (
    <div className="vermisst">
      <p className="achtung">
        {ordner.length === 1 ? "Ein Ordner wurde" : `${alsZahl(ordner.length)} Ordner wurden`} auf diesem Gerät nicht
        mehr gefunden: verschoben, gelöscht oder gerade nicht erreichbar. Im Register ist nichts entfernt.
      </p>
      {fehler && <p className="achtung">{fehler}</p>}
      <ul className="fehlende">
        {ordner.map((o) => (
          <li key={o.ordner}>
            <span className="pfad">{o.ordner}</span>
            <span className="leise">{alsZahl(o.dateien)} Dateien im Register</span>
            {erledigt.has(o.ordner) ? (
              <span className="leise">{erledigt.get(o.ordner)}</span>
            ) : arbeitet === o.ordner ? (
              <span className="leise arbeitet">
                <span className="kreisel" aria-hidden="true" /> Entfernt {alsZahl(o.dateien)} Einträge …
              </span>
            ) : frage === o.ordner ? (
              <>
                <button className="knopf knopf--gefahr" onClick={() => void entfernen(o)} disabled={arbeitet !== null}>
                  {alsZahl(o.dateien)} Einträge und die Regel entfernen
                </button>
                <button className="knopf knopf--leise" onClick={() => setFrage(null)} disabled={arbeitet !== null}>
                  Abbrechen
                </button>
              </>
            ) : (
              <>
                {!o.ordner.includes("/") && (
                  <button className="knopf knopf--leise" onClick={() => void zuweisen(o)} disabled={arbeitet !== null}>
                    Ort neu zuweisen …
                  </button>
                )}
                <button className="knopf knopf--leise" onClick={() => setFrage(o.ordner)} disabled={arbeitet !== null}>
                  Aus dem Register entfernen
                </button>
              </>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

const GRUENDE: { grund: Grund; titel: string }[] = [
  { grund: "fehlt", titel: "Datei gibt es nicht mehr (gelöscht, verschoben oder umbenannt)" },
  { grund: "typ", titel: "Dateityp wird ignoriert (kein Dokument, kein Bild)" },
  { grund: "bibliothek", titel: "Ordner fremder Bibliotheken" },
  { grund: "ausgeschlossen", titel: "Ordner ist ausgeschlossen" },
  { grund: "ohne_regel", titel: "Ordner hat keine Regel" },
  { grund: "nur_metadaten", titel: "Ordner steht auf „Nur Namen“: bleibt ohne Text und Vorschau" },
];

const endungVon = (pfad: string) => {
  const name = pfad.slice(pfad.lastIndexOf("/") + 1);
  const punkt = name.lastIndexOf(".");
  return punkt > 0 && punkt < name.length - 1 && !name.slice(punkt + 1).includes(" ")
    ? "." + name.slice(punkt + 1).toLowerCase()
    : "ohne Endung";
};

/**
 * Die Dateien, die nach den Ordnerregeln wegfallen, nach Grund und mit vollem Pfad. Damit lässt sich
 * vor der Bestätigung beurteilen, ob das Entfernen richtig ist.
 */
export function BetroffenListe({ dateien, gesamt }: { dateien: Betroffen[]; gesamt: number }) {
  const [kopiert, setKopiert] = useState(false);
  const gruppen = useMemo(
    () =>
      GRUENDE.map((g) => {
        const teil = dateien.filter((d) => d.grund === g.grund);
        const endungen = new Map<string, number>();
        for (const d of teil) endungen.set(endungVon(d.pfad), (endungen.get(endungVon(d.pfad)) ?? 0) + 1);
        return { ...g, teil, endungen: [...endungen].sort((a, b) => b[1] - a[1]) };
      }).filter((g) => g.teil.length > 0),
    [dateien],
  );
  if (dateien.length === 0) return null;

  const kopieren = async () => {
    const text = gruppen.map((g) => `${g.titel} (${g.teil.length})\n${g.teil.map((d) => d.pfad).join("\n")}`).join("\n\n");
    try {
      await navigator.clipboard.writeText(text);
      setKopiert(true);
      window.setTimeout(() => setKopiert(false), 2000);
    } catch {
      setKopiert(false);
    }
  };

  return (
    <div className="betroffen">
      <div className="betroffen__liste" tabIndex={0}>
        {gruppen.map((g) => (
          <section key={g.grund}>
            <h3>
              {g.titel} <span className="leise">({alsZahl(g.teil.length)})</span>
            </h3>
            {g.grund === "typ" && (
              <p className="betroffen__endungen">
                {g.endungen.map(([e, n]) => (
                  <span key={e}>
                    {e} <b>{alsZahl(n)}</b>
                  </span>
                ))}
              </p>
            )}
            <ul>
              {g.teil.map((d) => (
                <li key={d.pfad}>
                  <span className="betroffen__pfad">{d.pfad}</span>
                  {d.zuletzt !== null && (
                    <span className="betroffen__seiten" title="Zuletzt gesehen">
                      {alsZeitpunkt(d.zuletzt)}
                      {d.geraet && <> · {d.geraet}</>}
                    </span>
                  )}
                  {d.seiten > 0 && <span className="betroffen__seiten">{alsZahl(d.seiten)} S.</span>}
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
      <div className="betroffen__fuss">
        <span className="leise">
          {gesamt > dateien.length
            ? `Die ersten ${alsZahl(dateien.length)} von ${alsZahl(gesamt)} Dateien.`
            : `${alsZahl(dateien.length)} Dateien. Rechts: Seiten.`}
        </span>
        <button className="knopf knopf--leise" onClick={() => void kopieren()}>
          {kopiert ? "Kopiert" : "Liste kopieren"}
        </button>
      </div>
    </div>
  );
}

export function LaufMeldung({ lauf }: { lauf: LaufErgebnis }) {
  return (
    <div className="lauf">
      <p>
        Durchlauf {lauf.lauf} in {alsDauer(lauf.dauer_ms)}: {alsZahl(lauf.gesehen)} Dateien gesehen,{" "}
        <b>{alsZahl(lauf.neu)} neu</b>, <b>{alsZahl(lauf.geaendert)} geändert</b>,{" "}
        <b>{alsZahl(lauf.entfernt)} entfernt</b>, {alsZahl(lauf.unveraendert)} unverändert.
      </p>
      {(lauf.entfernt > 0 || lauf.reduziert > 0) && (
        <details className="stand-zeile lauf__betroffen">
          <summary>
            Entfernt: {alsZahl(lauf.entfernt - lauf.entfernt_regel)} nicht mehr vorhanden,{" "}
            {alsZahl(lauf.entfernt_regel)} nach den Ordner-Regeln. {alsZahl(lauf.reduziert)} nur noch mit Namen.
            Liste zeigen
          </summary>
          <BetroffenListe dateien={lauf.betroffen} gesamt={lauf.entfernt + lauf.reduziert} />
        </details>
      )}
      {(lauf.ignoriert > 0 || lauf.uebergangene_ordner > 0) && (
        <p className="stand-zeile">
          Nicht aufgenommen: {alsZahl(lauf.ignoriert)} Dateien anderer Typen (Quelltext, Programme, Daten)
          {lauf.uebergangene_ordner > 0 && <>, {alsZahl(lauf.uebergangene_ordner)} Ordner fremder Bibliotheken</>}.
        </p>
      )}
      {lauf.nur_online > 0 && (
        <p className="stand-zeile">
          {alsZahl(lauf.nur_online)} der gesehenen Dateien liegen nur online. Ihr Inhalt und ihre Vorschau entstehen
          erst, wenn sie in der Cloud offline verfügbar sind.
        </p>
      )}
      {lauf.fremd.length > 0 && (
        <details className="stand-zeile lauf__betroffen">
          <summary>
            {alsZahl(lauf.fremd.reduce((n, f) => n + f.dateien, 0))} Dateien in {alsZahl(lauf.fremd.length)} Ordnern
            liegen auf einem anderen Gerät und bleiben unberührt. Ordner zeigen
          </summary>
          <ul className="fehlende">
            {lauf.fremd.map((f) => (
              <li key={f.ordner}>
                <span className="pfad">{f.ordner}</span>
                <span className="leise">
                  {alsZahl(f.dateien)} Dateien{f.geraete.length > 0 && <> · gesehen auf {f.geraete.join(", ")}</>}
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
      <VermisstListe ordner={lauf.vermisst} />
      {lauf.nicht_lesbar > 0 && (
        <p className="achtung">
          {alsZahl(lauf.nicht_lesbar)} Einträge waren nicht lesbar ({lauf.unvollstaendige_verzeichnisse.join(", ")}).
          Für diese Ordner wurde nichts entfernt.
        </p>
      )}
    </div>
  );
}
