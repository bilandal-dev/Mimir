//! Termine ändern und löschen.
//!
//! Geplant wird hier, ausgeführt in `client.rs`. Beides ist unumkehrbar, deshalb
//! gilt für beides dasselbe wie beim Anlegen: nichts geschieht ohne Freigabe des
//! Benutzers, und die Vorschau zeigt den vollständigen Inhalt der Datei, die
//! verändert wird.
//!
//! Zwei Fälle prüft `pruefe_aenderbar`, weil sie Eigenschaften des Termins sind
//! und nichts mit dem Auftrag zu tun haben:
//!
//! - **Serientermine.** Eine Änderung an einem Termin mit `RRULE` gälte für die
//!   ganze Reihe. Mimir kann keinen einzelnen Termin einer Reihe ansprechen,
//!   „verschiebe meinen Termin“ verschöbe also fünf Termine.
//! - **Termine mit Teilnehmern.** Eine CalDAV-Instanz schickt beim Ändern oder
//!   Löschen eine Absage an alle Beteiligten. Das ist eine Nachricht an
//!   Menschen und gehört nicht von einer Sprachanweisung ausgelöst.
//!
//! Weitere Grenzen stehen in `plan_update` und `plan_delete`: ein leeres
//! `title`/`on_date`, eine `uid`, die nicht zum geladenen Termin passt, ein
//! Ergebnis ohne jede genannte Änderung sowie die Frage nach dem Kalender bei
//! mehreren ausgewählten. Ohne Treffer wird in keinem Fall etwas geraten.

use super::ics::Ics;
use super::write::{
    self, clean, escape_text, parse_datum, MAX_BESCHREIBUNG_CHARS, MAX_DAUER_TAGE,
    MAX_KATEGORIE_CHARS, MAX_ORTS_CHARS, MAX_RUECKLIEGEND_TAGE, MAX_UEBERSCHRIFT_CHARS,
};
use super::CalendarConfig;
use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use serde::Deserialize;
use serde::Serialize;

/// Der Name des Werkzeugs, das einen Termin ändert.
pub const UPDATE_TOOL: &str = "update_calendar_event";
/// Der Name des Werkzeugs, das einen Termin löscht.
pub const DELETE_TOOL: &str = "delete_calendar_event";

/// Argumente für das Ändern.
///
/// Von den Feldern, die **bestimmen**, welcher Termin gemeint ist, muss eines
/// da sein – `uid` oder `title`. Alles andere ist optional: Was nicht genannt
/// wird, bleibt stehen. Das ist der entscheidende Punkt – ein Ändern, das alle
/// Felder neu setzt, würde Erinnerungen, Kategorien und Anlagen vernichten.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    /// Welcher Termin, genau. Die Kennung aus list_calendar_events.
    #[serde(default)]
    pub uid: Option<String>,
    /// Welcher Termin, nach seinem jetzigen Titel gesucht. Für einen Menschen
    /// die natürliche Formulierung, für ein Modell leichter zu treffen als eine
    /// Kennung.
    #[serde(default)]
    pub title: Option<String>,
    /// Nur bei mehreren Terminen mit gleichem Titel: welches davon.
    #[serde(default)]
    pub on_date: Option<String>,
    #[serde(default)]
    pub calendar: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub all_day: Option<bool>,
    #[serde(default)]
    pub location: Option<String>,
    /// `null` lässt die Beschreibung stehen, ein leerer Text entfernt sie.
    #[serde(default)]
    pub description: Option<Option<String>>,
    /// Wie weit vor dem Beginn es erinnern soll, in den Worten des Benutzers.
    /// `keine Erinnerung` nimmt sie weg. Ohne das Feld bleibt die vorhandene
    /// Erinnerung stehen.
    #[serde(default)]
    pub reminder: Option<String>,
    /// Die Kategorien, etwa „Arbeit“. Mehrere mit Komma trennen, ein leerer Text
    /// nimmt sie weg.
    #[serde(default)]
    pub category: Option<String>,
}

/// Argumente für das Löschen.
///
/// Wie beim Ändern: `uid` oder `title` bestimmt den Termin.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteRequest {
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub on_date: Option<String>,
    #[serde(default)]
    pub calendar: Option<String>,
}

/// Was beim Ändern geschehen soll.
#[derive(Clone, Debug)]
pub struct UpdatePlan {
    pub calendar_display: String,
    /// Der Inhalt, wie er jetzt im Kalender steht.
    pub vorher: String,
    /// Der Inhalt, der geschrieben wird.
    pub nachher: String,
    pub summary: String,
}

/// Was beim Löschen geschehen soll.
#[derive(Clone, Debug)]
pub struct DeletePlan {
    pub calendar_display: String,
    /// Der Inhalt, der verschwindet. Steht in der Vorschau, damit der Benutzer
    /// sieht, was es erwischt.
    pub vorher: String,
    pub summary: String,
}

/// Prüft, ob an diesem Termin etwas geändert werden darf.
///
/// Steht hier und nicht im Aufrufer, weil die Antwort eine Eigenschaft des
/// Termins ist, nicht eine des Arguments.
pub fn pruefe_aenderbar(ics: &Ics) -> Result<(), String> {
    if ics.has("RRULE") || ics.has("RECURRENCE-ID") {
        // Ein Termin mit `RRULE` ist das erste Glied der Reihe, einer mit
        // `RECURRENCE-ID` ein verschobenes oder abgesagtes. In beiden Fällen
        // gälte die Änderung für die ganze Reihe, und ein einzelnes Glied kann
        // hier nicht adressiert werden.
        return Err(
            "Das ist ein Terminteil einer Serie; die Änderung oder das Löschen gälte für alle \\
             Termine der Reihe. Mimir kann einen einzelnen Termin einer Reihe nicht ansprechen. \\
             In Nextcloud geht das ohne Umweg."
                .to_string(),
        );
    }

    if ics.has("ATTENDEE") {
        return Err(
            "An diesem Termin sind andere beteiligt. Ein Ändern oder Löschen schickt ihnen eine \\
             Absage, und das soll der Benutzer selbst entscheiden. In Nextcloud geht das."
                .to_string(),
        );
    }

    Ok(())
}

/// Liest die Kennung aus den Werkzeugargumenten, falls das Modell sie nennt.
///
/// Sie kommt aus dem Kalender und kann deshalb Formen annehmen, die im Pfad
/// etwas bedeuten; für die Adresse wird sie deshalb kodiert.
///
/// Ist keine da, wird der Termin über seinen Titel gesucht. Der Grund ist eine
/// Erfahrung aus dem Betrieb: Das Modell hat den **Titel** als Kennung geschickt
/// („angelegt durch KI“) und der Vorgang schlug fehl. Ein Titel ist für ein Modell
/// leichter zu treffen als eine Kennung – und Mimir kann ihn auflösen.
pub fn kennung_aus(arguments: &serde_json::Value) -> Option<String> {
    let uid = arguments
        .get("uid")
        .and_then(|wert| wert.as_str())
        .map(clean)
        .filter(|wert| !wert.is_empty())?;

    if uid.chars().any(|c| c.is_control()) {
        return None;
    }

    Some(uid)
}

/// Prüft, dass überhaupt benannt wurde, **welcher** Termin gemeint ist.
pub fn pruefe_auswahl(arguments: &serde_json::Value) -> Result<(), String> {
    if kennung_aus(arguments).is_some() {
        return Ok(());
    }

    let titel = arguments
        .get("title")
        .and_then(|wert| wert.as_str())
        .map(clean)
        .filter(|wert| !wert.is_empty());

    if titel.is_some() {
        return Ok(());
    }

    Err(
        "Welcher Termin ist gemeint? Nenne entweder die Kennung aus list_calendar_events \
         oder seinen jetzigen Titel im Feld title."
            .to_string(),
    )
}

/// Der Dateiname, den CalDAV für diese Kennung erwartet.
pub fn datei_name(uid: &str) -> String {
    uid.chars()
        .map(|character| match character {
            ' ' => "%20".to_string(),
            '/' => "%2F".to_string(),
            '?' => "%3F".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// Baut den Plan für das Ändern.
///
/// `bestehend` ist der Inhalt, wie er jetzt im Kalender steht, und kommt
/// unmittelbar vom Server. Nur daraufhin wird der neue Inhalt gebildet.
pub fn plan_update(
    config: &CalendarConfig,
    kalender: &[(String, String)],
    arguments: &serde_json::Value,
    bestehend: &str,
    uid: &str,
) -> Result<UpdatePlan, String> {
    let request: UpdateRequest = serde_json::from_value(arguments.clone()).map_err(|fehler| {
        format!(
            "Die Angaben passen nicht zu update_calendar_event: {fehler}. Erlaubt sind \\
             \"uid\", \"title\", \"on_date\", \"calendar\", \"summary\", \"start\", \"end\", \\
             \"all_day\", \"location\", \"description\", \"reminder\" und \"category\"."
        )
    })?;

    if let Some(genannt) = &request.uid {
        // Der Vergleich sieht nach doppelter Prüfung aus, ist es aber nicht:
        // `kennung_aus` verwirft eine Kennung mit Steuerzeichen und sucht dann
        // nach Titel, `request.uid` enthält sie trotzdem. Weicht sie vom geladenen
        // Termin ab, wird abgelehnt, statt stillschweigend den gefundenen zu
        // ändern.
        if genannt.trim() != uid.trim() {
            return Err("Die Kennung im Auftrag passt nicht zu dem geladenen Termin.".to_string());
        }
    }

    // Eine leere Titelangabe dürfte nicht stillschweigend auf „jeder Termin“
    // hinauslaufen. Die Prüfung steht hier, weil sie eine Eigenschaft des
    // Aufrufs ist.
    if request
        .title
        .as_deref()
        .is_some_and(|titel| clean(titel).is_empty())
    {
        return Err(
            "Das Feld title ist leer. Nenne den Titel des Termins, oder lass es weg und nutze uid."
                .to_string(),
        );
    }

    if request
        .on_date
        .as_deref()
        .is_some_and(|tag| clean(tag).is_empty())
    {
        return Err(
            "Das Feld on_date ist leer. Lass es weg, oder nenne den Tag, etwa „morgen“."
                .to_string(),
        );
    }

    let (_, calendar_display) = zielkalender(config, kalender, request.calendar.as_deref(), uid)?;

    let mut ics = Ics::parse(bestehend)?;
    pruefe_aenderbar(&ics)?;

    let mut geaendert: Vec<String> = Vec::new();

    if let Some(titel) = &request.summary {
        let titel = clean(titel);

        if titel.is_empty() {
            return Err(
                "Ein Termin braucht eine Überschrift. Zum Leeren des Titels gibt es kein \\
                 Feld; löschen wäre der Weg."
                    .to_string(),
            );
        }

        if titel.chars().count() > MAX_UEBERSCHRIFT_CHARS {
            return Err(format!(
                "Die Überschrift ist zu lang (höchstens {MAX_UEBERSCHRIFT_CHARS} Zeichen)."
            ));
        }

        if ics.get("SUMMARY").as_deref() != Some(titel.as_str()) {
            ics.set("SUMMARY", &escape_text(&titel));
            geaendert.push(format!("Titel: {}", text_kurz(Some(&titel))));
        }
    }

    if let Some(ort) = &request.location {
        let ort = clean(ort);

        if ort.chars().count() > MAX_ORTS_CHARS {
            return Err(format!(
                "Der Ort ist zu lang (höchstens {MAX_ORTS_CHARS} Zeichen)."
            ));
        }

        let neu = (!ort.is_empty()).then(|| escape_text(&ort));
        if ics.get("LOCATION") != neu {
            match neu {
                Some(wert) => ics.set("LOCATION", &wert),
                None => ics.remove("LOCATION"),
            }
            geaendert.push(format!("Ort: {}", text_kurz(Some(&ort))));
        }
    }

    if let Some(beschreibung) = &request.description {
        let roh = beschreibung.as_deref().map(clean);
        if roh
            .as_ref()
            .is_some_and(|text| text.chars().count() > MAX_BESCHREIBUNG_CHARS)
        {
            return Err(format!(
                "Die Beschreibung ist zu lang (höchstens {MAX_BESCHREIBUNG_CHARS} Zeichen)."
            ));
        }

        let neu = roh
            .as_ref()
            .map(|text| escape_text(text))
            .filter(|t| !t.is_empty());
        if ics.get("DESCRIPTION") != neu {
            match neu {
                Some(wert) => ics.set("DESCRIPTION", &wert),
                None => ics.remove("DESCRIPTION"),
            }
            geaendert.push(format!("Beschreibung: {}", text_kurz(roh.as_deref())));
        }
    }

    if request.start.is_some() || request.end.is_some() || request.all_day.is_some() {
        let alter_beginn = liese_zeit(&ics, "DTSTART");
        let altes_ende = liese_zeit(&ics, "DTEND");
        aendere_zeit(&mut ics, &request, alter_beginn, altes_ende, &mut geaendert)?;
    }

    // Erinnerung und Kategorie stehen nicht am Termin selbst, sondern in eigenen
    // Zeilen und in einem eigenen Block. Sie werden deshalb über eigene Zugriffe
    // angesprochen – ein `set` auf `DESCRIPTION` oder `TRIGGER` würde sonst die
    // gleichnamigen Zeilen des Termins treffen.
    let erinnerung_geaendert = aendere_erinnerung(&mut ics, &request, &mut geaendert)?;
    let kategorie_geaendert = aendere_kategorie(&mut ics, &request, &mut geaendert)?;

    if geaendert.is_empty() {
        return Err(
            "Es wurde nichts zum Ändern genannt. Nenne wenigstens Titel, Ort, Beschreibung, \\
             Beginn, Ende, Erinnerung oder Kategorie – was du nicht nennst, bleibt stehen."
                .to_string(),
        );
    }

    // Nach jeder Änderung gehören Zeitstempel und Fortlaufnummer auf den neuen
    // Stand. Ohne das nimmt ein Kalender die Änderung nicht immer an.
    let sequence = ics
        .get("SEQUENCE")
        .and_then(|wert| wert.trim().parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    ics.set("DTSTAMP", &Utc::now().format("%Y%m%dT%H%M%SZ").to_string());
    ics.set("SEQUENCE", &sequence.to_string());

    let titel = ics
        .get("SUMMARY")
        .unwrap_or_else(|| "ohne Titel".to_string());
    let mut zusammen = vec![format!("Termin „{titel}“ wird geändert.")];
    zusammen.extend(geaendert.iter().map(|zeile| format!("  {zeile}")));
    zusammen.push(format!("  Kalender {calendar_display}"));

    // Die Felder, die Mimir nicht ändert, aber auch nicht verliert. Ohne diesen
    // Hinweis könnte jemand mit einer Erinnerung oder Kategorien annehmen, sie
    // fielen weg. Was gerade geändert wurde, steht ohnehin schon in der Liste
    // darüber und darf deshalb nicht noch einmal als „bleibt" erscheinen.
    let bleibt: Vec<&str> = [
        ("VALARM", "die Erinnerung", erinnerung_geaendert),
        ("CATEGORIES", "die Kategorien", kategorie_geaendert),
    ]
    .into_iter()
    .filter(|(feld, _, geaendert)| !geaendert && (ics.has_component(feld) || ics.has(feld)))
    .map(|(_, text, _)| text)
    .collect();

    if !bleibt.is_empty() {
        zusammen.push(format!("  Bleiben erhalten: {}.", bleibt.join(" und ")));
    }

    zusammen.push("  Lässt sich über Mimir nicht zurücknehmen.".to_string());

    Ok(UpdatePlan {
        calendar_display,
        vorher: bestehend.to_string(),
        nachher: ics.to_text(),
        summary: zusammen.join("\n"),
    })
}

/// Setzt die Erinnerung neu, legt sie an oder nimmt sie weg.
///
/// `true`, wenn sich etwas geändert hat. Eine Erinnerung, die es schon gibt und
/// auf denselben Wert gestellt wird, gilt als unverändert – sonst würde Mimir
/// eine wirkungslose Änderung als Erfolg melden.
fn aendere_erinnerung(
    ics: &mut Ics,
    request: &UpdateRequest,
    geaendert: &mut Vec<String>,
) -> Result<bool, String> {
    let Some(wort) = request
        .reminder
        .as_deref()
        .map(write::clean)
        .filter(|wert| !wert.is_empty())
    else {
        return Ok(false);
    };

    match write::parse_erinnerung(&wort)? {
        None => {
            if !ics.entferne_erinnerung() {
                return Ok(false);
            }

            geaendert.push("Erinnerung: keine".to_string());
            Ok(true)
        }
        Some(minuten) => {
            if ics.trigger_minuten() == Some(minuten) {
                return Ok(false);
            }

            // Der Text der Benachrichtigung ist der jetzige Titel – nach einer
            // Titeländerung in diesem Zug wäre der alte sonst hängen geblieben.
            let titel = write::clean(&ics.get("SUMMARY").unwrap_or_default());
            ics.setze_erinnerung(&write::trigger_text(minuten), &write::escape_text(&titel));
            geaendert.push(format!("Erinnerung: {}", write::erinnerung_text(minuten)));
            Ok(true)
        }
    }
}

/// Setzt die Kategorien neu oder nimmt sie weg.
fn aendere_kategorie(
    ics: &mut Ics,
    request: &UpdateRequest,
    geaendert: &mut Vec<String>,
) -> Result<bool, String> {
    let Some(wort) = request.category.as_deref().map(write::clean) else {
        return Ok(false);
    };

    if wort.is_empty() {
        if !ics.has("CATEGORIES") {
            return Ok(false);
        }

        ics.remove("CATEGORIES");
        geaendert.push("Kategorien: keine".to_string());
        return Ok(true);
    }

    if wort.chars().count() > MAX_KATEGORIE_CHARS {
        return Err(format!(
            "Die Kategorie ist zu lang (höchstens {MAX_KATEGORIE_CHARS} Zeichen)."
        ));
    }

    let neu = write::kategorien_text(&wort);

    if ics.get("CATEGORIES").as_deref() == Some(neu.as_str()) {
        return Ok(false);
    }

    ics.set("CATEGORIES", &neu);
    geaendert.push(format!("Kategorien: {wort}"));
    Ok(true)
}

/// Alles, was das Fenster zum Ändern eines Termins braucht.
///
/// Werte sind so aufbereitet, wie sie im Fenster stehen: Zeiten in der Zone des
/// Rechners, Kategorien in einer Zeile, die Erinnerung in den Worten, die der
/// Benutzer auch schreiben würde. Das Frontend soll nichts umrechnen – das hat
/// schon einmal zu einer verschobenen Uhrzeit geführt.
#[derive(Serialize, Clone, Debug)]
pub struct TerminDetails {
    pub uid: String,
    /// Der Anzeigename des Kalenders.
    pub calendar: String,
    /// Derselbe Kalender als Pfad. Das Fenster schickt ihn beim Speichern
    /// zurück, damit das Schreiben nicht über einen Namen läuft – zwei Kalender
    /// können gleich heißen.
    pub calendar_href: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    /// Die Kategorien, mit Komma getrennt, wie sie in der Datei stehen.
    pub categories: String,
    /// `JJJJ-MM-TTThh:mm` bei einem Termin mit Uhrzeit, `JJJJ-MM-DD` bei einem
    /// Ganztagestermin.
    pub start: String,
    pub end: String,
    pub all_day: bool,
    /// Steht im Kalender eine Uhrzeit ohne Zone, ist das gesetzt. Solche Zeiten
    /// werden nicht umgedeutet – das wäre geraten.
    pub floating: bool,
    /// Die Erinnerung in Worten, etwa „15 Minuten vorher“.
    pub reminder: String,
    /// Der Termin, wie er jetzt steht. Steht im Fenster, damit der Benutzer
    /// sieht, was er gerade ändert, und nicht nur ein Formular vor sich hat.
    pub vorher: String,
    /// Warum der Termin sich nicht ändern lässt, falls es daran liegt. Leer
    /// heißt: er lässt sich ändern.
    pub gesperrt: String,
}

/// Liest die Felder für das Fenster aus der Datei, wie sie im Kalender steht.
///
/// Liest, schreibt nichts und rechnet nichts um außer der Zeitzone: Was hier
/// ausgegeben wird, geht bei unverändertem Speichern wieder als derselbe Wert
/// herein.
pub fn termin_details(
    ics_text: &str,
    kalender: &str,
    kalender_href: &str,
) -> Result<TerminDetails, String> {
    let ics = Ics::parse(ics_text)?;
    let all_day = ics.ist_datum("DTSTART");

    let start = if all_day {
        datum_text(&ics, "DTSTART")?
    } else {
        zeit_text(&ics, "DTSTART")?
    };
    let ende = if all_day {
        datum_text(&ics, "DTEND")?
    } else {
        match zeit_text(&ics, "DTEND") {
            Ok(wert) => wert,
            // Ohne DTEND wäre der Termin ein Punkt; das Formular braucht aber
            // ein Ende, sonst wäre die Dauer nicht angebbar.
            Err(_) => start.clone(),
        }
    };

    let reminder = match ics.trigger_minuten() {
        Some(minuten) => write::erinnerung_text(minuten),
        // Es kann eine Erinnerung da sein, deren Wert Mimir nicht liest. Die
        // darf nicht stillschweigend wegfallen, wenn der Benutster speichert.
        None if ics.has_component("VALARM") => {
            "Erinnerung im Kalender, für Mimir nicht lesbar".to_string()
        }
        None => String::new(),
    };

    let mut gesperrt = String::new();
    if let Err(grund) = pruefe_aenderbar(&ics) {
        gesperrt = grund;
    }

    Ok(TerminDetails {
        uid: ics.get("UID").unwrap_or_default(),
        calendar: kalender.to_string(),
        calendar_href: kalender_href.to_string(),
        summary: ics.get("SUMMARY").unwrap_or_default(),
        location: ics.get("LOCATION").unwrap_or_default(),
        description: ics.get("DESCRIPTION").unwrap_or_default(),
        categories: ics.get("CATEGORIES").unwrap_or_default(),
        start,
        end: ende,
        all_day,
        floating: !all_day
            && !ics.zeile("DTSTART").is_some_and(|z| {
                let oben = z.to_ascii_uppercase();
                oben.contains("TZID=") || oben.ends_with('Z')
            }),
        reminder,
        vorher: ics_text.to_string(),
        gesperrt,
    })
}

/// `DTSTART` als `JJJJ-MM-TT` für einen Ganztagestermin.
fn datum_text(ics: &Ics, feld: &str) -> Result<String, String> {
    let wert = ics
        .get(feld)
        .ok_or_else(|| format!("Der Termin hat kein {feld}."))?;
    let text = wert.trim();

    if text.len() < 8 || !text.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{feld} steht nicht als Datum in der Datei: {wert}"));
    }

    Ok(format!("{}-{}-{}", &text[0..4], &text[4..6], &text[6..8]))
}

/// `DTSTART` oder `DTEND` als `JJJJ-MM-TTThh:mm` in der Zone des Rechners.
///
/// Die Werte stehen in der Regel in UTC drin. Sie werden hier einmal zurück
/// gerechnet, weil ein Formular nur eine Uhrzeit anzeigen kann – in der des
/// Rechners, weil der Benutzer die auch so erwartet.
fn zeit_text(ics: &Ics, feld: &str) -> Result<String, String> {
    let Some(zeile) = ics.zeile(feld) else {
        return Err(format!("Der Termin hat kein {feld}."));
    };

    let Some(wert) = zeile.split_once(':').map(|(_, wert)| wert.trim()) else {
        return Err(format!("{feld} ist unlesbar: {zeile}"));
    };

    let text = wert.trim_end_matches('Z');
    if text.len() < 15
        || !text
            .chars()
            .take(15)
            .all(|c| c.is_ascii_digit() || c == 'T')
    {
        return Err(format!(
            "{feld} steht nicht als Zeitpunkt in der Datei: {wert}"
        ));
    }

    let zahl = |von: usize, bis: usize| -> Result<i64, String> {
        text[von..bis]
            .parse()
            .map_err(|_| format!("{feld} ist unlesbar: {wert}"))
    };

    let jahr = zahl(0, 4)?;
    let monat = zahl(4, 6)?;
    let tag = zahl(6, 8)?;
    let stunde = zahl(9, 11)?;
    let minute = zahl(11, 13)?;

    let mut zeit = chrono::NaiveDate::from_ymd_opt(jahr as i32, monat as u32, tag as u32)
        .and_then(|datum| datum.and_hms_opt(stunde as u32, minute as u32, 0))
        .ok_or_else(|| format!("{feld} ist kein gültiger Zeitpunkt: {wert}"))?;

    // Steht die Uhrzeit in UTC in der Datei – weil Mimir selbst so schreibt oder
    // weil die Zone das so sagt –, wird sie in die Zone des Rechners gerechnet.
    // Fehlt die Angabe, ist es eine Wandzeit, und die bleibt stehen.
    if wert.ends_with('Z') || zeile.to_ascii_uppercase().contains("TZID=UTC") {
        zeit = Utc
            .from_utc_datetime(&zeit)
            .with_timezone(&Local)
            .naive_local();
    }

    Ok(zeit.format("%Y-%m-%dT%H:%M").to_string())
}

/// Liest DTSTART oder DTEND als Zeitpunkt.
///
/// Steht ein `Z` am Ende, ist der Wert UTC. Steht keiner, ist es eine Zeit in
/// der Zone des Termins; Mimir schreibt selbst in UTC, und für die Dauerrechnung
/// beim Verschieben ist die Deutung als Ortszeit die passende.
///
/// Für die Dauerrechnung wird die Wandzeit gebraucht, nicht der absolute
/// Zeitpunkt: Deshalb reicht diese Heuristik. Ein Fehler von einer Stunde
/// verschiebt nur den **neuen** Termin um eine Stunde, nicht die Dauer; für den
/// Zeitpunkt selbst ist der Kalender die Wahrheit.
fn liese_zeit(ics: &Ics, feld: &str) -> Option<DateTime<Utc>> {
    let zeile = ics.zeile(feld)?;
    let (wert, ist_utc) = match zeile.split_once(':') {
        Some((links, wert)) => (
            wert.to_string(),
            links.to_ascii_uppercase().contains("TZID=UTC") || wert.ends_with('Z'),
        ),
        None => return None,
    };

    if wert.len() < 15 {
        return None;
    }

    let jahr: i32 = wert[0..4].parse().ok()?;
    let monat: u32 = wert[4..6].parse().ok()?;
    let tag: u32 = wert[6..8].parse().ok()?;
    let stunde: u32 = wert[9..11].parse().ok()?;
    let minute: u32 = wert[11..13].parse().ok()?;

    if ist_utc {
        return Utc
            .with_ymd_and_hms(jahr, monat, tag, stunde, minute, 0)
            .single();
    }

    Local
        .with_ymd_and_hms(jahr, monat, tag, stunde, minute, 0)
        .single()
        .map(|ort| ort.with_timezone(&Utc))
}

/// Setzt Beginn und Ende, mit denselben Grenzen wie beim Anlegen.
fn aendere_zeit(
    ics: &mut Ics,
    request: &UpdateRequest,
    alter_beginn: Option<DateTime<Utc>>,
    altes_ende: Option<DateTime<Utc>>,
    geaendert: &mut Vec<String>,
) -> Result<(), String> {
    let ganztag = request.all_day.unwrap_or_else(|| ics.ist_datum("DTSTART"));

    let beginn = match &request.start {
        Some(angabe) => {
            if ganztag {
                Utc.from_utc_datetime(
                    &datum(angabe)
                        .and_hms_opt(0, 0, 0)
                        .expect("Mitternacht geht immer"),
                )
            } else {
                write::parse_term(angabe).map_err(|_| datums_fehler(angabe))?
            }
        }
        None => alter_beginn.ok_or_else(|| {
            "Der Termin hat keinen lesbaren Beginn; nenne den neuen Beginn.".to_string()
        })?,
    };

    let ende = match &request.end {
        Some(angabe) => {
            if ganztag {
                Utc.from_utc_datetime(
                    &datum(angabe)
                        .and_hms_opt(0, 0, 0)
                        .expect("Mitternacht geht immer"),
                )
            } else {
                write::parse_term(angabe).map_err(|_| datums_fehler(angabe))?
            }
        }
        None => {
            // Ohne eigenes Ende bleibt die bisherige Länge erhalten: „schieb auf
            // morgen“ soll nicht den Nachmittag fressen, und ein ganztägiger
            // Termin nicht auf einen Tag schrumpfen.
            //
            // Die Länge ergibt sich aus den beiden *alten* Werten. Rechnet man
            // das alte Ende vom neuen Beginn ab, kommt über Monate eine negative
            // Zahl heraus und die Ersatzdauer greift.
            let alt = match (alter_beginn, altes_ende) {
                (Some(von), Some(bis)) => bis - von,
                _ => Duration::zero(),
            };

            if ganztag {
                let tage = (alt.num_hours() / 24).max(1);

                beginn + Duration::days(tage)
            } else {
                beginn
                    + if alt > Duration::zero() {
                        alt
                    } else {
                        Duration::hours(1)
                    }
            }
        }
    };

    // Steht dieselbe Zeit im Auftrag wie im Kalender, ist nichts zu prüfen und
    // nichts zu schreiben. Das Fenster schickt den ganzen Termin, und ein
    // unveränderter Termin darf an keiner Grenze scheitern – auch dann nicht,
    // wenn er schon in der Vergangenheit liegt: Sonst ließe sich ein Termin von
    // gestern nicht einmal umbenennen. Die Grenzen unten gelten nur für Werte,
    // die neu benannt wurden.
    let alter_ganztag = ics.ist_datum("DTSTART");
    let neuer_beginn = alter_beginn != Some(beginn);
    let neuer_abschnitt = ganztag != alter_ganztag;
    let unveraendert = !neuer_beginn && !neuer_abschnitt && altes_ende == Some(ende);

    if unveraendert {
        return Ok(());
    }

    if ende <= beginn {
        return Err("Das Ende des Termins liegt nicht nach dem Beginn".to_string());
    }

    // Die Grenze gilt hier auch für Ganztagestermine, anders als beim Anlegen
    // (`write.rs` erlaubt dort ein Jahr). Ein bestehender längerer Urlaub lässt
    // sich deshalb nicht verschieben – solange niemand seine Zeit anfasst.
    if neuer_beginn && (ende - beginn) > Duration::days(MAX_DAUER_TAGE) {
        return Err(format!(
            "Ein Termin darf höchstens {MAX_DAUER_TAGE} Tage dauern. Für einen längeren \
             Zeitraum einen Kalendereintrag anlegen."
        ));
    }

    if neuer_beginn && beginn < Local::now() - Duration::days(MAX_RUECKLIEGEND_TAGE) {
        return Err(format!(
            "Der neue Termin liegt in der Vergangenheit: {} war vor {} Tagen. Frage nach, ob das \
             wirklich so gemeint ist.",
            beginn.with_timezone(&Local).format("%d.%m.%Y"),
            Local::now().signed_duration_since(beginn).num_days()
        ));
    }

    // Zeiten stehen in UTC und ohne Zone im Parametern: Ein behaltener
    // `TZID=Europe/Berlin` bei einem UTC-Wert hieße 07:00 Ortszeit. Ein
    // Ganztagestermin braucht dagegen `VALUE=DATE` – das ist ein Parameter, kein
    // Teil des Wertes.
    if ganztag {
        ics.set_datum("DTSTART", &beginn.format("%Y%m%d").to_string());
        ics.set_datum("DTEND", &ende.format("%Y%m%d").to_string());
    } else {
        ics.set_ohne_parameter("DTSTART", &beginn.format("%Y%m%dT%H%M%SZ").to_string());
        ics.set_ohne_parameter("DTEND", &ende.format("%Y%m%dT%H%M%SZ").to_string());
    }

    geaendert.push(format!(
        "Zeit: {} → {}",
        text_kurz(Some(
            &beginn
                .with_timezone(&Local)
                .format("%d.%m.%Y %H:%M")
                .to_string()
        )),
        text_kurz(Some(
            &ende
                .with_timezone(&Local)
                .format("%d.%m.%Y %H:%M")
                .to_string()
        ))
    ));
    Ok(())
}

/// Ein Datum aus einer Angabe, die auch eine Uhrzeit tragen darf.
///
/// **Achtung:** Diese Funktion rät nicht, sie fällt zurück: Eine unlesbare
/// Angabe wird als **heute** gesetzt, damit ein Ganztagestermin nicht an einem
/// Fehler scheitert. Beim Anlegen gilt das nicht – dort ist `parse_datum` ein
/// Fehler. Wer das hier ändern will, muss `aendere_zeit` auf `Result` umstellen
/// und `datums_fehler` auch für den Ganztagestermin verwenden.
fn datum(angabe: &str) -> chrono::NaiveDate {
    parse_datum(angabe).unwrap_or_else(|_| {
        // „2026-10-05T09:00“ enthält das Datum in den ersten zehn Zeichen.
        let text = clean(angabe);

        if text.len() >= 10 {
            if let Ok(gefunden) = parse_datum(&text[..10]) {
                return gefunden;
            }
        }

        Local::now().date_naive()
    })
}

fn datums_fehler(angabe: &str) -> String {
    format!(
        "„{angabe}“ ist keine brauchbare Zeitangabe. Nenne sie in den Worten des Benutzers, \\
         etwa „morgen 14:00“."
    )
}

/// Baut den Plan für das Löschen.
pub fn plan_delete(
    config: &CalendarConfig,
    kalender: &[(String, String)],
    arguments: &serde_json::Value,
    bestehend: &str,
    uid: &str,
) -> Result<DeletePlan, String> {
    let request: DeleteRequest = serde_json::from_value(arguments.clone())
        .map_err(|fehler| format!("Die Angaben passen nicht zu delete_calendar_event: {fehler}"))?;

    if let Some(genannt) = &request.uid {
        if genannt.trim() != uid.trim() {
            return Err("Die Kennung im Auftrag passt nicht zu dem geladenen Termin.".to_string());
        }
    }

    if request
        .title
        .as_deref()
        .is_some_and(|titel| clean(titel).is_empty())
    {
        return Err(
            "Das Feld title ist leer. Nenne den Titel des Termins, oder lass es weg und nutze uid."
                .to_string(),
        );
    }

    if request
        .on_date
        .as_deref()
        .is_some_and(|tag| clean(tag).is_empty())
    {
        return Err(
            "Das Feld on_date ist leer. Lass es weg, oder nenne den Tag, etwa „morgen“."
                .to_string(),
        );
    }

    let ics = Ics::parse(bestehend)?;
    pruefe_aenderbar(&ics)?;

    let (_, calendar_display) = zielkalender(config, kalender, request.calendar.as_deref(), uid)?;

    let titel = ics
        .get("SUMMARY")
        .unwrap_or_else(|| "ohne Titel".to_string());
    let beginn = ics
        .get("DTSTART")
        .map(|wert| wert.chars().take(15).collect::<String>())
        .unwrap_or_default();

    // Die Zusammenfassung entsteht vor dem Verschieben des Namens in die
    // Struktur: Sonst wäre der Kalender an dieser Stelle schon weg.
    let zusammenfassung = format!(
        "Termin „{titel}“ ({beginn}) wird aus dem Kalender {calendar_display} gelöscht. Das \
         lässt sich über Mimir nicht zurücknehmen."
    );

    Ok(DeletePlan {
        calendar_display,
        vorher: bestehend.to_string(),
        summary: zusammenfassung,
    })
}

/// Rechnet eine Datumsangabe in einen Tag, im Klartext oder als Zeitstempel.
///
/// Nennt der Benutzer „morgen“ und der Titel passt zu mehreren Terminen, ist das
/// der Unterschied zwischen einer eindeutigen Zuordnung und einer Nachfrage.
pub fn tag_aus(angabe: &str) -> Option<chrono::NaiveDate> {
    parse_datum(angabe)
        .ok()
        .or_else(|| write::parse_tag(angabe).ok())
}

/// Wie weit die Auflösung nach einem Treffer sucht.
///
/// Ein Titel reicht selten allein: „test“ kann mehrfach im Kalender stehen, und
/// ohne die Uhrzeit des Benutzers wäre jedes Raten falsch. Deshalb wird
/// eingegrenzt und bei mehreren Treffern nachgefragt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Auswahl {
    pub tag: Option<chrono::NaiveDate>,
    /// Nur die Uhrzeit, wenn der Benutzer keine Uhrzeit genannt hat.
    pub uhrzeit: Option<chrono::NaiveTime>,
}

/// Liest die Angabe des Benutzers zu Zeit und Termin.
///
/// Der Benutzer sagt „gestern um 14 Uhr“ oder „30. September 14:00“. Beides wird
/// getrennt gelesen: Ein Tag allein grenzt bei zwei Terminen am selben Tag nicht
/// weiter, eine Uhrzeit allein schon – aber nur, wenn der Benutzer sie genannt
/// hat. Ohne genannte Uhrzeit wird **nicht** auf die volle Stunde gerundet: Das
/// wäre geraten, und ein Raten trifft den falschen Termin.
pub fn auswahl_aus(angabe: &str) -> Auswahl {
    let text = clean(angabe);
    let tag = tag_aus(&text);

    Auswahl {
        tag,
        uhrzeit: uhrzeit_aus(&text),
    }
}

/// Liest nur dann eine Uhrzeit, wenn wirklich eine dasteht.
///
/// „morgen“ nennt keinen Tag und keine Uhrzeit, „14. September“ nur ein Datum.
/// Beides darf nicht als Uhrzeit gelesen werden: Sonst grenzt ein Datum den
/// Termin auf 00:00 ein und der echte Termin um 14 Uhr fällt heraus.
fn uhrzeit_aus(text: &str) -> Option<chrono::NaiveTime> {
    let klein = text.to_lowercase();

    let nennt_uhrzeit = klein.contains("uhr")
        || klein
            .split(|c: char| !c.is_ascii_digit() && c != ':' && c != '.')
            .any(|wort| {
                // „14:00“ und „14.30“ sind Uhrzeiten. „14“ allein nicht – das
                // ist genauso gut eine Tagesangabe wie im „14. September“.
                let teile: Vec<&str> = wort.split([':', '.']).collect();

                teile.len() == 2
                    && teile
                        .iter()
                        .all(|teil| !teil.is_empty() && teil.chars().all(|c| c.is_ascii_digit()))
                    && teile[1].len() == 2
            });

    if !nennt_uhrzeit {
        return None;
    }

    write::parse_wann(text).ok().map(|zeit| zeit.time())
}

/// Filtert Termine nach Titel, Zeitpunkt und Kalender.
///
/// Der Kalender ist Teil der Auswahl und nicht nur eine Angabe zum Schreiben:
/// Zwei Kalender können einen Termin gleichen Namens zur selben Uhrzeit haben,
/// und der Benutzer hat genau einen davon gemeint.
pub fn passt_zu_termin(
    termin: &crate::calendar::CalendarEvent,
    gesucht: &str,
    auswahl: &Auswahl,
    kalender: Option<&str>,
) -> bool {
    if termin.canceled {
        return false;
    }

    let titel = termin.summary.to_lowercase();
    if titel != gesucht && !titel.contains(gesucht) && !gesucht.contains(&titel) {
        return false;
    }

    if let (Some(wunsch), Some(ist)) = (kalender, Some(termin.calendar.as_str())) {
        if !kalender_passt(wunsch, ist) {
            return false;
        }
    }

    let Some(beginn) = chrono::TimeZone::timestamp_opt(&chrono::Local, termin.start, 0).single()
    else {
        return false;
    };

    if let Some(tag) = auswahl.tag {
        if beginn.date_naive() != tag {
            return false;
        }
    }

    if let Some(uhrzeit) = auswahl.uhrzeit {
        if beginn.time() != uhrzeit {
            return false;
        }
    }

    true
}

/// Vergleicht einen genannten Kalendernamen mit dem eines Termins.
///
/// Der Benutzer sagt „privat“, im Kalender steht „Privat“ oder ein Pfad wie
/// `/remote.php/dav/calendars/kai/personal/`. Ohne diesen Vergleich fände eine
/// Suche im falschen Kalender nichts, obwohl der Benutzer richtig lag.
pub fn kalender_passt(wunsch: &str, ist: &str) -> bool {
    let wunsch = wunsch.trim().to_lowercase();
    let ist = ist.trim().to_lowercase();

    if wunsch.is_empty() || ist.is_empty() {
        return false;
    }

    wunsch == ist
        // Das letzte Pfadglied eines href, etwa „personal“ aus „/calendars/kai/personal/“.
        || ist
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .is_some_and(|rest| rest == wunsch)
        // Teil des Namens: „privat“ findet „Privatkalender“.
        || ist.contains(&wunsch)
        || wunsch.contains(&ist)
}

/// Sucht den Kalender, in dem der Termin liegt.
///
/// Maßgeblich ist die Auswahl des Benutzers; ohne Auswahl gelten alle lesbaren
/// Kalender, wie in der Leiste. Bei mehreren wird nachgefragt – ein Ändern im
/// falschen Kalender wäre stiller Datenverlust.
///
/// Öffentlich, weil der Abruf des Termins dieselbe Antwort braucht. Zwei
/// getrennte Regeln an zwei Stellen wären die wahrscheinlichste Quelle dafür,
/// dass Mimir an einem anderen Kalender landet als geplant.
pub fn zielkalender(
    config: &CalendarConfig,
    kalender: &[(String, String)],
    wunsch: Option<&str>,
    uid: &str,
) -> Result<(String, String), String> {
    let sichtbar: Vec<&(String, String)> = kalender
        .iter()
        .filter(|(href, _)| {
            config.calendars.is_empty() || config.calendars.iter().any(|wahl| wahl == href)
        })
        .collect();

    if sichtbar.is_empty() {
        return Err(
            "Ohne Kalender kann Mimir nichts ändern oder löschen. Mit /calendar anmelden und \\
             einen Kalender wählen."
                .to_string(),
        );
    }

    if let Some(wunsch) = wunsch.map(clean).filter(|wert| !wert.is_empty()) {
        for (href, name) in &sichtbar {
            // Der Benutzer sagt „privat“, im Kalender steht „Privat“, und die
            // Auswahl liegt als Pfad vor. Ohne den unscharfen Vergleich fände
            // die Suche den richtigen Termin und das Schreiben scheiterte an
            // einem Namen, den der Benutzer nie gesehen hat.
            if kalender_passt(&wunsch, name) || kalender_passt(&wunsch, href) {
                return Ok((href.clone(), name.clone()));
            }
        }

        return Err(format!(
            "Den Kalender „{wunsch}“ gibt es nicht. Vorhanden sind: {}.",
            sichtbar
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if sichtbar.len() == 1 {
        return Ok(sichtbar[0].clone());
    }

    Err(format!(
        "Bei mehreren ausgewählten Kalendern muss der Kalender genannt werden. Vorhanden sind: \\
         {}. Der Termin {uid} liegt in einem davon.",
        sichtbar
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Kürzt einen Text für die Zusammenfassung.
///
/// Die Zusammenfassung steht in einem Fenster; eine sehr lange Beschreibung
/// gehört nicht hinein. Die Datei selbst bleibt davon unberührt.
fn text_kurz(wert: Option<&str>) -> String {
    match wert {
        None => "leer".to_string(),
        Some(text) if text.chars().count() > 40 => {
            let gekuerzt: String = text.chars().take(37).collect();
            format!("{gekuerzt}…")
        }
        Some(text) => text.to_string(),
    }
}

#[cfg(test)]
mod tests;
