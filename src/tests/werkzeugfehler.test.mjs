// Prüft, was ein gescheiterter Werkzeugaufruf im Chat hinterlässt.
//
// Der Fehler, den diese Datei verhindert: `delete_email` schlug fehl, die Mail
// wurde nicht gelöscht – und im Chat stand ein **zugeklappter** Schritt in
// blassem Grau. Der Benutzer sah ein Werkzeug, das nichts getan hatte. Das
// Modell, dem der Fehlertext korrekt zurückgegeben wurde, schrieb danach
// "Die E-Mail kann nun gelöscht werden" und legte den Aufruf als JSON hin.
//
// Zwei Fehler an einer Stelle, und beide sind derselbe: **Ein Fehler, den man
// nicht sieht, wird zu einem Erfolg, von dem niemand weiß, dass er keiner ist.**
//
// Geprüft wird nicht die Farbe, sondern die drei Zusagen:
//  1. Ein gescheiterter Schritt klappt auf und bekommt die Fehlermarke.
//  2. Der Text, den das Modell bekommt, sagt unmissverständlich, dass nichts
//     geschehen ist – nicht als Nebensatz in einer Fehlermeldung.
//  3. Weder der Fehlertext noch das Werkzeugergebnis werden verworfen.
//
// Aufruf:  node src/tests/werkzeugfehler.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(path.dirname(new URL(import.meta.url).pathname), '..', 'main.js');
const src = fs.readFileSync(QUELLE, 'utf8');

let fehler = 0;
const gleich = (name, ist, erwartet) => {
  if (ist === erwartet) {
    console.log(`ok   ${name}`);
    return true;
  }
  console.error(`FAIL ${name}\n  erwartet ${erwartet}\n  war      ${ist}`);
  fehler += 1;
  return false;
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

const schreibe = funktion('runWriteCall');
const setzeFehler = funktion('setAgentStepFehler');

if (!schreibe || !setzeFehler) {
  console.error('FAIL runWriteCall/setAgentStepFehler nicht gefunden');
  process.exit(2);
}

console.log('— Ein gescheiterter Vorschauaufruf ist ein Fehler, kein Vorgang —');

// **Beide Enden von `runWriteCall` laufen über dieselbe Funktion.** Vorher
// stand im Zweifelsfall `addAgentStep` mit dem Text im Ausgabefeld: Der Schritt
// bekam den Grund, aber weder die Fehlerfarbe noch das Aufklappen. Genau diese
// eine Zeile ist der ganze Unterschied zwischen „der Benutzer sieht, was
// schiefging" und „der Benutzer sieht ein Werkzeug, das nichts getan hat".
const fehlerpfade = schreibe.match(/if \(decision\.previewError\) \{[\s\S]*?\n    \}/);
if (!fehlerpfade) {
  console.error('FAIL der Zweig für einen gescheiterten Vorschauaufruf nicht gefunden');
  process.exit(2);
}
const vorschaufehler = fehlerpfade[0];

gleich(
  'der Vorschaufehler geht durch setAgentStepFehler',
  /setAgentStepFehler\(entry, decision\.previewError\)/.test(vorschaufehler),
  true,
);
// Und er darf sich nicht am Aufklappen vorbeimogeln.
gleich(
  'und nicht an der Fehlermarke vorbei',
  /classList\.add\('agent-step-done', 'agent-step-fehler'\)/.test(setzeFehler),
  true,
);
gleich(
  'und klappt auf',
  /aria-expanded', 'true'/.test(setzeFehler) && /output\.hidden = false/.test(setzeFehler),
  true,
);

// **Nicht `agent-step-failed`.** Diese Marke gibt es auch, sie ist aber die
// blasse: Sie gehört zu einem abgelehnten Vorgang, der gewollt scheiterte. Ein
// technischer Fehler sieht anders aus, und er soll es.
gleich(
  'nicht mit der blassen Reject-Marke',
  !/agent-step-failed/.test(vorschaufehler),
  true,
);

console.log();
console.log('— Auch ein Fehler nach dem Bestätigen ist ein Fehler —');

// **Der Benutzer hat hier schon auf „Bestätigen" geklickt.** Ein Fehler, der
// danach zugeklappt und blass bleibt, ist hier doppelt falsch: Er hat zugestimmt
// und sähe nicht, dass nichts geschehen ist.
const ausfuehrungsfehler = schreibe.match(/\} catch \(error\) \{[\s\S]*?\n    \}\n\}/);
if (!ausfuehrungsfehler) {
  console.error('FAIL der Zweig für eine gescheiterte Ausführung nicht gefunden');
  process.exit(2);
}
gleich(
  'der Ausführungsfehler geht durch setAgentStepFehler',
  /setAgentStepFehler\(entry, String\(error\)\)/.test(ausfuehrungsfehler[0]),
  true,
);

console.log();
console.log('— Das Modell darf daraus keinen Erfolg machen —');

// **Der eigentliche Schaden.** Das Modell bekam den Fehlertext und meldete dem
// Benutzer trotzdem, die Mail sei gelöscht. Grund: Der Text war ein Nebensatz in
// einer Meldung über einen anderen Vorgang. Diese drei Wörter stehen jetzt allein
// in einem Satz, und der Satz steht vor der Meldung, nicht danach.
//
// Ausgefiltert wird nach dem, woran ein Fehler zu erkennen ist – nicht nach der
// Position. `runWriteCall` schiebt auch den vom Benutzer **abgelehnten** Vorgang
// als Werkzeugergebnis zurück, und der gehört nicht in diese Prüfung: Dort ist
// nichts geschehen, aber aus freien Stücken, und der Benutzer weiß es.
const an_das_modell = schreibe.match(/role: 'tool',[\s\S]{0,900}?\n        \}\);/g) ?? [];
const texte = an_das_modell
  .map((block) => block.replace(/'\s*\+\s*'/g, ''))
  .filter((text) => /NICHT ausgeführt/.test(text));

gleich(
  'es gibt genau zwei Fehlertexte: Vorschaufehler und Ausführungsfehler',
  texte.length,
  2,
);
for (const [nr, text] of texte.entries()) {
  gleich(
    `Text ${nr + 1} sagt, dass nichts ausgeführt wurde`,
    /wurde NICHT ausgeführt/.test(text),
    true,
  );
  // Groß- und Kleinschreibung spielen keine Rolle: Der eine Text steht am
  // Satzanfang, der andere mitten im Satz.
  gleich(
    `Text ${nr + 1} sagt, dass nichts verändert wurde`,
    /es wurde nichts verändert/i.test(text),
    true,
  );
  // **Und es untersagt den Erfolg ausdrücklich.** Ohne diesen Satz bleibt die
  // Formulierung der des Modells überlassen – und die war falsch.
  gleich(
    `Text ${nr + 1} verbietet, einen Erfolg zu melden`,
    /Sag (dem Benutzer|ihm) nicht, dass es geschehen ist/.test(text),
    true,
  );
  // Und er nennt die Meldung, statt sie zu ersetzen: Eine Fehlermeldung, die das
  // Modell nicht kennt, kann es dem Benutzer auch nicht weitergeben.
  gleich(
    `Text ${nr + 1} behält die Meldung des Backends`,
    /decision\.previewError/.test(text) || /\$\{error\}/.test(text),
    true,
  );
}

// Und der abgelehnte Vorgang bleibt unangetastet: Dort ist „es wurde nichts
// verändert" richtig, aber die Warnung vor einem erfundenen Erfolg wäre dort
// eine Behauptung, die nichts belegt.
const abgelehnt = an_das_modell.filter((block) => /abgelehnt/.test(block));
gleich(
  'der abgelehnte Vorgang wird nicht zur Fehlermeldung umgedeutet',
  abgelehnt.length,
  1,
);

console.log();
console.log('— Nichts wird stillschweigend weggeworfen —');

// Ein `catch`, das den Fehler nur zählt, wäre dasselbe Problem noch einmal:
// Der Benutzer bekäme eine leere Antwort und wüsste nicht, warum.
gleich(
  'der Fehler wird nicht verschluckt',
  !/catch \([^)]*\) \{\s*\}/.test(schreibe) && !/console\.error\([^)]*error[^)]*\);\s*\n?\s*return;\s*\n\s*\}/.test(schreibe),
  true,
);

console.log();
console.log(fehler === 0 ? 'Ein Fehler ist zu sehen.' : `${fehler} Punkte nachzusehen.`);
process.exit(fehler === 0 ? 0 : 1);
