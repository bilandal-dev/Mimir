// Prüfung der Datumsangabe, die bei jeder Anfrage an das Modell geht.
//
// Ohne Fenster prüfbar, weil nur die Textbildung getestet wird: Die Systemuhr
// wird für jeden Durchlauf auf einen anderen Tag gestellt, und die Ausgabe wird
// gegen eine unabhängig nachgerechnete Erwartung gehalten.
//
// Warum das eine eigene Datei ist: Die Fehler, die hier auftraten, sind an einem
// einzigen Tag unsichtbar. „Sonntag“ wurde zur „übernächsten Woche“, und das fiel
// an drei von sieben Tagen nicht auf. Ein Test mit einem festen Startdatum prüft
// genau eine dieser Lagen – und läuft um Mitternacht von selbst schief.
//
// Aufruf:  node src/tests/datumsangabe.test.mjs

import fs from 'node:fs';
import path from 'node:path';

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
// Nur die Datumsbildung herausschneiden, nicht die ganze Oberfläche: Sie ruft
// Tauri-Befehle auf und wäre ohne Fenster nicht ladbar.
const teil = src.slice(src.indexOf('const WOCHENTAG_LANG'), src.indexOf('// Die Farbe eines Kalenders'));
// In eine Funktion einschließen, damit die Namen erreichbar sind.
const F = new Function(`${teil}\nreturn { datumsangabeFuerModell, wochenname };`)();

const Echt = Date;
const pad = (n) => String(n).padStart(2, '0');

function mitUhr(jahr, monat0, tag) {
  globalThis.Date = class extends Echt {
    constructor(...a) { a.length ? super(...a) : super(jahr, monat0, tag, 9, 0); }
    static now() { return new Echt(jahr, monat0, tag, 9, 0).getTime(); }
  };
  try { return F.datumsangabeFuerModell(); } finally { globalThis.Date = Echt; }
}

const WOCHENTAGE = ['Sonntag', 'Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag'];
const WOCHENNAMEN = { 0: 'laufenden', 1: 'nächsten', 2: 'übernächsten' };

// Die Zeilen aus der Ausgabe lesen: „  2026-10-04 ist ein Sonntag in der laufenden.“
function liesTage(text) {
  const tage = new Map();
  for (const zeile of text.split('\n')) {
    const treffer = zeile.match(/(\d{4}-\d{2}-\d{2}) ist ein ([^ ]+) in der ([^.]+)\./);
    if (treffer) tage.set(treffer[1], { wochentag: treffer[2], woche: treffer[3] });
  }
  return tage;
}

let geprueft = 0;
let fehler = 0;
const bemerkt = [];

// Drei Jahre, jeder einzelne Tag als HEUTE – auch über Monats-, Jahres- und
// Schaltjahresgrenzen hinweg.
for (let versatz = 0; versatz < 3 * 365; versatz += 1) {
  // UTC als Rechenbasis, damit die Schleife nicht an Sommerzeit hängt.
  const d = new Echt(Echt.UTC(2026, 9, 1) + versatz * 86_400_000);
  const heute = mitUhr(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
  const tage = liesTage(heute);

  for (let k = 1; k <= 7; k += 1) {
    const f = new Echt(Echt.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()) + k * 86_400_000);
    const schluessel = `${f.getFullYear()}-${pad(f.getMonth() + 1)}-${pad(f.getDate())}`;

    // Erwartung, unabhängig nachgerechnet: über den Montag der Woche.
    const montagHeute = Echt.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate())
      - ((d.getUTCDay() + 6) % 7) * 86_400_000;
    const montagFolge = Echt.UTC(f.getFullYear(), f.getMonth(), f.getDate())
      - ((f.getDay() + 6) % 7) * 86_400_000;
    const wochenNr = Math.round((montagFolge - montagHeute) / (7 * 86_400_000));

    const erwartetWochen = WOCHENNAMEN[wochenNr] ?? 'späteren';
    const erwartetWochentag = WOCHENTAGE[f.getDay()];
    const ist = tage.get(schluessel);

    geprueft += 1;
    if (!ist) {
      fehler += 1;
      if (bemerkt.length < 5) bemerkt.push(`fehlt ${schluessel}\n   Ausgabe heute=${d.getFullYear()}-${pad(d.getMonth()+1)}-${pad(d.getUTCDate())}:\n${[...tage].map(([k,v])=>'     '+k+' -> '+v.wochentag+' / '+v.woche).join('\n')}`);
    } else if (ist.wochentag !== erwartetWochentag) {
      fehler += 1;
      if (bemerkt.length < 5) bemerkt.push(`Wochentag ${schluessel}: ${ist.wochentag} statt ${erwartetWochentag}`);
    } else if (ist.woche !== erwartetWochen) {
      fehler += 1;
      if (bemerkt.length < 5) bemerkt.push(`Woche ${schluessel}: ${ist.woche} statt ${erwartetWochen}`);
    }
  }
}

console.log(bemerkt.join('\n'));
console.log();
console.log(`${geprueft} Folgetage an 1095 verschiedenen HEUTE geprüft, ${fehler} Abweichungen.`);
if (fehler === 0) {
  console.log('Damit ist das Datum über drei Jahre hinweg geprüft, nicht an einem Tag.');
}
process.exit(fehler === 0 ? 0 : 1);
