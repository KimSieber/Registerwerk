// `npm test` und `npm run pruefen` (nach dem Vorbild von Stellwerk).
//
//   npm test          cargo test über den ganzen Workspace, danach die Zusammenfassung
//   npm run pruefen   Build der Oberfläche und Rust-Tests mit Abdeckung (cargo llvm-cov)
//
// Die Ausgabe läuft wie gewohnt mit und wird zusätzlich in target/pruefen/*.log
// gesichert. Alle Teile laufen auch dann, wenn ein früherer rot ist, damit die
// Zusammenfassung vollständig ist. Exit-Code 1, wenn etwas rot ist.
// Weitere Argumente gehen an cargo, etwa `npm test -- ablauf` für einen Filter.

import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

import { zusammenfassen } from "./auswertung.mjs";

const ORDNER = "target/pruefen";
const mitAbdeckung = process.argv[2] === "--abdeckung";
const weitere = process.argv.slice(mitAbdeckung ? 3 : 2).join(" ");
// Mit `-p paket` nur dieses Paket, sonst der ganze Workspace
const umfang = /(^|\s)(-p|--package)\s/.test(weitere) ? "" : "--workspace";

// Hülle und Werkzeuge haben keine Tests, sie würden die Abdeckung nur verfälschen
const OHNE = "(src-tauri/|tools/)";
const TEILE = mitAbdeckung
  ? [
      ["build", "Build", "npm run build"],
      // Erstes `--color always`: cargo und llvm-cov, zweites: die Testprogramme
      ["rust", "Rust-Tests mit Abdeckung", `cargo llvm-cov ${umfang} --ignore-filename-regex "${OHNE}" --color always ${weitere} -- --color always`],
    ]
  : [["rust", "Rust-Tests", `cargo test ${umfang} --color always ${weitere} -- --color always`]];

function ausfuehren(befehl) {
  return new Promise((fertig) => {
    let ausgabe = "";
    const p = spawn(befehl, { shell: true, env: { ...process.env, FORCE_COLOR: "1", CARGO_TERM_COLOR: "always" } });
    const mit = (strom) => (d) => {
      ausgabe += d;
      strom.write(d);
    };
    p.stdout.on("data", mit(process.stdout));
    p.stderr.on("data", mit(process.stderr));
    p.on("close", (code) => fertig({ code: code ?? 1, ausgabe }));
  });
}

mkdirSync(ORDNER, { recursive: true });
const ergebnisse = {};
const logs = {};
for (const [schluessel, name, befehl] of TEILE) {
  const start = Date.now();
  console.log(`\n▶ ${name} …\n`);
  ergebnisse[schluessel] = await ausfuehren(befehl);
  logs[schluessel] = `${ORDNER}/${schluessel}.log`;
  writeFileSync(logs[schluessel], ergebnisse[schluessel].ausgabe);
  const ok = ergebnisse[schluessel].code === 0 ? "✓" : "✗";
  console.log(`\n▶ ${name} ${ok} (${Math.round((Date.now() - start) / 1000)} s)`);
}

// Ausführlicher Bericht je Funktion (v0.1.2), um nicht abgedeckte Funktionen
// zu finden. Liest nur die Messung dieses Laufs, ohne neuen Testlauf.
if (mitAbdeckung && ergebnisse.rust.code === 0) {
  await ausfuehren(
    `cargo llvm-cov report --json --ignore-filename-regex "${OHNE}" --output-path ${ORDNER}/abdeckung.json > /dev/null 2>&1`,
  );
}

// Die Grenze gilt nur für den ganzen Workspace ohne Filter (v0.1.2)
const ausnahmen =
  mitAbdeckung && weitere === ""
    ? JSON.parse(readFileSync(new URL("./abdeckung-ausnahmen.json", import.meta.url), "utf8")).ausnahmen
    : null;
const { text, gruen } = zusammenfassen(ergebnisse, logs, { farbe: process.stdout.isTTY, mitAbdeckung, ausnahmen });
console.log(`\n${text}\n`);
process.exit(gruen ? 0 : 1);
