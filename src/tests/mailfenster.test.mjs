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