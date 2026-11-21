// Schnittstelle zum Rust-Teil. Die Typen entsprechen den Strukturen im Kern
// (crates/registerwerk-core) und in src-tauri/src/main.rs.
import { Channel, invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  version: string;
  konfig_pfad: string | null;
  konfig_fehler: string | null;
  /** Name dieses Geräts */
  geraet: string;
  /** Äste dieses Geräts: Ordner, aus denen Unterlagen ins Register kommen können */
  aeste: { name: string; ort: string; fehlt: boolean }[];
  /** Ordnerregeln aus regeln.toml im Register-Ordner */
  regeln: RegelZeile[];
  regeln_fehler: string | null;
  register_ordner: string | null;
  pdfium_ordner: string | null;
  tessdata_ordner: string | null;
  tesseract_version: string;
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
  /** Von den entfernten: nicht mehr von den Ordnerregeln erfasst */
  entfernt_regel: number;
  entfernt_regel_seiten: number;
  /** Ordner jetzt "nur Metadaten": Text und Vorschauen entfallen */
  reduziert: number;
  reduziert_seiten: number;
  /** Beim Durchlauf wegen ihres Typs ignorierte Dateien */
  ignoriert: number;
  uebergangene_ordner: number;
  /** Nur geprüft, das Register ist unverändert. Die Oberfläche fragt nach. */
  probe: boolean;
  /** Die Dateien hinter entfernt und reduziert, nach Pfad, höchstens 5000 */
  betroffen: Betroffen[];
  /** Ordner, die dieses Gerät früher gesehen hat und jetzt nicht findet. Ihr Bestand bleibt. */
  vermisst: OrdnerBestand[];
  /** Ordner, die es auf diesem Gerät nie gab: Sie gehören zu einem anderen Gerät. */
  fremd: OrdnerBestand[];
}

/** Ein Ordner, der auf diesem Gerät fehlt, mit seinem Bestand im Register */
export interface OrdnerBestand {
  ordner: string;
  dateien: number;
  /** Geräte, die den Ordner gesehen haben */
  geraete: string[];
}

/** Warum eine Datei nach den Ordnerregeln wegfällt oder nur mit Namen bleibt (register.rs) */
export type Grund = "ohne_regel" | "ausgeschlossen" | "typ" | "bibliothek" | "nur_metadaten" | "fehlt";

export interface Betroffen {
  /** Pfad relativ zum Cloud-Ordner */
  pfad: string;
  grund: Grund;
  /** Seiten mit Text oder Vorschau, die verloren gehen */
  seiten: number;
  /** Zuletzt gesehen: Sekunden seit 1970 und Gerät */
  zuletzt: number | null;
  geraet: string | null;
}

/** Behandlung eines Ordners (regeln.rs) */
export type Stufe = "voll" | "metadaten" | "ausgeschlossen";

export interface RegelZeile {
  /** Pfad relativ zum Cloud-Ordner */
  ordner: string;
  stufe: Stufe;
}

/** Ein Ordner im Baum der Regeln */
export interface BaumEintrag {
  name: string;
  pfad: string;
  hat_unterordner: boolean;
  /** Ordner fremder Bibliotheken, wird ohne eigene Regel übergangen */
  bibliothek: boolean;
  /** Dateien im Register in diesem Ordner samt Unterordnern */
  im_register: number;
  /** Davon ausstehend und nur online */
  nur_online: number;
  /** Nur bei Ästen: Ort auf diesem Gerät, wenn hier einer zugewiesen ist */
  ort: string | null;
  /** Nur bei Ästen: Den Ordner gibt es auf diesem Gerät nicht */
  fehlt: boolean;
  /** Geräte, die den Ordner gesehen haben */
  geraete: string[];
}

/** Ein Ordner mit Dateien, die ausstehen und nur online liegen */
export interface OnlineOrdner {
  ordner: string;
  dateien: number;
}

/** Behandlung eines Dateityps in der Stufe Voll (formate.rs) */
export type Klasse = "gelesen" | "nur_name" | "ignoriert";

export interface EndungZeile {
  endung: string;
  klasse: Klasse;
  anzahl: number;
  bytes: number;
  nur_online: number;
}

export interface UnterordnerZeile {
  /** leer: Dateien direkt im geprüften Ordner */
  name: string;
  gelesen: number;
  nur_name: number;
  ignoriert: number;
  bytes: number;
  nur_online: number;
}

export interface OrdnerPruefung {
  ordner: string;
  dauer_ms: number;
  abgebrochen: boolean;
  dateien: number;
  gelesen: number;
  nur_name: number;
  ignoriert: number;
  nur_online: number;
  /** Summe der lokal vorhandenen Dateien */
  bytes: number;
  endungen: EndungZeile[];
  unterordner: UnterordnerZeile[];
  bibliotheken: string[];
  bibliotheken_anzahl: number;
  nicht_lesbar: number;
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

export interface ScheibeZeile {
  nr: number;
  datei: string;
  bytes: number;
  vorschauen: number;
  voll: boolean;
}

export interface VorschauStand {
  pdfs: number;
  erzeugt: number;
  ausstehend: number;
  ausstehend_online: number;
  fehler: number;
  seiten: number;
  farbig: number;
  bytes: number;
  scheiben: ScheibeZeile[];
  fehler_liste: FehlerZeile[];
}

export interface VorschauErgebnis {
  dauer_ms: number;
  ausstehend: number;
  erzeugt: number;
  seiten: number;
  bytes: number;
  fehler: number;
  nur_online: number;
  aufgeraeumt: number;
  abgebrochen: boolean;
  messung: Messung;
}

export interface Messung {
  faeden: number;
  render_ms: number;
  render_wartet_ms: number;
  kodier_ms: number;
  kodier_leer_ms: number;
  schreib_ms: number;
  pixel: number;
}

export interface VorschauSeite {
  nr: number;
  breite: number;
  hoehe: number;
  farbig: boolean;
}

export interface DateiVorschau {
  id: number;
  pfad: string;
  datum: string | null;
  seiten: VorschauSeite[];
}

export type Fall = "ohne_text" | "scan_schwach" | "digital_schwach";

export interface OcrStand {
  pdfs: number;
  erledigt: number;
  ausstehend: number;
  ausstehend_online: number;
  fehler: number;
  seiten_ohne_text: number;
  seiten_scan_schwach: number;
  seiten_digital_schwach: number;
  seiten_erkannt: number;
  seiten_uebernommen: number;
  erkenn_ms: number;
  nachher_ohne_text: number;
  nachher_schwach: number;
  fehler_liste: FehlerZeile[];
}

export interface OcrMessung {
  faeden: number;
  render_ms: number;
  render_wartet_ms: number;
  erkenn_ms: number;
  erkenn_leer_ms: number;
  schreib_ms: number;
  pixel: number;
}

export interface OcrErgebnis {
  dauer_ms: number;
  ausstehend: number;
  erkannt: number;
  seiten: number;
  uebernommen: number;
  fehler: number;
  nur_online: number;
  veraendert: number;
  abgebrochen: boolean;
  messung: OcrMessung;
}

export interface SeitenText {
  nr: number;
  pdf_text: string;
  pdf_guete: number | null;
  pdf_zeichen: number;
  bildanteil: number;
  fall: Fall | null;
  ocr_text: string | null;
  ocr_guete: number | null;
  ocr_zeichen: number | null;
  ocr_konfidenz: number | null;
  ocr_ms: number | null;
  uebernommen: boolean;
}

export interface TextebeneStand {
  dateien_ausstehend: number;
  seiten_ausstehend: number;
  dateien_ausstehend_online: number;
  dateien_geschrieben: number;
  seiten_in_pdf: number;
  fehler: number;
  fehler_liste: FehlerZeile[];
}

export interface TextebeneErgebnis {
  dauer_ms: number;
  probelauf: boolean;
  ziel: string | null;
  ausstehend: number;
  geschrieben: number;
  seiten: number;
  fehler: number;
  nur_online: number;
  veraendert: number;
  abgebrochen: boolean;
  bytes_vorher: number;
  bytes_nachher: number;
  fehler_liste: FehlerZeile[];
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
  vorschau: VorschauStand;
  ocr: OcrStand;
  textebene: TextebeneStand;
  typen: TypZeile[];
  jahre: JahrZeile[];
  letzter_lauf: LetzterLauf | null;
}

export const appInfo = () => invoke<AppInfo>("app_info");
export const registerStand = () => invoke<RegisterStand | null>("register_stand");
/** Ohne `bestaetigt` ändert der Lauf nichts, wenn er wegen der Regeln etwas entfernen würde (`probe`). */
export const dateibaumEinlesen = (bestaetigt: boolean) =>
  invoke<LaufErgebnis>("dateibaum_einlesen", { bestaetigt });
export const regelnLesen = () => invoke<RegelZeile[]>("regeln_lesen");
export const regelnSpeichern = (regeln: RegelZeile[]) => invoke<RegelZeile[]>("regeln_speichern", { regeln });
export const ordnerListe = (ordner: string) => invoke<BaumEintrag[]>("ordner_liste", { ordner });
export const ordnerPruefen = (ordner: string) => invoke<OrdnerPruefung>("ordner_pruefen", { ordner });
/** Kennzahlen des Registers für das Datenbank-Symbol */
export interface RegisterInfo {
  ordner: string;
  /** In diesem Ordner liegt ein Register */
  vorhanden: boolean;
  /** Der Ordner steht in der Konfiguration, sonst ist es der Datenordner der Anwendung */
  gewaehlt: boolean;
  index_bytes: number;
  scheiben: number;
  scheiben_bytes: number;
  dateien: number;
  seiten: number;
  vorschauen: number;
  /** Geräte mit dem Zeitpunkt ihres letzten Durchlaufs (Sekunden seit 1970) */
  geraete: [string, number | null][];
  geraet: string;
  regeln: number;
}

export const registerInfo = () => invoke<RegisterInfo>("register_info");
/** Ohne `bestaetigt` gilt ein Ordner ohne Register nicht. Liefert die Kennzahlen und ob der Ordner jetzt gilt. */
export const registerWaehlen = (ordner: string, bestaetigt: boolean) =>
  invoke<[RegisterInfo, boolean]>("register_waehlen", { ordner, bestaetigt });
export const registerImFinder = () => invoke<void>("register_im_finder");

/** Auswahldialog des Systems. `null`, wenn abgebrochen. */
export const ordnerWaehlen = () => invoke<string | null>("ordner_waehlen");
/** Nimmt einen Ordner als Ast auf. Mit `zuweisen` bekommt ein vorhandener Ast auf diesem Gerät diesen Ort. */
export const astHinzufuegen = (ort: string, name: string | null, zuweisen: boolean) =>
  invoke<string>("ast_hinzufuegen", { ort, name, zuweisen });
export const astLoesen = (name: string) => invoke<void>("ast_loesen", { name });
/** Entfernt einen Ordner, den es nicht mehr gibt, samt Regeln aus dem Register. */
export const ordnerEntfernen = (ordner: string) => invoke<number>("ordner_entfernen", { ordner });
/** Ordner mit „nur online“-Dateien, deren Inhalt aussteht, mit `vorschau` deren Vorschau aussteht. */
export const nurOnlineOrdner = (vorschau: boolean) => invoke<OnlineOrdner[]>("nur_online_ordner", { vorschau });
export const ordnerImFinder = (ordner: string) => invoke<void>("ordner_im_finder", { ordner });
export const ordnerPruefenAbbrechen = () => invoke<void>("ordner_pruefen_abbrechen");

/** Meldet allen Ansichten, dass sich Register oder Regeln geändert haben. */
export const GEAENDERT = "registerwerk:geaendert";
export const geaendertMelden = () => window.dispatchEvent(new Event(GEAENDERT));

export function inhalteLesen(beiFortschritt: (f: Fortschritt) => void) {
  const kanal = new Channel<Fortschritt>();
  kanal.onmessage = beiFortschritt;
  return invoke<InhaltErgebnis>("inhalte_lesen", { kanal });
}

export const inhalteAbbrechen = () => invoke<void>("inhalte_abbrechen");

export function vorschauenErzeugen(beiFortschritt: (f: Fortschritt) => void) {
  const kanal = new Channel<Fortschritt>();
  kanal.onmessage = beiFortschritt;
  return invoke<VorschauErgebnis>("vorschauen_erzeugen", { kanal });
}

export function ocrAusfuehren(beiFortschritt: (f: Fortschritt) => void) {
  const kanal = new Channel<Fortschritt>();
  kanal.onmessage = beiFortschritt;
  return invoke<OcrErgebnis>("ocr_ausfuehren", { kanal });
}

/** Mit `eine` endet der Lauf nach der ersten geschriebenen Datei. */
export function textebeneSchreiben(probelauf: boolean, eine: boolean, beiFortschritt: (f: Fortschritt) => void) {
  const kanal = new Channel<Fortschritt>();
  kanal.onmessage = beiFortschritt;
  return invoke<TextebeneErgebnis>("textebene_schreiben", { probelauf, eine, kanal });
}

export const dateienMitVorschau = (muster: string, nurOcr: boolean) =>
  invoke<DateiVorschau[]>("dateien_mit_vorschau", { muster, nurOcr });

export const seitentexte = (datei: number) => invoke<SeitenText[]>("seitentexte", { datei });

/** Adresse eines Vorschaubildes. Unter Windows bildet Tauri eigene Protokolle auf http ab. */
export const vorschauUrl = (datei: number, seite: number) =>
  (navigator.userAgent.includes("Windows") ? "http://vorschau.localhost" : "vorschau://localhost") +
  `/${datei}/${seite}`;

// ---------- Suche (Stufe 2, v0.0.6) ----------

export interface Anfrage {
  text: string;
  von: string | null;
  bis: string | null;
  endungen: string[];
  ordner: string | null;
  nach_datum: boolean;
  max: number;
}

export interface Treffer {
  datei: number;
  pfad: string;
  name: string;
  endung: string;
  datum: string | null;
  seiten: number | null;
  trefferseiten: number[];
  beste_seite: number | null;
  punkte: number;
  im_namen: boolean;
  unscharf: boolean;
}

export interface SuchErgebnis {
  treffer: Treffer[];
  gesamt: number;
  woerter: string[];
  uebergangen: string[];
  aehnliche: [string, string[]][];
  dauer_ms: number;
}

export interface Zaehler {
  wert: string;
  anzahl: number;
}

export interface SucheAngaben {
  dateien: number;
  ausstehend: number;
  ordner: Zaehler[];
  endungen: Zaehler[];
  jahre: Zaehler[];
}

export interface DateiDetails {
  id: number;
  pfad: string;
  name: string;
  endung: string;
  groesse: number | null;
  geaendert: number;
  nur_online: boolean;
  datum: string | null;
  datum_quelle: string | null;
  seiten: number | null;
  vorschau: VorschauSeite[];
}

export interface SucheAufbau {
  dateien: number;
  seiten: number;
  dauer_ms: number;
  abgebrochen: boolean;
}

export const suchen = (anfrage: Anfrage) => invoke<SuchErgebnis>("suchen", { anfrage });
export const sucheAngaben = () => invoke<SucheAngaben>("suche_angaben");
export const dateiDetails = (datei: number) => invoke<DateiDetails | null>("datei_details", { datei });
export const originalOeffnen = (datei: number) => invoke<void>("original_oeffnen", { datei });
export const imFinderZeigen = (datei: number) => invoke<void>("im_finder_zeigen", { datei });

export function sucheAufbauen(beiFortschritt: (f: Fortschritt) => void) {
  const kanal = new Channel<Fortschritt>();
  kanal.onmessage = beiFortschritt;
  return invoke<SucheAufbau>("suche_aufbauen", { kanal });
}
