import type { EndungZeile, OrdnerPruefung, Stufe } from "../api";
import { alsDauer, alsGroesse, alsZahl } from "../format";
import { TYPGRUPPEN } from "../suche/Suchfeld";
import { NAMEN } from "./stufen";

interface Props {
  ordner: string;
  wirksam: { stufe: Stufe; eigene: boolean; von: string | null };
  pruefung: OrdnerPruefung | null;
  laeuft: boolean;
  /** Es läuft eine Prüfung, auch für einen anderen Ordner */
  gesperrt: boolean;
  onPruefen: () => void;
  onAbbrechen: () => void;
  onUnterordner: (name: string) => void;
}

/** Wie viele Endungen einer Klasse einzeln genannt werden */
const GENANNT = 14;

const endungName = (e: string) => (e === "" ? "ohne Endung" : `.${e}`);

/** Gelesene Formate nach den Gruppen der Suche, größte zuerst. */
function gruppen(endungen: EndungZeile[]): { name: string; anzahl: number; bytes: number }[] {
  const gelesen = endungen.filter((e) => e.klasse === "gelesen");
  return TYPGRUPPEN.map((g) => {
    const teil = gelesen.filter((e) => g.endungen.includes(e.endung));
    return { name: g.name, anzahl: teil.reduce((s, e) => s + e.anzahl, 0), bytes: teil.reduce((s, e) => s + e.bytes, 0) };
  })
    .filter((g) => g.anzahl > 0)
    .sort((a, b) => b.anzahl - a.anzahl);
}

function EndungListe({ zeilen }: { zeilen: EndungZeile[] }) {
  const rest = zeilen.slice(GENANNT).reduce((s, e) => s + e.anzahl, 0);
  return (
    <p className="endungen">
      {zeilen.slice(0, GENANNT).map((e) => (
        <span key={e.endung} className="endungen__eintrag">
          {endungName(e.endung)} <b>{alsZahl(e.anzahl)}</b>
        </span>
      ))}
      {rest > 0 && (
        <span className="endungen__eintrag leise">
          {zeilen.length - GENANNT} weitere <b>{alsZahl(rest)}</b>
        </span>
      )}
    </p>
  );
}

/**
 * Prüfung eines Ordners (v0.0.7): wie viele Dateien welchen Typs darin liegen und was davon ins
 * Register käme. Hilft vor dem Einlesen, die Regeln zu wählen.
 */
export default function Pruefung({ ordner, wirksam, pruefung: p, laeuft, gesperrt, onPruefen, onAbbrechen, onUnterordner }: Props) {
  const nurNamen = wirksam.stufe === "metadaten";
  // Seit v0.1.5 kommt jede Datei wenigstens mit Namen ins Register
  const insRegister = p ? p.dateien : 0;
  const nurName = p?.endungen.filter((e) => e.klasse === "nur_name") ?? [];

  return (
    <section className="regel__karte pruefung">
      <div className="pruefung__kopf">
        <div>
          <h3>{ordner.split("/").pop()}</h3>
          <p className="pfad leise">{ordner}</p>
        </div>
        {laeuft ? (
          <button className="knopf knopf--leise" onClick={onAbbrechen}>
            Abbrechen
          </button>
        ) : (
          <button className="knopf" onClick={onPruefen} disabled={gesperrt}>
            {p ? "Erneut prüfen" : "Ordner prüfen"}
          </button>
        )}
      </div>
      <p className="leise">
        Stufe: <b>{NAMEN[wirksam.stufe]}</b>
        {wirksam.eigene
          ? " (eigene Regel)"
          : wirksam.von !== null
            ? ` (geerbt von „${wirksam.von}“)`
            : " (keine Regel, wird nicht gelesen)"}
      </p>

      {laeuft && <p className="leise">Zählt die Dateien …</p>}
      {!p && !laeuft && (
        <p className="leise">Die Prüfung liest nur die Verzeichnisse. Sie lädt nichts herunter und ändert nichts.</p>
      )}

      {p && !laeuft && (
        <>
          {p.abgebrochen && <p className="achtung">Abgebrochen. Die Zahlen sind unvollständig.</p>}
          <div className="pruefung__zahlen">
            <div>
              <span className="pruefung__wert">{alsZahl(insRegister)}</span>
              <span className="leise">kämen ins Register{wirksam.stufe === "ausgeschlossen" ? " (bei Stufe Voll)" : ""}</span>
            </div>
            <div>
              <span className="pruefung__wert">{alsZahl(nurNamen ? p.dateien : p.nur_name)}</span>
              <span className="leise">nur mit Namen</span>
            </div>
            <div>
              <span className="pruefung__wert">{alsZahl(p.nur_online)}</span>
              <span className="leise">nur online</span>
            </div>
            <div>
              <span className="pruefung__wert">{alsGroesse(p.bytes)}</span>
              <span className="leise">lokal vorhanden</span>
            </div>
          </div>
          {nurNamen && (
            <p className="leise">In der Stufe „Nur Namen“ kommt jede Datei mit Name und Datum ins Register, ohne Inhalt.</p>
          )}

          {p.gelesen > 0 && (
            <>
              <h4>Mit Text und Vorschau ({alsZahl(p.gelesen)})</h4>
              <table className="pruefung__tabelle">
                <tbody>
                  {gruppen(p.endungen).map((g) => (
                    <tr key={g.name}>
                      <td>{g.name}</td>
                      <td className="zahl">{alsZahl(g.anzahl)}</td>
                      <td className="zahl leise">{alsGroesse(g.bytes)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
          {nurName.length > 0 && (
            <>
              <h4>Nur mit Namen ({alsZahl(p.nur_name)})</h4>
              <EndungListe zeilen={nurName} />
            </>
          )}

          {p.unterordner.length > 0 && (
            <>
              <h4>Unterordner</h4>
              <table className="pruefung__tabelle">
                <thead>
                  <tr>
                    <th />
                    <th className="zahl">ins Register</th>
                    <th className="zahl">lokal</th>
                    <th className="zahl">nur online</th>
                  </tr>
                </thead>
                <tbody>
                  {p.unterordner.map((u) => (
                    <tr key={u.name}>
                      <td>
                        {u.name === "" ? (
                          <span className="leise">direkt im Ordner</span>
                        ) : (
                          <button className="pruefung__ordner" onClick={() => onUnterordner(u.name)} disabled={gesperrt} title="Diesen Unterordner prüfen">
                            {u.name}
                          </button>
                        )}
                      </td>
                      <td className="zahl">{alsZahl(u.gelesen + u.nur_name)}</td>
                      <td className="zahl leise">{alsGroesse(u.bytes)}</td>
                      <td className="zahl leise">{alsZahl(u.nur_online)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}

          {p.bibliotheken_anzahl > 0 && (
            <details className="pruefung__bibliotheken">
              <summary>
                {alsZahl(p.bibliotheken_anzahl)} Ordner fremder Bibliotheken übergangen (node_modules, target, .git …)
              </summary>
              <ul>
                {p.bibliotheken.map((b) => (
                  <li key={b} className="pfad">
                    {b}
                  </li>
                ))}
                {p.bibliotheken_anzahl > p.bibliotheken.length && (
                  <li className="leise">und {alsZahl(p.bibliotheken_anzahl - p.bibliotheken.length)} weitere</li>
                )}
              </ul>
            </details>
          )}
          {p.nicht_lesbar > 0 && <p className="achtung">{alsZahl(p.nicht_lesbar)} Einträge waren nicht lesbar.</p>}
          <p className="leise pruefung__fuss">
            {alsZahl(p.dateien)} Dateien gezählt in {alsDauer(p.dauer_ms)}. Die Größe von „nur online“-Dateien ist
            nicht bekannt.
          </p>
        </>
      )}
    </section>
  );
}
