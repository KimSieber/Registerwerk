// Tests der Auswertung: `node --test scripts/` (ohne weitere Abhängigkeiten)
import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { buildAuswerten, grenzePruefen, ohneFarben, rustAuswerten, zusammenfassen } from "./auswertung.mjs";

const RUST_GRUEN = `
running 3 tests
test a ... ok
test result: ok. 138 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.10s
test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.20s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Filename                                     Regions    Missed Regions     Cover   Functions  Missed Functions  Executed       Lines      Missed Lines     Cover    Branches   Missed Branches     Cover
-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
crates/registerwerk-core/src/ablauf.rs           400               40    90.00%          50                 5    90.00%         600                60    90.00%           0                 0         -
crates/registerwerk-core/src/register.rs         600               60    90.00%          50                 0   100.00%         400                 0   100.00%           0                 0         -
crates/registerwerk-ocr/src/lib.rs               100               50    50.00%          10                 5    50.00%         200               100    50.00%           0                 0         -
-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------
TOTAL                                           1100              150    86.36%         110                10    90.91%        1200               160    86.67%           0                 0         -
`;

const RUST_ROT = `
test ablauf::tests::eins ... FAILED
test ablauf::tests::zwei ... FAILED
test result: FAILED. 136 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.14s
error: test failed, to rerun pass \`-p registerwerk-core --lib\`
`;

const KOMPILIERT_NICHT = `
error[E0425]: cannot find value \`x\` in this scope
error: could not compile \`registerwerk-core\` (lib) due to 1 previous error
`;

const LOGS = { build: "target/pruefen/build.log", rust: "target/pruefen/rust.log" };
const ok = (ausgabe) => ({ code: 0, ausgabe });
const rot = (ausgabe) => ({ code: 1, ausgabe });

describe("Auswertung", () => {
  it("entfernt Farbzeichen, auch die des Macs", () => {
    assert.equal(ohneFarben("\x1b[32m✓\x1b[39m grün"), "✓ grün");
    const mac = "test a ... \x1b[31mFAILED\x1b(B\x1b[m\ntest result: \x1b[32mok\x1b(B\x1b[m. 60 passed; 1 failed; 0 ignored";
    const r = rustAuswerten(mac);
    assert.equal(r.bestanden, 60);
    assert.equal(r.fehler, 1);
    assert.deepEqual(r.fehlgeschlagen, ["a"]);
  });

  it("Rust grün: Summe, übersprungene, Abdeckung gesamt und je Paket", () => {
    const r = rustAuswerten(RUST_GRUEN);
    assert.equal(r.bestanden, 143);
    assert.equal(r.fehler, 0);
    assert.equal(r.ignoriert, 1);
    assert.deepEqual(r.abdeckung, { regionen: 86.36, funktionen: 90.91, zeilen: 86.67 });
    assert.deepEqual(r.jePaket, [
      { name: "registerwerk-core", funktionen: 95, zeilen: 94 },
      { name: "registerwerk-ocr", funktionen: 50, zeilen: 50 },
    ]);
  });

  it("Rust rot und Kompilierfehler", () => {
    const r = rustAuswerten(RUST_ROT);
    assert.equal(r.fehler, 2);
    assert.deepEqual(r.fehlgeschlagen, ["ablauf::tests::eins", "ablauf::tests::zwei"]);
    assert.equal(r.abdeckung, null);
    assert.equal(rustAuswerten(KOMPILIERT_NICHT).kompilierfehler, true);
  });

  it("Build: TypeScript-Fehler", () => {
    assert.deepEqual(buildAuswerten("src/a.ts(3,1): error TS2322: Typ falsch\nsonst"), ["src/a.ts(3,1): error TS2322: Typ falsch"]);
  });

  it("npm test grün", () => {
    const { text, gruen } = zusammenfassen({ rust: ok(RUST_GRUEN) }, LOGS);
    assert.equal(gruen, true);
    assert.deepEqual(text.split("\n"), [
      "═".repeat(56),
      "Registerwerk – Tests",
      "═".repeat(56),
      "Rust-Tests       ✓  143 von 143 bestanden, 1 übersprungen",
      "═".repeat(56),
      "GESAMT: ✓ GRÜN",
    ]);
  });

  it("npm run pruefen mit Abdeckung ohne Grenze (Filter): Abdeckung macht nicht rot", () => {
    const { text, gruen } = zusammenfassen({ build: ok(""), rust: ok(RUST_GRUEN) }, LOGS, { mitAbdeckung: true });
    assert.equal(gruen, true);
    assert.ok(text.includes("Build            ✓"));
    assert.ok(text.includes("Abdeckung        ·  Zeilen 86.67 % · Funktionen 90.91 % · Regionen 86.36 %"));
    assert.ok(text.includes("  registerwerk-core      Zeilen    94 % · Funktionen    95 %"));
    assert.ok(text.includes("Registerwerk – Prüfung"));
  });

  it("rot: Fehlschläge mit Namen und Protokoll", () => {
    const { text, gruen } = zusammenfassen(
      { build: rot("src/a.ts(3,1): error TS2322: Typ falsch"), rust: rot(RUST_ROT) },
      LOGS,
      { mitAbdeckung: true },
    );
    assert.equal(gruen, false);
    assert.ok(text.includes("Build            ✗  1 Fehler"));
    assert.ok(text.includes("Rust-Tests       ✗  136 von 138 bestanden, 2 fehlgeschlagen"));
    assert.ok(text.includes("Abdeckung        ·  nicht ermittelt"));
    assert.ok(text.includes("Rust:\n  ✗ ablauf::tests::eins\n  ✗ ablauf::tests::zwei\n  Einzelheiten: target/pruefen/rust.log"));
    assert.ok(text.includes("GESAMT: ✗ ROT"));
  });

  it("nichts auswertbar", () => {
    assert.ok(zusammenfassen({ rust: rot(KOMPILIERT_NICHT) }, LOGS).text.includes("Rust-Tests       ✗  Kompilierfehler"));
    assert.ok(zusammenfassen({ rust: rot("") }, LOGS).text.includes("keine Ergebnisse gefunden"));
  });

  it("Grenze: Dateien unter 90 % der Zeilen oder Funktionen", () => {
    const r = rustAuswerten(RUST_GRUEN);
    assert.deepEqual(r.jeDatei.map((d) => d.name), [
      "registerwerk-core/src/ablauf.rs",
      "registerwerk-core/src/register.rs",
      "registerwerk-ocr/src/lib.rs",
    ]);
    assert.deepEqual(grenzePruefen(r.jeDatei, r.abdeckung), {
      verstoesse: ["registerwerk-ocr/src/lib.rs: Zeilen 50 % unter 90 %", "registerwerk-ocr/src/lib.rs: Funktionen 50 % unter 90 %"],
      hinweise: [],
    });
    // Mit begründeter Ausnahme grün, auch mit führendem crates/
    const ausnahme = { datei: "crates/registerwerk-ocr/src/lib.rs", zeilen: 50, funktionen: 50, grund: "C-Schnittstelle" };
    assert.deepEqual(grenzePruefen(r.jeDatei, r.abdeckung, [ausnahme]).verstoesse, []);
    assert.deepEqual(grenzePruefen(r.jeDatei, r.abdeckung, [{ ...ausnahme, zeilen: 60 }]).verstoesse, [
      "registerwerk-ocr/src/lib.rs: Zeilen 50 % unter 60 %",
    ]);
    // Ausnahme nur für Zeilen: Funktionen gelten weiter mit 90 %
    const nurZeilen = { datei: "registerwerk-ocr/src/lib.rs", zeilen: 50, grund: "x" };
    assert.deepEqual(grenzePruefen(r.jeDatei, r.abdeckung, [nurZeilen]).verstoesse, [
      "registerwerk-ocr/src/lib.rs: Funktionen 50 % unter 90 %",
    ]);
    // Funktionen in der Summe
    assert.deepEqual(grenzePruefen([], { funktionen: 89.9 }).verstoesse, ["Funktionen gesamt 89.9 % unter 90 %"]);
  });

  it("Grenze: fehlerhafte und überflüssige Ausnahmen", () => {
    const r = rustAuswerten(RUST_GRUEN);
    const g = grenzePruefen(r.jeDatei, r.abdeckung, [
      { datei: "registerwerk-core/src/gibtsnicht.rs", zeilen: 50, grund: "x" },
      { datei: "registerwerk-core/src/ablauf.rs", zeilen: 80, grund: " " },
      { datei: "registerwerk-ocr/src/lib.rs", zeilen: 95, grund: "x" },
      { datei: "registerwerk-core/src/register.rs", zeilen: 80, funktionen: 80, grund: "früher niedrig" },
      { datei: "registerwerk-core/src/ablauf.rs", grund: "ohne Grenze" },
      {},
    ]);
    assert.deepEqual(g.verstoesse, [
      "Ausnahme für unbekannte Datei: registerwerk-core/src/gibtsnicht.rs",
      "Ausnahme ohne Grund: registerwerk-core/src/ablauf.rs",
      "Ausnahme mit ungültiger Grenze: registerwerk-ocr/src/lib.rs",
      "Ausnahme mit ungültiger Grenze: registerwerk-core/src/ablauf.rs",
      "Ausnahme für unbekannte Datei: (ohne Namen)",
      "registerwerk-ocr/src/lib.rs: Zeilen 50 % unter 90 %",
      "registerwerk-ocr/src/lib.rs: Funktionen 50 % unter 90 %",
    ]);
    assert.deepEqual(g.hinweise, [
      "registerwerk-core/src/register.rs: Ausnahme für Zeilen nicht mehr nötig (100 %)",
      "registerwerk-core/src/register.rs: Ausnahme für Funktionen nicht mehr nötig (100 %)",
    ]);
  });

  it("npm run pruefen mit Grenze: rot mit Dateiliste, grün mit Ausnahme", () => {
    const rotText = zusammenfassen({ build: ok(""), rust: ok(RUST_GRUEN) }, LOGS, { mitAbdeckung: true, ausnahmen: [] });
    assert.equal(rotText.gruen, false);
    assert.ok(rotText.text.includes("Grenze 90 %      ✗  2 unter der Grenze"));
    assert.ok(rotText.text.includes("Abdeckung:\n  ✗ registerwerk-ocr/src/lib.rs: Zeilen 50 % unter 90 %"));
    const ausnahmen = [
      { datei: "registerwerk-ocr/src/lib.rs", zeilen: 50, funktionen: 50, grund: "C-Schnittstelle" },
      { datei: "registerwerk-core/src/register.rs", zeilen: 80, grund: "früher niedrig" },
    ];
    const gruen = zusammenfassen({ build: ok(""), rust: ok(RUST_GRUEN) }, LOGS, { mitAbdeckung: true, ausnahmen });
    assert.equal(gruen.gruen, true);
    assert.ok(gruen.text.includes("Grenze 90 %      ✓  alle Dateien, 2 Ausnahmen"));
    assert.ok(gruen.text.includes("  Hinweis: registerwerk-core/src/register.rs: Ausnahme für Zeilen nicht mehr nötig (100 %)"));
    // Ohne Abdeckung keine Grenze
    assert.ok(!zusammenfassen({ rust: rot(RUST_ROT) }, LOGS, { mitAbdeckung: true, ausnahmen: [] }).text.includes("Grenze"));
  });

  it("mit Farbe", () => {
    const t = zusammenfassen({ rust: rot(RUST_ROT) }, LOGS, { farbe: true }).text;
    assert.ok(t.includes("\x1b[1m\x1b[31mGESAMT: ✗ ROT\x1b[0m"));
    assert.equal(ohneFarben(t), zusammenfassen({ rust: rot(RUST_ROT) }, LOGS).text);
  });
});
