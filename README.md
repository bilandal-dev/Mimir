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
- **Dateien anhängen** ohne Mimir Zugriff auf das Dateisystem zu geben

## Voraussetzungen

**Ein erreichbarer Ollama-Server.** Sonst nichts.

Läuft Ollama auf demselben Rechner, genügt `http://localhost:11434`. Läuft es woanders,
trägst du die Adresse beim ersten Start ein – siehe [Erster Start](#erster-start).

Für den Komfortbefehl `/server-start`, der Ollama über SSH auf dem Server neu startet,
braucht Mimir zusätzlich ein lokales `ssh`-Programm und einen Schlüssel. Das ist
freiwillig; ohne SSH funktioniert alles andere genauso.

Auf einem Linux-System braucht Mimir WebKitGTK – die Abhängigkeiten stehen in
[docs/entwicklung.md](docs/entwicklung.md).

## Installation

**Für Debian und Ubuntu:**

```bash
sudo apt install ./mimir_0.1.0_amd64.deb
```

**Für Arch:** Ein Paket für `pacman` liegt im AUR, sobald es dort eingetragen ist.
Ohne Rust-Installation, ohne selbst zu bauen:

```bash
yay -S mimir
```

Die Binary selbst geht überall, wo WebKitGTK und GTK3 installiert sind:

```bash
chmod +x mimir && ./mimir
```

Alle Pakete nennen ihre Abhängigkeiten selbst; `libwebkit2gtk` und `libgtk-3`
müssen nicht vorher von Hand installiert werden.

**Aus dem Quelltext:**

```bash
git clone https://github.com/bilandal-dev/Mimir.git
cd Mimir
cd src-tauri && cargo tauri build
```

Das Ergebnis liegt unter `src-tauri/target/release/`. Kein Node, kein npm, kein
Frontend-Werkzeug – das Frontend sind drei Dateien, die mitkompiliert werden.

Windows und macOS lassen sich nur auf diesen Systemen bauen. Ein AppImage gibt es
nicht, weil `linuxdeploy` unter Arch an einem Debian-spezifischen Plugin scheitert.
Einzelheiten in [docs/entwicklung.md](docs/entwicklung.md#verteilen).

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

## Befehle

Alles im Chat, mit `/` beginnend. `/help` zeigt dieselbe Liste in der Anwendung.

| Befehl | Wirkung |
| --- | --- |
| `/server-url` | zeigt die eingestellte Adresse |
| `/server-url <URL>` | ändert sie dauerhaft, nach Bestätigung |
| `/server-status` | prüft die Erreichbarkeit neu |
| `/server-start` | startet Ollama über das SSH-Ziel neu |
| `/system` | dauerhafte Anweisung an das Modell, z. B. Sprache oder Länge |
| `/context` | Größe des Kontextfensters, `0` übernimmt die Vorgabe |
| `/history` | Verlauf auf der Platte halten oder löschen, nur nach Rückfrage |
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
- Nur Ollama. Es gibt keine Anbindung an andere Modellanbieter.
- Die Anzeige spricht Deutsch.
