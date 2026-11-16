// Schnittstelle zum Rust-Teil. Die Typen entsprechen den Strukturen im Kern
// (crates/registerwerk-core) und in src-tauri/src/main.rs.
import { Channel, invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  version: string;
  konfig_pfad: string | null;
  konfig_fehler: string | null;
  cloud_ordner: string | null;
  verzeichnisse: string[];
  register_ordner: string | null;
  pdfium_ordner: string | null;
}

export interface LaufErgebnis {
  lauf: number;
  dauer_ms: number;
  gesehen: number;
  neu: number;
  geaendert: number;
  unveraendert: number;
  entfernt: number;
  nur_online: number;
  nicht_lesbar: number;
  fehlende_verzeichnisse: string[];
  unvollstaendige_verzeichnisse: string[];
}

export interface TypZeile {
  endung: string;
  anzahl: number;
  bytes: number;
  nur_online: number;
}

export interface JahrZeile {
  jahr: string;
  anzahl: number;
}

export interface LetzterLauf {
  lauf: number;
  beendet: number;
  neu: number;
  geaendert: number;
  entfernt: number;
}

export interface FehlerZeile {
  pfad: string;
  meldung: string;
}

export interface InhaltStand {
  pdfs: number;
  gelesen: number;
  ausstehend: number;
  ausstehend_online: number;
  fehler: number;
  seiten: number;
  seiten_ohne_text: number;
  seiten_schwach: number;
  seiten_scan: number;
  pdfs_ohne_text: number;
  pdfs_schwach: number;
  fehler_liste: FehlerZeile[];
}

export interface Fortschritt {
  erledigt: number;
  gesamt: number;
  datei: string;
}

export interface InhaltErgebnis {
  dauer_ms: number;
  ausstehend: number;
  gelesen: number;
  seiten: number;
  fehler: number;
  nur_online: number;
  abgebrochen: boolean;
}

export interface RegisterStand {
  dateien: number;
  bytes: number;
  nur_online: number;
  groesse_unbekannt: number;
  mit_datum: number;
  datum_pdf: number;
  datum_geaendert: number;
  inhalt: InhaltStand;
  typen: TypZeile[];
  jahre: JahrZeile[];
  letzter_lauf: LetzterLauf | null;
}

export const appInfo = () => invoke<AppInfo>("app_info");
export const registerStand = () => invoke<RegisterStand | null>("register_stand");
export const dateibaumEinlesen = () => invoke<LaufErgebnis>("dateibaum_einlesen");

export function inhalteLesen(beiFortschritt: (f: Fortschritt) => void) {
  const kanal = new Channel<Fortschritt>();
  kanal.onmessage = beiFortschritt;
  return invoke<InhaltErgebnis>("inhalte_lesen", { kanal });
}

export const inhalteAbbrechen = () => invoke<void>("inhalte_abbrechen");
