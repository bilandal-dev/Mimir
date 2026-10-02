// Prüft die Anleitung, die beim ersten Start im Chat erscheint.
//
// Ohne Fenster prüfbar, weil nur der Text gebildet wird.
//
// Der Text wird **nicht** aus dem Quelltext zerlegt. Das war der Fehler in einer
// früheren Fassung: Ein Schnitt über die Zeilenliste hat je nach Version 21 oder 44
// Zeilen geliefert, ohne dass eine der beiden Zahlen falsch gewesen wäre – nur die
// Zerlegung war falsch. Die Liste wird hier stattdessen ausgewertet, indem sie
// gleich läuft.

import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';

// Der Pfad ist **relativ zum Repository** und nicht absolut: Die Prüfskripte
// sollen auch dort laufen, wo Mimir nicht unter /home/bilandal/Documents/Mimir
// liegt. Ein absoluter Pfad ließ jedes Skript stillschweigend fehlschlagen,
// sobald jemand den Ordner kopiert oder aus dem Quelltext gebaut hat – und
// `ersteinrichtung.test.mjs` lädt die Datei, prüft daran den Text und meldet
// dann nichts, was auf den echten Fehler hindeutet.
const QUELLE = path.join(
  path.dirname(new URL(import.meta.url).pathname),
  '..',
  'main.js',
);
const src = fs.readFileSync(QUELLE, 'utf8');
const a = src.indexOf('async function zeigeErsteinrichtung');
const b = src.indexOf('// Reihenfolge: Erst');
if (a < 0 || b < 0) {
  console.error('zeigeErsteinrichtung nicht gefunden.');
  process.exit(2);
}

/**
 * Ruft die Anleitung auf und fängt das ab, was im Chat stünde.
 *
 * `zeigeErsteinrichtung` schreibt in den DOM. Hier wird nur der Text gebraucht,
 * also wird `appendMessageToUI` durch eine Attrappe ersetzt, die den Text
 * abgreift. Das ist der Weg, den man auch für den echten Ablauf gehen sollte:
  ausführen statt erraten, wie der Quelltext zu lesen ist.
 */
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

// Ohne Fenster asynchron ist das Warten der einzige Weg. Die Funktion ist asynchron,
// weil sie `invoke` aufruft.
async function holeTextAsync(optionen) {
  let ergebnis;
  const sandkasten = { console: { error() {} } };
  void sandkasten;
  ergebnis = await new Promise((fertig) => {
    const gefangen = [];
    const knoepfe = [];
    const {
      server_eingetragen = false,
      kalender_eingetragen = false,
      server_vorgabe = 'http://localhost:11434',
    } = optionen || {};

    const sandkasten = {
      console: { error() {} },
      invoke: async (befehl) => {
        if (befehl === 'get_einrichtung') {
          return {
            server_eingetragen,
            server_vorgabe,
            kalender_eingetragen,
          };
        }
        throw new Error(`Unerwarteter Aufruf: ${befehl}`);
      },
      // Die Anleitung erzeugt ihre Blase selbst über `document.createElement`; das
      //_attrappe muss also ein vollständiges Knotenobjekt liefern, nicht nur einen
      // Zettel.
      document: {
        createElement: (tag) => {
          // `knoten` wird unten in `classList` gebraucht, bevor es fertig ist.
          const knoten = {
            _t: '',
            set textContent(wert) {
              this._t = wert;
            },
            get textContent() {
              return this._t;
            },
            // Die Klassen stehen in `classList`, nicht in `className`. Der Knoten
            // wird deshalb über eine Schließvariable festgehalten: In `add` ist
            // `this` das Klassenobjekt, und ein `this._klassen` landete dort statt
            // am Knoten – die Rolle bliebe leer und die Prüfung wirkungslos.
            classList: {
              add(...klassen) {
                knoten._klassen = (knoten._klassen ? `${knoten._klassen} ` : '') + klassen.join(' ');
              },
            },
            after() {},
            remove() {},
            addEventListener(ereignis, funktion) {
              if (ereignis === 'click') knoten._klick = funktion;
            },
          };
          if (tag === 'button') knoepfe.push(knoten);
          return knoten;
        },
      },
      // Die Blase wird an `chatContainer` gehängt; hier ist das der Ort, an dem der
      // Text abgefangen wird. Die Funktion schreibt nicht mehr über
      // `appendMessageToUI` – sie erzeugt ihre Nachricht selbst, weil sie eine
      // eigene Klasse braucht und keine zentrierten Systemzeilen sein soll.
      chatContainer: {
        appendChild(knoten) {
          gefangen.push({ rolle: knoten._klassen || '', inhalt: knoten._t });
        },
      },
      openServerUrlDialog: () => gefangen.push({ rolle: 'aufruf', inhalt: 'dialog' }),
      scrollToBottom() {},
      focusPromptInput() {},
    };

    vm.createContext(sandkasten);
    vm.runInContext(
      `${src.slice(a, b)}\nglobalThis.__f = zeigeErsteinrichtung;`,
      sandkasten,
    );
    sandkasten.__f().then(() => fertig({ nachrichten: gefangen, knoepfe }));
  });
  return ergebnis;
}

const erst = await holeTextAsync();
const text = erst.nachrichten[0]?.inhalt || '';

console.log('— Der Text, wie er im Chat erscheint —');
console.log();
console.log(text);
console.log();
console.log(`(${text.split('\n').length} Zeilen, Rolle "${erst.nachrichten[0]?.rolle}")`);
console.log();

console.log('— Sie erscheint, solange nichts eingetragen ist —');
gleich('genau eine Nachricht', erst.nachrichten.length, 1);
gleich('ein Knopf zum Einstellen', erst.knoepfe.length, 1);
gleich('Nach dem Einrichten keine mehr', (await holeTextAsync({ server_eingetragen: true })).nachrichten.length, 0);
gleich(
  'Ohne Kalender trotzdem eine',
  (await holeTextAsync({ kalender_eingetragen: true })).nachrichten.length,
  1,
);

// Der Knopf muss den Dialog öffnen. Sonst steht die Anleitung da und der Nutzer
// weiß nicht, wohin mit dem Finger.
erst.knoepfe[0]?._klick?.();
gleich('der Knopf öffnet den Adressdialog', erst.nachrichten.some((m) => m.rolle === 'aufruf'), true);

console.log();
console.log('— Was drin sein muss —');
gleich('nennt /help', /\/help/.test(text), true);
gleich('nennt /calendar', /\/calendar/.test(text), true);
gleich('sagt, dass der Kalender freiwillig ist', /freiwillig/.test(text), true);
gleich('nennt einen Beispiel-Hostnamen', /ollama\.example\.org/.test(text), true);
gleich('nennt die Vorgabe', /http:\/\/localhost:11434/.test(text), true);
gleich('nennt ollama list', /ollama list/.test(text), true);
gleich('nennt ollama serve', /ollama serve/.test(text), true);

console.log();
console.log('— Was nicht drin sein darf —');
gleich('keine Adresse aus 192.168', /192\.168\./.test(text), false);
gleich('keine Adresse aus 10.', /\b10\.\d{1,3}\.\d{1,3}\.\d{1,3}\b/.test(text), false);
gleich('kein Name des Entwicklers', /bilandal|kaiweber/i.test(text), false);
gleich('kein absoluter Pfad', /\/home\/[a-z]/.test(text), false);

console.log();
console.log(fehler === 0 ? 'Die Anleitung ist in Ordnung.' : `${fehler} Punkte nachzusehen.`);
process.exit(fehler === 0 ? 0 : 1);
