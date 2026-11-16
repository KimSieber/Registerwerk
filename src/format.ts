const zahl = new Intl.NumberFormat("de-DE");
const zeit = new Intl.DateTimeFormat("de-DE", { dateStyle: "medium", timeStyle: "short" });

export const alsZahl = (n: number) => zahl.format(n);

export function alsGroesse(bytes: number): string {
  const stufen = ["Byte", "KB", "MB", "GB", "TB"];
  let wert = bytes;
  let i = 0;
  while (wert >= 1024 && i < stufen.length - 1) {
    wert /= 1024;
    i++;
  }
  const stellen = i === 0 || wert >= 100 ? 0 : 1;
  return `${wert.toLocaleString("de-DE", { maximumFractionDigits: stellen })} ${stufen[i]}`;
}

export const alsZeitpunkt = (sekunden: number) => zeit.format(new Date(sekunden * 1000));

export const alsDauer = (ms: number) =>
  ms < 1000 ? `${ms} ms` : `${(ms / 1000).toLocaleString("de-DE", { maximumFractionDigits: 1 })} s`;

/** Anteil in ganzen Prozent, "–" wenn die Gesamtzahl 0 ist. */
export const alsAnteil = (teil: number, gesamt: number) =>
  gesamt > 0 ? `${Math.round((teil / gesamt) * 100)} %` : "–";
