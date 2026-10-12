// Prüft, wie der Rumpf einer Mail im Fenster dargestellt wird.
//
// Der Grund für ein eigenes Skript: Die Darstellung ist eine reine
// Textverarbeitung – Absätze erkennen, Zitate zurücktreten lassen, Signaturen
// trennen – und sie entscheidet, ob eine Mail lesbar ist oder aussieht wie ein
// Protocolschnipsel. Sie braucht kein Fenster, um geprüft zu werden.
//
// Der zweite Grund ist wichtiger: Fremder Mailtext ist Angriffseingabe. Die
// Darstellung baut Absätze als eigene Elemente auf, und deshalb steht hier, dass
// kein Text aus einer Mail jemals über `innerHTML` in die Seite gelangt. Das ist
// die Zusage, die ein Test sonst nur schwer glauben würde.
//
// Aufruf:  node src/tests/mailtext.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(path.dirname(new URL(import.meta.url).pathname), '..', 'main.js');
const src = fs.readFileSync(QUELLE, 'utf8');
const index = fs.readFileSync(path.join(path.dirname(QUELLE), 'index.html'), 'utf8');

// Die reinen Funktionen herausschneiden: von der Konstanten bis vor die
// Darstellung, die ein DOM braucht.
const a = src.indexOf('const MAIL_UMBRUCH_MINDESTENS');
const b = src.indexOf('function zeichneMailRumpf');
if (a < 0 || b < 0 || b <= a) {
  console.error('Die Textaufteilung nicht gefunden.');
  process.exit(2);
}

const { mailBloecke, mailZeilenArt, mailAbsatzText } = new Function(
  `${src.slice(a, b)}\nreturn { mailBloecke, mailZeilenArt, mailAbsatzText };`,
)();

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

// --- Zitate und Signaturen ------------------------------------------------

// **`>` ist die verbreitetste Zitatform.**
gleich('eine Zitatzeile', mailZeilenArt('> Am Montag schrieb jemand:'), 'zitat');
// **Outlook und Gmail schreiben stattdessen einen Satz**, und beide Formen
// kommen im selben Postfach vor.
gleich('ein Zitatkopf im Satzform', mailZeilenArt('Am 12.10.2026 um 05:11 schrieb Max:'), 'zitat');
gleich('ein Zitatkopf mit Von', mailZeilenArt('Von: max@example.org'), 'zitat');
// **Die Trennlinie vor der Signatur** ist nach RFC 3676 genau `-- `.
gleich('eine Signaturlinie', mailZeilenArt('-- '), 'trenner');
gleich('eine leere Zeile', mailZeilenArt('   '), 'leer');
gleich('gewöhnlicher Text', mailZeilenArt('Hallo Max,'), 'text');

// --- Absätze --------------------------------------------------------------

gleich('eine leere Mail hat keine Absätze', mailBloecke(''), []);
gleich('nur Leerraum ebenso', mailBloecke('\n\n  \n'), []);

// Eine gewöhnliche Mail: zwei Absätze, getrennt durch eine Leerzeile.
const einfach = 'Hallo Max,\n\nbis morgen.';
gleich('zwei Absätze', mailBloecke(einfach).map((b) => b.art), ['text', 'text']);

// **Windows-Zeilenenden kommen an, als kämen sie aus einer fremden Datei.**
gleich('Wagenrückläufe werden zu Zeilenumbrüchen', mailBloecke('eins\r\nzwei').length, 1);
gleich('und deren Inhalt bleibt', mailBloecke('eins\r\nzwei')[0].zeilen, ['eins', 'zwei']);

// Zitat und neuer Text müssen getrennt bleiben – das ist der ganze Sinn.
const mitZitat = 'Mein Text.\n\n> Dein Text.\n> Noch mehr.';
gleich('Text und Zitat sind verschiedene Blöcke', mailBloecke(mitZitat).map((b) => b.art), ['text', 'zitat']);

// **Die Signatur wird vom Text getrennt**, sonst hinge sie mitten im Absatz.
const mitSignatur = 'Bis morgen.\n\n-- \nMax Mustermann';
gleich('die Signatur steht für sich', mailBloecke(mitSignatur).map((b) => b.art), ['text', 'trenner', 'text']);

// --- Harte Umbrüche -------------------------------------------------------

// **Ohne Zusammenfügen steht eine Mail als Treppenhaus da**: Sie bricht dort um,
// wo der Absender umbrochen hat, und nicht dort, wo er es meinte.
const lang = 'Dies ist eine sehr lange Zeile, die der Absender bei siebzig Zeichen umgebrochen hat';
gleich('ein harter Umbruch wird zusammengefügt', mailAbsatzText([lang, 'und hier geht es weiter']), `${lang} und hier geht es weiter`);

// **Kurze Zeilen sind gewollt.** Zwei kurze Zeilen sind ein Adressblock oder
// eine Signatur, kein umgebrochener Satz – sie bleiben stehen.
gleich('kurze Zeilen bleiben stehen', mailAbsatzText(['Max Mustermann', 'Straße 1']), 'Max Mustermann\nStraße 1');

// **Eine Aufzählung darf nicht an die vorige Zeile kleben.**
gleich(
  'eine Aufzählung bleibt auf eigener Zeile',
  mailAbsatzText([lang, '- erster Punkt']),
  `${lang}\n- erster Punkt`,
);
gleich(
  'eine nummerierte Liste ebenso',
  mailAbsatzText([lang, '1. erster Punkt']),
  `${lang}\n1. erster Punkt`,
);

// --- Die Zusage, auf die es ankommt -----------------------------------------

// **Kein fremder Text wird als Markup gelesen.** Die Darstellung baut Elemente
// auf, aber jede Textzelle wird gesetzt. Ein `innerHTML` in dieser Funktion –
// oder in ihrem Umfeld – hieße, dass eine Mail HTML im Fenster ausführen
// könnte, und die ganze Textentscheidung wäre hinfällig.
const rumpf = src.slice(src.indexOf('function zeichneMailRumpf'), src.indexOf('function oeffneMail'));
gleich('kein innerHTML beim Darstellen', /innerHTML/.test(rumpf), false);
gleich('jeder Text wird als Text gesetzt', /textContent/.test(rumpf), true);

// **Und dasselbe im ganzen Dokument**: Kein `insertAdjacentHTML`, kein `document.write`.
gleich(
  'kein weiterer Weg, Markup zu setzen',
  /insertAdjacentHTML|document\.write|outerHTML\s*=/.test(src),
  false,
);

// **Kein `<pre>` mehr im Fenster.** In einem `<pre>` wäre jede Ausgabe ein
// Absatz mit Leerzeichen – die Struktur, die hier entsteht, ginge nicht auf.
const pre = index.match(/<(pre|div)[^>]*id="mail-message-text"/);
gleich('der Rumpf steht in einem div', pre && pre[1], 'div');

console.log(fehler === 0 ? '\nalle Prüfungen bestanden' : `\n${fehler} Prüfung(en) fehlgeschlagen`);
process.exit(fehler === 0 ? 0 : 1);