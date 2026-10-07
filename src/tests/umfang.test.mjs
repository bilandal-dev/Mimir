// Prüft den Umfang: wie weit das Modell reichen darf.
//
// Ohne Fenster prüfbar, weil die Entscheidung im reinen Text liegt. Geprüft
// wird nicht die Oberfläche, sondern die eine Stelle, an der aus dem eingestellten
// Umfang eine Antwort wird – so wie `datumsangabe.test.mjs` die Datumsbildung
// herausschneidet und für sich rechnet.
//
// Warum das eine eigene Datei ist: Der Umfang entscheidet, welche Werkzeuge das
// Modell überhaupt sieht. Ein Fehler hier ist kein Textfehler, sondern einer, der
// dem Modell das Dateisystem anbietet oder es einen Termin anlegen lässt, den der
// Benutzer nicht wollte. Beides fällt im Chat nicht auf.
//
// Aufruf:  node src/tests/umfang.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(
  path.dirname(new URL(import.meta.url).pathname),
  '..',
  'main.js',
);
const src = fs.readFileSync(QUELLE, 'utf8');
// Die Kopfzeile ist die andere Hälfte desselben Providers und wird deshalb
// mitgelesen: Ein Wert, den es im Backend nicht gibt, ließe sich wählen, aber
// nicht verlassen.
const index = fs.readFileSync(path.join(path.dirname(QUELLE), 'index.html'), 'utf8');

// Nur die reinen Funktionen, die den Umfang in eine Antwort übersetzen: keine
// Tauri-Aufrufe, kein DOM.
const a = src.indexOf('const SCOPE_NAMES');
const b = src.indexOf('let cancelRequested');
if (a < 0 || b < 0 || b <= a) {
  console.error('Umfangs-Hilfsfunktionen nicht gefunden.');
  process.exit(2);
}

// `calendar` wird als Parameter übergeben, weil `umfangstext` den Anmeldestand
// liest; im echten Ablauf ist das die globale Zustandsvariable. Der Anmeldestand
// ist nicht fest verdrahtet, weil die Warnung genau dann erscheinen soll, wenn
// er fehlt – eine Prüfung mit immer angemeldetem Kalender würde sie nie sehen.
function ladeMitKalender(angemeldet) {
  return new Function(
    'calendar',
    `${src.slice(a, b)}\nreturn { werkzeugschleifeAktiv, umfangsname, zugname, umfangstext, providername, PROVIDER_NAMES, lokalesOllama };`,
  )({ status: { logged_in_hint: angemeldet } });
}

const F = ladeMitKalender(true);
const Fohne = ladeMitKalender(false);

// Wie `schleifeLaeuft` unten, nur für den Provider: Der Zustand wird im
// Ausschnitt ersetzt, nicht über einen zusätzlichen Aufrufer im Quelltext. So
// bleibt in `main.js` nichts stehen, das nur die Prüfung braucht.
function mitProvider(art) {
  const teil = src.slice(a, b).replace(/let provider = '[a-z]+';/, `let provider = '${art}';`);
  return new Function(`${teil}\nreturn { istLokal: lokalesOllama() };`)();
}

// Der Beschreibungssatz des Befehls aus der Hilfe. Er ist der Text, den ein
// Benutzer ohne Kenntnis der Konfigurationsdatei liest, wenn er `/provider`
// eintippt – deshalb wird er mitgeprüft und nicht nur der Quelltext.
function befehlstext(befehl) {
  const treffer = src.match(
    new RegExp(`name: '/${befehl}',[\\s\\S]{0,900}?usage: '/${befehl}[^']*'`),
  );
  if (!treffer) {
    console.error(`Befehl /${befehl} nicht in der Hilfe gefunden.`);
    process.exit(2);
  }
  // Die Texte stehen als aneinandergehängte Zeichenketten im Quelltext. Für die
  // Prüfung zählt der zusammengesetzte Satz: Ein Wort, das über zwei Zeilen
  // geteilt ist, wäre sonst ein Wort, das es nicht gibt.
  return treffer[0].replace(/'\s*\+\s*'/g, '');
}

// Der Zustand, den `umfangstext` liest. Bewusst anders benannt als im Quelltext,
// damit die Attrappe nicht mit dem echten Zustand verwechselt wird.
const zustand = (scope, weitere = {}) => ({
  scope,
  root: '',
  maxSteps: 8,
  writeEnabled: false,
  ...weitere,
});

let fehler = 0;
const gleich = (name, ist, erwartet) => {
  const x = JSON.stringify(ist);
  const y = JSON.stringify(erwartet);
  if (x !== y) {
    fehler += 1;
    console.log(`FEHL ${name}\n     ist:      ${x}\n     erwartet: ${y}`);
  } else {
    console.log(`ok   ${name}`);
  }
};

console.log('— Wann läuft die Werkzeugschleife —');
// Im Terminumfang läuft sie ohne Schalter: Der Umfang bringt die Werkzeuge mit.
// Andernfalls wäre `/scope termine` nur eine Anzeige, und ein zweiter Schalter
// stünde im Weg, ohne etwas zu sichern.
// `werkzeugschleifeAktiv` liest den Zustand aus der oben stehenden `agent`.
// Um beide Zweige zu prüfen, wird der Ausschnitt ein zweites Mal mit anderem
// Zustand ausgewertet – die Konstruktion bleibt die des Quelltextes.
function schleifeLaeuft(scope, enabled) {
  const teil = src.slice(a, b).replace(
    /const agent = \{[^}]*\};/,
    `const agent = { scope: '${scope}', enabled: ${enabled} };`,
  );
  const instanz = new Function(`${teil}\nreturn werkzeugschleifeAktiv();`)();
  return instanz;
}
gleich('Terminumfang ohne Schalter', schleifeLaeuft('termine', false), true);
gleich('Terminumfang mit Schalter', schleifeLaeuft('termine', true), true);
gleich('Agentenmodus mit Schalter', schleifeLaeuft('agent', true), true);
gleich('Agentenmodus ohne Schalter', schleifeLaeuft('agent', false), false);

console.log();
console.log('— Der Name des Laufs folgt dem Umfang —');
gleich('im Terminumfang', F.zugname(true), 'Terminlauf');
gleich('im Agentenmodus', F.zugname(false), 'Agentenlauf');
// Der Unterschied ist der ganze Punkt: Ein „Agentenlauf" ohne Arbeitsverzeichnis
// wäre eine Meldung über etwas, das es dort nicht gibt.
gleich('die beiden Namen sind verschieden', F.zugname(true) !== F.zugname(false), true);

console.log();
console.log('— Der Text nennt beide Umfänge und den Weg zurück —');
const termine = F.umfangstext('termine', zustand('termine'));
gleich('nennt die vier Kalenderwerkzeuge', /auflisten, anlegen, ändern und löschen/.test(termine), true);
gleich('sagt, dass keine Datei gelesen wird', /keine Dateien lesen oder schreiben/.test(termine), true);
gleich('sagt, dass kein Arbeitsverzeichnis nötig ist', /Arbeitsverzeichnis ist nicht nötig/.test(termine), true);
gleich('nennt die Vorschau', /Vorschau/.test(termine), true);
gleich('nennt den Weg zurück', /\/scope agent/.test(termine), true);

// Ohne Anmeldung hat der Umfang gar kein Werkzeug. Das muss im Text stehen,
// sonst stellt der Benutzer den Umfang ein und wundert sich, dass nichts
// passiert.
const ohneAnmeldung = Fohne.umfangstext('termine', zustand('termine'));
gleich('ohne Anmeldung wird gewarnt', /Ohne Anmeldung/.test(ohneAnmeldung), true);
gleich('die Warnung nennt /calendar', /\/calendar/.test(ohneAnmeldung), true);
gleich('mit Anmeldung steht das Gegenteil', /Der Kalender ist angemeldet/.test(termine), true);
gleich('mit Anmeldung keine Warnung', /Ohne Anmeldung/.test(termine), false);

const agententext = F.umfangstext('agent', zustand('agent', { root: '/srv/daten' }));
gleich('nennt das Arbeitsverzeichnis', /\/srv\/daten/.test(agententext), true);
gleich('nennt den Weg zum Terminumfang', /\/scope termine/.test(agententext), true);
gleich('lügt nicht über den Schreibmodus', /freigegeben/.test(agententext), false);

const agententextFrei = F.umfangstext('agent', zustand('agent', { root: '/srv/daten', writeEnabled: true }));
gleich('nennt den freigegebenen Schreibmodus', /freigegeben/.test(agententextFrei), true);

console.log();
console.log('— Kein Arbeitsverzeichnis, kein Erfinden —');
const ohneRoot = F.umfangstext('agent', zustand('agent', { root: '' }));
gleich('ohne Verzeichnis wird nicht erfunden', /sobald ein Arbeitsverzeichnis gesetzt ist/.test(ohneRoot), true);
gleich('ohne Verzeichnis steht kein Pfad drin', /\/srv/.test(ohneRoot), false);

console.log();
console.log('— Die Umfangsnamen —');
gleich('termine', F.umfangsname('termine'), 'Terminumfang');
gleich('agent', F.umfangsname('agent'), 'Agentenmodus');

console.log();
console.log('— Der Provider zieht den Umfang mit —');
// Der Umfang folgt dem Provider, nicht dem Benutzer: Das lokale Modell läuft auf
// diesem Rechner und bekommt dort keine Dateiwerkzeuge. Das Backend setzt das
// durch (`wirksamer_umfang`), hier wird nur geprüft, dass die Oberfläche denselben
// Zustand abfragt statt ihn zu erfinden – sonst zeigte sie Werkzeuge an, die es
// nicht gibt, oder verweigerte gültige Schreibzugriffe auf Termine.
gleich('die Provider-Namen', F.PROVIDER_NAMES, ['remote', 'local']);
gleich('entfernt ist nicht lokal', F.lokalesOllama(), false);
gleich('der lokale Provider ist erkannt', mitProvider('local').istLokal, true);
gleich('der entfernte Provider ist nicht lokal', mitProvider('remote').istLokal, false);

// Und der Weg zurück steht im Befehlssatz, sonst wäre der lokale Provider eine
// Einbahnstraße.
const befehlssatz = befehlstext('provider');
gleich('der Befehl nennt beide Provider', /\[remote\|local\]/.test(befehlssatz), true);
gleich('der Befehl nennt den Weg zurück', /\/provider remote/.test(befehlssatz), true);
gleich('lokal ohne Dateiwerkzeuge', /keine Dateiwerkzeuge/.test(befehlssatz), true);

// Die Auswahl im Kopf muss genau die Provider anbieten, die es im Backend gibt.
// Sonst gibt es einen Zustand, den man wählen, aber nicht wieder verlassen kann –
// oder umgekehrt eine Auswahl, die gar nichts bewirkt, weil das Backend sie nicht
// kennt. Die Wörter („Server“, „Lokal“) sind Absicht und dürfen sich gerne ändern,
// die Werte nicht.
const auswahl = [...index.matchAll(/<option value="([a-z]+)"/g)].map((treffer) => treffer[1]);
gleich('die Auswahl bietet genau die Provider', auswahl, F.PROVIDER_NAMES);

console.log();
console.log('— Was nicht vorkommen darf —');
// Diese Prüfung liest den Quelltext, nicht eine Ausgabe: Sie fängt einen späteren
// Umbau ab, bei dem die Rückkehr in den Quelltext wandert, ohne dass eine der
// obigen Zahlen sich ändert.
const verboten = [
  ['in der Terminumfang-Anzeige steht kein Dateipfad', () => !/\/home\/|\/srv\/daten/.test(F.umfangstext('termine', zustand('termine', { root: '/srv/daten' })))],
  ['kein Netzwerkzugriff im Terminumfang', () => !/fetch\(|XMLHttpRequest/.test(src.slice(a, b))],
  ['der Umfang wird nicht aus dem Arbeitsverzeichnis geraten', () => /SCOPE_NAMES\.includes\(config\.scope\)/.test(src)],
];
for (const [name, pruef] of verboten) {
  gleich(name, pruef(), true);
}

console.log();
console.log(fehler === 0
  ? 'Der Umfang verhält sich, wie er soll.'
  : `${fehler} Punkte nachzusehen.`);
process.exit(fehler === 0 ? 0 : 1);
