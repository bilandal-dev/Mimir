# Entwicklung

Bauen, Prüfen und Verteilen. Für alle, die an Mimir arbeiten oder es für ihr eigenes
System bauen.

Voraussetzungen sind Rust/Cargo und die benötigten Tauri-Systemabhängigkeiten. Für den normalen Chat wird ein erreichbarer Ollama-Server benötigt; für den SSH-Start zusätzlich ein lokales `ssh`-Programm und entweder ein vorbereiteter SSH-Schlüssel/`ssh-agent` oder die Berechtigung für den Passwort-Fallback.

Anwendung im Entwicklungsmodus starten:

```bash
cargo run --manifest-path src-tauri/Cargo.toml
```

Rust-Tests ausführen:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Clippy-Prüfung ausführen:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Produktionsbinary bauen:

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml
```

Die erzeugte Binary liegt anschließend unter `src-tauri/target/release/mimir`.

Die Oberfläche lässt sich ohne Fenster prüfen: Ein Nachbau von `main.js` unter jsdom spiegelt alle Tauri-Befehle samt Fehlerfällen und fährt den Ablauf durch. Er liegt nicht im Repository, weil er sich mit dem Backend gemeinsam weiterentwickelt; die Prüfungen dafür stehen in `src-tauri/src/calendar/*/tests.rs` und in den Modultests von `lib.rs`.

## Verteilen

### Das Debian-Paket

```bash
cd src-tauri
cargo tauri build
```

Das Ergebnis liegt unter `target/release/bundle/deb/`. Installieren:

```bash
sudo apt install ./mimir_0.1.0_amd64.deb
```

Das Paket nennt seine Abhängigkeiten selbst (`libwebkit2gtk-4.1-0`, `libgtk-3-0`),
die also nicht vorher von Hand zu installieren sind.

**Warum kein AppImage.** `linuxdeploy-plugin-gtk` ist für Debian geschrieben und
läuft unter Arch nicht:

```
[gtk/stderr] realpath: /usr/lib/gdk-pixbuf-2.0/2.10.0: No such file or directory
ERROR: Failed to run plugin: gtk (exit code: 1)
```

Zusätzlich verlangt `linuxdeploy` selbst FUSE, das hier nicht vorhanden ist. Für
Arch und Fedora ist ein eigenes AppImage nötig; ein Container-Build auf Debian-Basis
wäre der Weg, lohnt sich bei dieser Anwendung aber nicht.

### Windows und macOS

Beide lassen sich nur auf diesen Systemen zuverlässig bauen. Die Ziele stehen in
`bundle.targets` in `tauri.conf.json` und lassen sich beim Bauen einschränken:

```bash
cargo tauri build --bundles msi      # Windows, nur unter Windows
cargo tauri build --bundles app,dmg  # macOS, nur unter macOS
```

Für alle drei Formate aus einer Quellkopie braucht es einen CI-Lauf auf drei
Maschinen. Windows ohne Signatur zeigt SmartScreen mit einer Warnung; die
umgeht man nur mit einem Zertifikat.

### Icons

Alle Größen entstehen aus einer Quelle:

```bash
cargo tauri icon src-tauri/icons/source.svg
```

Das erzeugt `icon.png`, `icon.ico`, `icon.icns`, die HICON-Dateien für Windows,
`StoreLogo.png` sowie die `android/`- und `ios/`-Verzeichnisse. Vorhanden sein
müssen die in `bundle.icon` genannten.

Die Quelle ist `src-tauri/icons/source.svg` – die Rune Mannr aus dem älteren
Futhark, schwarz mit Cyan. Bei 16 Pixeln wird daraus ein H: Das Kreuz zwischen den
Stäben verschwindet, die Stäbe selbst nicht. Die Strichstärke ist 48 von 512
Einheiten; dünner überlebt die Taskleiste nicht, dicker wird im großen Format ein
Klecks.

### Lizenz

Mimir steht unter der MIT-Lizenz, in `LICENSE` im Wurzelverzeichnis. Das Feld
`license` in `src-tauri/Cargo.toml` sagt dasselbe – sind die beiden verschieden,
gilt für das Paket die Datei und das Manifest erzählt etwas anderes.

Ohne Lizenzdatei ist die Veröffentlichung nicht erlaubt, gleichgültig wie offen
der Quelltext liegt. `erzeuge-archiv.sh` verweigert ohne sie die Arbeit und legt
sie als `mimir.license` in das Archiv, damit das fertige Paket sie nennen kann.

### Arch: Paket für pacman

Arch-Nutzer brauchen kein Debian-Paket und kein AppImage. Sie bekommen ein Paket
für `pacman`, gebaut aus `packaging/arch/PKGBUILD`.

**Der entscheidende Punkt: Die PKGBUILD baut nichts, sie lädt die Binary herunter.**

Ein AUR-Eintrag, der selbst kompiliert, zwingt jedem Nutzer eine Rust-Toolchain
und einen zehnminütigen Build auf. Mimir hängt an 143 Systembibliotheken, und wer
ein Chat-Programm installieren will, soll dafür nicht zwanzig Minuten warten. Wer
selbst bauen will, nimmt das Repository.

```bash
# Einmalig: Binary bauen, prüfen, Archiv schnüren
./packaging/arch/erzeuge-archiv.sh

# Prüfsumme in packaging/arch/PKGBUILD eintragen
sha256sum -b packaging/arch/mimir-0.1.0-x86_64.tar.gz

# Archiv als Release hochladen, Adresse in der PKGBUILD anpassen

# Prüfen, ob das Paket baubar ist
cd packaging/arch && makepkg -si
```

`erzeuge-archiv.sh` bricht ab, wenn die `LICENSE` fehlt, und ruft vorher die
Binary-Prüfung auf. Ein Archiv mit fremden Adressen zu veröffentlichen wäre der
schlimmste der möglichen Fehler – die landen sonst dauerhaft im Release.

Das fertige Paket installiert:

```
/usr/bin/mimir
/usr/share/applications/mimir.desktop
/usr/share/icons/hicolor/128x128/apps/mimir.png
/usr/share/licenses/mimir/LICENSE
```

und nennt `webkit2gtk-4.1` und `gtk3` als Abhängigkeiten. Ohne WebKitGTK startet
das Programm nicht, und es ist besser, `pacman` sagt es vorher.

### Prüfen, bevor es herausgeht

`node src/tests/binary-pruefen.mjs` prüft das gebaute Binary **und** die
Bundle-Konfiguration. Sie schlägt an, wenn

- eine fremde Adresse oder ein Pfad des Baurechners im Binary steht,
- `bundle.active` fehlt oder keine Ziele benannt sind,
- ein in `bundle.icon` genanntes Icon fehlt oder `icon.png` kleiner als 256 Pixel ist,
- `authors` noch der Vorgabewert `you` ist – er landet als `Maintainer` im Paket,
- `productName` kleingeschrieben ist – er erscheint als `Name=` in der Desktop-Datei,
- `shortDescription` oder `longDescription` einen Zeilenumbruch enthalten – im
  Debian-Format ist die erste Zeile der Kurztext, und ein mehrzeiliger Wert erzeugt
  eine Control-Datei, die kein `dpkg` liest.

Diese drei letzten Punkte sind nicht theoretisch: Sie sind beim ersten Bauen
aufgetreten und in den Paketmetadaten sichtbar gewesen.

### Das öffentliche Repository

Quelltext: <https://github.com/bilandal-dev/Mimir>

Die Adresse steht in `packaging/arch/PKGBUILD` (`url` und `_release_url`) und in
`src-tauri/Cargo.toml` (`repository`).

**Vor dem ersten Push die Historie prüfen:**

```bash
git log -S 192.168.2      # findet Adressen, die längst entfernt sind
git log --all --diff-filter=A --name-only | rg -i "ollama.json|secret"
```

Adressen, die einmal im Quelltext standen, bleiben über `git log -S` auffindbar.
Zwei Wege: die Historie umschreiben (`git filter-repo`) oder – leichter – das
Repository neu anlegen und einen einzigen sauberen Stand hineincommitten. Für ein
öffentliches Repository ist der zweite Weg der ehrlichere: Die alte Historie
erzählt, wie Mimir entstanden ist, und das gehört nicht in ein fremdes
Archiv.

**Für den AUR-Eintrag ist ein Release nötig**, kein Commit. Die PKGBUILD lädt:

```
https://github.com/bilandal-dev/Mimir/releases/download/v0.1.0/mimir-0.1.0-x86_64.tar.gz
```

Ohne hochgeladenes Archiv findet der AUR-Bot nichts und der Eintrag ist tot.

### Noch zu erledigen

- Das **Archiv muss als Release hochgeladen** werden. `erzeuge-archiv.sh` legt es
  unter `packaging/arch/mimir-0.1.0-x86_64.tar.gz` ab, aber die PKGBUILD lädt es
  von GitHub.
- Die **Paketkennung** `com.bilandal.mimir` enthält den Namen des Entwicklers. Sie
  bestimmt zugleich, wo Mimir seine Einstellungen ablegt – sie umzubenennen ist
  nicht folgenlos, siehe [Konfiguration](konfiguration.md). Für ein öffentliches
  Paket wäre `dev.bilandal.mimir` die naheliegende Form.

## Das Binary prüfen

Was andere installieren, wird am **Artefakt** geprüft, nicht am Quelltext. Eine Konstante, die im Quelltext harmlos aussieht, landet als Zeichenkette im Binary und ist mit `strings` sichtbar:

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml
node src/tests/binary-pruefen.mjs
```

Das Skript schlägt fehl, sobald eine fremde Netzadresse oder ein Pfad des Baurechners auftaucht. Ausgenommen sind `localhost` und `0.0.0.0`: Die erste ist der Vorgabewert, die zweite die Bindungsadresse im Startskript für Ollama.

**Was deshalb im Quelltext steht.** Der Vorgabewert für den Ollama-Server war die Adresse des Entwicklers:

```rust
const DEFAULT_OLLAMA_BASE_URL: &str = "http://192.168.178.42:11434";
```

Sie war nicht nur konfigurierbar – sie war der Wert, mit dem **jeder** startete, der die App ohne `ollama.json` zum ersten Mal öffnete. Ein Nutzer mit eigenem Server hätte auf einem fremden Rechner gelandet, und `strings` auf dem Binary hätte die Adresse jedem gezeigt. Sie steht jetzt als `http://localhost:11434` da, was zugleich der wahrscheinlichste Wert ist, wenn Ollama auf demselben Rechner läuft.

Der Kalender macht es von Anfang an richtig: `CalendarConfig::server_url` ist **leer** und bedeutet „noch keine Instanz eingetragen". Nur der Platzhalter im Eingabefeld zeigt eine Adresse, und die ist ein Beispiel – `https://cloud.example.org`, keine konkrete IP.

**Die Paketkennung darf nicht geändert werden.** Sie steht auf `com.bilandal.mimir` und bestimmt, wo Mimir seine Einstellungen ablegt:

```
~/.config/com.bilandal.mimir/ollama.json
```

Der Versuch, sie auf `de.mimir.app` umzustellen, hat beim ersten Start ein **leeres** Konfigurationsverzeichnis angelegt: ohne Serveradresse, ohne Kalender, ohne Agentenverzeichnis. Dadurch war der Server nicht erreichbar – und es gab keinen Weg, die Adresse zu ändern, weil die Eingabe im Chat lag. Bei einem ausgefallenen Server kommt man aber gerade nicht in den Chat. Der Benutzername des Entwicklers im Bundle-Metadaten ist das nicht wert. Wer die Kennung trotzdem umstellen will, muss die Konfiguration mitnehmen; `src/tests/binary-pruefen.mjs` zeigt, wo die installierte Version ihre Dateien erwartet.

## Beim ersten Start

Wer Mimir zum ersten Mal öffnet, sieht sonst nichts: keinen Server, keine Modelle, einen leeren Chat. Die Kopfzeile sagt „Server: Offline", und die einzige Reaktion darauf ist ein Knopf, dessen Zwass man raten muss.

Deshalb steht beim ersten Start eine Anleitung im Chat:

```
Willkommen bei Mimir.

Mimir spricht mit Ollama. Ollama muss auf diesem Rechner laufen – dann
genügt dieser eine Schritt. Läuft es auf einem anderen Rechner, steht die
Adresse weiter unten.

1. Prüfen, ob Ollama läuft:
      im Terminal:  ollama list
   Läuft das, sollten dort Modelle stehen. Läuft es nicht, starten mit:
      ollama serve

2. Adresse eintragen. Beim ersten Start steht hier die Vorgabe
      http://localhost:11434
   Läuft Ollama woanders, hier die Adresse eintragen – zum Beispiel
      ollama.example.org:11434
   ...

[Server-Adresse jetzt eintragen]
```

Sie erscheint **nur**, solange keine Adresse eingetragen ist. Der Befehl `get_einrichtung` meldet das dem Backend, und der Vergleich läuft über den normalisierten Wert: `localhost:11434` und `http://localhost:11434/` sind derselbe Server.

Der Knopf öffnet denselben Dialog wie im Kopf. Ohne ihn müsste die Anleitung abgearbeitet statt angeklickt werden.

**Keine fremde Adresse im Text.** Sie wird bei jedem ersten Start angezeigt, und eine konkrete IP darin wäre genau die, die gerade aus dem Binary entfernt wurde. Der Beispielname `ollama.example.org` ist von Mimir auch annehmbar – geprüft, denn sonst führte der erste Schritt in eine Fehlermeldung.

Ohne Fenster prüfbar, weil die Anleitung wirklich **ausgeführt** wird und nicht aus dem Quelltext gelesen:

```bash
node src/tests/ersteinrichtung.test.mjs
```

Der Unterschied ist nicht theoretisch: Ein Schnitt über die Zeilenliste im Quelltext lieferte je nach Fassung 21 oder 44 Zeilen, ohne dass eine der Zahlen falsch gewesen wäre – nur die Zerlegung. Ausführen kann man sich nicht missverstehen.

## Die Adresse des Ollama-Servers

Solange kein Server eingetragen ist, gilt `http://localhost:11434`. Das deckt den Fall ab, dass Ollama auf demselben Rechner läuft.

Läuft er woanders, trägt der Knopf **Adresse** im Kopf die Adresse ein. Er ist sichtbar, sobald der Server nicht antwortet oder die Verbindung schwankt.

Zwei Eigenschaften, die der Chat-Befehl `/server-url` nicht hat:

- **Prüfen vor dem Speichern.** Ein Tippfehler wird nicht still angenommen und danach als „Server: Offline" gemeldet.
- **Nicht verwerfen bei einem nicht erreichbaren Server.** Der Rechner kann aus, die Firewall kann zu, der Port kann falsch sein – die Adresse kann trotzdem richtig sein. „Abbrechen" stellt den Ausgangszustand wieder her.

**Was drinbleibt, und warum.** Ein Pfad des Baurechners steht an **einer** Stelle im Binary:

```
/pfad/des/baurechners/Mimir/src-tauri   cacheTauri-Response…
```

Er ist das Asset-Präfix, das Tauri aus `frontendDist` bildet. Es lässt sich nicht umschreiben, ohne den Build zu brechen: Tauri braucht dasselbe `CARGO_MANIFEST_DIR`, um `tauri.conf.json` zu finden, und

```
error: unable to read Tauri config file at /mimir/tauri.conf.json
```

beweist, dass beide Zwecke nicht trennbar sind. Ein neutraler Zielpfad über `target-dir` wurde versucht und brachte nichts – die Ursache ist der Speicherort der Konfiguration, nicht der des Buildverzeichnisses. Die Notiz steht in `src-tauri/build.rs`, damit niemand es erneut versucht.

Der Pfad nennt den Benutzernamen und das Projektverzeichnis. Beides steht schon im Namen der Anwendung, und auf einem fremden Rechner ist es ein anderes. Für eine Veröffentlichung an unbekannte Empfänger ist das der einzige verbleibende Punkt; wer ihn loswerden will, baut aus einem Verzeichnis ohne den eigenen Namen – etwa unter `/build`.

**Was nicht im Binary steht.** Keine Passwörter, Token, Kalendernamen oder Benutzernamen – nur Feldnamen wie `app_password` aus der Datenstruktur. Die Zugangsdaten des Kalenders liegen in `calendar-secret.json` mit `0o600`, getrennt von `ollama.json`, und entstehen erst zur Laufzeit auf dem Rechner, auf dem Mimir läuft. `panic = "abort"` verhindert, dass ein Absturz den Chatverlauf in eine Meldung schreibt.

Pfade aus dem **eigenen** Quelltext lassen sich mit `--remap-path-prefix` in `src-tauri/.cargo/config.toml` entschärfen. Das ist eingerichtet und hat 422 Fundstellen auf einen gebracht.

## Die Datumsangabe prüfen

Die Datumsangabe an das Modell ist als eigenes Skript prüfbar, weil sie sich ohne Fenster bilden lässt. Die Systemuhr wird dafür für jeden Durchlauf auf einen anderen Tag gestellt und die Ausgabe gegen eine unabhängig nachgerechnete Erwartung gehalten:

```bash
node src/tests/datumsangabe.test.mjs
```

Das ist keine Zierde: Die Fehler, die hier auftraten, sind an einem einzigen Tag unsichtbar. „Sonntag“ wurde zur „übernächsten Woche“, und das fiel an drei von sieben Tagen nicht auf. Ein Test mit einem festen Startdatum prüft genau eine dieser Lagen – und läuft um Mitternacht von selbst schief, wie es ein Test mit einem festgeschriebenen Datum getan hat.

Stand der Prüfungen: 351 Rust-Tests, dazu drei Skripte ohne Fenster. Zusätzlich gibt es einen Nachbau des Ablaufs unter jsdom mit 129 Prüfungen; er liegt nicht im Repository und ist deshalb von hier aus nicht nachprüfbar. Sechs seiner Prüfungen brauchen einen wirklich erreichbaren Ollama-Server und werden ohne ihn ausdrücklich übersprungen, statt eine Ersatzliste vorzutäuschen.


## Sicherheits- und Datenhinweise

- Modellantworten werden als Markdown gerendert, anschließend jedoch serverseitig mit `ammonia` sanitized.
- HTML aus Modellantworten, Bilder, relative URLs und potenziell unsichere Link-Schemata werden entfernt.
- `innerHTML` im Frontend wird ausschließlich mit der bereinigten Backend-Ausgabe verwendet; die Tauri-WebView verwendet eine restrictive CSP und eingefrorene JavaScript-Prototypen.
- Server-URLs werden auf HTTP/HTTPS beschränkt, Redirects werden nicht verfolgt; URLs mit eingebetteten Zugangsdaten werden abgelehnt. Steht als Host eine IP-Adresse, wird nur eine Loopback- oder private Adresse akzeptiert – Ollama soll nicht auf einen Host im Internet zeigen können.
- Ollama wird bewusst fest auf `0.0.0.0` gestartet und mit `OLLAMA_ORIGINS=*` freigegeben, damit der Server aus dem LAN erreichbar ist. Damit ist die unauthentifizierte Ollama-API für alle Geräte im Netz erreichbar und jede besuchte Website im Browser darf Anfragen an den Server stellen. Für eine rein lokale Nutzung muss der Befehl `OLLAMA_HOST="127.0.0.1" ollama serve` direkt auf dem Server ausgeführt werden.
- SSH-Ziele, Ports und Schlüsselpfade werden auch beim Laden der Konfiguration validiert; Benutzer-SSH-Config, Proxy-/Jump-Konfiguration und Agent-Forwarding werden für den Mimir-Start deaktiviert.
- Für den SSH-Start wird genau eine Verbindung pro Versuch aufgebaut; es gibt weder Multiplexing noch eine wiederverwendete Sitzung, weil der Remote-Befehl den Server startet und sofort beendet wird.
- Der Passwort-Fallback ist unter Linux und macOS aktiv, nutzt ein kurzlebiges `0700`-Verzeichnis mit `SSH_ASKPASS`-Helper und `0600`-Passwortdatei und legt das Passwort nicht in der Prozessumgebung ab. Der Helper enthält das Passwort nicht, er liest es aus der Datei, auf die `SSH_ASKPASS` zeigt. Die Datei wird vor dem Löschen überschrieben, die Kopie im Arbeitsspeicher beim Freigeben.
- Der ausgeführte Remote-Befehl ist im Backend fest codiert und lässt sich nicht aus Chat-Eingaben zusammensetzen. Auch der Schlüsselpfad wird nur als Wert einer eigenen Option an `ssh` gereicht, nie als Teil des Remote-Befehls.
- SSH verwendet `StrictHostKeyChecking=yes`; unbekannte oder geänderte Host-Keys werden abgelehnt. Der SSH-Prozess selbst hat 30 Sekunden Zeit.
- Chat-, Modell-, Stream- und Markdown-Daten sowie SSH-Antworten besitzen harte Größen-, Zeit- und Mengenlimits: 2 KiB für die Server-URL, 256 Byte je Modellname, 1000 Modelle, 1 MiB Modelllistenantwort, 4 MiB Verlaufsdatei, 64 KiB je Nachricht, 512 KiB je Anfrage, 4 MiB Antwortstrom, 256 KiB Zeile, 100 000 Zeilen, 256 KiB Denktext je Nachricht, 512 KiB Markdown-Eingabe, 2 MiB bereinigtes HTML und 4 KiB je SSH-Fehlertext. Das Werkzeugangebot ist auf 16 Schemata und 64 KiB begrenzt.
- Der Agentenmodus arbeitet nur unterhalb eines festen Arbeitsverzeichnisses und nur nach Bestätigung pro Aufruf; die Werkzeugauswahl ist eine feste Liste im Backend.
- Schreibende Werkzeuge sind standardmäßig gesperrt, gelten nur für die laufende Sitzung und werden nie gespeichert. Sie legen neue Dateien an oder ersetzen genau eine eindeutig auffindbare Stelle; Löschen, Umbenennen und Ausführen gibt es nicht. Vor jedem Schreibvorgang wird der Unterschied gezeigt, geschrieben wird atomar, und bei Dateien lässt sich der Vorgang im Chat zurücknehmen – bei Terminen nicht, siehe [Termine anlegen](#termine-anlegen).
- Die Ollama-Verbindung verwendet standardmäßig unverschlüsseltes HTTP; sie sollte nur in einem vertrauenswürdigen privaten Netzwerk betrieben werden.
- Für einen produktiven Einsatz sollten TLS und gegebenenfalls Authentifizierung ergänzt werden.

### Kalender

- Mimir fragt ein **App-Passwort** ab, nie das Hauptpasswort. Ein App-Passwort ist dafür gedacht, in fremden Programmen zu liegen, und lässt sich in Nextcloud jederzeit widerrufen.
- Das App-Passwort steht nur auf Wunsch auf der Platte, und dann in `calendar-secret.json` mit den Rechten `0600`, niemals in `ollama.json`. Ohne das Kästchen „Passwort merken" lebt es nur im Arbeitsspeicher, in einer Hülle, die beim Freigeben überschrieben wird. `/calendar aus` löscht es aus dem Speicher und von der Platte, ein Wechsel des Benutzernamens ebenfalls.
- Bei `https` wird die Zertifikatskette nicht geprüft. Stattdessen gilt genau ein bestätigtes Zertifikat, und es wird **vor** dem Senden des Passworts verglichen. Eine Freigabe gilt nur für die Adresse, für die sie erteilt wurde; ein Wechsel des Zertifikats sperrt den Zugang. Wer ein selbstsigniertes Zertifikat im Fenster bestätigt, ohne den Fingerabdruck nachzusehen, nimmt genau die Prüfung aus, die es zu ersetzen galt – deshalb steht der Befehl zum Nachsehen dort.
- Umleitungen werden nicht verfolgt, damit das Passwort nicht unbemerkt an einen anderen Host geht. Der Preis ist eine eigene Fehlermeldung, wenn eine Adresse falsch ist.
- Termine stehen nur in der Leiste und im Arbeitsspeicher. Sie gehen nicht von allein an das Modell, sondern erst, wenn im Chat danach gefragt wird. Was über `/termine` in den Chat geschrieben wird, landet allerdings im Kontext der Folgefragen – wie jede andere Chatnachricht.
- Gespeichert und angezeigt werden Titel, Ort, Zeiten (also auch die Dauer), Kalendername, die technische Kennung, die Erinnerung und die Kategorien. **Beschreibung, Teilnehmer und Anlagen werden gar nicht erst mitgeliefert** – sie stehen in der Datei, werden aber nicht gelesen und könnten in der Leiste ohnehin nichts anfangen. Die Kennung ist Teil jedes Termins und wird in der Leiste nicht angezeigt; das Modell sieht sie über `list_calendar_events`.
- **Termine werden nur nach ausdrücklicher Freigabe angelegt.** Das Modell darf das Werkzeug nur nennen, wenn der Schreibmodus offen ist und eine Anmeldung vorliegt; der Benutzer sieht vorher den vollständigen Inhalt der Datei und kann ablehnen. Ohne Bestätigung geht nichts an den Server.
- **Ein Termin lässt sich auch von Hand ändern** – über das Fenster, das ein Klick in der Leiste öffnet. Das braucht keine Freischaltung, weil der Benutzer selbst der Auslöser ist, aber auch dort den Unterschied erst sieht, bevor geschrieben wird. Der Pfad des Kalenders wird mitgeschickt, damit nie in einen ähnlich benannten Nachbarkalender geschrieben wird. Serientermine und Termine mit Teilnehmern sind dort gesperrt, aus denselben Gründen wie beim Modell.
- **Was ein Termin nicht kann, sagt Mimir dem Benutzer.** Anlagen, Teilnehmer und Serientermine werden mit Begründung abgelehnt, und die Systemanweisung sagt dem Modell ausdrücklich, dass es das dem Benutzer melden soll, statt es zu behaupten. Ohne diesen Satz hat ein Modell im Betrieb eine Erinnerung behauptet, die es nicht gab, und dafür den Termin verschoben.
- Der Abruf liest von gestern bis 92 Tage voraus und höchstens 200 Termine; was darüber liegt, wird stillschweigend abgeschnitten. Abgewiesen wird nur ein Antwortkörper über 8 MiB. Die Leiste zeigt die nächsten acht Termine.
