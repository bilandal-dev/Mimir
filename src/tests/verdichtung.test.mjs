// Prüft das Leeren des Verlaufs und die automatische Verdichtung.
//
// Der Fehler, den diese Datei verhindert, ist nicht einer, den man beim Ausprobieren
// sieht – sondern vier, die man erst sieht, wenn es zu spät ist:
//
//  1. Ein leerer Verlauf, der beim nächsten Start wieder aus der Datei kommt. Wer
//     den Kontext geleert hat, glaubt, das Modell wisse nichts mehr; es wüsste
//     sehr wohl etwas, nur eine Woche später, beim ersten Satz nach dem Start.
//  2. Verdichtung, die sich selbst in den Verlauf schreibt. Das Modell bekäme seine
//     eigene Zusammenfassung als Antwort und redete darüber.
//  3. Verdichtung mit der Systemanweisung des Benutzers. „Antworte immer in drei
//     Wörtern" ergäbe eine Drei-Wort-Zusammenfassung – und der Verlauf wäre danach
//     wertlos, obwohl er vorher voll war.
//  4. Verdichtung nach jedem Zug. Ein ausführlich schreibendes Modell kostete dann
//     mehr Aufträge als das Gespräch, das diese Aufträge retten sollen.
//
// Geprüft wird deshalb nicht das Verhalten im Browser, sondern die Zusagen an den
// Stellen, an denen es hier schon gescheitert ist.
//
// Aufruf:  node src/tests/verdichtung.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(path.dirname(new URL(import.meta.url).pathname), '..', 'main.js');
const src = fs.readFileSync(QUELLE, 'utf8');
const html = fs.readFileSync(path.join(path.dirname(QUELLE), 'index.html'), 'utf8');

let fehler = 0;
const gleich = (name, ist, erwartet) => {
  const x = JSON.stringify(ist);
  const y = JSON.stringify(erwartet);
  if (x === y) {
    console.log(`ok   ${name}`);
    return;
  }
  console.error(`FAIL ${name}\n  erwartet ${y}\n  war      ${x}`);
  fehler += 1;
};

/** Schneidet eine Funktion samt Rumpf aus `main.js` heraus. */
function funktion(name) {
  const start = src.indexOf(`function ${name}`);
  if (start < 0) return '';
  const ende = [src.indexOf('\nfunction ', start), src.indexOf('\nasync function ', start)]
    .filter((i) => i > start)
    .sort((x, y) => x - y)[0];
  return ende ? src.slice(start, ende) : '';
}

const leeren = funktion('leereVerlauf');
const verdichten = funktion('verdichteVerlauf');
const faellig = funktion('verdichtungsfaellig');
const kuerzen = funktion('kuerzeVerdichtung');

if (!leeren || !verdichten || !faellig || !kuerzen) {
  console.error('FAIL leereVerlauf/verdichteVerlauf/verdichtungsfaellig/kuerzeVerdichtung nicht gefunden');
  process.exit(2);
}

// --- Leeren -----------------------------------------------------------------

// **Die drei müssen zusammenpassen.** Kontext, Fenster und Datei. Ließe man eines
// stehen, zeigte die Oberfläche etwas, das nicht stimmt: ein leeres Fenster, in dem
// das Modell alles weiß, oder ein gefülltes Fenster, dessen Kontext leer ist.
gleich('der Kontext wird geleert', /messageHistory = \[\];/.test(leeren), true);
gleich('das Fenster wird geleert', /chatContainer\.replaceChildren\(\)/.test(leeren), true);
gleich('und die Datei', /invoke\('delete_chat_history'\)/.test(leeren), true);

// **`persistHistory()` wäre falsch.** Das Backend weist eine leere Nachrichtenliste
// ab – `save_chat_history` prüft über `validate_chat_input`, und dort ist eine
// leere Liste ein Fehler. Ein leerer Verlauf ist keine Datei, sondern das Fehlen
// einer Datei.
//
// Geprüft wird deshalb der **Aufruf**, nicht der Begriff: Der Grund steht als
// Kommentar daneben, und ein Test, der jedes Vorkommen im Rumpf zählt, prüft
// den Kommentar mit.
gleich('und nicht über das Sichern', /invoke\('save_chat_history'/.test(leeren), false);

// **Die Systemanweisung bleibt.** Sie ist eine Einstellung dieses Geräts und keine
// Sitzung; sie zu löschen hieße, eine Einstellung zu ändern, nach der nicht gefragt
// wurde.
gleich('die Systemanweisung bleibt', /chat\.systemPrompt = ''/.test(leeren), false);
gleich('und es steht dabei, dass sie bleibt', /Die Systemanweisung bleibt erhalten/.test(leeren), true);

// **Die Anzeige folgt sofort**, sonst stünde oben rechts weiter die Belegung des
// Verlaufs, den es nicht mehr gibt.
gleich('die Anzeige wird nachgezogen', /updateContextUsage\(\)/.test(leeren), true);

// **Und die Sperre für die nächste Verdichtung fällt mit**, weil sie eine Zahl aus
// genau diesem Verlauf ist.
gleich('die Verdichtungssperre fällt mit', /verdichtungBoden = 0/.test(leeren), true);

// **Ein Fehler beim Löschen der Datei wird sichtbar.** Sonst glaubt der Benutzer
// beim nächsten Start, das Leeren hätte nicht geholfen, und tut es noch einmal.
gleich(
  'ein Fehler beim Löschen wird sichtbar',
  /catch \(error\)[\s\S]*?appendMessageToUI\('system'/.test(leeren),
  true,
);

// --- Wann verdichtet wird ---------------------------------------------------

// **Die Schwelle liegt vor der Warnung, nicht darauf.** Die Schätzung zählt Zeichen
// statt Token und fällt bei mehrsprachigem Text zu niedrig aus. Bei 0.75 der
// eingestellten Größe liegt die wirkliche Belegung schon um 0.85.
const schwelle = src.match(/const VERDICHTUNG_SCHWELLE = ([\d.]+);/);
const warnung = src.match(/const CONTEXT_WARNING_RATIO = ([\d.]+);/);
if (!schwelle || !warnung) {
  console.error('FAIL Schwellen nicht gefunden');
  process.exit(2);
}
gleich('die Schwelle liegt unter der Warnung', Number(schwelle[1]) < Number(warnung[1]), true);
gleich('und ist keine gerundete Zahl', schwelle[1], '0.75');

// **Ohne `/context` muss trotzdem etwas gelten.** Sonst gäbe es dort gar keine
// Verdichtung, und ausgerechnet bei der Voreinstellung liefe das Fenster über.
const angenommen = src.match(/const ANGENOMMENES_FENSTER = (\d+);/);
gleich('ohne /context wird ein Fenster angenommen', angenommen !== null, true);
gleich('die Annahme ist Ollamas Vorgabe', angenommen && Number(angenommen[1]), 4096);

// **Und sie geht nie an Ollama.** Der Wert ist eine Anzeige- und Auslösegrenze, keine
// gesendete Größe: Gesendet wird weiterhin `context_tokens`, und dort ist 0 weiterhin
// „die Vorgabe des Modells". Hinge die Annahme am Auftrag, würde Mimir einem Modell
// mit einem 32k-Fenster vorschreiben, dass es 4096 hat.
//
// Nur als **Wert eines Feldes** gezählt, und ohne Leerzeichen vor dem Doppelpunkt:
// Ein Feld im Anruf lautet `{ contextTokens: … }`, eine Verzweigung dagegen
// `chat.contextTokens > 0 ? … :`. Beide haben einen Doppelpunkt, nur der erste ist
// etwas, das abgeschickt wird.
const annahmeZeilen = src.split('\n').filter((zeile) => zeile.includes('ANGENOMMENES_FENSTER'));
gleich(
  'und wird nirgends hingesendet',
  annahmeZeilen.every(
    (zeile) => !/invoke\(|contextTokens:\s*ANGENOMMENES_FENSTER|context_tokens:\s*ANGENOMMENES_FENSTER/.test(zeile),
  ),
  true,
);


// **Kurz wird nicht verdichtet.** Die Zusammenfassung eines kurzen Verlaufs ist
// länger als der Verlauf; sie würde den Kontext vergrößern statt ihn zu entlasten.
gleich('unter der Mindestgröße bleibt es', /tokens >= VERDICHTUNG_MIN_TOKEN/.test(faellig), true);

// **Und nur, wenn überhaupt etwas zu ersetzen ist.** Die letzten Nachrichten
// bleiben immer stehen; eine Verdichtung darüber wäre ein Umweg durchs Modell
// zurück zum selben Verlauf.
gleich('mit zu wenig Verlauf passiert nichts', /nachrichten\.length <= VERDICHTUNG_REZENZ \+ 1/.test(faellig), true);

// **Nach einer Verdichtung nicht sofort wieder.** Der Boden hebt die Schwelle, damit
// ein ausführlich schreibendes Modell nicht bei jedem Zug verdichtet.
gleich('der Boden hebt die Schwelle', /verdichtungBoden \* 1\.25/.test(faellig), true);

// --- Wie verdichtet wird ----------------------------------------------------

// **Nicht `performChat`.** Die schreibt das Ergebnis nach `messageHistory` – die
// Zusammenfassung läge dann in dem Verlauf, aus dem sie entstanden ist.
// **Nicht `runChatAttempt`:** Die rendert in eine Chatblase, und eine Zusammenfassung
// ist keine Antwort auf eine Frage.
gleich('nicht über performChat', /performChat/.test(verdichten), false);
gleich('nicht über runChatAttempt', /runChatAttempt/.test(verdichten), false);

// **Nicht die Systemanweisung des Benutzers.** „Antworte immer in drei Wörtern"
// ergäbe eine Drei-Wort-Zusammenfassung. Nur die Verdichtungsanweisung geht raus.
gleich(
  'die Systemanweisung des Benutzers geht nicht mit',
  !/system:\s*systemPromptForRequest/.test(verdichten),
  true,
);
gleich('nur die Verdichtungsanweisung', /system: anweisung/.test(verdichten), true);
gleich('und keine Werkzeuge', /tools: null/.test(verdichten), true);

// **Ohne das Datum.** Eine Zusammenfassung datiert sich nicht, und das Datum wäre das
// Einzige, was sie aus dem Tag machte, an dem sie entstanden ist.
gleich('und kein Datum', /datumsangabeFuerModell/.test(verdichten), false);

// **Die Rekursion ist ausgeschlossen.** Ohne `VERDICHTUNG_REZENZ` würde die
// Rekursion den Verlauf von hinten beschneiden und die Frage des Benutzers, die
// gerade getippt ist, mit abschneiden.
gleich('die Rezenz bleibt wörtlich', /slice\(-VERDICHTUNG_REZENZ\)/.test(verdichten), true);
gleich('und geht nicht in die Verdichtung', /slice\(0, -VERDICHTUNG_REZENZ\)/.test(verdichten), true);

// **Die Nachricht baut das Backend.** Marker und Anweisung stehen dort nebeneinander
// und behaupten dasselbe; aus dem Frontend geholt wären sie getrennt gepflegt – und
// getrennt gepflegt laufen sie auseinander.
gleich('das Backend baut die Nachricht', /invoke\('verdichtungsnachricht'/.test(verdichten), true);

// **Das Ergebnis ist sichtbar.** Ein Modell, das den Wortlaut verloren hat, ist von
// außen nicht von einem zu unterscheiden, das ihn nie bekommen hat.
gleich('die Zusammenfassung steht im Chat', /verdichtungseintrag\(/.test(verdichten), true);
const eintrag = funktion('verdichtungseintrag');
gleich('und ist aufklappbar', /createElement\('details'\)/.test(eintrag), true);

// **Als `textContent`, nicht als `innerHTML`.** Das hier ist Modelltext aus einem
// Auftrag, den der Benutzer nicht gestellt hat. Die einzige Stelle, die HTML setzt,
// ist `render_markdown`.
//
// Geprüft wird die **Zuweisung**; der Satz darüber nennt das Wort noch einmal.
gleich('ohne innerHTML', /\.innerHTML\s*=/.test(eintrag), false);
gleich('mit textContent', /text\.textContent = zusammenfassung/.test(eintrag), true);

// **Sichtbar während des Laufs.** Ohne das stünde die Oberfläche bei einem
// langsamen lokalen Modell minutenlang still, und ein stiller Bildschirm sieht aus
// wie ein Fehler.
gleich('der Lauf ist sichtbar', /appendMessageToUI\('system', 'Kontext wird verdichtet/.test(verdichten), true);

// **Und ein Fehler lässt den Verlauf unverändert.** Nach einem Fehler wäre eine
// halb verdichtete Unterhaltung das Schlimmste: zusammengefasst und trotzdem
// lückenhaft.
gleich(
  'nach einem Fehler bleibt der Verlauf',
  /catch \(error\)[\s\S]*?Der Verlauf bleibt unverändert/.test(verdichten),
  true,
);
gleich('und die Meldung steht im Chat', /blase\.remove\(\)/.test(verdichten), true);

// --- Kürzen -----------------------------------------------------------------

// **An einer Absatzgrenze.** Mitten in einem Satz sähe der Rest sich wie ein Fehler
// aus, und das Modell schriebe über Inhalte, die es nicht bekommen hat.
gleich('an einer Absatzgrenze', /lastIndexOf\('\\n\\n'/.test(kuerzen), true);
gleich('mit einem sichtbaren Hinweis', /gekürzt/.test(kuerzen), true);
gleich('und nur, wenn es nötig ist', /text\.length <= VERDICHTUNG_MAX_ZEICHEN/.test(kuerzen), true);

// --- Der Knopf --------------------------------------------------------------

// **Ein Knopf, kein zweiter.** Die Eingabeleihe trägt schon zwei Umschalter, das
// Eingabefeld und drei Knöpfe; sie umbricht sonst, und umbrochen verschiebt sie die
// Leiste (siehe den Kommentar über `#command-hint`).
gleich('es gibt genau einen Knopf', (html.match(/id="verlauf-btn"/g) || []).length, 1);

// **Er steht in derselben Leiste wie die Eingabe**, und nicht irgendwo im Fenster:
// Außerhalb von `.input-container` wäre er eine Schaltfläche ohne erkennbaren
// Bezug zu dem, was er leert.
const leiste = html.slice(html.indexOf('class="input-container"'), html.indexOf('id="server-url-dialog"'));
gleich('und er steht neben der Eingabe', /id="attach-btn"[\s\S]*id="verlauf-btn"/.test(leiste), true);

// **Er wird während eines Zugs gesperrt.** Eine Verdichtung läuft als eigener
// Auftrag durch `setGenerating(true)`; ohne das Sperren könnte ein zweiter Verdichtung
// anlaufen, während der erste noch schreibt, und beide griffen in denselben
// `messageHistory`.
const setzt = funktion('setGenerating');
gleich('der Knopf wird während des Zugs gesperrt', /verlaufBtnAktualisieren\(\)/.test(setzt), true);

// **Geleert wird nur nach Rückfrage.** Ohne Fenster stünde ein Knopf da, der beim
// ersten Klick unwiederbringlich weg nimmt, was man noch lesen wollte.
gleich('vorher kommt ein Fenster', /verlaufDialog\.showModal\(\)/.test(src), true);
gleich('das Fenster nennt den Stand', /verlaufStand\.textContent/.test(src), true);
gleich('und sagt, dass die Systemanweisung bleibt', /verlaufSystem\.textContent/.test(src), true);
gleich('und bricht ab', /verlaufCancel/.test(src), true);

// **Und der Knopf steht in der Hilfe.** `COMMAND_HELP` ist die einzige Quelle für
// Hilfe, Vervollständigung und Nutzungshinweis; ein Knopf ohne Eintrag wäre einer,
// den niemand findet.
const hilfe = src.slice(src.indexOf('const COMMAND_HELP'), src.indexOf('function usageHint'));
gleich('der Knopf steht in der Hilfe', /Verlauf \(Knopf neben der Eingabe\)/.test(hilfe), true);
gleich('und ohne Schrägstrich, damit die Vervollständigung ihn ignoriert', /name: 'Verlauf \(/.test(hilfe), true);

// --- Wann gesendet wird ------------------------------------------------------

// **Nach einem Abbruch geht die Frage nicht raus**, und die Eingabe bleibt im Feld.
// Wäre `resetPromptInput()` vor der Verdichtung, wie es dort sonst steht, würde ein
// Klick auf „Abbrechen" die gerade getippte Frage mitnehmen – und sie danach doch
// noch senden.
const senden = funktion('handleSendMessage');
const verdichtungAufruf = senden.indexOf('verdichteVerlauf(');
const abschnitt = senden.slice(verdichtungAufruf);
gleich('nach einem Abbruch geht die Frage nicht raus', /if \(cancelRequested\)[\s\S]*?return;/.test(abschnitt), true);
// **Gesucht wird erst hinter der Verdichtung.** `handleSendMessage` leert das Feld
// auch im Zweig für `/`-Befehle, und der steht weiter oben.
gleich(
  'und das Eingabefeld wird erst danach geleert',
  abschnitt.indexOf('resetPromptInput()') > 0,
  true,
);

console.log(fehler === 0 ? '\nalle Prüfungen bestanden' : `\n${fehler} Prüfung(en) fehlgeschlagen`);
process.exit(fehler === 0 ? 0 : 1);
