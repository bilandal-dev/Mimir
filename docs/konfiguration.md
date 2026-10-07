# Konfiguration

Alles, was sich einstellen lässt und wo es gespeichert wird: Provider, Serveradresse,
SSH, Kontextfenster, Systemanweisung und Verlauf.

Die Einstellungen liegen als JSON in `~/.config/com.bilandal.mimir/ollama.json`.

## Provider: woher die Modelle kommen

Vorher gab es nur eine Adresse, und die zeigte auf einen Rechner im Netz. Ein Modell
auf dem eigenen Rechner war damit nicht erreichbar, außer man stellte die Adresse um
– mitten im Betrieb, und der Verlauf des anderen Servers blieb dabei stehen.

Deshalb gibt es zwei Einstellungen: den **Provider** und die Adresse.

```json
{
  "provider": "remote",
  "server_url": "http://192.168.178.42:11434"
}
```

- `remote` (Vorgabe) benutzt `server_url`. Fehlt das Feld `provider` in einer älteren
  Konfiguration, gilt das weiterhin – bestehende Installationen starten unverändert.
- `local` benutzt immer die Vorgabeadresse `http://localhost:11434` auf diesem
  Rechner. Ein Ollama auf diesem Rechner liefe unter keiner anderen Adresse.

Gestellt wird der Provider in der Kopfzeile über die Auswahl **Ollama** (**Server** oder
**Lokal**) oder im Chat:

```text
/provider
/provider remote
/provider local
```

Der Wechsel bleibt über einen Neustart erhalten und verändert `server_url` **nicht** –
wer zurückwechselt, muss die Adresse nicht neu eintragen. Was beim Wechsel passiert:
Der Chatverlauf wird verworfen (er gehört zum anderen Server) und die Modellliste neu
geladen.

Ein Wechsel prüft nichts: Ein lokales Ollama kann laufen oder nicht. Läuft es nicht,
zeigt die Kopfzeile das wie bei jedem anderen Server als „Offline“. Die Knöpfe
**Adresse** und **SSH starten** sind im lokalen Provider ausgeblendet und
`/server-url` lehnt dort ab – beides hätte keine Wirkung, und eine Änderung, die
nichts bewirkt, sollte man nicht als Erfolg melden.

Im lokalen Provider gilt außerdem immer der [Terminumfang](#der-umfang-des-modells):
`wirksamer_umfang` in `src-tauri/src/lib.rs` gibt dort die Kalenderwerkzeuge heraus, und
`/scope agent` wird abgelehnt. Der gespeicherte `agent.scope` bleibt dabei unberührt –
wer zurückwechselt, findet seinen Umfang so vor, wie er war. Der Grund ist nicht
Misstrauen in das lokale Modell, sondern seine Größe: Ein kleines Modell, das neben
dem Assistenten auf demselben Rechner läuft, hat keinen Grund, dessen Arbeitsverzeichnis
zu durchsuchen. Auf einem eigenen Ollama in `ollama.service` ist das Modell ohnehin
gegen alle anderen erreichbar – das ist eine Ollama-Eigenschaft, keine von Mimir.

## Serveradresse

Die initiale Ollama-Adresse ist als Vorgabewert in `src-tauri/src/lib.rs` hinterlegt:

```rust
const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
```

Der Vorgabewert zeigt auf den eigenen Rechner. Läuft Ollama woanders, trägt der Knopf
**Adresse** in der Kopfzeile die Adresse ein – oder im Chat:

```text
/server-url
/server-url 192.168.178.42:11434
/server-url https://ollama.example.com
```

Das Backend normalisiert die URL, akzeptiert nur `http` und `https`, lehnt eingebettete Zugangsdaten ab und speichert die Einstellung als `ollama.json` im Tauri-Konfigurationsverzeichnis. So bleibt die Adresse auch nach einem Neustart erhalten. Die Modellliste wird anschließend automatisch neu geladen.

Das SSH-Ziel wird getrennt davon konfiguriert:

```text
/ssh-target
/ssh-target benutzer@ollama.example.org
/ssh-target benutzer@ollama.example.org 2222
```

Der private Schlüssel wird mit `/ssh-key` festgelegt:

```text
/ssh-key
/ssh-key ~/.ssh/id_ed25519
/ssh-key aus
```

Mimir startet `ssh` mit `-F /dev/null` und wertet `~/.ssh/config` deshalb **nicht** aus. Ein dort konfiguriertes `IdentityFile`, `User` oder `Port` wird ignoriert. Schlüssel, die nicht in den OpenSSH-Standardpfaden liegen, müssen deshalb über `/ssh-key` gesetzt werden. Ist ein Schlüssel gesetzt, verwendet Mimir ihn mit `IdentitiesOnly=yes` exklusiv; ohne Vorgabe bleiben `ssh-agent` und die Standardpfade wirksam.

Der gestartete Ollama-Server lauscht immer auf `0.0.0.0`; erreichbar ist er über die mit `/server-url` eingegebene Adresse. **Achtung:** Ollama hat keine eigene Authentifizierung und spricht unverschlüsseltes HTTP. Im LAN kann damit jedes Gerät Modelle laden und ausführen. Für den Betrieb außerhalb eines vertrauenswürdigen Netzes gehört eine zusätzliche Absicherung nach außen (Firewall, WireGuard, Reverse-Proxy mit Auth) oder ein an `127.0.0.1` gebundener Server mit SSH-Portweiterleitung.

Persistentes Konfigurationsformat:

```json
{
  "provider": "remote",
  "server_url": "http://localhost:11434",
  "ssh": {
    "target": "benutzer@ollama.example.org",
    "port": 22,
    "identity_file": "/pfad/zum/schluessel/id25519"
  }
}
```

`provider` ist optional; fehlt das Feld, gilt das entfernte Ollama – eine bestehende Konfiguration startet damit unverändert.

`identity_file` ist optional; fehlt das Feld oder ist es leer, gelten die OpenSSH-Standardpfade und der `ssh-agent`. Der Pfad muss absolut sein, darf keine Platzhalter und kein `..` enthalten und wird auch beim Laden der Konfiguration validiert.

Für den SSH-Start werden ein vorhandener SSH-Schlüssel oder `ssh-agent` benötigt. Mimir speichert keine privaten Schlüssel und das SSH-Passwort nicht. Das **Nextcloud-App-Passwort** kann dagegen auf Wunsch in `calendar-secret.json` liegen, siehe [Anmelden](#anmelden). Das lokale `ssh`-Programm muss im `PATH` verfügbar sein; der Remote-Benutzer benötigt eine SSH-Anmeldung und Ollama an einem der im Backend festgelegten System-/Benutzerpfade oder im geerbten `PATH`.

## SSH-Schlüssel einrichten

Die folgenden Schritte werden einmal auf dem Rechner ausgeführt, auf dem Mimir läuft. `SERVER_IP` und `BENUTZER` müssen durch die Werte des Ollama-Servers ersetzt werden.

1. Prüfen, ob bereits ein Standard-Schlüssel vorhanden ist:

   ```bash
   ls -l ~/.ssh/id_ed25519 ~/.ssh/id_ed25519.pub
   ```

   Sind beide Dateien vorhanden, kann Schritt 2 übersprungen werden. Vorhandene Schlüssel nicht ungefragt überschreiben.

2. Neuen ED25519-Schlüssel erzeugen:

   ```bash
   ssh-keygen -t ed25519 -a 100
   ```

   Den vorgeschlagenen Dateipfad bestätigen. Für die Passphrase können eine sichere Passphrase gewählt oder für einen unverschlüsselten, ausschließlich für dieses Gerät bestimmten Schlüssel zweimal die Eingabetaste gedrückt werden.

3. Öffentlichen Schlüssel auf den Server kopieren. Dabei wird einmalig das normale SSH-Passwort des Remote-Benutzers benötigt:

   ```bash
   ssh-copy-id -i ~/.ssh/id_ed25519.pub BENUTZER@SERVER_IP
   ```

4. Verbindung ohne Passwort prüfen:

   ```bash
   ssh BENUTZER@SERVER_IP
   ```

   Danach `exit` eingeben. Bei einem anderen SSH-Port lautet der Aufruf `ssh -p PORT BENUTZER@SERVER_IP`.

5. Falls für den privaten Schlüssel eine Passphrase vergeben wurde, den `ssh-agent` starten und den Schlüssel hinzufügen:

   ```bash
   eval "$(ssh-agent -s)"
   ssh-add ~/.ssh/id_ed25519
   ```

   Die Desktop-Umgebung, aus der Mimir gestartet wird, muss dabei `SSH_AUTH_SOCK` erben. Andernfalls kann der Grafik-Anwendung die Schlüssel-Passphrase nicht automatisch übergeben werden.

6. SSH-Ziel in Mimir setzen und die Funktion prüfen:

   ```text
   /ssh-target BENUTZER@SERVER_IP
   /server-status
   /server-start
   ```

   Bei einem anderen SSH-Port wird beispielsweise `/ssh-target BENUTZER@SERVER_IP 2222` verwendet. Der Host-Key muss vorher durch den obigen normalen `ssh`-Aufruf in `~/.ssh/known_hosts` bestätigt worden sein. Mimir verwendet `StrictHostKeyChecking=yes` und akzeptiert unbekannte oder geänderte Host-Keys nicht automatisch.

   Liegt der Schlüssel nicht unter `~/.ssh/id_ed25519`, `~/.ssh/id_ecdsa` oder `~/.ssh/id_rsa`, muss er zusätzlich mit `/ssh-key ~/.ssh/PFAD` gesetzt werden, weil Mimir `~/.ssh/config` nicht auswertet.

## Passwort-Fallback für eine Sitzung

1. Zuerst wird immer die Key-/Agent-Anmeldung versucht.
2. Meldet OpenSSH einen echten Authentifizierungsfehler, öffnet der Button `SSH starten` einen maskierten Passwortdialog.
3. Host-Key-Fehler, ein überladener Schlüsselvorrat und nicht erreichbare Server lösen **keinen** Passwortdialog aus, sondern eine Fehlermeldung mit konkreter Handlungsanweisung.
4. Das Passwort wird weder als Slash-Befehl noch in `ollama.json` oder im Chatverlauf gespeichert.
5. Es wird für den laufenden SSH-Prozess in einer kurzlebigen Datei mit restriktiven Rechten an `SSH_ASKPASS` übergeben; die Datei wird anschließend überschrieben und gelöscht.
6. Nach Beenden und erneutem Starten von Mimir muss das Passwort erneut eingegeben werden.

Der `SSH_ASKPASS`-Helper enthält das Passwort nicht, er liest es nur aus der Datei, auf die `SSH_ASKPASS` verweist. Helper und Passwortdatei liegen in einem eigenen Verzeichnis mit `0700` im temporären Verzeichnis, nicht direkt in `/tmp`. Der Inhalt von `/tmp` ist für andere Benutzer auflistbar; dadurch bleibt die Datei auch dann geschützt, wenn Mimir hart beendet wird und die normale Aufräumroutine nicht mehr läuft.

Der Fallback ist unter Linux **und macOS** aktiv; die Grenze im Code ist `unix`. Nur unter Windows meldet Mimir, dass der Passwort-Fallback dort nicht unterstützt wird.

Verwendete Ollama-Endpunkte:

- `GET /api/tags` für die verfügbare Modellliste
- `POST /api/chat` für den Chat mit Streaming-Antworten

### Der Umfang des Modells

Wie weit das Modell reichen darf, steht in `ollama.json` unter `agent.scope` und wird mit `/scope` gesetzt:

```json
{
  "agent": {
    "root": "/home/benutzer",
    "max_steps": 8,
    "scope": "agent"
  }
}
```

`agent` ist die Vorgabe und verhält sich wie bisher. `termine` gibt dem Modell ausschließlich die vier Kalenderwerkzeuge, ohne Dateizugriff und ohne Arbeitsverzeichnis. Der Umfang gilt über einen Neustart hinweg und kann den Zugriff nur verkleinern. Einzelheiten in [docs/agentenmodus.md](agentenmodus.md#der-umfang-alles-oder-nur-termine).

Im lokalen Provider ist der wirksame Umfang immer `termine`, auch wenn hier `agent`
steht: Das Modell läuft dann auf diesem Rechner und bekommt dort keine
Dateiwerkzeuge. Gespeichert wird trotzdem, was eingestellt war, damit der gespeicherte
Umfang über einen Providerwechsel hinweg erhalten bleibt.


## Kontextfenster, Systemanweisung und Verlauf

### Was das Modell bekommt

An das Modell gehen vier getrennte Felder: die Nachrichten (`messages`), eine optionale Systemanweisung (`system`), das Werkzeugangebot (`tools`) und – nur wenn `/context` gesetzt ist – `options` mit der Fenstergröße. Die Systemanweisung ist kein Teil des Verlaufs und wird nicht bei jedem Gespräch wiederholt, sie steht in jedem Auftrag an eigener Stelle.

Die Anweisung aus `/system` liegt in `ollama.json` neben der Server-URL und gilt damit für alle Sitzungen auf diesem Gerät. Sie ist auf 8 KiB begrenzt, Steuerzeichen außer Newline, Tab und Wagenrücklauf werden abgelehnt. Im Agentenmodus steht die Werkzeuganweisung vorn, die eigene Anweisung dahinter. Ein Wechsel der Server-URL oder ein Neustart ändert daran nichts.

Ollamas Vorgabe für das Kontextfenster kennt Mimir nicht. Deshalb wird die Belegung aus der Textlänge geschätzt, etwa 3,2 Zeichen je Token, und oben rechts angezeigt (ab tausend Token gekürzt auf `1.2k`, ab zehntausend auf `12k`). Mit `/context <token>` lässt sich die Größe festlegen; der Wert wird auf ein Vielfaches von 256 gerundet und muss zwischen 2048 und 262144 liegen, 0 steht für die Modellvorgabe. Ab 90 Prozent der eingestellten Größe färbt sich die Anzeige orange. Die Anzeige ist eine Schätzung, keine Messung des Modells – und eher zu niedrig als zu hoch, weil mehrsprachiger Text mehr Zeichen je Token braucht.

### Verlauf nur nach Rückfrage

`/history an` fragt vorher nach, weil dabei alles im Klartext auf die Platte kommt. Wird abgelehnt, entsteht keine Datei. Nach der Freischaltung schreibt das Backend nach jeder fertigen Antwort `chat-history.json` neben `ollama.json`; die Datei bekommt die Rechte `0600` und wird über eine temporäre Datei atomar ersetzt. Werkzeugergebnisse und leere Nachrichten landen nicht darin, weil sie für eine spätere Fortsetzung nichts beitragen. Gespeichert werden höchstens die letzten 100 Nachrichten; ältere bleiben nur im Arbeitsspeicher und werden beim Start nicht nachgeladen. Auch die Anfrage selbst wird vorher auf die letzten 100 Nachrichten gekürzt.

Ohne Freischaltung liefert das Backend beim Laden immer `null`, die Datei lässt sich also auch nicht auslesen. `/history aus` stellt die Einstellung ab und löscht die Datei, `/history löschen` löscht nur die Datei. Ein Wechsel der Server-URL löscht sie ebenfalls, weil der alte Verlauf zum alten Server gehört und sonst beim nächsten Start wieder auftauchen würde.

Beim Start wird ein gespeicherter Verlauf nur dann geladen, wenn das Speichern eingeschaltet ist.

### Dateianhänge

Über die Schaltfläche `Datei` lassen sich Textdateien an die nächste Nachricht hängen. Jede Datei wird eine eigene Nachricht mit Name und Inhalt, die eigene Eingabe bleibt unverändert; mehrere Dateien werden in der Reihenfolge der Auswahl angehängt. Eine Datei mit Nullbyte gilt als Binärdatei und wird abgelehnt. Anhänge werden wie normale Nachrichten behandelt, also im gespeicherten Verlauf mitgesichert und in der Kontextanzeige mitgezählt.

Mimir erhält dadurch keinen Zugriff auf das Dateisystem: Die Datei wird im Browser gelesen und nur als Text übergeben.

**Grenzen und Kürzung.** Eine angehängte Datei geht als *eine* Nachricht an das Modell, und das Backend lässt pro Nachricht 64 KiB zu. Bis zu dieser Größe kommt die Datei vollständig mit. Alles darüber wird **gekürzt, nicht abgelehnt**: Mimir schneidet an einer Absatzgrenze ab, hängt einen sichtbaren Hinweis an das Ende und meldet im Chat, dass der Text gekürzt wurde. Der Grund für die Kürzung steht im Kopf der Nachricht (`– gekürzt`), damit das Modell nicht über Inhalte schreibt, die es gar nicht bekommen hat.

Bis 1 MiB wird eine angehängte Datei überhaupt eingelesen. Darüber weist Mimir sie mit einer Meldung ab, die die Grenze nennt.

Diese Grenzen hängen **nicht** am Modell. Sie schützen lediglich davor, dass eine einzelne Nachricht den Auftrag blockiert; maßgeblich sind `MAX_MESSAGE_BYTES` (64 KiB pro Nachricht), `MAX_PROMPT_BYTES` (512 KiB für den ganzen Verlauf) und `MAX_ATTACHMENT_BYTES` (1 MiB pro Datei) in `src-tauri/src/lib.rs`. Sie liegen bewusst unterhalb dessen, was ein großes Modell könnte: Bei einem Fenster von 262144 Token passen rund 819 KiB Text hinein, eine einzelne Nachricht darf aber nur 64 KiB fassen.

Damit bleibt eine Lücke, die sich nicht durch Einstellen beheben lässt: **Ein Anhang gilt nur für die Nachricht, an die er gehängt wurde.** Für ein ganzes Dokument ist das falsche Werkzeug. Leg es stattdessen mit `/agent-dir <pfad>` ins Arbeitsverzeichnis und lies es im Agentenmodus über `read_file` – das liefert immer die ganze Datei, mit der großzügigeren Grenze von 128 KiB. Eine Datei darüber lässt sich mit diesem Werkzeug nicht lesen, weil es nicht kürzt, sondern verweigert; abschnittweise lesen kann Mimir nicht, es gibt dafür keinen Parameter. Der Inhalt bleibt dafür über mehrere Fragen hinweg verfügbar, während ein Anhang nur für die eine Nachricht gilt.

Diese Grenze ist genau der Punkt, an dem ein Fehler aus dem Betrieb kam: Das Frontend prüfte eine eigene Grenze (128 KiB), das Backend eine strengere (64 KiB). Eine Datei dazwischen wurde als „Angehängt" quittiert und ließ anschließend die Anfrage scheitern, ohne erkennbare Begründung. Die Grenze liegt jetzt nur noch im Backend (`prepare_attachment`); das Frontend hält bewusst keine eigene und lässt sich Kürzung oder Ablehnung zurückmelden. Der Preis dafür: Eine riesige Datei wird im Browser vollständig gelesen und erst danach abgewiesen.

### Fehlertexte von Ollama

Bisher wurde bei einem HTTP-Fehler nur der Status gemeldet. Das Backend liest jetzt bis zu 8 KiB des Antwortkörpers, zieht das Feld `error` aus Ollamas JSON-Fehterantwort und nennt den Text in der Meldung. Damit steht bei einem abgelehnten Modellnamen oder einem vollen Speicher die eigentliche Begründung da, statt nur „Fehler 400".
