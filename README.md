# Mimir

Mimir ist eine schlanke Desktop-Oberfläche für einen **eigenen Ollama-Server**.

Die Anwendung ist kein Chat mit eingebautem Modell, sondern ein Fenster, durch das mit
einem Ollama-Server gesprochen wird, den du selbst betreibst – auf einem Rechner im
LAN, auf einem Server im Rechenzentrum oder auf dem eigenen. Modelle, Verlauf und
Einstellungen bleiben damit bei dir.

Mimir verwendet Tauri 2, ein statisches Frontend aus HTML, CSS und JavaScript sowie
ein Rust-Backend. Modellliste, Chat-Anfragen und Streaming-Antworten kommen über
Ollamas HTTP-API; das Frontend muss den Server nicht selbst erreichen.

## Was es kann

- **Chat** mit jedem Modell, das auf dem Server liegt – die Liste wird von `/api/tags`
  geholt, nichts ist fest eingestellt
- **Streaming** mit Denktext für Reasoning-Modelle, Abbruch jederzeit, erneuter
  Versand bei Verbindungsabbruch vor dem ersten Token
- **Markdown** mit Überschriften, Listen, Tabellen, Links, Zitaten und Codeblöcken,
  gegen unsicheres HTML und unsichere Links abgesichert
- **Kalenderleiste** aus einer Nextcloud-Instanz im eigenen Netz: Termine am rechten
  Rand, ein Klick öffnet den Termin zum Ändern
- **Termine anlegen, ändern und löschen** im Agentenmodus, jeweils mit Vorschau des
  Unterschieds und ausdrücklicher Freigabe
- **Agentenmodus** mit lesenden Werkzeugen, feste Sandbox, jeder Aufruf wird
  bestätigt; Schreiben nur nach Freischaltung und als sichtbarer Unterschied
- **Umfang** wählbar: alles wie bisher oder nur die vier Kalenderwerkzeuge, dann
  ohne Dateizugriff und ohne Arbeitsverzeichnis, jede Änderung weiterhin als
  sichtbarer Unterschied vorab bestätigt
- **Dateien anhängen** ohne Mimir Zugriff auf das Dateisystem zu geben

## Voraussetzungen

**Ein erreichbarer Ollama-Server.** Sonst nichts.

Läuft Ollama auf demselben Rechner, genügt `http://localhost:11434`. Läuft es woanders,
trägst du die Adresse beim ersten Start ein – siehe [Erster Start](#erster-start).

Für den Komfortbefehl `/server-start`, der Ollama über SSH auf dem Server neu startet,
braucht Mimir zusätzlich ein lokales `ssh`-Programm und einen Schlüssel. Das ist
freiwillig; ohne SSH funktioniert alles andere genauso.

## Installation

Mimir wird aus dem Quelltext gebaut. Es gibt kein fertiges Paket: Wer es haben
will, klont das Repository und baut es selbst. Der Build dauert einmal rund
17 Minuten, danach ist die Binary da.

### Linux

Werkzeug und Bibliotheken — auf Arch:

```bash
sudo pacman -S --needed base-devel rust webkit2gtk-4.1 gtk3
```

Dann bauen und starten:

```bash
git clone https://github.com/bilandal-dev/Mimir.git
cd Mimir
cargo build --release --manifest-path src-tauri/Cargo.toml
./src-tauri/target/release/mimir
```

Drei Dinge, die den ersten Build aufhalten:

- **Er braucht Platz.** Rund 1,3 GB landen in `src-tauri/target/`. Das ist kein
  Versehen, sondern `lto = true` und `codegen-units = 1` in `Cargo.toml`
  geschuldet.
- **Nicht in ein `tmpfs` bauen.** Ein `/tmp` von 4 GB reicht nicht; dort bricht
  der Linker mit `Disk quota exceeded` ab, obwohl der Quelltext in Ordnung ist.
  `/var/tmp` oder das Home-Verzeichnis sind sicher.
- **Kein Node, kein npm, kein `cargo install tauri-cli`.** Das Frontend sind
  drei Dateien, die Tauri mitkompiliert. Auf Rust und den Systembibliotheken
  genügt der obige Befehl.

`cargo build` erzeugt nur die Binary — kein Menüeintrag, kein Icon. Beides
legst du selbst ab:

```bash
install -Dm755 src-tauri/target/release/mimir ~/.local/bin/mimir

install -Dm644 src-tauri/icons/128x128.png \
  ~/.local/share/icons/hicolor/128x128/apps/mimir.png

mkdir -p ~/.local/share/applications
cat > ~/.local/share/applications/mimir.desktop <<EOF
[Desktop Entry]
Type=Application
Name=Mimir
Comment=Schlanke Desktop-Oberfläche für einen eigenen Ollama-Server
Exec=$HOME/.local/bin/mimir
Icon=mimir
Terminal=false
Categories=Development;
StartupWMClass=mimir
EOF
```

### Windows

Der Bau braucht mehr als Rust. `rustup` installiert keinen Linker, und Rust sucht
den Visual-Studio-Compiler nicht von allein:

- **Microsoft C++ Build Tools** mit der Workload *Desktop development with C++*.
  Ohne sie scheitert der Build beim Binden mit `link.exe not found`.
- **WebView2 Runtime**, in dem die Oberfläche läuft. Auf Windows 10 ab 21H2 und
  auf Windows 11 ist sie vorhanden; auf einem Server ohne Desktop fehlt sie.
- **WebKitGTK und GTK3 werden nicht gebraucht.** Das sind Linux-Bibliotheken.

Der Befehl ist derselbe, aber das Bundle-Ziel passt nicht:

```powershell
cargo build --release --manifest-path src-tauri/Cargo.toml
cargo tauri build --bundles msi
```

`bundle.targets` in `src-tauri/tauri.conf.json` steht auf `["deb"]` — ohne
`--bundles` versucht Tauri unter Windows ein Debian-Paket zu bauen. Ohne
Signaturzertifikat zeigt der Installer beim Start eine SmartScreen-Warnung; die
umgehst nur ein Zertifikat.

### macOS

- **Xcode Command Line Tools**: `xcode-select --install`. Ohne sie findet der
  Build keinen `clang`.
- **WebKit ist Teil von macOS.** Es ist nichts zu installieren, und anders als
  unter Linux gibt es hier keine WebKitGTK-Abhängigkeit.

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml
cargo tauri build --bundles app,dmg
```

Wie unter Windows gilt: `bundle.targets` nennt `deb`, also braucht es
`--bundles`. Auf einem eigenen Rechner startet die App ohne Problem. Wer sie an
andere weitergeben will, muss sie signieren und notarisieren — sonst blockiert
Gatekeeper den Start, und der Umweg über die Datenschutz-Einstellungen des
Empfängers ist nicht dauerhaft.

Details zu allen drei Systemen in
[docs/entwicklung.md](docs/entwicklung.md#verteilen).

## Erster Start

Beim ersten Mal steht eine Anleitung im Chat und die Kopfzeile zeigt `Server: Offline`.
Dann zwei Schritte:

**1. Prüfen, ob Ollama läuft**

```bash
ollama list
```

Kommt eine Liste mit Modellen, läuft der Server. Kommt nichts, startest du ihn mit
`ollama serve`.

**2. Adresse eintragen**

Knopf **Adresse** in der Kopfzeile. Es öffnet sich ein Fenster mit einem Feld:

- Ollama läuft hier: Feld leer lassen, `localhost:11434` ist der Vorgabe
- Ollama läuft woanders: `192.168.178.42:11434` oder `mein-server:11434`
- `http://` darf davorstehen, muss es aber nicht
- ohne Portangabe wird `11434` verwendet

Die Adresse wird geprüft, bevor sie gespeichert wird, und der Verlauf wird
zurückgesetzt, wenn der Server wechselt – er gehört zum alten Server.

Ist der Server nicht erreichbar, bleibt die Adresse trotzdem stehen. Ein Rechner, der
aus ist, und eine falsche Adresse sind zwei verschiedene Fehler, und die eingetragene
Adresse ist im ersten Fall richtig.

## Ollama hier oder im Netz

Die Auswahl **Ollama** in der Kopfzeile – **Server** oder **Lokal** – entscheidet,
woher die Modelle kommen:

- **Server** nimmt die eingetragene Adresse aus dem Netz, wie oben beschrieben
- **Lokal** nimmt das Ollama auf diesem Rechner, immer unter
  `http://localhost:11434`

Das sind zugleich `/provider remote` und `/provider local`.

Der Wechsel bleibt über einen Neustart erhalten und verändert die eingetragene Adresse
nicht. Er setzt den Chatverlauf zurück – er gehört zum anderen Server – und lädt die
Modellliste neu. Ein Wechsel prüft nichts: Läuft das Ollama, das du gewählt hast,
nicht, zeigt die Kopfzeile das wie bei jeder anderen Adresse.

Im lokalen Provider gibt es nur den Terminumfang, also die vier Kalenderwerkzeuge und
keinen Dateizugriff. `/scope agent` wird dort abgelehnt. Der Grund ist die Größe des
Modells: Es läuft auf demselben Rechner wie Mimir und hat keinen Grund, dessen
Arbeitsverzeichnis zu durchsuchen. Zurück zu den Dateiwerkzeugen geht es mit
`/provider remote`; der gespeicherte Umfang bleibt dabei erhalten.

Einzelheiten in [Konfiguration](docs/konfiguration.md#provider-woher-die-modelle-kommen).

## Befehle

Alles im Chat, mit `/` beginnend. `/help` zeigt dieselbe Liste in der Anwendung.

| Befehl | Wirkung |
| --- | --- |
| `/provider` | zeigt oder stellt ein, woher die Modelle kommen |
| `/provider remote` | entferntes Ollama aus dem Netz |
| `/provider local` | Ollama auf diesem Rechner, nur Kalenderwerkzeuge |
| `/server-url` | zeigt die eingestellte Adresse |
| `/server-url <URL>` | ändert sie dauerhaft, nach Bestätigung |
| `/server-status` | prüft die Erreichbarkeit neu |
| `/server-start` | startet Ollama über das SSH-Ziel neu, nur bei `/provider remote` |
| `/system` | dauerhafte Anweisung an das Modell, z. B. Sprache oder Länge |
| `/context` | Größe des Kontextfensters, `0` übernimmt die Vorgabe |
| `/history` | Verlauf auf der Platte halten oder löschen, nur nach Rückfrage |
| `/scope` | zeigt oder stellt ein, wie weit das Modell reicht |
| `/scope agent` | Dateiwerkzeuge im Arbeitsverzeichnis |
| `/scope termine` | nur die vier Kalenderwerkzeuge, ohne Dateizugriff |
| `/agent` | Agentenmodus ein oder aus, nur lesende Werkzeuge |
| `/agent-write` | schreibende Werkzeuge freigeben oder sperren |
| `/agent-dir` | zeigt das Arbeitsverzeichnis des Agentenmodus |
| `/tools` | listet die verfügbaren Werkzeuge |
| `/calendar` | Nextcloud-Instanz eintragen oder anmelden |
| `/calendar zertifikat` | Fingerabdruck des Zertifikats bestätigen |
| `/termine` | die nächsten Termine als Agenda im Chat |
| `/help` | Übersicht aller Befehle |

`/ssh-target` und `/ssh-key` setzen das SSH-Ziel und den Schlüssel für
`/server-start`. Details in [docs/konfiguration.md](docs/konfiguration.md).

## Wo die Einstellungen liegen

```
~/.config/com.bilandal.mimir/ollama.json          Server, SSH, Agent, Chat, Kalender
~/.config/com.bilandal.mimir/calendar-secret.json App-Passwort des Kalenders, nur diese Sitzung
```

`ollama.json` enthält **kein** Passwort und ist frei lesbar. Das App-Passwort liegt in
einer eigenen Datei mit restriktiven Rechten und wird beim Beenden verworfen.

## Daten

Mimir schickt deine Nachrichten an den Server, den du eingetragen hast, und sonst
nirgendwo hin. Es gibt keinen Telemetrie, keinen Update-Check und keine Analytics.

Der Agentenmodus liest ausschließlich im Verzeichnis, das mit `/agent-dir` gesetzt ist,
und nur nach Bestätigung. Schreibvorgänge gibt es nur mit `/agent-write` und werden
vorher als Unterschied gezeigt.

Näheres in [docs/entwicklung.md](docs/entwicklung.md#sicherheits--und-datenhinweise).

## Dokumentation

| Datei | Inhalt |
| --- | --- |
| [docs/entwicklung.md](docs/entwicklung.md) | Bauen, Prüfen, Verteilen, Sicherheit |
| [docs/konfiguration.md](docs/konfiguration.md) | Server, SSH, Kontextfenster, Verlauf |
| [docs/agentenmodus.md](docs/agentenmodus.md) | Werkzeuge, Bestätigung, Sandbox |
| [docs/kalender.md](docs/kalender.md) | Nextcloud, Termine, Erinnerungen, Kategorien |
| [docs/architektur.md](docs/architektur.md) | Aufbau, Datenfluss, Gestaltung |

## Anmerkungen

- Getestet unter Linux mit Hyprland. Andere Desktop-Umgebungen verwenden die
  native Fensterdekoration und funktionieren, sind aber nicht erprobt.
- **Windows und macOS sind nicht erprobt.** Die Schritte in der
  [Installation](#installation) stammen aus den Anforderungen von Tauri, nicht
  von einer Messung auf diesen Systemen. Sie sind unvollständig geprüft; wer sie
  braucht, sollte sie auf dem eigenen Zielrechner nachvollziehen.
- Nur Ollama. Es gibt keine Anbindung an andere Modellanbieter.
- Die Anzeige spricht Deutsch.
