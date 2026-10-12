/**
 * **Prüft, dass kein Passwort in den Quelltext gerät.**
 *
 * `binary-pruefen.mjs` schaut in das *gebaute* Programm, ob fremde Adressen
 * darin stehen. Diese Prüfung schaut in den *Quelltext* – also in genau das, was
 * in den nächsten Commit geht. Beides ist nötig und nichts ersetzt das andere:
 * Eine fremde Adresse landet als Zeichenkette im Binary, ein Passwort tut es
 * nicht, sondern steht schon vorher im Repository.
 *
 * **Der Fall, um den es hier geht.** Google-App-Passwörter sind sechzehn
 * Kleinbuchstaben. Wer eines zum Ausprobieren in `mail_login` einträgt, baut es
 * ein; das Programm funktioniert, der Commit geht raus, und das Passwort ist
 * fort – bei Google bis zum nächsten Widerruf, bei einem eigenen Server für
 * immer, weil es in der Historie steht. Ein `git commit --amend` holt es nicht
 * mehr zurück.
 *
 * Aufruf:  node src/tests/keine-geheimnisse.mjs
 */

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, relative, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const WURZEL = fileURLToPath(new URL('../..', import.meta.url));

/** Was nicht in den Commit gehört – auch nicht als Testwert. */
const GEHEIM_DATEIEN = [
  'ollama.json',
  'chat-history.json',
  'calendar-secret.json',
];

/** Dateiendungen, die hier überhaupt gelesen werden. */
const ENDUNGEN = new Set(['.rs', '.js', '.mjs', '.html', '.css', '.toml', '.json', '.yml', '.md', '.sh']);

/** Verzeichnisse, die nie eingelesen werden. */
const AUSGESCHLOSSEN = new Set([
  '.git', 'target', 'node_modules', 'gen', 'icons', 'dist', 'build',
]);

/**
 * Werte, die in einem Passwortfeld stehen dürfen, weil sie nichts sind.
 *
 * **Nur offensichtliche Platzhalter.** Die Liste ist kurz und bleibt kurz: Jeder
 * Eintrag hier ist ein Wort, bei dem jeder im Prüfskript liest „ja, das ist
 * keine echte Zugangsangabe". Ein echter Schlüssel darf nicht neben echten
 * Dingen in dieser Liste stehen.
 */
const PLATZHALTER = [
  'geheim', 'passwort', 'password', 'test', 'example', 'beispiel', 'dummy',
  'leer', 'leerzeichen', 'todo', 'platzhalter', 'app-passwort', 'app_password',
  'kaitexample', 'kai@example.org', 'a@example.org', 'x', 'n/a', '-',
];

/** Nimmt Kommentare und Zeichenketlen escaped aus dem Weg. */
function ohneKommentare(inhalt) {
  return inhalt
    // `/* … */`, auch über mehrere Zeilen
    .replace(/\/\*[\s\S]*?\*\//g, (block) => block.replace(/[^\n]/g, ' '))
    // `// …` bis zum Zeilenende
    .replace(/\/\/[^\n]*/g, '');
}

const funde = [];

function melde(datei, regel, wert, kontext) {
  funde.push({ datei, regel, wert, kontext });
}

/**
 * **Wert in einer Passwortzeile.**
 *
 * `password: "…"` und `trimmed_app_password("…")` – überall dort, wo ein Wert
 * feststeht, ist er verdächtig: Im laufenden Programm kommt er aus der
 * Oberfläche oder aus dem Schlüsselbund, nie aus dem Quelltext.
 *
 * **Nicht** jede Funktion, in deren Namen „Passwort" steht, zählt: `password(id)`
 * ist ein Nachschlagen nach einem Platz, `set_password(id, passwort)` bekommt die
 * Kennung *und* das Passwort. Wer beides als Treffer führt, meldet beim ersten
 * echten Fund fünf erfundene daneben – und wird dann die Meldung wegklicken.
 *
 * Bei mehreren Argumenten zählt deshalb nur das **letzte**, und auch das nur,
 * wenn der Aufruf überhaupt nach einem Geheimnis benannt ist.
 */
const WERT_IN_PASSWORTZEILE = new RegExp(
  [
    // `password: "…"`, `"passwort": "…"` – die Zuweisung.
    String.raw`\b(?:password|passwort|passwd|secret|api[_-]?key|token)\s*[:=]\s*"(?<zuweisung>[^"\n]{4,})"`,
    // Dasselbe an einer Variablen, deren *Name* mit einem Wort für ein Geheimnis
    // endet: `let test_pw = "…"` heißt genauso viel wie `password: "…"`. Ohne diese
    // Form bliebe die häufigste Art daneben, wie jemand beim Ausprobieren ein
    // Passwort einträgt.
    //
    // **Am Ende des Namens, nicht am Anfang.** Ein Suffix statt eines Wortes:
    // `PASSWORD_HINT` ist eine Beschriftung und soll durchgehen, `TEST_PW` ein
    // Schlüssel und nicht. Eine Wortgrenze würde `TEST_PW` ohnehin verfehlen,
    // weil zwischen `_` und `P` keine liegt.
    String.raw`\b[\w]*(?:password|passwort|passwd|secret|token|apikey|api_key|pw)\s*(?::[^=\n]*)?=\s*"(?<benannt>[^"\n]{4,})"`,
    // `trimmed_app_password("…")` – einargumentig, also eindeutig.
    String.raw`\btrimmed_app_password\s*\(\s*"(?<aufbereitet>[^"\n]{4,})"`,
    // `write_stored_credential(pfad, benutzer, "…")` – das dritte Argument.
    String.raw`\bwrite_stored_credential\s*\([^)]*,[^)]*,\s*"(?<abgelegt>[^"\n]{4,})"`,
  ].join('|'),
  'gi',
);

/**
 * Der gefundene Wert, gleich welcher der drei Formen ihn gestellt hat.
 *
 * **Benannte Gruppen statt einer**, weil die Alternativen jede ihre eigene
 * Fanggruppe haben: In `a|x|y` ist die von `x` gefangene Gruppe die *zweite*, und
 * wer `treffer[1]` liest, bekommt bei einem Treffer aus `x` nichts.
 */
function gefangenerWert(treffer) {
  const { zuweisung, benannt, aufbereitet, abgelegt } = treffer.groups ?? {};
  return zuweisung ?? benannt ?? aufbereitet ?? abgelegt;
}

/**
 * **Sechzehn Kleinbuchstaben als Zeichenkette**, und die Zeile, in der sie steht,
 * muss von einem Geheimnis handeln.
 *
 * Beides zusammen, und nicht einzeln: Ohne die Anführungszeichen schlägt die
 * Prüfung an jeder deutschen Prosa an – „selbstsigniertes" steht in
 * `docs/entwicklung.md` in derselben Zeile wie das Wort „Passwort", ist aber ein
 * Wort und kein Schlüssel. Ohne die Zeile dagegen stünde jedes solche Kompositum
 * im Quelltext im Verdacht, auch fernab jeder Zugangsangabe.
 *
 * Der blinde Fleck bleibt und soll benannt sein: Ein App-Passwort, das in einer
 * Zeile ohne jedes passwortartige Wort steht – etwa `let x = "…";` – wird nicht
 * erkannt. In der Praxis ist das fast nie der Fall, weil dort auch der Fehler
 * steckt, den man beim Einfügen macht.
 */
const APP_PASSWORT = /(?<=["'])[a-z]{16}(?=["'])/g;

/**
 * **Wörter, in deren Umgebung sechzehn Kleinbuchstaben verdächtig sind.**
 *
 * Ohne diese Einschränkung schlägt die Prüfung an jedem zweiten deutschen Satz
 * an: „synchronisierbar", „selbstsigniertes" und „zwischenzeitlich" haben
 * genau die Form eines App-Passworts, und dieses Repository ist auf Deutsch
 * geschrieben. Eine Prüfung, die siebenunddreißigmal an einem Fensterlosen
 * Kompositum anschlägt, wird binnen einer Woche abgeschaltet – und dann prüft
 * sie gar nichts mehr.
 *
 * **Nur dieselbe Zeile, und nur enge Wörter.** „smtp", „imap" und „anmelden"
 * kommen in diesem Repository in jedem zweiten Absatz vor; sie zu suchen hieße,
 * die Hälfte der Dokumentation zu melden. Gesucht wird nur nach einem Wort, das
 * **unmittelbar** ein Geheimnis meint.
 *
 * Der blinde Fleck bleibt und soll benannt sein: Ein App-Passwort, das in einer
 * Zeile ohne jedes passwortartige Wort steht – etwa `let x = "…";` – wird nicht
 * erkannt. In der Praxis ist das fast nie der Fall, weil dort auch der Fehler
 * steckt, den man beim Einfügen macht.
 */
const PASSWORT_UMGEBUNG = /passwort|password|passwd|secret|token|keyring|schlüssel|schluessel|credential/i;

/** Steht der Treffer in einer Zeile, in der es um Zugangsdaten geht? */
function inPasswortumgebung(ohne, stelle) {
  const zeilenAnfang = ohne.lastIndexOf('\n', stelle) + 1;
  let zeilenEnde = ohne.indexOf('\n', stelle);
  if (zeilenEnde === -1) zeilenEnde = ohne.length;

  return PASSWORT_UMGEBUNG.test(ohne.slice(zeilenAnfang, zeilenEnde));
}

/**
 * Nur Platzhalter ab fünf Zeichen dürfen **als Teil** eines Wertes gelten.
 *
 * Der Unterschied entscheidet alles: Ein echtes Passwort enthält mit hoher
 * Wahrscheinlichkeit ein `x` – und „Passwort *x* passt nicht" ist damit keine
 * Treffermeldung, sondern ein Loch in der Prüfung. Ein Wort wie `geheim` sagt
 * dagegen auch als Teil etwas aus: `geheim123` ist kein Schlüssel.
 */
const TEIL_MINDESTLAENGE = 5;

/**
 * **Doch wieder kein „enthält".**
 *
 * `S3cret-Kalender-Passwort` enthält „passwort" – und wäre mit einer
 * Teilstring-Regel durchgewunken, während es genau die Art Wert ist, die diese
 * Prüfung sucht. Ein Platzhalter gilt deshalb nur, wenn der Wert **genau**
 * einer ist, höchstens mit Ziffern oder Punkten dahinter: `geheim`, `geheim123`,
 * `test.test`.
 */
function istPlatzhalter(wert) {
  const klein = wert.trim().toLowerCase();
  const ohneAnhang = klein.replace(/[0-9._-]+$/, '');

  return PLATZHALTER.some((platzhalter) => {
    if (platzhalter.length < TEIL_MINDESTLAENGE) return klein === platzhalter;
    return klein === platzhalter || ohneAnhang === platzhalter;
  });
}

/**
 * Ein Wert mit einem Steuerzeichen-Escape ist fast immer ein Test darauf, dass
 * genau dieser Wert abgelehnt wird – `set_password("mit\nZeilenumbruch")`
 * prüft die Prüfung, nicht das Merken.
 */
function istSteuerzeichenTest(wert) {
  return /\\[nrt]/.test(wert);
}

function pruefe(datei, inhalt) {
  const ohne = ohneKommentare(inhalt);

  for (const treffer of ohne.matchAll(WERT_IN_PASSWORTZEILE)) {
    const wert = gefangenerWert(treffer);
    if (!wert || istPlatzhalter(wert) || istSteuerzeichenTest(wert)) continue;

    melde(datei, 'Fest verdrahtetes Passwort', wert, zeilenUmgebung(ohne, treffer.index));
  }

  for (const treffer of ohne.matchAll(APP_PASSWORT)) {
    const wert = treffer[0];
    if (istPlatzhalter(wert) || istSteuerzeichenTest(wert)) continue;
    if (!inPasswortumgebung(ohne, treffer.index)) continue;

    melde(
      datei,
      '16 Kleinbuchstaben in einer Passwortzeile',
      wert,
      zeilenUmgebung(ohne, treffer.index),
    );
  }
}

/** Die Zeile, um die es geht, mit etwas Kontext. */
function zeilenUmgebung(inhalt, stelle) {
  const zeilenAnfang = inhalt.lastIndexOf('\n', stelle) + 1;
  const zeilenEnde = inhalt.indexOf('\n', stelle);
  return inhalt
    .slice(zeilenAnfang, zeilenEnde === -1 ? inhalt.length : zeilenEnde)
    .trim()
    .slice(0, 110);
}

// --- Dateien sammeln ----------------------------------------------------------

function sammle(verzeichnis, raus) {
  for (const eintrag of readdirSync(verzeichnis)) {
    if (AUSGESCHLOSSEN.has(eintrag) || eintrag.startsWith('.')) continue;

    const pfad = join(verzeichnis, eintrag);
    const angaben = statSync(pfad);

    if (angaben.isDirectory()) {
      sammle(pfad, raus);
    } else if (ENDUNGEN.has(extname(eintrag))) {
      raus.push(pfad);
    }
  }
}

const dateien = [];
sammle(WURZEL, dateien);

for (const pfad of dateien) {
  pruefe(relative(WURZEL, pfad), readFileSync(pfad, 'utf8'));
}

// --- Zugangsdateien im Repository? -------------------------------------------

const versioniert = new Set(
  execFileSync('git', ['ls-files'], { cwd: WURZEL, encoding: 'utf8' })
    .split('\n')
    .map((z) => z.trim())
    .filter(Boolean),
);

const geheimDateien = [...versioniert].filter((z) =>
  GEHEIM_DATEIEN.includes(z.split('/').pop()),
);

for (const z of geheimDateien) {
  funde.push({
    datei: z,
    regel: 'Zugangsdatei versioniert',
    wert: z,
    kontext: 'Diese Datei gehört nicht ins Repository.',
  });
}

// --- Ergebnis -----------------------------------------------------------------

console.log(`Geprüft: ${dateien.length} Dateien, ${versioniert.size} versioniert.`);
console.log();

if (funde.length === 0) {
  console.log('ok   kein Passwort im Quelltext');
  console.log('ok   keine Zugangsdatei im Repository');
  process.exit(0);
}

console.log(`${funde.length} Fundstelle(n):`);
console.log();

for (const { datei, regel, wert, kontext } of funde) {
  console.log(`  ${datei}`);
  console.log(`    Grund:  ${regel}`);
  console.log(`    Wert:   ${wert}`);
  console.log(`    Zeile:  ${kontext}`);
  console.log();
}

console.log('Ein Passwort im Quelltext ist mit einem normalen Commit dauerhaft');
console.log('verloren. Erst die Zugangsangabe widerrufen, dann den Commit');
console.log('korrigieren – oder, falls schon gepusht, die Historie umschreiben.');
process.exit(1);