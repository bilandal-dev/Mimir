// Prüft, woher die Mailleiste ihre Kontenliste nimmt.
//
// Der Fehler, den diese Datei verhindert: Der Kontodialog zeigte ein Postfach,
// die Leiste daneben „Kein Mailkonto eingetragen." Beide lasen dieselbe Datei,
// aber über zwei verschiedene Befehle – und die Leiste entschied die Frage
// „gibt es überhaupt ein Konto" über die Länge der Statusliste. Fiel der
// Statusabruf einmal leer aus, war das eingetragene Konto unsichtbar.
//
// Also wird hier nicht die Oberfläche geprüft, sondern die eine Zusammenführung,
// an der die Frage kippt: Die Konfiguration entscheidet über das Dasein eines
// Kontos, `mail_statuses` nur über seinen Zustand.
//
// Aufruf:  node src/tests/mailkonten.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(path.dirname(new URL(import.meta.url).pathname), '..', 'main.js');
const src = fs.readFileSync(QUELLE, 'utf8');

// `mailKontenliste` ruft `mailAnzeigename` auf, deshalb beide zusammen
// herausschneiden – hintereinander, so wie sie im Quelltext stehen.
const a = src.indexOf('function mailKontenliste');
const b = src.indexOf('function mailAnzeigename');
const c = src.indexOf('/** Trägt ein Konto in die Felder');
if (a < 0 || b < 0 || c < 0 || !(a < b && b < c)) {
  console.error('mailKontenliste/mailAnzeigename nicht gefunden.');
  process.exit(2);
}

const { mailKontenliste } = new Function(
  `${src.slice(a, c)}\nreturn { mailKontenliste };`,
)();

// Ein eingetragenes Konto, wie `get_mail_config` es liefert.
const konto = {
  id: 'konto-1',
  name: '',
  imap_server: 'imap.example.org',
  smtp_server: '',
  username: 'bilandal@example.org',
  address: 'bilandal@example.org',
  folder: '',
  max_entries: 40,
};

// Der Zustand dazu: eingetragen, aber nicht angemeldet, Passwort nicht gemerkt.
const status = {
  id: 'konto-1',
  name: 'bilandal@example.org',
  adresse: 'bilandal@example.org',
  ordner: 'INBOX',
  eingerichtet: true,
  angemeldet: false,
  gemerkt: false,
  letzter_erfolg: null,
};

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

// **Der eigentliche Fehler.** Die Konfiguration kennt ein Konto, der
// Statusabruf liefert nichts – etwa weil er fehlschlug. Das Konto muss
// trotzdem erscheinen, sonst behauptet die Leiste wieder, es sei keines da.
const ohneStatus = mailKontenliste([konto], []);
gleich('Konto ohne Status bleibt sichtbar', ohneStatus.length, 1);
gleich('es ist nicht angemeldet', ohneStatus[0].angemeldet, false);
gleich('es ist aber eingerichtet', ohneStatus[0].eingerichtet, true);
gleich('Name ist die Absenderadresse', ohneStatus[0].name, 'bilandal@example.org');
gleich('leerer Ordner bedeutet INBOX', ohneStatus[0].ordner, 'INBOX');
gleich('kein gemerktes Passwort', ohneStatus[0].gemerkt, false);
gleich('kein letzter Erfolg', ohneStatus[0].letzter_erfolg, null);

// **Kein erfundenes Konto.** Eine Statusliste ohne Gegenstück in der
// Konfiguration darf keines ins Bild bringen: Sonst zeigt die Leiste ein
// Postfach, das es gar nicht mehr gibt.
gleich('Status ohne Konto wird verworfen', mailKontenliste([], [status]).length, 0);

// **Der Zustand kommt durch, wenn er da ist.** Der Angemeldet-Zustand stammt
// aus der Sitzung im Backend und darf nicht von der Konfiguration überschrieben
// werden – sonst wäre ein angemeldetes Postfach nach jedem Neuladen wieder
// abgemeldet.
const angemeldet = mailKontenliste([konto], [{ ...status, angemeldet: true, gemerkt: true }]);
gleich('angemeldet bleibt angemeldet', angemeldet[0].angemeldet, true);
gleich('gemerkt bleibt gemerkt', angemeldet[0].gemerkt, true);
gleich('und behält seinen Erfolgszeitpunkt', angemeldet[0].letzter_erfolg, null);

// Ein eigener Anzeigename schlägt die Adresse – dieselbe Reihenfolge wie im
// Backend, sonst hieße dasselbe Konto in Dialog und Leiste verschieden.
const benannt = mailKontenliste([{ ...konto, name: 'Privat' }], []);
gleich('eigener Name schlägt Adresse', benannt[0].name, 'Privat');

// Ein halb eingetragenes Konto ist da, aber nicht benutzbar. Die Leiste muss es
// zeigen und den Grund nennen, nicht es verschweigen.
const halb = mailKontenliste([{ ...konto, imap_server: '' }], []);
gleich('halb eingetragenes Konto bleibt da', halb.length, 1);
gleich('und gilt als nicht eingerichtet', halb[0].eingerichtet, false);

// **Ein Konto ohne Server und ohne Benutzername ist kein Konto**, sondern das
// Skelett, das der Dialog zum Anlegen benutzt. Die Leiste meldet dafür
// „Kein Mailkonto eingetragen" – dafür ist die Länge der Liste richtig, aber
// nicht über den Status zu beurteilen.
const leer = mailKontenliste([{ id: 'neu', name: '', imap_server: '', username: '', address: '', folder: '' }], []);
gleich('Skelett ist nicht eingerichtet', leer[0].eingerichtet, false);

// **Kein Kontostand heißt nicht: kaputt.** Die Leiste zeichnet „Kein
// Mailkonto eingetragen" bei einer leeren Liste, nicht bei einer fehlenden.
// Beides sieht gleich aus, deshalb wird hier der Weg über die Liste geprüft
// und nicht der Text.
gleich('leere Konfiguration ergibt leere Liste', mailKontenliste([], []).length, 0);
gleich('fehlende Kontenliste ist kein Absturz', mailKontenliste(undefined, undefined), []);

// **Der Rumpf von `ladeUndZeichneMail`, sauber abgeschnitten.** Beide Prüfungen
// unten gelten nur für diese eine Funktion: `mailZustand.statuse` ist längst die
// zusammengeführte Liste, ihre Länge zu prüfen ist richtig, und der Dialog
// darf eigene Aufrufe haben. Deshalb wird bis zur nächsten Funktionsgrenze
// geschnitten und nicht bis zur nächsten Funktion, die zufällig passt.
const start = src.indexOf('async function ladeUndZeichneMail');
const ende = [src.indexOf('\nfunction ', start), src.indexOf('\nasync function ', start)]
  .filter((i) => i > start)
  .sort((x, y) => x - y)[0];
const rumpf = start >= 0 && ende ? src.slice(start, ende) : '';

// **Die eigentliche Bedingung, an der es kippte.** Die Leiste meldete „Kein
// Mailkonto eingetragen", wenn die Statusliste leer war – und nicht, wenn die
// Konfiguration leer war. Diese eine Zeile im Quelltext ist der Fehler, den
// die Zusammenführung oben behebt; ohne sie käme er beim nächsten Umbau
// zurück. Geprüft wird deshalb der Aufruf, nicht die Wirkung.
if (!rumpf) {
  console.error('FAIL ladeUndZeichneMail nicht gefunden');
  fehler += 1;
} else if (/(?<!mailZustand\.)\bstatuse\s*\.\s*length/.test(rumpf)) {
  console.error('FAIL die Leiste entscheidet „kein Konto" wieder über mail_statuses');
  fehler += 1;
} else {
  console.log('ok   die Leiste befragt nicht die Statusliste auf ihre Länge');
}

// **Die Leiste darf keine Behauptung im HTML stehen haben, bevor sie etwas
// geholt hat.** Genau dort stand „Kein Mailkonto eingetragen" – ein Satz über
// einen Bestand, den Mimir zum Zeitpunkt des Ladens noch gar nicht kannte. Der
// Unterschied ist entscheidend: „Es ist keines da" und „ich habe noch nicht
// gefragt" sehen gleich aus, führen aber zu verschiedenen Schlüssen.
const index = fs.readFileSync(path.join(path.dirname(QUELLE), 'index.html'), 'utf8');
const zustand = index.match(/<p id="mail-state"[^>]*>([^<]*)</);
if (!zustand) {
  console.error('FAIL die Mailleiste hat keinen Zustandstext');
  fehler += 1;
} else if (/kein mailkonto/i.test(zustand[1])) {
  console.error(`FAIL die Leiste behauptet im HTML schon einen Bestand: „${zustand[1].trim()}“`);
  fehler += 1;
} else {
  console.log(`ok   die Leiste behauptet im HTML nichts (${zustand[1].trim()})`);
}

// **Ein umgebrochener Textbaustein, der beim Umbruch Leerzeilen bekommen hat.**
// Zwei Sätze waren so zerlegt, dass zwischen zwei Wörtern eine halbe Bildschirm-
// breite Leerzeichen stand – in Rust ein String, in JavaScript ein Regex. Man
// sieht es nur beim Lesen des Fehlers, und der Fehler ist genau die Stelle, an
// der jemand nachsehen muss.
const QUELLEN = ['main.js', 'index.html', 'styles.css'].map((d) => path.join(path.dirname(QUELLE), d)).concat(
  ['lib.rs', 'mail.rs', 'mail/imap.rs'].map((d) => path.join(path.dirname(QUELLE), '..', 'src-tauri', 'src', d)),
);
let verdorbene = 0;
for (const datei of QUELLEN) {
  const zeilen = fs.readFileSync(datei, 'utf8').split('\n');
  zeilen.forEach((zeile, n) => {
    const treffer = zeile.match(/"[^"]*[a-zäöüA-ZÄÖÜ][ ]{6,}[^"]*"/);
    if (treffer) {
      console.error(`FAIL ${path.basename(datei)}:${n + 1} verdorbener Text: ${treffer[0].slice(0, 60)}`);
      verdorbene += 1;
    }
  });
}
if (verdorbene === 0) {
  console.log('ok   kein verdorbener Text in den Quellen');
}
fehler += verdorbene;

// **Kein Aufruf in der Leiste ohne Frist.** `invoke` gibt ein Versprechen
// zurück, das ohne Antwort für immer offen bleibt – die Leiste steht dann auf
// „wird geladen", ohne Fehler und ohne Konsole, und „Neu laden" wiederholt
// genau denselben Aufruf. Deshalb läuft jeder Befehl der Leiste über
// `invokeMail`, das nach spätestens `MAIL_ANTWORT_SEKUNDEN` abbricht und den
// Befehl im Fehler nennt. Ein nacktes `invoke(` im Rumpf wäre genau die
// Regression, die hier wieder hereinrutscht.
const ladt = rumpf;
if (!ladt) {
  console.error('FAIL ladeUndZeichneMail nicht gefunden');
  fehler += 1;
} else {
  const nackt = [...ladt.matchAll(/(?<!Mail\()\binvoke\(/g)];
  if (nackt.length > 0) {
    console.error(`FAIL ${nackt.length} Aufruf(e) ohne Frist in der Leiste`);
    fehler += 1;
  } else {
    console.log('ok   jeder Aufruf der Leiste hat eine Frist');
  }
  if (!/MAIL_ANTWORT_SEKUNDEN/.test(src)) {
    console.error('FAIL keine Frist für Mailbefehle gesetzt');
    fehler += 1;
  } else {
    console.log('ok   die Frist ist gesetzt');
  }
}

// --- Wie viele Nachrichten die Leiste zeigt ------------------------------

// `mailEintraege` entscheidet, ob die eingetragene Zahl das Backend erreicht
// oder der eingetragene Wert unverändert bleibt. Das ist die Frage, an der die
// Zahl des Benutzers verloren gehen kann – deshalb wird sie hier ausgezogen und
// mit einer Attrappe ausgeführt, statt nur im Quelltext nachzusehen.
const e = src.indexOf('function mailEintraege');
if (e < 0) {
  console.error('FAIL mailEintraege nicht gefunden');
  process.exit(2);
}

// Die Attrappe ist ein **Objekt**, nicht ein Wert: `mailEintraege` liest
// `.value` erst im Aufruf, und deshalb muss dasselbe Ding über mehrere Prüfungen
// hinweg verändert werden können.
const feld = { value: '' };

const eintraege = new Function(
  'mailEntriesInput',
  `${src.slice(e, src.indexOf('\n}', e) + 2)}\nreturn mailEintraege;`,
)(feld);

const mitFeld = (wert) => () => {
  feld.value = wert;
  return eintraege();
};

// **Leer heißt: nichts aussagen.** Das Backend nimmt `null` als „unverändert
// lassen" – so kommt der Kontoeintrag zurück, statt auf `0` geklemmt zu werden.
// `0` war genau der Wert, der die Leiste auf eine einzige Nachricht brachte.
gleich('ein leeres Feld lässt den eingetragenen Wert', mitFeld('')(), null);

// **Eine brauchbare Zahl wird durchgelassen.**
gleich('eine eingetragene Zahl kommt an', mitFeld('7')(), 7);
gleich('getrimmt', mitFeld(' 12 ')(), 12);

// **Und alles, was keine Zahl ist, ebenso.** Wer „viele" eintippt, hat einen
// Tippfehler gemacht – der darf den eingetragenen Wert nicht überschreiben.
for (const unbrauchbar of ['abc', '0', '-5', '3.5', '5e2']) {
  gleich(
    `„${unbrauchbar}" ändert nichts`,
    mitFeld(unbrauchbar)(),
    null,
  );
}

// **Die Obergrenze klemmt das Backend**, nicht diese Funktion – sonst stünde
// die Zahl an zwei Stellen.
gleich('eine zu grosse Zahl wird durchgelassen und oben geklemmt', mitFeld('999')(), 999);

console.log(fehler === 0 ? '\nalle Prüfungen bestanden' : `\n${fehler} Prüfung(en) fehlgeschlagen`);
process.exit(fehler === 0 ? 0 : 1);