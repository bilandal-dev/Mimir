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

## Der Umfang: alles, oder nur Termine

Neben dem Sitzungsschalter gibt es den **Umfang**, der angibt, welche Werkzeuge das Modell überhaupt bekommt. Er steht in `ollama.json` unter `agent.scope` und wird mit `/scope` gesetzt:

```text
/scope
/scope agent
/scope termine
```

| Umfang | Werkzeuge | Arbeitsverzeichnis |
|---|---|---|
| `agent` (Vorgabe) | die drei lesenden, dazu die freigegebenen schreibenden, dazu der Kalender bei Anmeldung | nötig |
| `termine` | ausschließlich die vier Kalenderwerkzeuge | nicht nötig |

Der Umfang ist das, was den Unterschied ausmacht. Im Terminumfang gilt:

- **Keine Dateiwerkzeuge, weder lesend noch schreibend.** Sie sind nicht abgeschaltet, sondern gar nicht erst im Angebot, und stehen auch nicht im Prompt – ein nur abgeschaltetes Werkzeug ließe die Anweisung stehen und das Modell glauben, es gäbe es doch.
- **Kein Arbeitsverzeichnis.** Ein eingetragenes wird weder geprüft noch im Prompt genannt, und `/agent-dir` verlangt keines.
- **Der Schalter ist gegenstandslos.** Knopf **Agent** und `/agent-write` haben dort nichts zu tun und sagen das. Die Werkzeugschleife läuft ohnehin: Der Umfang ist hier die Freischaltung, sonst wären drei Schalter für eine Fähigkeit nötig – eine Hürde ohne Sicherheitsgewinn.

Und das bleibt unverändert: **Jeder einzelne Vorgang wird vorher als Unterschied gezeigt** und wartet auf die Freigabe. Der Umfang ersetzt diese Bestätigung nicht, er betrifft nur die Auswahl der Werkzeuge.

Der Umfang gilt über einen Neustart hinweg und wird – anders als der Schreibmodus – gespeichert. Das ist unbedenklich, weil er den Zugriff nur verkleinern kann: Es gibt keine Form von `termine`, die mehr erlaubt als `agent`. Ein unbekannter Wert aus einer von Hand bearbeiteten Datei bricht den Start ab, statt stillschweigend als breiter Umfang gelesen zu werden.

Ohne Anmeldung über `/calendar` hat das Modell im Terminumfang gar kein Werkzeug; Mimir sagt das beim Umschalten und in `/tools`.

## Der lokale Provider hat nur Termine

Mit `/provider local` – in der Kopfzeile **Lokal** – läuft das Modell auf demselben Rechner wie Mimir. Dort gilt immer der Terminumfang, auch wenn in `agent.scope` der Agentenmodus steht: `wirksamer_umfang` in `src-tauri/src/lib.rs` gibt die Kalenderwerkzeuge heraus, `/scope agent` wird abgelehnt, und `list_tools` braucht kein Arbeitsverzeichnis. Die Durchsetzung sitzt im Backend an jeder Stelle, an der Werkzeuge angeboten oder eine Schreibfreigabe geprüft wird – eine nur in der Oberfläche gesetzte Anzeige wäre bei der nächsten Änderung wieder weg.

Gespeichert wird der Umfang dabei nicht überschrieben. Wer auf `remote` zurückwechselt, findet seinen Umfang so vor, wie er war.

Das ist keine Sicherheitsgrenze gegen das Modell, sondern eine Festlegung: Ein Ollama auf diesem Rechner ist ohnehin gegen alles erreichbar, was auf diesem Rechner läuft – das ist eine Ollama-Eigenschaft, keine von Mimir. Die Grenze liegt bei Mimir, also gilt sie dort. Die [Konfiguration](konfiguration.md#provider-woher-die-modelle-kommen) nennt den Grund ebenfalls.

Bei **mehreren** ausgewählten Kalendern stehen deren Namen im Prompt **und** in der Beschreibung des Werkzeugfeldes `calendar`. Der Kalendername wird absichtlich exakt verglichen (`waehle_kalender`), weil der Schreibpfad entscheidet, in welchem Kalender geschrieben wird – ein Treffer auf Verdacht wäre stiller Datenverlust. Ein erfundener Name fällt deshalb auf, und zwar bevor etwas geschrieben wird.

## Was die Läufe zeigen

Der Terminumfang ist mehrfach mit einem echten Modell durchlaufen worden, mit `qwen2.5:7b` und deutschen Sätzen. Gemessen wurde zweierlei: ob das Modell das richtige Werkzeug wählt, und ob Mimir aus den gelieferten Argumenten einen Termin bauen kann (`plan_event`, dieselbe Funktion wie die Vorschau).

| | erster Lauf | nach der Überarbeitung |
|---|---|---|
| richtiges Werkzeug | 10 von 12 | 5 von 6 |
| vollständig brauchbar | **1 von 12** | **5 von 6** |

Die drei Befunde des ersten Laufs und was daraus wurde:

**1. Der Kalendername stand in keiner Anweisung.** Häufigster Grund. Das Schema sagte `calendar` sei „Name des Zielkalenders, wenn mehrere ausgewählt sind" – welche Kalender das sind, stand nirgends. Das Modell riet: „Arbeit", „Arbeitskalender", „WorkCalendar". Behoben an zwei Stellen: Die Namen stehen jetzt im Prompt (`kalender_aufgabe`) **und** in der Beschreibung des Feldes `calendar`. Die zweite Stelle war nötig – nach der ersten Korrektur ließ das Modell den Wert noch in zwei von fünf Sätzen weg. Das Feld, das gefüllt werden soll, hat offenbar die kürzeste Aufmerksamkeit.

**2. Uhrzeiten ohne Zahl.** „übermorgen früh" und „Donnerstagmittag" kamen an, und Mimir lehnte ab, weil keine Uhrzeit im Text stand. Das war die richtige Vorsicht – Mimir rechnet keine Uhrzeit aus „früh" – aber es führte zu einer Rückfrage, wo der Benutzer gar keine Lücke gelassen hatte. Jetzt sind sieben Tageszeiten festgelegt: `nachts` 22:00, `frühmorgens` 6:30, `früh` 8:00, `vormittag` und `vormittags` 9:00, `mittag` 12:00, `abend` 18:00. Gesucht wird nur in **ganzen Wörtern**, sonst würde aus „Frühstück mit Anna" ein Vormittagstermin.

**3. Erfundene Felder.** Aus „Freitagmittag" wurde `start: "Freitag um 14:00"` mit `end: "15:00"` – die Uhrzeit geraten, das Ende ebenso. Daraus sind zwei Regeln geworden. Fehlt `end`, gilt die Vorgabedauer, statt eine zu raten. Und: **ein Ende ohne Tagesangabe gehört zum Tag des Beginns**, weil „15:00" für sich allein der heutige Tag ist und der Termin sonst in der Vergangenheit läge. Ein Ende *mit* Tag bleibt, wie es ist.

**Was übrig bleibt:** Bei „Vergiss bitte das Fitnessstudio am Donnerstag" fragt das Modell nach Kalender und genauer Uhrzeit, statt `delete_calendar_event` zu rufen. Das ist keine Fehlfunktion, sondern eine berechtigte Rückfrage – der Satz nennt weder Kalender noch Uhrzeit.

### Nachfragen: versucht und zurückgenommen

Der Wunsch war, das Modell bei unvollständigen Angaben ausdrücklich nachfragen zu lassen. Ein eigener Prompt-Absatz mit den vier Lücken – fehlender Titel, fehlende Zeit, fehlender Kalendername, fehlender Bezug zu einem Termin – und passende Hinweise an den Pflichtfeldern.

**Das Ergebnis war schlechter, und der Versuch ist zurückgenommen.** Die brauchbaren Sätze gingen von 5 von 6 auf 3 von 12. Die Anweisung hat das Modell nicht zum Nachfragen gebracht, sondern zum Aufrufen des Werkzeugs mit **erfundenen** Werten: Aus „Mach mir morgen um 15 Uhr einen Termin" wurde ein Aufruf mit `summary`, `start` und `calendar` – drei Feldern, von denen der Benutzer genau keines genannt hatte. Und aus „Trag morgen um 14 Uhr einen Termin mit der Hausärztin ein" wurde ein Kalendername, den niemand genannt hatte.

Die Ursache ist im Prompt selbst: Ein Kalender ist **Pflicht**, und die Feldbeschreibung sagt „Nimm genau einen von: Persönlich, Arbeit". Diese beiden Sätze zusammen lesen sich als Auftrag, das Feld zu füllen – nicht als Erlaubnis zu fragen. Ein Modell, das zum Ausfüllen angewiesen wird, füllt.

Hinzu kam eine Fehlmessung auf meiner Seite: Die Prüfung auf ein Fragezeichen im Antworttext wertete „Um einen Termin mit Sarah einzutragen, benötige ich mehr Informationen" als kein Nachfragen, obwohl es eines ist. Die Messung ist jetzt breiter – ein Fragezeichen **oder** eine der üblichen Formulierungen.

Was bleibt, ist deshalb die Formulierung in `calendar_event_prompt`, die schon vorher da war: *„Kannst du eine Angabe nicht auflösen, frag nach, statt zu raten."* Sie wirkt – der Sarah-Satz zeigt es. Ein zusätzlicher Absatz mit denselben Regeln hat sie nur überschrieben, und zwar zum Nachteil.

**Beide Punkte sind inzwischen umgesetzt – und beide brauchten einen Eingriff am Werkzeug, nicht im Prompt.**

1. **Ort und Beschreibung** werden gegen die Worte des Benutzers geprüft und fallen weg, wenn er sie nicht genannt hat, siehe [docs/kalender.md](kalender.md#erfundene-felder).
2. **`calendar` ist ein optionales Feld geworden.** Trägt das Modell nur ein, wenn der Benutzer einen Kalender genannt hat. Sonst bleibt es leer, die Planung antwortet mit der Frage *„In welchen Kalender soll der Termin?"*, und das Modell gibt sie im Chat weiter.

Punkt 2 bestätigt, was der Fehlschlag oben zeigte: Der Prompt war nie die richtige Stelle. *„Nenn für jeden Termin einen dieser Namen"* war zusammen mit einem Pflichtfeld ein Auftrag, und ein Modell, das zum Füllen angewiesen wird, füllt. Erst als die **Feldbeschreibung** sagte, dass ein leerer Wert richtig ist, blieb das Feld leer.

**Was die Messung nicht zeigt:** Das Prüfprogramm stellt eine Frage, bekommt eine Antwort und misst – es führt keinen zweiten Zug. Der Fall „Termin ohne genannten Kalender" ist damit als *unbrauchbar* gezählt, obwohl er der richtige Verlauf ist: Der erste Zug stellt die Frage, der zweite legt an. Eine ehrliche Bewertung bräuchte einen zweiten Durchlauf mit der Frage im Verlauf.

Ein Nebenumstand des Laufs: Der Server im LAN verschwand unter der Last mehrfach und kam nach ein bis zwei Minuten wieder. Der Durchlauf setzt deshalb `keep_alive`, damit Ollama das Modell nicht je Satz neu lädt.

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
