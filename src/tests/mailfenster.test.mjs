// Prüft, was beim Öffnen einer Nachricht aus der Mailleiste geschieht.
//
// Der Fehler, den diese Datei verhindert: Ein Doppelklick auf eine Nachricht
// tat nichts. Der Abruf lief, und schlug er fehl, landete der Grund in der
// Konsole – wo ihn niemand liest. Vor allem aber lief er zweimal: der zweite
// Klick holte dieselbe Nachricht ein zweites Mal und rief `showModal` auf ein
// bereits offenes Fenster.
//
// Also wird nicht die Anzeige geprüft, sondern die drei Zusagen, an denen es
// hier gescheitert ist: Der Abruf hat eine Frist, er läuft nur einmal, und ein
// Fehler landet in der Leiste statt in der Konsole – ohne die Liste zu leeren.
//
// Aufruf:  node src/tests/mailfenster.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(path.dirname(new URL(import.meta.url).pathname), '..', 'main.js');
const src = fs.readFileSync(QUELLE, 'utf8');

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

const oeffnet = funktion('oeffneMail');
const hinweis = funktion('meldeMailHinweis');

if (!oeffnet || !hinweis) {
  console.error('FAIL oeffneMail/meldeMailHinweis nicht gefunden');
  process.exit(2);
}

// **Nur einmal.** Ein Doppelklick ist kein zweiter Wunsch, sondern derselbe:
// Ohne die Sperre lief der Abruf zweimal und der zweite Versuch, das Fenster zu
// öffnen, scheiterte an einem bereits offenen.
gleich(
  'ein zweiter Klick wird abgewiesen',
  /if \(mailZustand\.oeffnet\) return;/.test(oeffnet),
  true,
);
gleich('die Sperre wird wieder freigegeben', /finally\s*\{[^}]*mailZustand\.oeffnet = false;/.test(oeffnet), true);

// **Mit Frist.** Der Abruf baut eine neue Verbindung zum Mailserver auf und
// geht über das Netz. Ohne Frist ist ein schweigender Server wieder das
// endlose Warten, nur jetzt hinter einem Klick.
gleich('der Abruf hat eine Frist', /invokeMail\(\s*'mail_message'/.test(oeffnet), true);
gleich('und keine offene Lesezeit', /[^.]\binvoke\('mail_message'/.test(oeffnet), false);

// **Der Fehler erscheint in der Leiste.** Nicht in der Konsole: Dort hat ihn
// der Benutzer nicht gesehen, und sein Doppelklick tat scheinbar nichts.
gleich('der Fehler landet in der Leiste', /catch \(error\)[\s\S]*?meldeMailHinweis\(/.test(oeffnet), true);
gleich('und nicht in der Konsole', /catch \(error\)[\s\S]*?console\./.test(oeffnet), false);

// **Die Liste bleibt stehen.** Eine Zeile zu leeren, um „wird geholt" zu sagen,
// wäre nach einem Fehler dasselbe, als hätte der Klick die Nachrichten
// gelöscht – und bis zum nächsten „Neu laden" wären sie weg.
gleich('der Hinweis leert die Liste nicht', /mailList\.replaceChildren/.test(hinweis), false);
gleich('der Hinweis steht über der Liste', /mailState\.textContent = text;/.test(hinweis), true);

// **Kein `catch`, der den Fehler wegwirft.** Der Klick-Handler darf kein
// Versprechen zurücklassen, das unbehandelt bleibt: Genau dorthin verschwand
// vorher der Grund.
const zeilenStuecke = src.slice(src.indexOf('eintrag.addEventListener'), src.indexOf('mailList.append'));
gleich(
  'der Klick-Handler wirft den Fehler nicht weg',
  /oeffneMail\([^)]*\)\.catch/.test(zeilenStuecke),
  false,
);

// **Und das Fenster geht wirklich auf.** Der ganze Zweck der Kette.
gleich('das Fenster wird geöffnet', /mailMessageDialog\.showModal\(\)/.test(oeffnet), true);

// --- Als gelesen und als ungelesen markieren -----------------------------

const schalten = funktion('schalteMailGelesen');
const knopf = funktion('zeichneMailGelesen');

// **Der Knopf sagt, was ein Klick tun würde.** „Gelesen" als reiner Zustand
// müsste der Benutzer erst raten; ein Knopf, der etwas anderes tut als er
// sagt, ist schlimmer als gar keiner.
if (!schalten || !knopf) {
  console.error('FAIL schalteMailGelesen/zeichneMailGelesen nicht gefunden');
  process.exit(2);
}
gleich('der Knopf bietet das Umkehren an', /gelesen \? 'Als ungelesen markieren' : 'Als gelesen markieren'/.test(knopf), true);

// **Alle drei Orte werden nachgezogen.** Knopf, Zeile in der Liste und der
// Zustand im Fenster dürfen sich nicht widersprechen – zwei Anzeigen desselben
// Zustands sind zwei Anzeigen, und eine davon ist irgendwann falsch.
gleich('die Liste wird nachgezogen', /zeichneMailListe\(\)/.test(schalten), true);
gleich('die Liste kennt den neuen Zustand', /eintrag\.gelesen = neu/.test(schalten), true);
gleich('auch der Zustand im Fenster', /mailZustand\.offen\.kopf\.gelesen = neu/.test(schalten), true);

// **Während des Laufs gesperrt.** Sonst legt ein zweiter Klick einen zweiten
// Befehl über den ersten, und der Server führt sie in unbekannter Reihenfolge
// aus – die Nachricht steht am Ende in einem Zustand, den niemand geklickt hat.
gleich('der Knopf ist während des Markierens gesperrt', /mailMessageRead\.disabled = true/.test(schalten), true);
gleich('und danach wieder frei', /finally \{[^}]*mailMessageRead\.disabled = false/s.test(schalten), true);

// **Der Befehl hat eine Frist** – er baut eine neue Verbindung zum Mailserver
// auf, genau wie das Öffnen der Nachricht.
gleich('das Markieren hat eine Frist', /invokeMail\(\s*'mail_gelesen_setzen'/.test(schalten), true);

// **Und ein Fehler landet in der Leiste**, wie bei jedem anderen Schritt.
gleich('ein Fehler landet in der Leiste', /catch \(error\)[\s\S]*?meldeMailHinweis\(/.test(schalten), true);

// **Und im Fenster, denn die Leiste ist hinter dem Backdrop.** Das Mailsfenster
// ist modal; eine Meldung über der Liste ist unsichtbar, solange es offen ist.
// Ein Knopf, der scheitert, ohne dass sich etwas tut, sieht genau aus wie ein
// Knopf, der nichts tut – das war der Befund, den man nicht erklären konnte.
const fensterFehler = funktion('meldeMailFensterFehler');
gleich(
  'ein Fehler landet auch im Fenster',
  /catch \(error\)[\s\S]*?meldeMailFensterFehler\(/.test(schalten),
  true,
);
// **Und er wird wieder geleert** – sonst stünde der Fehler der letzten Nachricht
// beim nächsten Fenster noch da.
gleich(
  'das Fenster meldet einen Fehler nur, wenn einer da ist',
  /mailMessageError\.hidden = !text/.test(fensterFehler),
  true,
);
gleich(
  'und der Fehler verschwindet wieder',
  /meldeMailFensterFehler\(''\)/.test(schalten) && /meldeMailFensterFehler\([\s\S]*?: ''/.test(oeffnet),
  true,
);

// **Und das Öffnen selbst kann einen Fehler melden.** Lesen macht gelesen; geht
// das nicht, kommt die Nachricht trotzdem – aber der Benutzer soll erfahren,
// dass ihr Zustand unverändert blieb, statt es beim nächsten Nachladen zu
// merken. Ein verschwiegener Fehler ist genau das, woran sich dieser Knopf
// nicht erklären liess.
gleich(
  'das Öffnen meldet ein gescheitertes Markieren',
  /voll\.markierungsfehler/.test(oeffnet),
  true,
);

// **Der Gelesen-Stand kommt vom Server.** Der Abruf des Textes meldet die Flags
// mit; die Liste kann sie nicht kennen, wenn zwischen dem Zeichnen und dem
// Klick eine andere Mail zugestellt wurde.
gleich(
  'der Fensterkopf kommt aus der Antwort',
  /mailZustand\.offenKopf = \{ \.\.\.voll\.kopf/.test(oeffnet),
  true,
);
gleich(
  'und die Liste folgt dem Server',
  /nachricht\.gelesen = voll\.kopf\.gelesen/.test(oeffnet),
  true,
);

// --- Löschen --------------------------------------------------------------

const loeschen = funktion('loescheMail');
const zurueck = funktion('schalteLoeschenZurueck');

if (!loeschen || !zurueck) {
  console.error('FAIL loescheMail/schalteLoeschenZurueck nicht gefunden');
  process.exit(2);
}

// **Zweimal klicken statt Rückfragen.** `EXPUNGE` ist unumkehrbar, und
// `window.confirm` fällt in einer Webview nicht überall aus. Ohne die scharfe
// Stufe wäre ein Klick daneben der Verlust einer Nachricht.
gleich(
  'der erste Klick löscht noch nichts',
  /if \(!mailZustand\.loeschenArmiert\)[\s\S]*?return;/.test(loeschen),
  true,
);
gleich('und macht den Knopf scharf', /loeschenArmiert = true/.test(loeschen), true);
gleich(
  'und sagt, was geschehen wird',
  /Wirklich löschen/.test(loeschen) && /mailMessageWarnung\.textContent/.test(loeschen),
  true,
);

// **Der Befehl hat eine Frist** – er baut eine neue Verbindung auf und
// expungiert, das ist kein augenblickliches Geschäft.
gleich(
  'das Löschen hat eine Frist',
  /invokeMail\(\s*'mail_loeschen'/.test(loeschen),
  true,
);

// **Danach wird die Liste neu geholt, nicht geändert.** `EXPUNGE` rückt alle
// Nummern hinter der gelöschten um eins nach vorn; eine von Hand gepatchte
// Liste träfe beim nächsten Klick die Nachricht daneben.
gleich(
  'die Liste wird neu geholt',
  /aktualisiereMail\(\)/.test(loeschen),
  true,
);

// **Und das Fenster geht zu**, weil es sonst über einer Nachricht stünde, die
// es nicht mehr gibt.
gleich(
  'das Fenster wird geschlossen',
  /mailMessageDialog\.close\(\)/.test(loeschen),
  true,
);

// **Der Knopf wird mit dem Fenster zurückgesetzt** – sonst wäre die nächste
// Nachricht schon beim ersten Klick scharf.
gleich(
  'der Knopf wird zurückgesetzt',
  /mailZustand\.loeschenArmiert = false/.test(zurueck) &&
    /mailMessageDelete\.classList\.remove\('loeschen-bereit'\)/.test(zurueck),
  true,
);
gleich(
  'und zwar beim Öffnen wie beim Schließen',
  /schalteLoeschenZurueck\(\)/.test(oeffnet) && /schalteLoeschenZurueck\(\)/.test(src),
  true,
);

// **Ein Fehler landet im Fenster** – dort ist er lesbar, hinter dem Backdrop
// in der Leiste nicht.
gleich(
  'ein Fehler beim Löschen landet im Fenster',
  /catch \(error\)[\s\S]*?meldeMailFensterFehler\(/.test(loeschen),
  true,
);

// --- Schreiben und Senden ---------------------------------------------------

const schreiben = funktion('sendeMail');
const auf = funktion('oeffneSchreiben');

if (!schreiben || !auf) {
  console.error('FAIL sendeMail/oeffneSchreiben nicht gefunden');
  process.exit(2);
}

// **Zweimal klicken, wie beim Löschen.** Eine abgeschickte Nachricht ist nicht
// zurückzuholen. Der erste Klick muss nur den Knopf scharf machen und nichts
// schicken – sonst wäre die Bestätigung genau die Sache, die man gerade nicht
// hat.
gleich(
  'der erste Klick sendet noch nichts',
  /if \(!mailZustand\.sendenArmiert\)[\s\S]*?return;/.test(schreiben),
  true,
);
gleich('und macht den Knopf scharf', /sendenArmiert = true/.test(schreiben), true);
gleich(
  'und nennt Empfänger und Betreff zum Nachprüfen',
  /mailComposeWarnung\.textContent/.test(schreiben) && /Wirklich senden/.test(schreiben),
  true,
);

// **Der Befehl hat eine Frist und schickt wirklich.** `mail_senden` geht
// denselben Weg wie das Werkzeug des Agenten – deshalb dieser Name und nicht
// irgendein eigener.
gleich(
  'das Senden hat eine Frist',
  /invokeMail\(\s*'mail_senden'/.test(schreiben),
  true,
);
gleich(
  'und die geschlossenen Felder werden mitgeschickt',
  /cc:/.test(schreiben) && /bcc:/.test(schreiben) && /inAntwortAuf:/.test(schreiben),
  true,
);

// **CC und BCC gehen nur mit, wenn etwas drinsteht.** Sonst schickte Mimir ein
// leeres CC-Feld und der Server könnte daran ebenso scheitern wie an einer
// kaputten Adresse.
gleich(
  'leere Zusatzfelder gehen als „nichts" raus',
  /mailComposeCc\.value\.trim\(\) \|\| null/.test(schreiben) &&
    /mailComposeBcc\.value\.trim\(\) \|\| null/.test(schreiben),
  true,
);

// **Ein Fehler bleibt im Fenster** – hinter dem Backdrop in der Leiste wäre er
// unsichtbar, und ein Knopf, der scheitert ohne dass man es sieht, sieht aus
// wie ein Knopf, der nichts tut.
gleich(
  'ein Fehler landet im Schreibfenster',
  /catch \(error\)[\s\S]*?meldeSchreibenFehler\(/.test(schreiben),
  true,
);
// **Und nicht im Fenster der geöffneten Nachricht**, das beim Schreiben gar
// nicht offen ist.
gleich(
  'nicht im Fenster der Nachricht',
  !/catch \(error\)[\s\S]*?meldeMailFensterFehler\(/.test(schreiben),
  true,
);

// **Antworten füllt dasselbe Fenster** – kein zweites, das auseinanderlaufen
// könnte. Und der Knopf setzt die Kennung, an die der Server die Antwort
// hängen soll.
gleich(
  'Antworten nutzt dasselbe Fenster',
  /oeffneSchreiben\('antwort', mailZustand\.offen\)/.test(src) &&
    /oeffneSchreiben\('weiterleitung', mailZustand\.offen\)/.test(src),
  true,
);
gleich(
  'und setzt „Re: " vor den Betreff, „Wg: " davor',
  /`Re: \$\{betreff\}`/.test(auf) && /`Wg: \$\{betreff\}`/.test(auf),
  true,
);

// **Eine Weiterleitung ist keine Antwort.** `In-Reply-To` hängt die Nachricht
// in das Gespräch der Originalnachricht ein – der Empfänger sähe einen Verlauf,
// in dem er nie etwas gesagt hat. Die Kennung gehört deshalb *nur* in die
// Antwort.
gleich(
  'nur die Antwort trägt die Kennung',
  /: istAntwort\s*\? nachricht\.message_id\s*: ''/.test(auf),
  true,
);

// **Eine Weiterleitung geht an wen der Benutzer will**, nicht an den Absender.
gleich(
  'die Weiterleitung lässt das Empfängerfeld leer',
  /: istAntwort\s*\? nachricht\.kopf\.absender\s*: ''/.test(auf),
  true,
);

// **Beide zitieren das Original.** Eine Weiterleitung ohne den Text ist eine
// Nachricht, die niemand versteht.
const zitat = funktion('mailZitat');
gleich('das Original wird zitiert', zitat.length > 0 && /nachricht\.text/.test(zitat), true);
gleich(
  'mit „> " vor jeder Zeile',
  /=> \$\{zeile\}/.test(zitat) || /map\(\(zeile\) => `> /.test(zitat),
  true,
);
gleich(
  'und mit dem Kopf der Nachricht',
  /Von:/.test(zitat) && /Betreff:/.test(zitat),
  true,
);
gleich(
  'und beide Modi benutzen es',
  /: nachricht\s*\? mailZitat\(nachricht\)\s*: ''/.test(auf),
  true,
);

// --- Entwürfe aus dem Modell ----------------------------------------------

const entwurfsKnopf = funktion('setzeEntwurfsKnopf');

if (!entwurfsKnopf) {
  console.error('FAIL setzeEntwurfsKnopf nicht gefunden');
  process.exit(2);
}

// **Das Ergebnis des Werkzeugs trägt die Felder**, und nur `draft_email` tut das.
gleich(
  'ein Entwurf kommt als Feld an',
  /if \(output\.entwurf\)[\s\S]*?setzeEntwurfsKnopf\(entry, output\.entwurf\)/.test(src),
  true,
);

// **Der Knopf öffnet das Schreibfenster – und nichts anderes.** Es darf keinen
// zweiten Versandweg geben: Einer, an dem die Bestätigung vorbeiliegt, wäre
// genau die Zusage, die hier geschützt werden soll.
gleich(
  'der Knopf öffnet das Schreibfenster',
  /oeffneSchreiben\('neu', null, entwurf\)/.test(entwurfsKnopf),
  true,
);
gleich(
  'und sendet nichts von sich aus',
  !/mail_senden/.test(entwurfsKnopf) && !/sendeMail\(/.test(entwurfsKnopf),
  true,
);

// **Der Entwurf füllt das Fenster, statt es im Chat zu ersetzen.** Wer den Text
// abtippen müsste, würde den Entwurf nicht abschicken – und bei der ersten
// Ungenauigkeit ginge etwas verloren, ohne dass es jemand bemerkt.
gleich(
  'der Entwurf hat Vorrang vor dem, was Mimir ableitet',
  /mailComposeAn\.value = entwurf/.test(auf) &&
    /mailComposeBetreff\.value = entwurf/.test(auf) &&
    /mailComposeText\.value = entwurf/.test(auf),
  true,
);
// **Und Mimir setzt kein „Re: " vor einen Betreff, den das Modell gewählt hat** –
// das hieße, das Modell zu überstimmen.
gleich(
  'kein „Re: " vor einem fertigen Betreff',
  /entwurf\s*\? betreff\s*:/.test(auf),
  true,
);

// **Ein Fenster, nicht drei.** Wäre es automatisch, müsste Mimir damit
// rechnen, dass das Modell drei Briefe nacheinander entwirft – und drei
// `showModal` auf ein offenes Fenster wären drei Fehler.
gleich(
  'es öffnet sich nicht von selbst',
  !/setzeEntwurfsKnopf\(entry, output\.entwurf\);[\s\S]{0,80}showModal\(\)/.test(src),
  true,
);

// --- Ein Fehler muss sichtbar sein -----------------------------------------

const schrittFehler = funktion('setAgentStepFehler');

if (!schrittFehler) {
  console.error('FAIL setAgentStepFehler nicht gefunden');
  process.exit(2);
}

// **Ein Fehler, den man nicht sieht, ist schlimmer als keiner.** Vorher stand
// der Grund eines gescheiterten Werkzeugaufrufs im **eingeklappten** Ausgabetext –
// und ein zugklappter Schritt sieht aus wie ein Werkzeug, das nichts getan hat.
// Genau so ist ein Entwurf unauffindbar geworden: `draft_email` lief, schlug
// fehl, und der Grund stand in einer Zeile, die niemand aufgeklappt hat.
gleich(
  'ein Fehler wird aufgeklappt',
  /aria-expanded', 'true'/.test(schrittFehler) && /output\.hidden = false/.test(schrittFehler),
  true,
);
gleich(
  'und der Schritt sieht anders aus',
  /agent-step-fehler/.test(schrittFehler),
  true,
);
gleich(
  'und der Lesepfad benutzt den Fehlerweg',
  /catch \(error\)[\s\S]*?setAgentStepFehler\(entry, String\(error\)\)/.test(src),
  true,
);

// **Und der Knopf für einen Entwurf steht nicht im Ausgabetext**, sonst wäre er
// mit weggeklappt – ein Entwurf, den man erst aufklappen muss, um ihn prüfen
// zu können, ist einer, den niemand prüft.
gleich(
  'der Entwurfsknopf steht außerhalb des Ausgabetexts',
  /insertBefore\(knopf, entry\.output\)/.test(entwurfsKnopf),
  true,
);

// **Der Knopf wird beim Schließen zurückgesetzt** – sonst wäre die nächste
// Nachricht schon beim ersten Klick weg.
gleich(
  'das Fenster setzt beim Schließen zurück',
  /mailComposeDialog\.addEventListener\('close'[\s\S]*?sendenZuruecksetzen\(\)/.test(src),
  true,
);

// **Escape räumt auf, nicht nur der Knopf.** Hängt das Aufräumen am Knopf,
// behält der Zustand über das Fenster hinaus – und die Liste wäre die einzige
// Stelle, die ihn kennt.
gleich(
  'das Aufräumen hängt am Schließen des Fensters',
  /mailMessageDialog\.addEventListener\('close',[\s\S]*?offenKopf = null/.test(src),
  true,
);

// --- Der Versand muss sooner sprechen als die Leiste -----------------------

// **Zwei Fristen auf dieselbe Sekunde, und die falsche gewinnt.** Die Leiste
// bricht einen Mailbefehl nach 60 Sekunden ab und meldet dann
// „mail_senden hat nach 60 Sekunden nicht geantwortet" – eine Meldung über
// *Mimir*, die nichts über den Mailserver sagt. Standen beide gleich, kam diese
// Nachricht an und die erklärende nie.
const smtpQuelle = fs.readFileSync(
  path.join(path.dirname(new URL(import.meta.url).pathname), '..', '..', 'src-tauri', 'src', 'mail', 'smtp.rs'),
  'utf8',
);

const sendeFrist = smtpQuelle.match(/SEND_TIMEOUT[^=]*=\s*[^;]*?from_secs\((\d+)\)/);
const leistenFrist = src.match(/'mail_senden',[\s\S]*?,\s*(\d+),\s*\)/);

if (!sendeFrist || !leistenFrist) {
  console.error('FAIL Fristen für den Versand nicht gefunden');
  process.exit(2);
}

gleich(
  'das Backend spricht vor der Leiste',
  Number(sendeFrist[1]) < Number(leistenFrist[1]),
  true,
);
gleich(
  'und mit genug Spielraum, dass die Erklärung ankommt',
  Number(leistenFrist[1]) - Number(sendeFrist[1]) >= 15,
  true,
);

console.log(fehler === 0 ? '\nalle Prüfungen bestanden' : `\n${fehler} Prüfung(en) fehlgeschlagen`);
process.exit(fehler === 0 ? 0 : 1);
