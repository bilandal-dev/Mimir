// Prüft den Aufbau der lokalen Modellliste: was in welcher Gruppe steht, wann
// die Auswahl gesperrt ist und wie eine Größe ausgewiesen wird.
//
// Ohne Fenster prüfbar, weil die Entscheidungen in einem eigenen Stück stehen,
// das kein DOM anfasst (siehe `// --- Der Aufbau der Liste, ohne DOM ---` in
// `main.js`). Geprüft wird nicht das Aussehen, sondern was der Benutzer zu sehen
// bekommt – ein Modell, das in der falschen Gruppe steht oder eine Auswahl
// freigibt, obwohl nichts laufen kann, fällt erst beim Senden auf.
//
// Aufruf:  node src/tests/lokale-modelle.test.mjs

import fs from 'node:fs';
import path from 'node:path';

const QUELLE = path.join(
  path.dirname(new URL(import.meta.url).pathname),
  '..',
  'main.js',
);
const src = fs.readFileSync(QUELLE, 'utf8');

// Nur der reine Teil zwischen den beiden Markern. Der Rest braucht ein Fenster,
// `document` und den Tauri-Aufruf – und würde hier nichts über die Entscheidung
// aussagen.
const a = src.indexOf('// --- Der Aufbau der Liste, ohne DOM ---');
const b = src.indexOf('// --- Ende der prüfbaren Stücke ---');
if (a < 0 || b < 0 || b <= a) {
  console.error('Der prüfbare Teil der Modellliste wurde nicht gefunden.');
  process.exit(2);
}

const F = new Function(
  `${src.slice(a, b)}\nreturn { gruppeAngebote, groessentext, empfehlungsgrund, MODELLE_DIALOG };`,
)();

// Ein Angebot, wie es das Backend schickt. Der Katalog liefert vier Felder dazu,
// die hier nichts entscheiden – die Auswahl schaut nur auf `geladen` und
// `empfohlen`.
const angebot = (id, geladen, empfohlen) => ({
  id,
  name: id,
  geladen,
  empfohlen,
  groesse_mib: 469,
  quantisierung: 'Q4_K_M',
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

console.log('— Die Auswahl trennt nach „liegt es hier“ —');
// Der Katalog ist aufsteigend sortiert: Das kleinste passende Modell steht vorn.
const voll = [
  angebot('qwen2.5-0.5b', false, true),
  angebot('qwen2.5-1.5b', false, true),
  angebot('qwen2.5-3b', false, true),
];
const gruppen = F.gruppeAngebote(voll);
gleich('ohne Download ist nichts geladen', gruppen.geladen, []);
gleich('alle drei sind Empfehlung', gruppen.empfohlen.map((a) => a.id), [
  'qwen2.5-0.5b',
  'qwen2.5-1.5b',
  'qwen2.5-3b',
]);

// Ein geladenes Modell kommt in die erste Gruppe – auch wenn der Katalog es
// heute nicht mehr empfiehlt. Der Benutzer hat es sich ausgesucht; das Backend
// entscheidet, nicht die Empfehlung.
const gemischt = F.gruppeAngebote([
  angebot('qwen2.5-0.5b', false, true),
  angebot('qwen2.5-3b', true, false),
  angebot('qwen2.5-1.5b', false, true),
]);
gleich('das geladene steht vorn', gemischt.geladen.map((a) => a.id), ['qwen2.5-3b']);
// Und es taucht **nicht** noch einmal als Empfehlung auf: Wer die Auswahl
// aufklappt, soll es nicht doppelt sehen.
gleich('das geladene steht nicht auch als Empfehlung drin', gemischt.empfohlen.map((a) => a.id), [
  'qwen2.5-0.5b',
  'qwen2.5-1.5b',
]);

console.log();
console.log('— Ein Modell, das weder geladen noch empfohlen ist, bleibt sichtbar —');
// Das ist der Fall eines Rechners ohne AVX2 oder mit zu wenig Speicher: Der Katalog
// empfiehlt nichts, aber die Modelle sind vorhanden und lauchbar. Sie wegzulassen
// hieße, die Engine sei auf diesem Rechner unerreichbar, obwohl sie im Programm
// steckt – die Empfehlung ist ein Rat, kein Verbot.
const zu_klein = F.gruppeAngebote([
  angebot('qwen2.5-0.5b', false, false),
  angebot('qwen2.5-3b', false, false),
]);
gleich('nichts geladen', zu_klein.geladen, []);
gleich('nichts empfohlen', zu_klein.empfohlen, []);
gleich(
  'beide stehen trotzdem in der Auswahl',
  zu_klein.ohneEmpfehlung.map((a) => a.id),
  ['qwen2.5-0.5b', 'qwen2.5-3b'],
);

// Und in der Mischung: Empfehlung und Rest stehen getrennt, aber beide da. Wer
// eine Empfehlung befolgt, verliert dadurch nichts.
const gemischt_ohne_geladen = F.gruppeAngebote([
  angebot('qwen2.5-0.5b', false, true),
  angebot('qwen2.5-1.5b', false, true),
  angebot('qwen2.5-3b', false, false),
]);
gleich(
  'die Empfehlung steht für sich',
  gemischt_ohne_geladen.empfohlen.map((a) => a.id),
  ['qwen2.5-0.5b', 'qwen2.5-1.5b'],
);
gleich(
  'der Rest steht auch da',
  gemischt_ohne_geladen.ohneEmpfehlung.map((a) => a.id),
  ['qwen2.5-3b'],
);

// Ein heruntergeladenes Modell, das der Katalog nicht mehr empfiehlt, gehört in
// die erste Gruppe und **nicht** in den Rest – sonst stünde es zweimal.
const geladen_ohne_empfehlung = F.gruppeAngebote([
  angebot('qwen2.5-3b', true, false),
  angebot('qwen2.5-0.5b', false, false),
]);
gleich('das geladene steht vorn', geladen_ohne_empfehlung.geladen.map((a) => a.id), ['qwen2.5-3b']);
gleich(
  'das geladene steht nicht auch im Rest',
  geladen_ohne_empfehlung.ohneEmpfehlung.map((a) => a.id),
  ['qwen2.5-0.5b'],
);

console.log();
console.log('— Der Dialogeintrag kann kein Modell sein —');
// Der Eintrag, der den Dialog öffnet, trägt einen Wert, den keine Kennung haben
// kann. Andernfalls würde er beim Senden als unbekanntes Modell gehen.
gleich('der Wert unterscheidet sich vom Muster', /\s/.test(F.MODELLE_DIALOG), false);
gleich('der Wert ist kein gewöhnlicher Name', F.MODELLE_DIALOG.startsWith('__'), true);

console.log();
console.log('— Größen —');
// `469 MB` und nicht `0,5 GB`: 469 MiB sind 0,46 GB, und eine aufgerundete Zahl
// läge über der Wahrheit.
gleich('unter einem Gigabyte in MB', F.groessentext(469), '469 MB');
gleich('ein Gigabyte', F.groessentext(1024), '1,0 GB');
// 2008 MiB sind 1,96 GB – hier wird nicht auf 2 gerundet, denn „2 GB“ läge über
// der Wahrheit und der Download wäre um 40 MB größer als vorhergesagt.
gleich('über einem Gigabyte mit Nachkommastelle', F.groessentext(2008), '2,0 GB');
gleich('1,5 GB mit deutschem Komma', F.groessentext(1536), '1,5 GB');

console.log();
console.log('— Der Grund, den der Benutzer zu hören bekommt —');
// Hier entscheidet sich, was jemand über seinen Rechner erfährt. Geprüft wird der
// Satz, nicht die Zahl: „3 GB" sagt niemandem etwas, „dafür fehlen dir 1 GB“ schon.
const hardware = (frei, schnell) => ({
  ram_gib: 32,
  ram_frei_gib: frei,
  befehlssaetze: { avx2: schnell },
});
const klein = { ram_mindestens_gib: 2 };
const gross = { ram_mindestens_gib: 4 };

// Der Fall dieses Rechners: kein AVX2, aber Speicher genug für das kleine Modell.
const prozessor = F.empfehlungsgrund(klein, hardware(3, false));
gleich('nennt, dass der Speicher reicht', /Arbeitsspeicher reicht/.test(prozessor), true);
gleich('nennt den Prozessor als Grund', /Prozessor/.test(prozessor), true);
gleich('sagt, dass Laden möglich ist', /trotzdem möglich/.test(prozessor), true);

// Und der andere: schnelle Kerne, aber zu wenig Speicher. Dann darf der Satz
// *nicht* den Prozessor zum Grund erklären – der ist hier in Ordnung.
const speicher = F.empfehlungsgrund(gross, hardware(2, true));
gleich('nennt die fehlende Menge', /fehlen dir 2 GB/.test(speicher), true);
gleich('nennt den freien Speicher', /frei sind 2 GB/.test(speicher), true);
gleich(
  'erklärt hier nicht den Prozessor zum Grund',
  /Prozessor nicht/.test(speicher),
  false,
);

// Genau auf der Grenze: 3 GB frei bei 3 GB Bedarf. Es reicht – knapp.
gleich(
  'auf der Grenze reicht es',
  /reicht dafür/.test(F.empfehlungsgrund({ ram_mindestens_gib: 3 }, hardware(3, false))),
  true,
);

// Ohne auslesbaren Speicher darf kein erfundener Grund stehen.
gleich(
  'ohne Speicherangabe wird das gesagt',
  /ließ sich nicht auslesen/.test(
    F.empfehlungsgrund(klein, { ram_frei_gib: null, befehlssaetze: null }),
  ),
  true,
);

console.log();
console.log(fehler === 0
  ? 'Die Modellliste verhält sich, wie sie soll.'
  : `${fehler} Punkte nachzusehen.`);
process.exit(fehler === 0 ? 0 : 1);