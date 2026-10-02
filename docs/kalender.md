# Kalender

Anmeldung an der Nextcloud-Instanz, die Leiste am rechten Rand, Erinnerungen und
Kategorien sowie die drei Werkzeuge, mit denen das Modell Termine anlegt, ändert
und löscht.

## In fünf Schritten eingerichtet

1. In Nextcloud unter **Einstellungen → Sicherheit** ganz unten ein **App-Passwort** erzeugen. Nicht das Hauptpasswort verwenden; ein App-Passwort gilt nur für CalDAV und lässt sich jederzeit widerrufen, ohne das Hauptpasswort zu ändern.
2. Die Basisadresse der Instanz heraussuchen, etwa `http://192.168.2.176:8080` oder `http://192.168.2.176/nextcloud`. In den Nextcloud-Einstellungen steht sie unter WebDAV.
3. In Mimir `/calendar` eingeben. Es öffnet sich ein Fenster mit Adresse, Benutzername, App-Passwort und dem Kästchen **Passwort merken**.
4. Läuft die Instanz über `https`, erscheint vorher ein zweites Fenster mit dem Fingerabdruck des Zertifikats. Der Wert lässt sich auf dem Rechner nachsehen, siehe [Zertifikat](#zertifikat). Ohne Bestätigung wird das Passwort nicht gesendet.
5. Danach steht die Leiste rechts mit den nächsten Terminen. `/termine` schreibt dieselben Termine als Agenda in den Chat, nach Tagen gruppiert und mit der Farbe des Kalenders. `/calendar` zeigt den Stand, `/calendar aus` meldet ab.

## Was sie kann und was nicht

Die Leiste am rechten Rand zeigt die nächsten Termine aus einer Nextcloud-Instanz im lokalen Netz, gelesen über CalDAV. Die Leiste selbst bleibt **nur lesend**; sie zeigt und aktualisiert, mehr nicht.

Angelegt wird über das Modell, im Agentenmodus und mit ausdrücklicher Freigabe: `create_calendar_event` schreibt eine einzelne `.ics`-Datei in den gewählten Kalender. Geändert und gelöscht wird über `update_calendar_event` und `delete_calendar_event`, ebenfalls mit Vorschau und Freigabe. Beide nehmen den Termin über **Titel und Uhrzeit** entgegen, wie du sie sagst – die Kennung muss nur das Modell wissen und ermittelt sie selbst; siehe [Termine lesen, ändern und löschen](#termine-lesen-ändern-und-löschen).

Die Termine werden von Mimir selbst geholt und angezeigt, nicht vom Modell. Sie landen deshalb nicht im Prompt und kosten keinen Kontext. Erst wenn du im Chat nach ihnen fragst, geht Text an das Modell.

## Termine anlegen

Im Agentenmodus kann das Modell einen Termin anlegen. Das Werkzeug `create_calendar_event` erscheint nur, wenn **beides** zutrifft: Der Schreibmodus ist freigegeben (`/agent-write`) und Mimir ist im Kalender angemeldet. Ohne Anmeldung bekommt das Modell das Werkzeug gar nicht erst angeboten, weil es sich sonst ankündigen und an jedem Aufruf scheitern würde. Melden und Abmelden leeren den Zwischenspeicher der Werkzeugliste, damit der Stand im nächsten Zug stimmt.

Der Ablauf ist derselbe wie bei einer Datei: Das Modell meldet einen Termin an, Mimir zeigt den **genauen Inhalt** der Datei im Bestätigungsfenster, und erst nach deinem Ja geht der Auftrag an den Kalender. Ablehnen bewirkt gar nichts.

```
› Trage mich morgen um 9 zum Zahnarzt ein, 15 Minuten vorher erinnern

  ┌ create_calendar_event: Kalender Persönlich ──────────────┐
  │ Neuer Termin: Zahnarzt                                   │
  │ 14.09.2026, 09:00 Uhr bis 14.09.2026, 10:00 Uhr          │
  │ Kalender Persönlich                                      │
  │ Erinnerung 15 Minuten vorher                             │
  │                                                           │
  │ + BEGIN:VCALENDAR                                         │
  │ + VERSION:2.0                                            │
  │ + BEGIN:VEVENT                                           │
  │ + SUMMARY:Zahnarzt                                        │
  │ + DTSTART:20260914T070000Z                                │
  │ + BEGIN:VALARM                                           │
  │ + TRIGGER:-PT15M                                         │
  │ ...                                                       │
  │ Wird im Kalender angelegt und lässt sich über Mimir       │
  │ nicht zurücknehmen.                            [Abrechen] │
  └───────────────────────────────────────────────────────────┘
```

Nach dem Speichern holt die Leiste sofort neu, damit der neue Termin ohne Warten auftaucht. Im Agentenlauf erscheint **kein** Rückgängig-Knopf: Eine Datei lässt sich aus dem vorherigen Stand wiederherstellen, ein Kalendereintrag nicht.

Was das Werkzeug nicht kann:

- **Keine Teilnehmer.** `attendees` wird abgelehnt, mit der Begründung im Fehlertext. Mimir verschickt keine Einladungen; lädt die Beteiligten in Nextcloud selbst ein. Ein Aufruf mit Teilnehmern legt nichts an, damit niemand eine Einladung für einen Termin erwartet, den es nicht gibt.
- **Kein Löschen.** `delete_calendar_event` macht das; hier nicht, damit kein Aufruf versehentlich löscht, wer anlegen wollte.
- **Keine Anlagen.** `attachments` und `attach` werden mit Begründung abgelehnt. Eine Datei als `ATTACH` müsste base64 in die Termin-Datei, und die wäre dann nur an eine Kopie des Termins gehängt – in Nextcloud lädt man sie besser selbst hoch. Das Modell sagt das dem Benutzer, statt es zu behaupten.
- **Keine wiederkehrenden Termine.** `RRULE` kennt das Werkzeug nicht.

## Erinnerung und Kategorie

Beides sind Angaben, die nicht am Termin selbst stehen, sondern an ihm hängen. Mimir rechnet die Worte des Benutzers – wie bei der Uhrzeit –, und schreibt sie in die Termin-Datei:

| Feld | Was es aufnimmt | Wie es in der Datei landet |
| --- | --- | --- |
| `reminder` | „5 Minuten vorher“, „eine halbe Stunde vorher“, „eine Stunde vorher“, „am Vorabend“, „eine Woche vorher“; zum Abschalten „keine Erinnerung“ | `BEGIN:VALARM` … `TRIGGER:-PT5M` … `END:VALARM` |
| `category` | „Arbeit“, „privat, Wichtig“ | `CATEGORIES:Arbeit;privat;Wichtig` |

Verstanden werden Ziffern und die Zahlwörter bis sechzig, die Einheiten Minute, Stunde, Tag und Woche in allen gebräuchlichen Schreibweisen samt Abkürzungen, die Bruchformen halb/viertel/dreiviertel sowie „am Vorabend". Jedes andere Wort ist ein Fehler mit der Liste der verstandenen Formen – **nicht** eine geratene Zahl. Genau das ist der Punkt: Im Betrieb hat das Modell eine Erinnerung behauptet, die es nicht gab, und dafür den Termin von 15:00 auf 15:05 verschoben. Heute bekommt es dafür eine Fehlermeldung und eine Nachfrage.

Beim **Ändern** gilt dasselbe, und was nicht genannt wird, bleibt stehen – inklusive einer Erinnerung, die Nextcloud angelegt hat, samt Zeilen wie `REPEAT`, die Mimir gar nicht schreibt. Geändert wird nur der `TRIGGER`, eine fehlende `DESCRIPTION` wird ergänzt, damit die Meldung nicht leer ist. „keine Erinnerung" nimmt den ganzen Block weg. Steht eine Erinnerung schon auf demselben Wert, gilt das als wirkungslos und nicht als Erfolg. Der Bestätigungsfenster nennt die Wirkung im Klartext („Erinnerung: 5 Minuten vorher") und lässt den Unveränderten daneben stehen („Bleiben erhalten: die Kategorien").

Termin und Erinnerung werden an der Wurzel getrennt behandelt, nicht nur beim Schreiben: Beide tragen eine `DESCRIPTION`, und ein Zugriff auf die Terminbeschreibung darf nicht die Erinnerung treffen. Deshalb sucht `ics.rs` Eigenschaften des Termins nur außerhalb des `VALARM`-Blocks, und für die Erinnerung gibt es eigene Zugriffe.

## Worte statt gerechneter Daten

Das Modell hat keine Uhr. Ohne Angabe rät es ein Datum, und im Betrieb stand so ein Termin im Jahr 2023 im Kalender, drei Jahre in der Vergangenheit, weil der Benutzer „heute 14 Uhr" gesagt hatte. Desweise gilt:

1. **Jede Anfrage nennt den heutigen Tag.** Er steht im Systemfeld, und zwar bei **jeder** Anfrage neu berechnet – nicht beim Laden der Seite und nicht einmal vor einem Agentenlauf, der über eine Mitternacht dauern könnte.
2. **Das Werkzeug rechnet, nicht das Modell.** Die Worte des Benutzers gehen unverändert durch und werden hier aufgelöst: `heute`, `morgen`, `übermorgen`, `in N Tagen`, `montag` bis `sonntag` (jeweils die nächste Gelegenheit), `14.9.`, `14.09.2026`, `5. Oktober`, `14:00`, `14.00`, `14 Uhr`, `um 14 Uhr`. Ohne Jahr meint `1.1.` den **nächsten** 1. Januar, nicht den vergangenen. Die Schreibweise mit Versatz (`2026-09-14T09:00+02:00`) bleibt exakt erhalten.
3. **Ein Sicherheitsnetz fängt geratene Daten ab.** Mehr als sieben Tage in der Vergangenheit werden abgelehnt, mit der Begründung und dem Vorschlag, die Worte des Benutzers zu übernehmen. Sieben Tage reichen für „das habe ich gestern vergessen". Ältere Einträge bleiben Nextcloud vorbehalten.

## Dynamisch, nicht fest – und wie das geprüft wird

Nichts davon ist eingetragen. Beide Seiten lesen die Uhr zur **Aufrufzeit**:

- **JS** ruft `datumsangabeFuerModell()` in `systemPromptForRequest()` auf, und das läuft bei **jeder** Anfrage – nicht beim Laden der Seite und nicht einmal vor einem Agentenlauf. Ein langer Lauf über eine Mitternacht bekommt deshalb für jeden Schritt ein frisches Datum.
- **Rust** rechnet über `Local::now().date_naive()` bzw. `client::now()`; auch im Kalenderwerkzeug steckt kein Datum im Quelltext.

Der Text entsteht aus `new Date()` plus `setDate`. Das ist wichtig: `setDate` rechnet mit dem **Kalender**, nicht mit der Uhr. Ein `+ 86_400_000` wäre an den beiden Tagen der Sommerzeitumstellung eine Stunde daneben.

Die Zahlen sind ausgerechnet, nicht vom Modell verlangt: „MORGEN ist der 2026-10-02 (Freitag)". Das Modell rechnet also nicht selbst – es liest. Die Wochenzuordnung rechnet über den **Montag** der Woche, weil der Vergleich der Tage danebenliegt: Der Sonntag derselben Woche ist vom Donnerstag aus sieben Tage entfernt.

Geprüft wird das über **Zeiträume**, nicht an Stichproben:

| Ebene | Umfang | Ergebnis |
| --- | --- | --- |
| JS-Ausgabe | 1095 verschiedene HEUTE × 7 Folgetage = 7665 | 0 Abweichungen |
| Rust-Wochentage | 3 Jahre × 365 Tage × 3 Wochentage | 0 Abweichungen |

Beim Nachrechnen der Rust-Erwartung kam ein Sonderfall heraus, der eine falsche Annahme in meinem Test aufdeckte: Am Donnerstag 1.1.2026 ist der nächste Freitag der 2.1. – und der gehört **noch zur laufenden Woche**. Die Erwartung „der nächste Wochentag liegt immer in der kommenden Woche“ war falsch; der Wochentag wird in derselben oder in der kommenden Woche liegen, nie weiter.

Das ist der Punkt, an dem keine Absicherung hilft: **Das Modell muss den Wochentag nicht selbst bestimmen.** Es liest ihn aus dem Systemfeld und aus der Werkzeugantwort. Ob es das tut, ist keine Frage des Codes, sondern des Modells – und genau daran ist der „Sonntag der übernächsten Woche“ gescheitert, nicht an der Rechnung.

## Und wie das abgesichert ist

Die auffälligen Fälle – Monatsanfang, 31. Dezember, Schaltjahr – fallen nur **einmal im Jahr** auf und werden dann als seltsamer Einzelfall abgetan, nicht als Fehler. Ein Test, der an dem Tag läuft, an dem er geschrieben wurde, prüft sie nie. Die Prüfung läuft deshalb über ein ganzes Jahr mit **vorgegebenem** Startdatum statt über die Systemuhr; `Angabe::neu_am` nimmt den heutigen Tag als Parameter, im Betrieb führt `Angabe::neu` an die Uhr. Geprüft werden für jeden der 366 Tage:

- `morgen` ist der Folgetag, `übermorgen` der dritte, `gestern` der vorige
- jeder Wochentagsname und jede Kurzform findet denselben Wochentag
- jeder Wochentagsname liegt in der Zukunft und höchstens eine Woche entfernt

Das hat drei Fehler gefunden, die vorher keiner gesehen hat:

1. **Jeder Wochentag landete einen Tag zu früh.** `naechster_wochentag` zog den heutigen Wochentag über `number_from_monday()` ab – das bei **eins** beginnt –, verglich aber gegen `WOCHENTAGE`, das bei **null** beginnt. „montag“ an einem Montag ergab den **Sonntag**. Chrono hat beide Zählweisen, `num_days_from_monday()` ist die passende.
2. **„mittwoch“ war nicht lesbar** – in der Liste stand „mitwoch“.
3. **„sonnabend“ war nicht lesbar** – es stand in der Liste, „sonntag“ als langes Wort fehlte ganz.

Punkt 2 und 3 hatten dieselbe Folge, und sie ist schlimmer als ein falscher Wochentag: Der Wochentag war nicht zu finden, also blieb nichts übrig als der **heutige Tag**. „Trage mich mittwoch ein“ legte einen Termin auf heute.

Auf der Anzeigeseite gab es denselben Fehler in anderer Form: `tagLabel` in der Leiste rechnete den Folgetag über `86_400_000` Millisekunden. Ein Tag ist aber nicht immer 24 Stunden – in der Nacht der Sommerzeitumstellung sind es 23 oder 25. An genau zwei Tagen im Jahr erkannte die Leiste „Heute“, „Morgen“ und „Gestern“ deshalb nicht. Jetzt steht dort `setDate`, das mit dem Kalender rechnet und nicht mit der Uhr.

## Das Datum im Systemfeld

Ein Satz wie „rechne relative Angaben aus dem heutigen Datum" hat **nicht** gereicht: Auf die Frage nach den Terminen von morgen antwortete das Modell mit dem Vortag. Eine Regel verlangt Rechnung, aber sie sagt nicht, **wovon** – das Modell rechnet dann aus seinem Trainingsstand oder aus einem Datum im Verlauf. Was trägt, ist der Wert selbst, ausgeschrieben:

```
HEUTE IST: 2026-10-01
Das ist Donnerstag, 1. Oktober 2026, 18:33 Uhr Europe/Berlin.

Grundregel für jede Datumsangabe: Relative Wörter – „heute", „morgen",
„übermorgen", „gestern", „in drei Tagen", „nächste Woche", „letzten Montag",
„am Wochenende" – werden IMMER aus HEUTE ausgerechnet. Nicht aus deinem
Trainingsstand, nicht aus einem Datum, das im Gesprächsverlauf steht, und
nicht aus der Uhrzeit, zu der dieser Verlauf angefangen hat. Rechne selbst
und nenne in deiner Antwort das ausgerechnete Datum.

Dieselbe Rechnung, jeweils aus HEUTE – zur Kontrolle:
- GESTERN war der 2026-09-30.
- MORGEN ist der 2026-10-02 (Freitag).
- ÜBERMORGEN ist der 2026-10-03 (Samstag).
- DIESE WOCHE ist 2026-09-28 bis 2026-10-04 (Montag bis Sonntag).
- NÄCHSTE WOCHE ist 2026-10-05 bis 2026-10-11.
- In 4 Tagen beginnt die nächste Woche.
- Die nächsten sieben Tage, jeweils mit Wochentag:
  2026-10-02 ist ein Freitag.
  2026-10-03 ist ein Samstag.
  2026-10-04 ist ein Sonntag.
  ...
```

Die sieben Einzeltage stehen aus zwei Gründen drin. Erstens ist ein Wochentag allein **nicht eindeutig**: „Sonntag“ passt auf jedes Jahr. Als das Modell auf „welche Termine habe ich am Sonntag“ mit „Sonntag, den 04.10“ antwortete, war die Rechnung richtig – der 4. Oktober 2026 ist ein Sonntag –, aber ohne Jahreszahl sah die Antwort aus wie ein Ausflug in die Trainingsdaten. Zweitens lässt sich am Kalender direkt ablesen, ob die Zuordnung stimmt.

Der Schlusssatz des Systemfelds sagt ausdrücklich, dass ein **Wort** im Werkzeugaufruf sogar sicherer ist als eine Zahl: Es kann nicht aus dem Trainingsstand stammen, sondern nur aus diesem Systemfeld.

Die ausgerechneten Folgetage sind der eigentliche Träger: Sie lassen sich am Kalender gegenprüfen, und ein Modell, das danebenliegt, widerspricht sich selbst. Die Woche läuft Montag bis Sonntag, nicht ab „jetzt".

Das Datum steht **nur** dort. Als es zusätzlich in der Werkzeuganweisung stand, waren zwei Zeitangaben nebeneinander im Systemfeld, und welche galt, war nicht mehr entscheidbar. Die Werkzeuganweisung verweist deshalb auf „HEUTE IST" und rät das Modell ausdrücklich **nicht**, wenn die Angabe fehlt.

## Der Zeitraum beim Lesen

Beim **Lesen** rechnet Mimir die Worte nicht auf, weil der Zeitraum dort vom Modell als Datumsgrenze kommt. Der ursprüngliche einzige Parameter war eine Zahl Tage ab jetzt, und die konnte „morgen" nicht treffen: Ihr frühester Start ist „jetzt minus ein Tag", wer den Folgetag meint, bekommt den heutigen mitgeliefert und nennt daraufhin den falschen Tag. Genau das ist passiert.

Deshalb nimmt `list_calendar_events` jetzt `from` und `to` als `JJJJ-MM-TT`, beide einschließlich:

| Parameter | Bedeutung |
| --- | --- |
| `from` | erster Tag des Zeitraums, als `JJJJ-MM-TT` **oder als Wort** |
| `to` | letzter Tag, **einschließlich**; fehlt, ist es genau ein Tag |
| `range` | Tage ab jetzt, für unbestimmte Fragen wie „was steht an" |

Gelesen werden `JJJJ-MM-TT`, `4.10.`, `4.10.2026`, `4. Oktober`, `Oktober 4`, dazu die Worte `heute`, `morgen`, `übermorgen`, `gestern`, `vorgestern`, `montag` bis `sonntag` und `in drei Tagen` – dieselben, die das Schreiben versteht.

Bei Wochentagen zählt, **wochentags** nicht: Am Donnerstag, 1.10.2026,

| Angabe | Ergebnis | |
| --- | --- | --- |
| `freitag` | 2.10. | der nächste Freitag, nicht der volle Montag voraus |
| `montag` | 5.10. | |
| `montag nächste woche` | 12.10. | |
| `übernächster montag` | 12.10. | eine ganze Woche weiter als `montag` |
| `montag diese woche` | 28.9. | die laufende Woche, auch schon vorbei |
| `montag` am Montag, 4.1.2027 | 11.1. | derselbe Wochentag ist **7 Tage** entfernt |

Zwei Fehler waren dabei, beide im gleichen Ausdruck: „übernächster“ enthielt „nächst“ und wurde deshalb nicht erkannt – „übernächster Montag“ landete auf demselben Tag wie „Montag“. Und die Null-Differenz („heute ist Montag“) wurde mit **einem** Tag statt **sieben** übersprungen, woraus „montag“ an einem Montag den Dienstag machte.

Beide Schreibweisen mit Umlaut und mit „ue“ werden gelesen: „uebernachster“ soll keinen anderen Tag bedeuten als „übernächster“. Nur Datumsgrenzen anzunehmen war zu streng: Das Modell schickte `from: übermorgen`, das Parsen scheiterte, und **ohne Fehlermeldung** fielen 30 Tage an. Die Antwort lautete „in diesem Zeitraum von übermorgen keine Termine“ und sah damit nach einer leeren Agenda aus, war aber eine Abfrage nach einem völlig anderen Zeitraum. Das Wort ist sogar die sicherere Form, weil es nicht aus dem Trainingsstand stammen kann.

Deshalb nennt **jede** Antwort den abgefragten Zeitraum mit Datum:

```
Abgefragter Zeitraum: So, 04.10.2026 bis So, 04.10.2026. Das ist die laufende
Woche, Montag bis Sonntag.
1 Termin(e):
- 04.10.2026 15:00  Fußball  (Kalender Familienkalender, Kennung 6586c8…)
```

Ohne diese Zeile gab es keinen Anhaltspunkt dafür, dass ein anderer Zeitraum abgefragt wurde als gemeint war.

## Warum die Woche dabeisteht

Der Teil mit dem Datum allein genügte nicht. Auf die Frage nach dem „Sonntag" antwortete das Modell mit „am Sonntag **übermächste Woche**, den 4. Oktober 2026" – das Datum stimmte, die Bezeichnung nicht: Der 4. Oktober 2026 ist der Sonntag, mit dem **diese** Woche endet (Mo 28.09. – So 04.10.).

Aus einem Datum allein geht die Woche nicht hervor. „Sonntag" passt auf jede Woche, und die Angabe „NÄCHSTE WOCHE ist 2026-10-05 bis 2026-10-11" stand zehn Zeilen weiter oben. Deshalb steht sie jetzt an zwei Stellen:

- **In der Antwort des Werkzeugs**, wo das Modell sie direkt vor sich hat, wenn es die Frage beantwortet.
- **Bei jedem der sieben nächsten Tage** im Systemfeld: `2026-10-04 ist ein Sonntag in der laufenden.`

Die Zuordnung rechnet über den **Montag** der Woche, nicht über den Abstand in Tagen. Der Sonntag derselben Woche ist vom Donnerstag aus sieben Tage entfernt und läge bei einem Vergleich der Tage in der nächsten Woche.

Ohne `from` bleibt `range` bei 30 Tagen. Intern beginnt das Fenster einen Tag früher, damit eine über den Rand laufende Serie nicht fehlt; dieser Vortag wird aus der Antwort wieder herausgefiltert, sonst stünde in der Antwort zu „morgen" auch der heutige Tag.

Was es prüft, bevor irgendetwas gesendet wird:

| Angabe | Regel |
| --- | --- |
| `summary` | Pflicht, höchstens 200 Zeichen, Zeilenumbrüche werden escaped |
| `start` | Pflicht, `JJJJ-MM-TTThh:mm`, auch mit Sekunden, `Z` oder `+02:00` |
| `end` | Optional, ohne Angabe eine Stunde, immer nach dem Beginn |
| `all_day` | `true` schreibt `DTSTART;VALUE=DATE`, das Ende ist der erste Tag danach |
| `location` | Optional, höchstens 200 Zeichen |
| `description` | Optional, höchstens 2000 Zeichen, wird nach 74 Oktett sauber umgebrochen |
| `calendar` | Pflicht, sobald mehrere Kalender ausgewählt sind; Name oder Pfad |
| `reminder` | Optional, „vorher" in Worten: „5 Minuten vorher", „eine Stunde vorher", „am Vorabend"; frühestens eine Minute, höchstens 90 Tage |
| `category` | Optional, höchstens 100 Zeichen, mehrere mit Komma; ein Semikolon im Namen wird escaped |
| unbekannte Felder | werden namentlich abgelehnt, statt stillschweigend wegzulassen |
| weit in der Vergangenheit | abgelehnt; sieben Tage Rückblick sind erlaubt |
| ohne Uhrzeit | es wird nachgefragt statt geraten |

Eine leere Auswahl in der Leiste heißt wie dort „alle Kalender". Anlegen darf Mimir trotzdem nicht in den ersten Kalender, den die Serverantwort nennt: Bei mehreren wird der Benutzer gefragt, welcher gemeint ist.

Ohne Zeitzonenangabe gilt die Uhr des Rechners: 12 Uhr bleibt 12 Uhr, im Januar wie im Juli. Mit Versatz wird der Zeitpunkt umgerechnet und nicht doppelt gelesen. Jahreszahlen von 1970 bis 2200 gelten als plausibel, alles andere wird nachgefragt – ein Termin im Jahr 1700 kommt fast immer von einem gerechneten Datum. Termine mit Uhrzeit dürfen höchstens sieben Tage dauern, Ganztagestermine ein Jahr; ein zweiwöchiger Termin mit Uhrzeit ist mit hoher Wahrscheinlichkeit ein Rechenfehler. Beim **Ändern** gilt die Sieben-Tage-Grenze auch für Ganztagestermine, damit die Regel an einer Stelle bleibt – ein bestehender längerer Urlaub lässt sich deshalb nicht verschieben.

Geschrieben wird mit `PUT` und `If-None-Match: *` auf `<Sammelpfad>/<Kennung>.ics`. Der Kopf `If-None-Match` sorgt dafür: Sollte die Kennung schon vergeben sein, antwortet der Server mit `412`, und der Termin bleibt unangetastet. Die Kennung entsteht aus Zeitstempel und Zähler, die Datei trägt sie im Namen. Der Pfad wird gegen den DAV-Wurzelbereich geprüft und kodiert, ein `..` darin wird abgelehnt. Zertifikat und Umleitungsregel gelten wie beim Lesen: Das Zertifikat wird vor dem Senden geprüft, und eine Umleitung ist kein Speichern.

## Termine lesen, ändern und löschen

Anlegen kann das Modell seit Anfang an, alles Weitere ist bewusst schmaler gehalten. Die Reihenfolge im Chat ist immer dieselbe: erst nachsehen, dann handeln.

**`list_calendar_events` – nur lesen.** Liefert Titel, Zeit, Kalender, Erinnerung, Kategorien und **Kennung** je Termin. Ohne Bestätigung, denn es wird nichts verändert. Der Zeitraum ist eine Zahl in Tagen ab jetzt, ohne Angabe kommen die nächsten 30 Tage. Die Kennung ist der Rückfall, wenn Titel und Uhrzeit nicht eindeutig sind; im Normalfall genügt, was der Benutzer gesagt hat. Erinnerung und Kategorien stehen mit drin, damit das Modell sie kennt – eine unbekannte Erinnerung legt es sonst beim Ändern erfunden an.

**Ein Termin wird auf zwei Arten benannt – und das hat einen Grund.** Beim ersten Einsatz schickte das Modell den **Titel** als Kennung: Der Benutzer hatte „den Termin ‚angelegt durch KI' von heute auf morgen verschieben" gesagt, das Modell kannte nur den Titel, schickte ihn ins Feld `uid`, und der Vorgang scheiterte mit „dieser Termin wurde bereits entfernt oder verschoben". Danach:

| Feld | Bedeutung |
| --- | --- |
| `uid` | die Kennung aus `list_calendar_events`, unverändert |
| `title` | **welcher** Termin gemeint ist: sein jetziger Titel |
| `on_date` | wo der Termin **jetzt** liegt, im Klartext des Benutzers: „gestern 14:30“ |
| `calendar` | der Kalender, wenn der Benutzer einen genannt hat |
| `summary` | der **neue** Titel – nicht mit `title` verwechseln |
| `start` | der **neue** Beginn: „auf heute 15 Uhr“ gehört hierher |

Mimir löst den Titel selbst auf: ein Treffer wird genommen, mehrere führt zur Nachfrage mit Datum, Titel und Kennung der Kandidaten, keiner zu einer Meldung, die sagt, dass es den Termin nicht gibt. Damit muss das Modell keine Kennung erfinden, und ein Titel, den der Benutzer gesagt hat, reicht als Angabe.

Bei `update_calendar_event` trennt Mimir dabei `on_date` von `start`: Steht ein neuer Beginn **oder ein neues Ende** im Auftrag, meint `on_date` das **Ziel** und darf die Suche nicht einschränken. Ohne diese Regel suchte das Modell bei „verschiebe den Termin auf heute 15 Uhr“ nach einem Termin am heutigen Tag – also gerade nicht dem, der woanders liegt und verschoben werden soll.

Der genannte Kalendername wird unscharf verglichen, weil der Benutzer „privat“ sagt, im Kalender aber `Privat` oder ein Pfad wie `/remote.php/dav/calendars/kai/personal/` steht.

**`update_calendar_event` – nur die genannten Felder.** Der entscheidende Punkt ist, was *nicht* passiert: Mimir baut die Datei nicht neu. Es ersetzt **nur die genannten Eigenschaften** (`SUMMARY`, `LOCATION`, `DESCRIPTION`, `DTSTART`, `DTEND`, `CATEGORIES`, der `TRIGGER` der Erinnerung) in der Datei, wie sie im Kalender liegt, und schreibt alles andere unverändert zurück. Ein Neuaufbau würde Anlagen, Teilnehmer und alles, was ein anderes Programm hineingeschrieben hat, stillschweigend vernichten.

- Nicht genannte Felder bleiben stehen. `description: null` lässt die Beschreibung, ein leerer Text entfernt sie. Ohne `reminder` bleibt eine vorhandene Erinnerung samt `REPEAT` und `DURATION` unangetastet; ohne `category` bleiben die Kategorien stehen.
- `reminder` und `category` rechnen und prüfen wie beim Anlegen, siehe [Erinnerung und Kategorie](#erinnerung-und-kategorie). Eine Erinnerung, die schon auf demselben Wert steht, gilt als wirkungslos und nicht als Erfolg.
- Ohne eigenes Ende bleibt die bisherige **Dauer** erhalten. „Schieb auf morgen 14 Uhr“ frisst also nicht den Nachmittag. Die Dauer wird aus den beiden alten Werten gebildet; rechnet man das alte Ende vom neuen Beginn ab, kommt über Monate eine negative Zahl heraus.
- Zeiten werden in UTC geschrieben und **ohne** den alten `TZID`. `DTSTART;TZID=Europe/Berlin:20260914T070000Z` hieße 07:00 Ortszeit – das Ändern verschöbe den Termin dann stillschweigend um Stunden.
- `DTSTAMP` wird gesetzt und `SEQUENCE` erhöht; sonst nimmt ein Kalender die Änderung nicht immer an.
- Die Zusammenfassung nennt jede Änderung einzeln und sagt, was erhalten bleibt – was gerade geändert wurde, steht dabei nicht noch einmal in der Liste der Erhaltenen.

**`delete_calendar_event` – endgültig.** Der vollständige Termin steht im Bestätigungsfenster, das neue Feld ist leer: Der Benutzer sieht genau das, was verschwindet. Ein Rückgängig-Knopf erscheint nicht, weil es ihn nicht gibt.

**Was beides abgelehnt wird**, und warum das keine Vorsicht um der Vorsicht willen ist:

| Fall | Warum |
|---|---|
| **Serientermin** (`RRULE`) | Eine Änderung gälte für die ganze Reihe. Mimir kann keinen einzelnen Termin einer Reihe ansprechen, „verschiebe meinen Termin“ verschöbe also vier Termine. |
| **Termin mit Teilnehmern** | Eine CalDAV-Instanz schickt beim Ändern oder Löschen eine **Absage** an alle Beteiligten. Das ist eine Nachricht an Menschen und gehört nicht von einer Sprachanweisung ausgelöst. |
| **Termin in weiter Vergangenheit** | Abgelehnt wie beim Anlegen; sieben Tage Rückblick sind erlaubt. Beim Löschen wird das Alter nicht geprüft – dort spielt es keine Rolle, wann der Termin war. |
| **Mehrere ausgewählte Kalender** | Es wird nachgefragt. Ein Ändern im falschen Kalender wäre stiller Datenverlust. |
| **Ohne Änderung** | Es wird nichts geschrieben. Ein Aufruf, der dasselbe noch einmal sendet, ist kein Erfolg – auch nicht für eine Erinnerung, die schon auf diesem Wert steht. |
| **Fremde Kennung im Auftrag** | Wird abgelehnt statt übergangen. |
| **Falsches Feld** | `reminder` und `category` gibt es, unbekannte Namen nicht: Sie werden namentlich abgelehnt, damit kein Tippfehler stillschweigend nichts bewirkt. |

**Der Schutz gegen Überschreiben.** Beim Lesen holt Mimir die Änderungskennung des Termins (`ETag`) und schickt sie beim Schreiben als `If-Match` mit. Wurde der Termin zwischenzeitlich in Nextcloud oder von einem zweiten Gerät verändert, antwortet der Server mit `412` und Mimir fasst nichts an – der Termin bleibt unangetastet, und die Meldung sagt genau das. Ohne ETag vom Server wird gar nicht erst geschrieben. Die Adresse wird außerdem gegen den DAV-Wurzelbereich geprüft und kodiert, ein `..` darin kann nicht aus dem Kalender herausführen.

An der Reihenfolge liegt etwas: Der Termin wird erst **unmittelbar vor** dem Schreiben geholt, nicht schon für die Vorschau. Zwischen Vorschau und Klick können Sekunden liegen – und in genau diesem Fenster kann Nextserver jemand anderes sein.

## Anmelden

`/calendar` zeigt, wenn bereits angemeldet ist, Adresse, Terminstand und die Anzahl der Termine in der Leiste. Sonst öffnet es – bei hinterlegter Adresse nach einer Rückfrage – ein Fenster mit drei Feldern: Adresse, Benutzername und **App-Passwort**. Dafür wird ein App-Passwort gebraucht, nicht das Hauptpasswort des Kontos. In Nextcloud unter Einstellungen, Sicherheit ganz unten wird eines erzeugt; es lässt sich jederzeit widerrufen, ohne das Hauptpasswort zu ändern.

Das App-Passwort steht nie in `ollama.json`, sondern auf Wunsch in `calendar-secret.json` neben der Konfiguration, mit den Rechten `0600` und atomar geschrieben. Beim Start wird es wieder eingelesen, damit die Anmeldung nicht nach jedem Neustart neu gemacht werden muss. Ohne das Kästchen „Passwort merken" gilt es nur für die laufende Sitzung, und `/calendar aus` entfernt es aus dem Speicher und von der Platte, die Adresse und der Benutzername bleiben stehen.

Beim Wechsel des Benutzernamens wird die Datei gelöscht, weil ein Passwort immer zu genau einem Konto gehört. Die Leiste zeigt am Fuß, ob das Passwort gemerkt wurde. Das Hauptpasswort des Kontos gehört nicht in Mimir; ein App-Passwort ist dafür gedacht und lässt sich in Nextcloud jederzeit widerrufen, ohne das Hauptpasswort zu ändern.

Umleitungen werden nicht verfolgt. Eine Umleitung auf einen anderen Host dürfte das Passwort nicht mitnehmen, deshalb bleibt es beim reinen HTTP keine andere Wahl; bei `https` hätte reqwest ohnehin ein Problem mit der Verbindung. Weist eine Adresse auf `https://` um, nennt Mimir genau das zur Hand, weil sich die Instanz selbst so entschieden hat. Auf eine andere Umleitung nennt Mimir die WebDAV-Adresse aus den Nextcloud-Einstellungen.

Adressiert wird die Basis der Instanz, etwa `http://192.168.2.176:8080` oder `http://192.168.2.176/nextcloud`. Fehlt das Schema, wird `http://` ergänzt.

Die vollständige WebDAV-Adresse aus den Nextcloud-Einstellungen wird ebenfalls verstanden: Sie zeigt auf den eigenen Principal, etwa `https://cloud.example.org/remote.php/dav/principals/users/benutzer`. Mimir schneidet den DAV-Anhang ab, benutzt den Sammelpfad der Kalender selbst und übernimmt den Benutzernamen aus dem Pfad ins Feld – er muss also nicht zusätzlich getippt werden. Gespeichert wird immer nur die Basis der Instanz, nie der kopierte Pfad. `https` ist ebenfalls erlaubt, weil viele Nextcloud-Instanzen im LAN nichts anderes ausliefern: Viele Instanzen haben `force_ssl` gesetzt und leiten jeden Klartextaufruf selbst auf `https://` um, gemessen wurden dafür `301` für `GET` und `308` für `PROPFIND`, `REPORT` und `PUT`. Mimir verlangt also keine Verschlüsselung – die Instanz schon.

## Zertifikat

Bei `https` prüft Mimir die Zertifikatskette nicht, denn eine selbstsignierte Instanz ist darüber nicht erreichbar, und sie ungeprüft zu nutzen hieße, jedem TLS-Server zu trauen. Stattdessen wird genau ein Zertifikat bestätigt und danach verglichen:

1. Mimir fragt `status.php` ab, ohne Zugangsdaten zu senden, und liest das vorgelegte Zertifikat aus.
2. Passt es zum bestätigten, geht es weiter. Passt es nicht, ist der Zugang gesperrt, bis der Fingerabdruck neu bestätigt wurde. Ohne bestätigtes Zertifikat wird gar nicht erst das Passwort gesendet.
3. Der Fingerabdruck ist `SHA-256` über das Zertifikat, geschrieben genau wie `openssl x509 -noout -fingerprint -sha256`: zwei Hexzeichen pro Oktett, Großbuchstaben, Doppelpunkte als Trenner, nur ohne das Präfix `SHA256 Fingerprint=`. Er steht im Fenster und lässt sich auf dem Rechner nachsehen:

```text
openssl s_client -connect HOST:443 </dev/null 2>/dev/null | openssl x509 -noout -fingerprint -sha256
```

Bestätigt wird über `/calendar zertifikat` oder automatisch, sobald eine neue `https://`-Adresse eingegeben wird. Sie ist auch ohne gespeicherten Benutzernamen möglich, weil beim ersten Anmelden der Name erst im Fenster eingetragen wird und das Zertifikat davor geprüft wird; Adresse und Zertifikat werden gemerkt, der Benutzername trägt erst die anschließende Anmeldung nach. Eine Freigabe gilt nur für genau die Adresse, für die sie erteilt wurde; für eine andere Adresse gilt keine. Wird das Zertifikat auf der Instanz erneuert, verweigert Mimir den Zugang und nennt den alten und den neuen Fingerabdruck. Das ist beabsichtigt: Ein stillschweigend ausgetauschtes Zertifikat wäre sonst ein Weg, das Passwort abzufangen.

Als Anbieter kommt `ring` zum Einsatz, weil es ohne zusätzliche Werkzeuge baut; ein systemweites OpenSSL wird nicht gebraucht. Wichtig dabei: rustls bringt absichtlich keinen Anbieter mit, und reqwest verlangt einen schon beim Bauen *jedes* Clients – auch für reine Klartext-Verbindungen wie die zu Ollama. Mimir setzt ihn deshalb einmal ganz vorn beim Programmstart; fehlt er, bricht schon die erste Serverprüfung ab.

## Was gelesen wird

Zwei DAV-Arten, mehr nicht. Je Aktualisierung sind es aber mehr als zwei HTTP-Aufrufe: erst die Zertifikatsprüfung, dann ein `PROPFIND` und **ein `REPORT` je ausgewähltem Kalender**.

1. `PROPFIND` mit `Depth: 1` auf den Sammelpfad der Kalender liefert Namen, Pfad, Änderungskennung und Farbe. Technische Ordner wie Papierkorb und Posteingang tragen weder Kennung noch Namen und fallen damit weg.
2. `REPORT` mit `calendar-query` und einem Zeitraum von gestern bis 92 Tage voraus (93 Tage, damit laufende Termine sichtbar bleiben) liefert nur die Termine in diesem Fenster, keine ganze Kalenderdatei. Bis zu 200 Termine kommen zurück, die Leiste zeigt die nächsten acht.

Wiederholungen werden für den Zeitraum aufgefaltet. Unterstützt sind Frequenz, Intervall, Zähler, Ende, Monate, Ausnahmen und verschobene Termine über `RECURRENCE-ID`; Wochentage mit vorangestellter Zahl und Monatstage gelten bei monatlichen und jährlichen Regeln. Ganztagestermine, Zeiten mit Zone, Zeiten ohne Zone sowie gefaltete Zeilen und maskierte Sonderzeichen werden richtig gelesen.

Zwei Grenzen, die man kennen sollte: `BYDAY` und `BYMONTHDAY` werden nur bei `MONTHLY` und `YEARLY` ausgewertet – `FREQ=DAILY;BYDAY=MO,WE` erzeugt also jeden zweiten Tag und nicht nur Montage und Mittwoche. Und eine **wöchentliche** Regel, die ausschließlich ordinale Wochentage nennt (`BYDAY=1MO`), trifft damit auf keinen Tag: Der Termin verschwindet dann ganz, statt einmalig zu erscheinen. Was hier nicht abgebildet wird, wird nicht geraten: Eine Regel mit `BYSETPOS`, `BYYEARDAY` oder `BYWEEKNO` erscheint einmalig statt falsch aufgeklappt, und eine unbekannte Zeitzone lässt die Uhrzeit so stehen, wie sie geschrieben wurde, statt sie umzudeuten.

## Eine unvollständige Konfiguration

Beim Start darf die Kalenderkonfiguration die Anwendung nie aufhalten. Nach einem abgebrochenen Anmeldevorgang kann eine Adresse ohne Benutzernamen hinterleg sein, und eine kopierte WebDAV-Adresse kann ein Pfad sein, den Mimir nicht kennt. Beides wird beim Laden stillschweigend in eine brauchbare Form gebracht: Der DAV-Anhang wird abgeschnitten, ein unbrauchbarer Benutzername verworfen, eine unlesbare Adresse geleert. Der nächste `/calendar` trägt dann den Rest neu ein. Der eigentliche Benutzername wird erst an der einen Stelle verlangt, wo er gebraucht wird: bei der Anmeldung.

## Zustände und Fehler

Fehler stehen in der Leiste und nicht als Nachricht im Chat, sonst mischt sich ein Serverproblem in die Unterhaltung. Die Meldungen nennen den nächsten Handgriff: 403 eine fehlende Kalender-App oder eine falsche Adresse, 404 ein falsches Unterverzeichnis, 405 eine fehlende Kalender-App **oder** ein Proxy, der `/remote.php` nicht an PHP weiterreicht, 415 denselben Proxy.

Bei 401 nennt Mimir die drei Gründe, die fast immer zutreffen, statt den Standardtext des Servers auszubreiten: Das App-Passwort wurde widerrufen, es wurde ein neues erzeugt und das alte steht noch im Fenster, oder der Benutzername ist anders geschrieben als in Nextcloud – im DAV-Pfad zählt Groß- und Kleinschreibung. Bei allen anderen Status wird der Text des Servers zusätzlich genannt, weil er dort etwas beiträgt. Ein aus dem Fenster kopiertes App-Passwort wird vor dem Senden getrimmt, weil Nextcloud es in einem Dialog mit Zeilenumbruch zeigt und ein unbemerkt mitgeschlepptes Leerzeichen sonst wie ein falsches Passwort aussieht.

Abgeholt wird **alle fünf Stunden**, aber nur bei sichtbarem Fenster und bei ausgeklappter Leiste – im Hintergrund und bei eingeklapptem Panel entfallen die Abrufe. Läuft gerade eine Unterhaltung, kann ein Abruf sie zeitlich überlagern; er schreibt seinen Fehler in seine eigene Zeile, und die Termine sind für das Modell ohnehin ohne Bedeutung. Fünf Stunden sind so gewollt: Die Leiste ist ein Anzeigegerät und kein Meldedienst. Wer früher etwas braucht, drückt den Erfrischungsknopf, fragt mit `/termine` oder legt einen Termin an – danach wird ohnehin sofort neu geholt.

## Ein Termin im Fenster ändern

Ein Klick auf einen Termin in der Leiste öffnet ihn in einem Fenster mit allen Feldern: Titel, Beginn, Ende, ganztägig, Erinnerung, Ort, Kategorien und Beschreibung. Gespeichert wird in zwei Schritten – der erste Klick auf **Speichern** holt den Unterschied zum Stand im Kalender und zeigt ihn, der zweite Klick schreibt. Grund ist derselbe wie beim Werkzeugaufruf: Der Benutzer soll sehen, was sich ändert, und nicht nur ein Formular absegnen. Der Unterschied kommt aus derselben Planung wie das Schreiben, er kann also nicht etwas anderes zeigen, als geschrieben würde. Eine Änderung an einem Feld macht die gezeigte Vorschau ungültig, damit der zweite Klick nie etwas anderes schreibt als das, was gerade zu sehen ist.

Das Fenster geht denselben Weg wie das Modell:

1. Beim Öffnen wird der Termin **frisch vom Server** geholt, nicht aus der Liste der Leiste – die kann Stunden alt sein.
2. Das Fenster bekommt die Felder so aufbereitet, wie sie im Kalender stehen: Zeiten in der Zeitzone des Rechners, Kategorien in einer Zeile, die Erinnerung in Worten wie „15 Minuten vorher". Das Frontend rechnet nichts um. Was unverändert zurückkommt und unverändert gespeichert wird, ist für Mimir dieselbe Datei.
3. Beim Speichern wird der Termin **noch einmal unmittelbar davor** geholt. Zwischen der Vorschau und dem Klick können Sekunden liegen, in denen Nextcloud den Termin ändert – genau dafür gibt es die Änderungskennung (`If-Match`).
4. Geschrieben wird über dieselbe Planung wie beim Werkzeug: dieselben Prüfungen, dieselbe Zeilenbearbeitung, alles Ungenannte bleibt stehen.

Der Zielkalender steht als **Pfad** in der Terminliste, nicht als Name. Zwei Kalender können gleich heißen, und ein Schreibvorgang im falschen Kalender wäre stiller Datenverlust. Der Pfad wird auch zurückgeschickt, sodass beim Speichern nicht noch einmal über einen Namen aufgelöst wird.

Was das Fenster nicht kann:

- **Serientermine und Termine mit Teilnehmern.** Die Felder sind dann gesperrt und der Grund steht dabei – eine Änderung gälte für die ganze Reihe, ein Löschen schickt eine Absage an Menschen. Dieselbe Grenze wie beim Modell.
- **Kein Anlegen und kein Löschen.** Das Fenster ändert einen Termin, der schon da ist. Anlegen und Löschen bleiben beim Agentenmodus mit Bestätigung.
- **Keine Anlagen und keine Teilnehmer.** Sie stehen in der Datei und bleiben unangetastet.
- **Eine Erinnerung, die Mimir nicht lesen kann**, wird nicht stillschweigend weggeworfen: Das Feld zeigt dann einen Hinweis, und beim Speichern bleibt der Block stehen.
- **Eine Uhrzeit ohne Zone im Kalender** wird als die hingenommen, die im Fenster steht. Sie umzudeuten wäre geraten; der Fenstertext sagt, worum es geht.

Ein Termin, an dem nichts geändert wurde, wird nicht geschrieben – auch dann nicht, wenn er schon in der Vergangenheit liegt. Sonst ließe sich ein Termin von gestern nicht einmal umbenennen.

## Kalenderauswahl

Ohne Auswahl zeigt die Leiste alle lesbaren Kalender. Über „Auswahl" lässt sich einschränken; die Auswahl wird gemerkt und nur diese Kalender werden abgefragt. „Alle" blendet die volle Liste ein, wenn mehr Termine anfallen als die Leiste zeigen kann.

## Erinnerung, Kategorie und Dauer in der Anzeige

Erinnerung und Kategorien stehen in der Leiste und in der Agenda von `/termine` als eigene Zeile unter dem Termin – die Erinnerung etwas lauter, die Kategorien danach. Beim Lesen wird getrennt zwischen den Zeilen des Termins und denen seines `VALARM`-Blocks: Beide können eine `DESCRIPTION` tragen, und die Benachrichtigung darf nicht als Beschreibung des Termins in der Leiste landen.

Die Formulierung kommt aus dem Backend (`reminder_text`, aus derselben Funktion wie im Bestätigungsfenster). Das Frontend übersetzt die Minuten nicht ein zweites Mal – sonst würden Leiste, Agenda und Bestätigungsfenster irgendwann verschiedene Wörter für dieselbe Angabe zeigen. Ein `TRIGGER`, der keine lesbare Dauer ist, wird nicht geraten: Dann fehlt die Zeile ganz, statt dass eine Zahl erfunden wird. Höchstens zwölf Kategorien je Termin, mehr ist Rauschen aus der Datei.

Die **Dauer** wird aus Ende minus Beginn gerechnet und in beiden Anzeigen genannt, aber an unterschiedlichen Stellen, weil die Rahmen unterschiedlich sind:

| | Anzeige |
| --- | --- |
| Leiste | in der Zeitzeile: `09:00 · 1 Std. 30 Min.`, bei einem mehrtägigen Ganztagestermin `5 Tage ganztägig` |
| `/termine` | in der eigenen Zeile unter dem Titel, wie Ort und Kategorien: `1 Std. 30 Min.` |

Leer bleibt die Angabe bei einem Punkttermin (Beginn = Ende), bei einem eintägigen Ganztagestermin und unter einer Minute – „0 Minuten" oder „1 Tag" stünde sonst bei fast jedem Termin da und wäre nur Rauschen. Ganztagestermine werden in Tagen gezählt und gerundet, weil ein Tag über eine Zeitumstellung hinweg 23 oder 25 Stunden hat. Die Leiste ist mit Erinnerung, Kategorien und Dauer um bis zu drei Zeilen je Termin gewachsen; auf 260 px Breite war deshalb keine zusätzliche Zeile für die Dauer möglich.

## Ohne Netz im LAN

Verbindungsprobleme werden beim Lesen genau einmal wiederholt, HTTP-Fehler nie: 401 bleibt 401. Beim Lesen eines einzelnen Termins und beim Schreiben wird gar nicht wiederholt – dort wäre ein zweiter Versuch ein zweiter Schreibvorgang. Geteilt mit dem Ollama-Client sind HTTP/1.1 und die Regel, Umleitungen nicht zu folgen; die Keepalive-Zeit ist mit 60 s kürzer als dort, und die Anfrage selbst hat 5 s für den Verbindungsaufbau, 15 s für die Kalenderliste und 30 s für den Abruf. Der Chat wartet weiterhin ohne Gesamtgrenze, das darf ein Kalenderabruf nicht erben.

Ein nicht erreichbarer Kalender blockiert nichts: Der Chat läuft weiter, und der Fehler steht in der Zeile der Leiste. Der letzte Terminstand wird dabei allerdings verworfen – ein liegen gebliebener Termin wäre nicht als veraltet erkennbar und damit schlimmer als eine leere Leiste mit Fehlermeldung. Nach dem nächsten erfolgreichen Abruf steht er wieder da.

## Was noch fehlt

Bewusst offen und nicht etwa vergessen:

- **Keine Serientermine und keine Termine mit Teilnehmern.** Beides wird abgelehnt, und die Begründung steht in der Meldung: Eine Änderung gälte für die ganze Reihe, eine Absage geht an Menschen. Beides gehört in Nextcloud, wo es der Benutzer selbst entscheidet.
- **Keine Einladungen.** `create_calendar_event` lehnt `attendees` ab. Einladungen zu verschicken ist eine eigene Aufgabe mit eigenen Fehlerfällen: Zustellung, Rückfragen, Änderungen durch die Eingeladenen. Das gehört in Nextcloud, wo es geprüft wird.
- **Keine Anlagen.** `attachments` und `attach` werden mit Begründung abgelehnt, weil eine Anlage base64 in der Termin-Datei stünde und damit nur an eine Kopie gehängt wäre. Erinnerung und Kategorie lassen sich dagegen sehr wohl setzen, siehe [Erinnerung und Kategorie](#erinnerung-und-kategorie).
- **Keine wiederkehrenden Termine anlegen.** `RRULE` kennt das Anlegen nicht; nur beim Lesen werden Serien aufgefaltet – und auch dort nicht jede Regel vollständig, siehe [Was gelesen wird](#was-gelesen-wird).
- **Keine Monats- oder Wochenansicht.** Angezeigt wird die Liste der nächsten Termine. Die Daten für 92 Tage liegen bereits vor, eine Rasteransicht wäre also vor allem Darstellung – aber sie braucht ihre eigene Bedienung, und die Leiste ist dafür zu schmal.
- **Kein Zugriff auf Aufgaben oder Kontakte.** CalDAV kann beides, Mimir liest nur Termine.
- **Keine Kalender im Modellkontext ohne Nachfrage.** `/termine` schreibt Termine in den Chat und damit in den Kontext der nächsten Anfrage; bei vielen Terminen kostet das Fenster. Bewusst so, weil 50 Termine leicht 2 bis 4 Tausend Token ausmachen.
- **Kein Klartext über eine Instanz, die es erzwingt.** Mimir kann beides, aber eine Nextcloud mit `force_ssl` leitet Klartext selbst um. Wer dort Klartext möchte, setzt auf dem Server `'force_ssl' => false`.
