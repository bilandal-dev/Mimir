# Agentenmodus

Die drei lesenden Werkzeuge, die Bestätigung vor jedem Aufruf und die schreibenden
Werkzeuge hinter einer Freischaltung.

Der Agentenmodus lässt das Modell selbst Werkzeuge aufrufen, statt nur zu antworten. Er wird im Chat über den Knopf **Agent: aus** oder mit `/agent` eingeschaltet und gilt nur für die laufende Sitzung: Nach einem Neustart wird wieder normal gechatten, damit die Anwendung nie ungefragt Werkzeuge anbietet.

## Umfang: lesend, Schreiben nur auf Freischaltung

Diese drei Werkzeuge sind immer vorhanden, fest im Backend verdrahtet:

| Werkzeug | Wirkung |
|---|---|
| `list_directory(path)` | listet Einträge eines Verzeichnisses, Verzeichnisse mit `/`, Symlinks mit `->` gekennzeichnet; höchstens 500 Einträge |
| `read_file(path)` | liefert den vollständigen Inhalt einer Textdatei bis 128 KiB; größere Dateien werden abgelehnt, nicht gekürzt |
| `search_files(pattern, path)` | findet eine Textstelle und nennt Datei und Zeilennummer |

Zwei weitere gibt es erst, wenn der Benutzer sie mit `/agent-write` oder dem Knopf **Schreiben: aus** freischaltet:

| Werkzeug | Wirkung |
|---|---|
| `write_file(path, content)` | legt eine **neue** Datei an; bestehende Dateien werden abgelehnt |
| `edit_file(path, old_string, new_string)` | ersetzt genau eine Stelle; `old_string` muss wörtlich und **eindeutig** sein |
| `create_calendar_event(summary, start, …)` | legt einen Termin im Nextcloud-Kalender an, samt Erinnerung und Kategorie, siehe [Termine anlegen](#termine-anlegen) |
| `list_calendar_events(range, search)` | **liest** Termine mit ihrer Kennung; dafür genügt die Anmeldung, keine Freigabe. `range` ist eine Zahl in Tagen ab jetzt (Vorgabe 30) |
| `update_calendar_event(title, on_date, start, …)` | ändert einen Termin, benannt über Titel und jetzige Zeit; was nicht genannt wird, bleibt stehen, siehe [Termine lesen, ändern und löschen](#termine-lesen-ändern-und-löschen) |
| `delete_calendar_event(title, on_date)` | löscht einen Termin endgültig, ebenfalls über Titel und Zeit benannt, siehe [Termine lesen, ändern und löschen](#termine-lesen-ändern-und-löschen) |

Solange der Schreibmodus aus ist, sind die Schemata dieser Werkzeuge nicht einmal im Anfrage enthalten, das Modell kann sie also nicht verlangen. Die drei schreibenden Kalenderwerkzeuge kommen zusätzlich nur dazu, wenn Mimir auch im Kalender angemeldet ist. Und weiterhin gibt es **keine** Funktion, um Dateien umzubenennen oder Befehle auszuführen: `run_command` oder `delete_file` werden abgelehnt, ein Modell kann keine eigenen Werkzeuge definieren. Lesen und Schreiben laufen über getrennte Funktionen (`execute_read_only_tool` und `execute_write_tool`), damit der Lesepfad seine Eigenschaft „kann nichts verändern" nicht verliert.

Beim Anlegen wird der genannte Kalendername **exakt** verglichen, absichtlich ohne unscharfen Vergleich: Der Schreibpfad entscheidet, in welchem Kalender geschrieben wird, und ein Treffer auf Verdacht wäre stiller Datenverlust. Beim Ändern und Löschen wird dagegen unscharf verglichen, weil dort der Termin vorher gefunden und gelesen wird.

Der Schreibmodus gilt **nur für die laufende Sitzung** und wird bewusst nicht in `ollama.json` gespeichert: Ein dauerhaft gesetzter Schreibzugriff würde einen Zugang hinterlassen, den niemand mehr erwartet.

## Jeder Schreibvorgang wird als Unterschied gezeigt

Vor jedem Schreibvorgang erscheint ein Fenster mit dem **Vorher/Nachher-Unterschied** der Datei, nicht mit den Argumenten des Modells. Die Vorschau entsteht im Backend aus derselben Funktion (`plan_write`), die auch das Schreiben ausführt, sie kann also gar nicht von der tatsächlichen Wirkung abweichen. Erst nach „Schreiben" wird ausgeführt; bei „Ablehnen" bekommt das Modell die Ablehnung als Werkzeugergebnis und es wird nichts verändert. Gleiches gilt bei einem Fehler: Ein unmöglicher Schreibvorgang scheitert schon in der Vorschau, das Modell sieht den Grund und arbeitet weiter.

Unveränderter Kontext steht im Fenster um die Änderung herum, lange unveränderte Abschnitte werden zu einer Zeile zusammengefasst. Nach dem Schreiben bietet der Schritt einen **Rückgängig**-Knopf: Er stellt den vorherigen Inhalt wieder her oder entfernt eine neu angelegte Datei wieder.

## Feste Sandbox

Alles, was gelesen wird, liegt unterhalb genau eines Arbeitsverzeichnisses, das mit `/agent-dir <pfad>` gesetzt wird (Vorgabe: das Home-Verzeichnis des Benutzers). Die Befehlszeile wird an Leerzeichen getrennt, deshalb funktioniert ein Pfad mit Leerzeichen nicht – der Rest der Zeile müsste als ein Argument gelten. Jeder Zugriff prüft:

- Nur relative Pfade. Absolute Pfade, `..` und Backslashes werden abgelehnt.
- `canonicalize` löst alle Symlinks auf, danach wird geprüft, ob das Ergebnis wirklich unterhalb des Arbeitsverzeichnisses liegt. Ein Symlink nach außen wird abgelehnt, bei der Suche werden Symlinks ganz übersprungen.
- Harte Grenzen: höchstens 128 KiB je gelesener Datei, 200 Fundstellen, 8 Verzeichnisebenen, 2000 besuchte Dateien, 1 MiB je durchsuchter Datei, 500 Einträge je Verzeichnisliste, 256 Zeichen Suchmuster, 64 KiB Werkzeugausgabe (zuzüglich des Kürzungshinweises) und 8 Werkzeugaufrufe je Nachricht. Jede Kürzung wird in der Oberfläche als „gekürzt" ausgewiesen.
- Binärdateien und ungültiges UTF-8 werden nicht ausgegeben, sondern abgelehnt.
- Höchstens `max_steps` Schritte je Nachricht (Vorgabe 8, Obergrenze 20, in `ollama.json` unter `agent` einstellbar).

Beim Schreiben gelten dieselben Regeln, mit fünf Ergänzungen, weil es hier nichts zu lesen gab:

- Der Pfad muss nicht existieren. Deshalb wird das **Elternverzeichnis** kanonisiert und der Dateiname angehängt; ein `..` kann so nicht aus dem Arbeitsverzeichnis herausführen, und der Dateiname selbst ist kein Sprung nach außen.
- Ein Symlink als Ziel wird nicht beschrieben, und die temporäre Datei entsteht mit `O_NOFOLLOW`, folgt also auch unterwegs keinem Link. Geschrieben wird atomar über eine neue Datei im selben Verzeichnis plus `rename`; ein Absturz hinterlässt keine halbe Datei.
- Die Konfigurationsdatei von Mimir und jeder `.git`-Ordner sind gesperrt.
- Höchstens 12 Schreibvorgänge und 512 KiB je Zug. Das Budget steht im Backend und wird zu Beginn jedes Zuges zurückgesetzt; die Anzeige im Chat holt sich beide Grenzen von dort, damit Anzeige und Durchsetzung nicht auseinanderlaufen. Dazu kommen Grenzen an den einzelnen Werten: 256 KiB je geschriebenem Text, 64 KiB für das gesuchte `old_string`, 512 KiB für den ganzen Argumentumfang und 64 KiB für die Vorschau.
- Der Inhaltstext wird **ungekürzt** gelesen: Ein `trim` würde Einrückung und abschließende Zeilen still verändern.

## Jeder Aufruf wird bestätigt

Bevor ein Werkzeug läuft, erscheint ein Dialog mit Werkzeugname, Pfad, den Argumenten und dem Arbeitsverzeichnis. Bei genau einem Argument steht dieses als `name: wert` im Feld, ab zwei kommt das JSON eingerückt darunter – eine einzelne Zeile wäre unlesbar. Erst nach Bestätigung wird ausgeführt. Wird abgelehnt, bekommt das Modell das als Werkzeugergebnis mitgeteilt und kann mit dem arbeiten, was es hat, oder sagen, was ihm fehlt. Ein Werkzeugfehler ist ausdrücklich **kein** Abbruch des Laufs.

## Was das Modell sieht

Das Modell erhält den Inhalt gelesener Dateien im Prompt. Damit verlassen diese Daten den Rechner und gehen an den Ollama-Server im LAN. Das ist beabsichtigt, aber es ist die wichtigste Datenabfluss-Stelle des Programms: Alles, was im Arbeitsverzeichnis liegt, kann gelesen werden. Ein enges Arbeitsverzeichnis ist deshalb die wirksamste Begrenzung.

## Prompt-Injection ist das eigentliche Risiko

Jeder gelesene Dateiinhalt landet im Prompt und kann das Modell anweisen, etwas zu tun - etwa eine scheinbar harmlose Anweisung in einer Quelldatei, die später eine Exfiltration nahelegt. Gegen das hilft kein Filter im Code, sondern nur die Bestätigung pro Aufruf: Der Benutzer sieht jedes Mal, welches Werkzeug mit welchen Argumenten laufen soll. Schreibende Werkzeuge würden dieses Risiko erheblich verschärfen und sind deshalb nicht enthalten.

## Kein Netzzugriff

Mimir spricht ausschließlich mit dem konfigurierten Ollama-Server und der eingetragenen Nextcloud-Instanz im lokalen Netz. Es gibt **keine** Websuche, kein Abrufen einer Adresse und keinen Weg, auf dem eine Anfrage aus Mimir hinausgeht.

Das ist eine bewusste Entscheidung, keine Lücke. Eine Suche hätte zwei Folgen: Der Suchanbieter sähe jede Frage – bei einer Frage nach dem eigenen Kalender also, was als Suchbegriff hinausgeht – und eine geladene Seite wäre Fremdtext im Prompt, der das Modell direkt anweisen kann, etwas zu tun. Der letzte Punkt ließe sich nur durch die Bestätigung pro Abruf begrenzen, und für einzelne Seiten ist eine Suchmaschine ohnehin die falsche Abkürzung.

Aktuelle Fragen beantwortet Mimir aus dem Trainingsstand des Modells. Für Wissen aus dem eigenen Bestand ist der Weg über das Arbeitsverzeichnis gedacht: Datei ablegen, Agentenmodus, `read_file`. Die Daten bleiben dabei auf dem eigenen Rechner.

## Schrittanzeige

`/tools` beantwortet eine Frage: Was darf das Modell gerade? Deshalb steht die Liste nicht als Fließtext, sondern zweispaltig und nach **lesend** und **schreibend** getrennt – man muss nicht jede Zeile lesen, um zu sehen, dass `write_file` gar nicht dabei ist, solange der Schreibmodus gesperrt ist. Über der Liste steht das Arbeitsverzeichnis, weil es die Grenze ist, an der das Modell scheitert, und man es sonst in den Einstellungen sucht. Darunter steht bei gesperrtem Schreibmodus, **welche** Werkzeuge mit `/agent-write` dazukämen – ohne Anmeldung im Kalender sind es nur die beiden Dateiwerkzeuge, und diese Zahl wird nicht fest behauptet. Die Beschreibungen kommen unverändert aus dem Angebot an das Modell; sie hier zu kürzen hieße eine zweite Fassung pflegen, die irgendwann nicht mehr stimmt.

Jeder Werkzeugschritt erscheint in der Antwort als Zeile mit Werkzeugname und Pfad, aufklappbar auf die vollständige Ausgabe. Abgelehnte Schritte sind rot, fehlgeschlagene ebenfalls. Die Zeilen bleiben über alle Modellanfragen eines Zuges stehen. Die Werkzeugbox richtet sich nach ihrem Inhalt (`width: fit-content`) und ist damit unabhängig von der Länge der Antwort; der Rand fällt höchstens auf die verfügbare Breite um. Nach dem Zug landen nur Nutzer- und Antworttexte im Chatverlauf; Werkzeugaufrufe und -ergebnisse werden nicht mitgenommen.

Konfiguration in `ollama.json`:

```json
{
  "server_url": "http://ollama.example.org:11434",
  "ssh": { "target": "benutzer@ollama.example.org", "port": 22 },
  "agent": { "root": "/pfad/zum/verzeichnis", "max_steps": 8 }
}
```
