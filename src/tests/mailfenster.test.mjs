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

// **Escape räumt auf, nicht nur der Knopf.** Hängt das Aufräumen am Knopf,
// behält der Zustand über das Fenster hinaus – und die Liste wäre die einzige
// Stelle, die ihn kennt.
gleich(
  'das Aufräumen hängt am Schließen des Fensters',
  /mailMessageDialog\.addEventListener\('close',[\s\S]*?offenKopf = null/.test(src),
  true,
);

console.log(fehler === 0 ? '\nalle Prüfungen bestanden' : `\n${fehler} Prüfung(en) fehlgeschlagen`);
process.exit(fehler === 0 ? 0 : 1);