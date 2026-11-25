// Auswertung der Ausgaben von `npm test` und `npm run pruefen` (Build, cargo test,
// cargo llvm-cov) zu einer Zusammenfassung auf einen Blick. Nach dem Vorbild von
// Stellwerk. Reine Funktionen, getestet in auswertung.test.mjs. Das Starten der
// Befehle übernimmt pruefen.mjs.
//
// Anders als in Stellwerk gibt es keine Abdeckungsgrenze: Registerwerk hat keine
// Vorgabe von 100 %. Die Abdeckung wird gezeigt, macht die Prüfung aber nicht rot.

/** Farb- und Steuerzeichen (ANSI) entfernen, auch die Zeichensatz-Umschaltung der
 *  Rust-Testprogramme (`\x0f` unter Linux, `ESC ( B` auf dem Mac). */
export function ohneFarben(text) {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "").replace(/\x1b[()][A-Za-z0-9]/g, "").replace(/[\x0e\x0f]/g, "");
}

const prozent = (s) => (s === "-" ? null : Number(s.replace("%", "")));

/** Eine Zeile der Tabelle von llvm-cov: Name, dann je drei Spalten für Regionen,
 *  Funktionen, Zeilen (und Zweige): Anzahl, fehlend, Prozent. */
function tabellenZeile(zeile) {
  const t = zeile.trim().split(/\s+/);
  if (t.length < 10) return null;
  const zahl = (i) => Number(t[i]);
  if ([1, 2, 4, 5, 7, 8].some((i) => Number.isNaN(zahl(i)))) return null;
  return {
    name: t[0],
    regionen: [zahl(1), zahl(2)],
    funktionen: [zahl(4), zahl(5)],
    zeilen: [zahl(7), zahl(8)],
    prozent: { regionen: prozent(t[3]), funktionen: prozent(t[6]), zeilen: prozent(t[9]) },
  };
}

const quote = ([gesamt, fehlend]) => (gesamt === 0 ? null : Math.round(((gesamt - fehlend) / gesamt) * 1000) / 10);

/**
 * Ausgabe von cargo test oder cargo llvm-cov: Summe über alle Testprogramme,
 * fehlgeschlagene Tests, Abdeckung gesamt und je Paket.
 */
export function rustAuswerten(roh) {
  const text = ohneFarben(roh);
  let bestanden = 0;
  let fehler = 0;
  let ignoriert = 0;
  for (const m of text.matchAll(/test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored/g)) {
    bestanden += Number(m[1]);
    fehler += Number(m[2]);
    ignoriert += Number(m[3]);
  }
  const fehlgeschlagen = [...new Set([...text.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((m) => m[1]))];
  const kompilierfehler = /^error(\[E\d+\])?: /m.test(text) && bestanden + fehler === 0;

  let abdeckung = null;
  const pakete = new Map();
  const totalZeile = text.match(/^TOTAL\s+.+$/m)?.[0];
  if (totalZeile) {
    const t = tabellenZeile(totalZeile);
    abdeckung = t ? t.prozent : null;
    for (const z of text.split("\n")) {
      const zeile = tabellenZeile(z);
      if (!zeile || zeile.name === "TOTAL" || zeile.name === "Filename") continue;
      // Pfade relativ zum Workspace: crates/<paket>/…, sonst der erste Ordner
      const teile = zeile.name.split("/");
      const paket = teile[0] === "crates" && teile.length > 2 ? teile[1] : teile[0];
      const p = pakete.get(paket) ?? { funktionen: [0, 0], zeilen: [0, 0] };
      p.funktionen = [p.funktionen[0] + zeile.funktionen[0], p.funktionen[1] + zeile.funktionen[1]];
      p.zeilen = [p.zeilen[0] + zeile.zeilen[0], p.zeilen[1] + zeile.zeilen[1]];
      pakete.set(paket, p);
    }
  }
  const jePaket = [...pakete.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, p]) => ({ name, funktionen: quote(p.funktionen), zeilen: quote(p.zeilen) }));
  return { bestanden, fehler, ignoriert, fehlgeschlagen, kompilierfehler, abdeckung, jePaket };
}

/** TypeScript-Fehler aus dem Build (`datei(z,s): error TS…`). */
export function buildAuswerten(roh) {
  return [...ohneFarben(roh).matchAll(/^(.+: error TS\d+: .+)$/gm)].map((m) => m[1].trim());
}

const GRUEN = "\x1b[32m";
const ROT = "\x1b[31m";
const LEISE = "\x1b[2m";
const FETT = "\x1b[1m";
const AUS = "\x1b[0m";

const pz = (p) => (p === null || p === undefined ? "–" : `${p} %`);

/**
 * Zusammenfassung als Text. `teile`: je vorhandenem Teil (`build`, `rust`) Exit-Code
 * und Ausgabe. `logs`: Protokolldatei je Teil. `mitAbdeckung`: Lauf mit llvm-cov.
 */
export function zusammenfassen(teile, logs, { farbe = false, mitAbdeckung = false } = {}) {
  const zeilen = [];
  const fehlerListe = [];
  const f = (ok, text) => (farbe ? `${ok ? GRUEN : ROT}${text}${AUS}` : text);
  const leise = (text) => (farbe ? `${LEISE}${text}${AUS}` : text);
  const teil = (ok, name, text) => zeilen.push(f(ok, `${name.padEnd(16)} ${ok ? "✓" : "✗"}  ${text}`.trimEnd()));

  if (teile.build) {
    const tsFehler = buildAuswerten(teile.build.ausgabe);
    const ok = teile.build.code === 0;
    teil(ok, "Build", ok ? "" : `${tsFehler.length || "?"} Fehler`);
    if (!ok) fehlerListe.push(["Build", tsFehler, logs.build]);
  }

  const r = rustAuswerten(teile.rust.ausgabe);
  const gesamt = r.bestanden + r.fehler;
  const testsOk = teile.rust.code === 0 && gesamt > 0 && r.fehler === 0;
  const zusatz = r.ignoriert > 0 ? `, ${r.ignoriert} übersprungen` : "";
  teil(
    testsOk,
    "Rust-Tests",
    r.kompilierfehler
      ? "Kompilierfehler"
      : gesamt > 0
        ? `${r.bestanden} von ${gesamt} bestanden${r.fehler > 0 ? `, ${r.fehler} fehlgeschlagen` : ""}${zusatz}`
        : "keine Ergebnisse gefunden",
  );
  if (!testsOk) fehlerListe.push(["Rust", r.fehlgeschlagen, logs.rust]);

  if (mitAbdeckung) {
    const a = r.abdeckung;
    zeilen.push(
      `${"Abdeckung".padEnd(16)} ·  ${
        a ? `Zeilen ${pz(a.zeilen)} · Funktionen ${pz(a.funktionen)} · Regionen ${pz(a.regionen)}` : "nicht ermittelt"
      }`,
    );
    for (const p of r.jePaket) {
      zeilen.push(leise(`  ${p.name.padEnd(22)} Zeilen ${pz(p.zeilen).padStart(7)} · Funktionen ${pz(p.funktionen).padStart(7)}`));
    }
  }

  const gruen = fehlerListe.length === 0;
  const strich = "═".repeat(56);
  const schluss = `GESAMT: ${gruen ? "✓ GRÜN" : "✗ ROT"}`;
  const aus = [strich, mitAbdeckung ? "Registerwerk – Prüfung" : "Registerwerk – Tests", strich, ...zeilen, strich];
  aus.push(farbe ? `${FETT}${f(gruen, schluss)}` : schluss);
  for (const [name, liste, log] of fehlerListe) {
    aus.push("", `${name}:`);
    for (const t of liste) aus.push(f(false, `  ✗ ${t}`));
    aus.push(`  Einzelheiten: ${log}`);
  }
  return { text: aus.join("\n"), gruen };
}
