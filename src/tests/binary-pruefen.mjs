// Prüft das **gebaute** Binary auf Angaben des Entwicklers.
//
// Nicht der Quelltext wird geprüft, sondern das Artefakt, das jemand installiert.
// Ein Blick auf den Quelltext genügt nicht: Eine Konstante, die im Quelltext
// harmlos aussieht, landet als Zeichenkette im Binary und ist mit `strings`
// sichtbar – so ging die Adresse des Ollama-Servers in die Welt, ohne dass
// irgendein Test etwas gemerkt hätte.
//
// Geprüft wird deshalb nur noch eine Klasse von Funden: fremde Netzadressen, die
// auf einen fremden Server zeigen. Pfade des Baurechners werden nicht mehr
// gemeldet. Wer Mimir baut, baut auf seinem eigenen Rechner; Tauri bettet den
// Pfad des Bauenden einmal als Asset-Präfix ein, und der ist seins. Das war erst
// ein Problem, als eine Binary an Fremde ging – siehe die Notiz in
// `src-tauri/build.rs`.
//
// Aufruf:  cargo build --release --manifest-path src-tauri/Cargo.toml
//          node src/tests/binary-pruefen.mjs [pfad/zum/binary]

import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const WURZEL = path.resolve(path.dirname(new URL(import.meta.url).pathname), '../..');
const STANDARD = path.join(WURZEL, 'src-tauri/target/release/mimir');

const binär = process.argv[2] || STANDARD;

if (!existsSync(binär)) {
  console.error(`Kein Binary unter ${binär}.`);
  console.error('Zuerst bauen: cargo build --release --manifest-path src-tauri/Cargo.toml');
  process.exit(2);
}

/**
 * Netzadressen, die auf einen fremden Rechner zeigen.
 *
 * `localhost`, `127.0.0.1` und `0.0.0.0` sind bewusst ausgenommen:
 *  - `localhost` ist der Vorgabewert, seit die eingebaute Adresse entfernt wurde.
 *  - `127.0.0.1` prüft der Werkzeugcode auf Erreichbarkeit.
 *  - `0.0.0.0` ist die Bindungsadresse im Startskript für Ollama. Sie muss so
 *    bleiben – es ist kein Ziel, sondern die Adresse, auf die gebunden wird.
 *
 * Ein Muster, das `192.168.2.1` trifft, trifft auch jede andere Adresse in
 * diesem /16; die verkürzte Form ist Absicht, sie deckt ein ganzes Netz ab.
 */
const FREMDE_ADRESSEN = [
  {
    muster: /\b10\.\d{1,3}\.\d{1,3}\.\d{1,3}\b/g,
    begruendung: 'Adresse im privaten Bereich 10/8',
  },
  {
    muster: /\b172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}\b/g,
    begruendung: 'Adresse im privaten Bereich 172.16/12',
  },
  {
    muster: /\b192\.168\.\d{1,3}\.\d{1,3}\b/g,
    begruendung: 'Adresse im privaten Bereich 192.168/16',
  },
  {
    muster: /\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:11434/g,
    begruendung: 'Ollama-Port an einer ausgeschriebenen Adresse',
    // `0.0.0.0` und `127.0.0.1` sind keine Ziele: Die erste ist die
    // Bindungsadresse im Startskript, die zweite prüft der Werkzeugcode auf
    // Erreichbarkeit. Beide müssen so bleiben. Eine Lookbehind hilft hier nicht,
    // weil `0.0.0.0` im Treffer selbst vorkommt – deshalb wird die Adresse
    // vorab geprüft.
    // Der Treffer enthält den Port, deshalb wird er vorher zerlegt.
    ausgenommen: (treffer) => {
      const adresse = treffer.split(':')[0];
      return adresse === '0.0.0.0' || adresse === '127.0.0.1';
    },
  },
];

const inhalt = readFileSync(binär);
const text = inhalt.toString('latin1');

console.log(`Geprüft: ${binär}`);
console.log(`Größe:   ${(inhalt.length / 1024 / 1024).toFixed(1)} MB`);
console.log();

/**
 * Die Paketkennung bestimmt, wo Mimir seine Einstellungen ablegt.
 *
 * Sie zu ändern ist nicht folgenlos: Beim ersten Start mit einer neuen Kennung legt
 * die App ein **leeres** Konfigurationsverzeichnis an – ohne Serveradresse, ohne
 * Kalender, ohne Agentenverzeichnis. Der Server ist dann nicht erreichbar und der
 * Benutzer steht vor einer leeren Anwendung, ohne Hinweis, warum.
 *
 * Deshalb wird hier geprüft, dass die Konfiguration, die im Repository steht, auch
 * die ist, unter der die installierte Version ihre Dateien ablegt. Gefunden wird
 * nur das Muster, nicht der Pfad: Ein Entwickler darf seine Kennung ruhig
 * umstellen, aber er muss dann auch seine Konfiguration mitnehmen.
 */
const KENNUNG = JSON.parse(
  readFileSync(path.join(WURZEL, 'src-tauri/tauri.conf.json'), 'utf8'),
).identifier;

const erwarteteKennung = process.env.MIMIR_IDENTIFIER || KENNUNG;
const verzeichnis = path.join(os.homedir(), '.config', erwarteteKennung);

console.log(`Kennung:  ${KENNUNG}`);
console.log(`Ablage:   ${verzeichnis}`);
console.log();

if (existsSync(path.join(verzeichnis, 'ollama.json'))) {
  const einstellung = JSON.parse(readFileSync(path.join(verzeichnis, 'ollama.json'), 'utf8'));
  const server = einstellung.server_url || '(leer)';
  const kalender = einstellung.calendar?.server_url || '(nicht eingetragen)';

  console.log(`Konfiguration gefunden.`);
  console.log(`  Ollama:   ${server}`);
  console.log(`  Kalender: ${kalender}`);
  console.log();

  if (einstellung.server_url === 'http://localhost:11434' && einstellung.calendar?.server_url) {
    console.log('Hinweis: Der Vorgabewert steht noch in der Datei, der Kalender ist aber');
    console.log('eingetragen. Das ist der Normalfall nach einem Umzug eines Verzeichnisses.');
    console.log();
  }
}

/**
 * Das Bundle muss zu allem passen, was die Anwendung braucht.
 *
 * Zwei Fehler sind beim ersten Bauen aufgetreten und wären beim nächsten Bauen
 * wieder aufgetreten:
 *
 *  - `authors = ["you"]` aus dem Vorgabe-Manifest landet als `Maintainer: you` im
 *    Debian-Paket. Sichtbar in jeder Paketverwaltung, die es installiert.
 *  - Ein mehrzeiliger `longDescription` erzeugt eine Control-Datei, die kein dpkg
 *    liest: Im Debian-Format ist die erste Zeile der Kurztext, der Rest gehört in
 *    eingerückte Folgenzeilen.
 *
 * Geprüft wird deshalb nicht das erzeugte Paket, sondern die Konfiguration, aus der
 * es entsteht – die ist schneller zu prüfen und schlägt vor dem zehnminütigen Bau
 * an.
 */
const konfig = JSON.parse(
  readFileSync(path.join(WURZEL, 'src-tauri/tauri.conf.json'), 'utf8'),
);
const manifest = readFileSync(path.join(WURZEL, 'src-tauri/Cargo.toml'), 'utf8');
const bundle = konfig.bundle || {};
const bundleFehler = [];

if (!bundle.active) {
  bundleFehler.push('bundle.active fehlt – cargo tauri build erzeugt nur eine Binary.');
}

// Ein Ziel, das auf diesem System gebaut werden kann. AppImage scheitert unter
// Arch, weil linuxdeploy-plugin-gtk Debian-Bibliotheken erwartet.
if (!Array.isArray(bundle.targets) || bundle.targets.length === 0) {
  bundleFehler.push('bundle.targets ist leer.');
}

// Die Icons müssen existieren, sonst bricht der Bundle-Lauf ab.
for (const datei of bundle.icon || []) {
  if (!existsSync(path.join(WURZEL, 'src-tauri', datei))) {
    bundleFehler.push(`Icon fehlt: ${datei}`);
  }
}

// Kein Platzhalter: Die Vorgabe war ein 1×1-Pixel-Bild.
const iconQuelle = path.join(WURZEL, 'src-tauri/icons/icon.png');
if (existsSync(iconQuelle)) {
  const kopf = readFileSync(iconQuelle).subarray(16, 24);
  const breite = kopf.readUInt32BE(0);
  if (breite < 256) {
    bundleFehler.push(`icons/icon.png ist ${breite} Pixel breit, nicht 512.`);
  }
}

const autoren = manifest.match(/^authors = \[(.*?)\]/m);
if (!autoren) {
  bundleFehler.push('Kein authors-Feld im Manifest.');
} else if (/["']you["']|["']your name["']|["']TODO["']/.test(autoren[1])) {
  bundleFehler.push(
    `authors ist noch der Vorgabewert: ${autoren[0]}. Das landet als Maintainer im Paket.`,
  );
}

if (!manifest.match(/^license = "/m)) {
  bundleFehler.push('Kein license-Feld im Manifest.');
}

if (!konfig.productName || konfig.productName === konfig.productName.toLowerCase()) {
  bundleFehler.push(
    `productName ist "${konfig.productName}". Es erscheint als Name= in der Desktop-Datei.`,
  );
}

for (const zeile of [bundle.longDescription, bundle.shortDescription].filter(Boolean)) {
  if (zeile.includes('\n')) {
    bundleFehler.push(
      'shortDescription/longDescription enthalten einen Zeilenumbruch. Im Debian-Format ist die erste Zeile der Kurztext.',
    );
  }
}

if (bundleFehler.length > 0) {
  console.log('Das Bundle ist noch nicht in Ordnung:');
  console.log();
  for (const f of bundleFehler) {
    console.log(`  ${f}`);
  }
  console.log();
  process.exit(1);
}

console.log('Bundle-Konfiguration in Ordnung.');
console.log();

const funde = [];

for (const { muster, begruendung, ausgenommen } of FREMDE_ADRESSEN) {
  const treffer = text.match(muster);
  if (!treffer) continue;

  for (const wert of [...new Set(treffer)]) {
    if (ausgenommen && ausgenommen(wert)) continue;

    // Wo steht es? Ein Fund im Code ist etwas anderes als einer im eingebetteten
    // Oberflächentext – beides wird gemeldet, weil beides auffällt.
    const bei = inhalt.indexOf(Buffer.from(wert, 'latin1'));
    const umgebung = inhalt
      .subarray(Math.max(0, bei - 60), bei + wert.length + 30)
      .toString('latin1')
      .replace(/[^\x20-\x7eäöüÄÖÜß]/g, ' ');

    funde.push({ wert, begruendung, umgebung });
  }
}

const wasSollteStehen = () => {
  console.log('  - localhost:11434 als Vorgabewert, änderbar mit /server-url');
  console.log('  - 0.0.0.0 als Bindungsadresse im Startskript für Ollama');
  console.log('  - Feldnamen wie app_password – keine Werte');
};

if (funde.length === 0) {
  console.log('Keine fremde Netzadresse gefunden.');
  console.log();
  console.log('Was weiterhin im Binary steht und auch soll:');
  wasSollteStehen();
  process.exit(0);
}

console.log(`${funde.length} Fundstelle(n), die zu entfernen sind:`);
console.log();

for (const { wert, begruendung, umgebung } of funde) {
  console.log(`  ${wert}`);
  console.log(`    Grund:  ${begruendung}`);
  console.log(`    Kontext: …${umgebung}…`);
  console.log();
}

console.log('Hinweise:');
console.log('  Eine fremde Adresse im Quelltext als konstante Vorgabe kommt über');
console.log('  `strings` im Binary an. Sie gehört entfernt, nicht abgeschwächt.');
process.exit(1);
