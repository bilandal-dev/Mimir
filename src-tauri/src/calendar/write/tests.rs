//! Tests für das Planen von Terminen. Geprüft wird, was der Kalender bekommt,
//! und vor allem, was er nicht bekommt: keine Teilnehmer, keine fremden Zeiten.

use super::*;
use crate::calendar::client::CalendarInfo;
use crate::calendar::ics::Ics;
use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
use serde_json::json;

fn config() -> CalendarConfig {
    CalendarConfig {
        server_url: "https://cloud.example.org".to_string(),
        username: "kai".to_string(),
        // So trägt es der Lesepfad ein: der letzte Pfadabschnitt, nicht der
        // ganze Pfad.
        calendars: vec!["persoenlich".to_string()],
        server_certificate: None,
    }
}

/// Ein Datum so viele Tage in der Zukunft, wie ein Modell es liefern würde.
///
/// Feste Daten in Tests sind eine Zeitbombe: „2026-09-14“ war heute noch in der
/// Zukunft und morgen ist es Vergangenheit, woran die Prüfung zur Vergangenheit
/// anschlägt.
fn spaeter(tage: i64) -> String {
    (Local::now() + chrono::Duration::days(tage))
        .format("%Y-%m-%d")
        .to_string()
}

fn kalender(pfad: &str, name: &str) -> (String, String) {
    (pfad.to_string(), name.to_string())
}

/// Ohne Benutzertext wird nichts verworfen: `feld_gedeckt` lässt dann jedes Feld
/// stehen. Deshalb genügt hier ein leerer Text – die Prüfung selbst hat eigene
/// Tests in `modellausgaben`.
fn termin(arguments: serde_json::Value) -> Result<EventPlan, String> {
    termin_mit(arguments, "")
}

fn termin_mit(arguments: serde_json::Value, benutzertext: &str) -> Result<EventPlan, String> {
    let verfuegbar = vec![kalender("persoenlich", "Persönlich")];
    plan_event(&config(), &verfuegbar, &arguments, benutzertext)
}

/// Entfaltet die Zeilen wieder, damit Tests den Inhalt prüfen können.
fn entfalten(ics: &str) -> String {
    ics.replace("\r\n ", "")
}

/// Dasselbe, aber als Liste: Für Vergleiche Zeile für Zeile.
fn zeilen(ics: &str) -> Vec<String> {
    crate::calendar::ics::entfalten(ics)
}

#[test]
fn a_termin_becomes_an_ics_with_the_given_times() {
    let plan = termin(json!({"summary": "Zahnarzt", "start": format!("{}T09:00", spaeter(7)).as_str(), "end": format!("{}T10:15", spaeter(7)).as_str()})).unwrap();
    let ics = entfalten(&plan.ics);

    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"), "{}", ics);
    assert!(ics.contains("\r\nBEGIN:VEVENT\r\n"), "{}", ics);
    assert!(ics.ends_with("END:VEVENT\r\nEND:VCALENDAR"), "{}", ics);
    assert!(ics.contains("SUMMARY:Zahnarzt"), "{}", ics);
    assert!(ics.contains("END:VCALENDAR"), "{}", ics);
    assert!(
        !ics.contains("BEGIN:VTIMEZONE"),
        "Kein Zeitzonenblock: {}",
        ics
    );

    // Die Vorschau muss genau den Inhalt zeigen, der gespeichert wird.
    assert!(plan.summary.contains("Zahnarzt"), "{}", plan.summary);
    assert!(plan.summary.contains("Persönlich"), "{}", plan.summary);
    assert_eq!(plan.file_name, format!("{}.ics", plan.uid));
    assert!(plan.uid.ends_with("@mimir"), "{}", plan.uid);
}

#[test]
fn the_file_name_carries_the_uid_and_the_text_stays_readable() {
    let plan = termin(
        json!({"summary": "Teammeeting", "start": format!("{}T09:00", spaeter(7)).as_str()}),
    )
    .unwrap();

    assert!(plan.file_name.ends_with(".ics"));
    assert!(!plan.file_name.contains('/'));
    assert!(!plan.file_name.contains(" "));
    // Zwei Termine dürfen nicht dieselbe Kennung bekommen.
    let zweite = termin(
        json!({"summary": "Teammeeting", "start": format!("{}T09:00", spaeter(7)).as_str()}),
    )
    .unwrap();
    assert_ne!(plan.uid, zweite.uid);
}

#[test]
fn without_an_end_the_termin_lasts_an_hour() {
    let mit_ende = termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str(), "end": format!("{}T10:00", spaeter(7)).as_str()})).unwrap();
    let ohne_ende =
        termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()})).unwrap();
    let ics = entfalten(&ohne_ende.ics);

    // Die Kennungen unterscheiden sich, also werden die Zeiten verglichen.
    assert_eq!(
        zeiten(&mit_ende.ics),
        zeiten(&ohne_ende.ics),
        "Ohne Ende gilt eine Stunde"
    );
    assert!(
        ics.contains(&format!("DTSTART:{}", utc_stunde(9))),
        "{}",
        ics
    );
    assert!(
        ics.contains(&format!("DTEND:{}", utc_stunde(10))),
        "{}",
        ics
    );
}

#[test]
fn a_whole_day_termin_uses_dates_and_does_not_skip_a_day() {
    let start = Local::now().date_naive() + chrono::Duration::days(7);
    let ende = start + chrono::Duration::days(2);
    let plan = termin(json!({
        "summary": "Urlaub",
        "start": start.format("%Y-%m-%d").to_string(),
        "end": ende.format("%Y-%m-%d").to_string(),
        "all_day": true
    }))
    .unwrap();
    let ics = entfalten(&plan.ics);

    assert!(
        ics.contains(&format!("DTSTART;VALUE=DATE:{}", start.format("%Y%m%d"))),
        "{}",
        ics
    );
    assert!(
        ics.contains(&format!("DTEND;VALUE=DATE:{}", ende.format("%Y%m%d"))),
        "{}",
        ics
    );
    assert!(
        !ics.contains("DTSTART:"),
        "Keine Uhrzeit am Ganztag: {}",
        ics
    );
    assert!(
        plan.when.contains(&start.format("%d.%m.%Y").to_string()),
        "{}",
        plan.when
    );
    assert!(plan.summary.contains("Ganztagestermin"), "{}", plan.summary);
}

#[test]
fn a_whole_day_block_names_the_span_and_not_only_the_first_day() {
    let start = Local::now().date_naive() + chrono::Duration::days(10);
    let ende = start + chrono::Duration::days(4);
    let plan = termin(json!({
        "summary": "Urlaub",
        "start": start.format("%Y-%m-%d").to_string(),
        "end": ende.format("%Y-%m-%d").to_string(),
        "all_day": true
    }))
    .unwrap();

    assert!(
        plan.when.contains(&start.format("%d.%m.%Y").to_string()),
        "{}",
        plan.when
    );
    // Der letzte Tag gehört noch zum Urlaub, das Ende im Kalender ist der Tag danach.
    assert!(
        plan.when.contains(
            &(ende - chrono::Duration::days(1))
                .format("%d.%m.%Y")
                .to_string()
        ),
        "{}",
        plan.when
    );
    assert!(
        entfalten(&plan.ics).contains(&format!("DTEND;VALUE=DATE:{}", ende.format("%Y%m%d"))),
        "{}",
        plan.ics
    );
    assert!(
        plan.summary.contains(&start.format("%d.%m.%Y").to_string()),
        "{}",
        plan.summary
    );
}

#[test]
fn a_single_day_termin_names_only_that_day() {
    let tag = Local::now().date_naive() + chrono::Duration::days(7);
    let plan = termin(json!({"summary": "Feiertag", "start": tag.format("%Y-%m-%d").to_string(), "all_day": true}))
        .unwrap();

    assert_eq!(plan.when, format!("{} ganztägig", tag.format("%d.%m.%Y")));
}

#[test]
fn a_whole_day_termin_ends_the_day_after() {
    let tag = Local::now().date_naive() + chrono::Duration::days(7);
    let plan = termin(json!({"summary": "Feiertag", "start": tag.format("%Y-%m-%d").to_string(), "all_day": true}))
        .unwrap();
    let ics = entfalten(&plan.ics);

    assert!(
        ics.contains(&format!(
            "DTEND;VALUE=DATE:{}",
            (tag + chrono::Duration::days(1)).format("%Y%m%d")
        )),
        "{}",
        ics
    );
}

#[test]
fn no_attendees_are_written_because_mimir_invites_nobody() {
    let plan =
        termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()})).unwrap();
    let ics = &plan.ics;

    assert!(!ics.to_uppercase().contains("ATTENDEE"), "{}", ics);
    assert!(!ics.to_uppercase().contains("ORGANIZER"), "{}", ics);
    assert!(!ics.to_uppercase().contains("SEQUENCE"), "{}", ics);
}

#[test]
fn attendees_in_the_request_are_refused_with_a_reason() {
    let fehler = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "attendees": ["team@example.org"]
    }))
    .unwrap_err();

    assert!(fehler.contains("attendees"), "{}", fehler);
    assert!(fehler.contains("Einladungen"), "{}", fehler);
}

#[test]
fn unknown_fields_are_refused_so_nothing_is_silently_dropped() {
    let fehler = termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str(), "recur": "FREQ=DAILY"}))
        .unwrap_err();

    assert!(fehler.contains("recur"), "{}", fehler);
}

#[test]
fn attachments_in_the_request_are_refused_with_a_reason() {
    let fehler = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "attachments": ["datei.pdf"]
    }))
    .unwrap_err();

    assert!(fehler.contains("Anlagen"), "{}", fehler);
}

// --- Erinnerung --------------------------------------------------------------

#[test]
fn a_reminder_is_written_as_a_valarm_block() {
    let plan = termin(json!({
        "summary": "Zahnarzt",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "reminder": "15 Minuten vorher"
    }))
    .unwrap();
    let zeilen = zeilen(&plan.ics);

    assert!(zeilen.contains(&"BEGIN:VALARM".to_string()), "{}", plan.ics);
    assert!(
        zeilen.contains(&"ACTION:DISPLAY".to_string()),
        "{}",
        plan.ics
    );
    assert!(
        zeilen.contains(&"TRIGGER:-PT15M".to_string()),
        "{}",
        plan.ics
    );
    assert!(zeilen.contains(&"END:VALARM".to_string()), "{}", plan.ics);
    // Die Benachrichtigung soll den Termin nennen, nicht leer sein.
    assert!(
        zeilen.iter().any(|z| z == "DESCRIPTION:Zahnarzt"),
        "{}",
        plan.ics
    );
    // Der Block gehört in den Termin und nicht daneben.
    let beginn = zeilen.iter().position(|z| z == "BEGIN:VEVENT").unwrap();
    let alarm = zeilen.iter().position(|z| z == "BEGIN:VALARM").unwrap();
    let ende = zeilen.iter().position(|z| z == "END:VEVENT").unwrap();
    assert!(beginn < alarm && alarm < ende, "{}", plan.ics);
}

#[test]
fn no_reminder_is_written_when_none_is_asked_for() {
    let plan =
        termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()})).unwrap();

    assert!(!plan.ics.contains("VALARM"), "{}", plan.ics);
    assert!(!plan.ics.contains("TRIGGER"), "{}", plan.ics);
}

#[test]
fn the_words_of_the_user_become_the_offset() {
    for (worte, minuten) in [
        ("5 Minuten vorher", 5),
        ("5 min", 5),
        ("5min", 5),
        ("eine Stunde vorher", 60),
        ("eine halbe Stunde vorher", 30),
        ("eine viertelstunde vorher", 15),
        ("eine dreiviertelstunde vorher", 45),
        ("eine halbe Stunde", 30),
        ("2 Stunden vorher", 120),
        ("am Vorabend", 1440),
        ("1 Tag vorher", 1440),
        ("eine Woche vorher", 10080),
        ("fünfzehn Minuten vor", 15),
        ("90 Minuten vorher", 90),
    ] {
        assert_eq!(
            parse_erinnerung(worte).unwrap(),
            Some(minuten),
            "„{worte}“ wurde falsch gelesen"
        );
    }
}

#[test]
fn a_reminder_is_switched_off_by_saying_so() {
    for worte in [
        "keine Erinnerung",
        "ohne Erinnerung",
        "keine",
        "nicht mehr",
        "",
    ] {
        assert_eq!(
            parse_erinnerung(worte).unwrap(),
            None,
            "„{worte}“ hätte die Erinnerung nicht abschalten dürfen"
        );
    }
}

#[test]
fn a_reminder_that_cannot_be_read_is_a_question_not_a_guess() {
    // Genau das ist der Fehler aus dem Betrieb: Eine Erinnerung wurde behauptet,
    // aber nicht geschrieben. Was nicht gelesen werden kann, muss scheitern.
    for worte in [
        "bald",
        "5",
        "5 Irgendwas",
        "5 Minuten nach dem Termin",
        "0 Minuten vorher",
        "eine halbe Stunde nachher",
        "200 Tage vorher",
    ] {
        assert!(
            parse_erinnerung(worte).is_err(),
            "„{worte}“ hätte abgelehnt werden müssen"
        );
    }
}

#[test]
fn the_offset_is_written_as_a_duration_and_read_back() {
    assert_eq!(trigger_text(5), "-PT5M");
    assert_eq!(trigger_text(60), "-PT1H");
    assert_eq!(trigger_text(90), "-PT1H30M");
    assert_eq!(trigger_text(1440), "-P1D");
    assert_eq!(trigger_text(10080), "-P1W");
    assert_eq!(trigger_text(0), "-PT0M");

    // Der Wert, der in der Datei landet, ist kein Benutzereingang: Wer ihn
    // zurückgibt, bekommt eine Frage und nicht eine Erinnerung.
    for minuten in [5, 15, 60, 90, 1440, 10080] {
        assert!(
            parse_erinnerung(&trigger_text(minuten)).is_err(),
            "der TRIGGER-Wert {minuten} darf nicht als Angabe gelesen werden"
        );
    }
}

#[test]
fn the_trigger_value_comes_back_as_minutes() {
    let datei = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:A\r\nDTSTART:20261001T090000Z\r\n\
         BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR";
    let ics = Ics::parse(datei).unwrap();

    assert_eq!(ics.trigger_minuten(), Some(15));
}

// --- Kategorien -------------------------------------------------------------

#[test]
fn a_category_is_written_and_several_are_separated() {
    let plan = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "category": "Arbeit, privat"
    }))
    .unwrap();
    let zeilen = zeilen(&plan.ics);

    assert!(
        zeilen.iter().any(|z| z == "CATEGORIES:Arbeit;privat"),
        "{}",
        plan.ics
    );
    assert!(
        plan.summary.contains("Kategorien Arbeit, privat"),
        "{}",
        plan.summary
    );
}

#[test]
fn a_semicolon_inside_a_category_name_does_not_split_it() {
    // Ohne Maskierung würde aus „Müller; Meier" die Kategorie „Müller“ und ein
    // zweites Feld – der Kalender zeigte dann etwas anderes als beabsichtigt.
    let plan = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "category": "Müller; Meier"
    }))
    .unwrap();
    let zeilen = zeilen(&plan.ics);

    assert!(
        zeilen.iter().any(|z| z == r"CATEGORIES:Müller\; Meier"),
        "{:?}",
        zeilen
    );
}

#[test]
fn a_category_that_is_too_long_is_refused() {
    let fehler = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "category": "x".repeat(MAX_KATEGORIE_CHARS + 1)
    }))
    .unwrap_err();

    assert!(fehler.contains("Kategorie"), "{}", fehler);
}

#[test]
fn a_termin_needs_a_title_and_a_start() {
    assert!(
        termin(json!({"start": format!("{}T09:00", spaeter(7)).as_str()}))
            .unwrap_err()
            .contains("summary")
    );
    assert!(
        termin(json!({"summary": "  ", "start": format!("{}T09:00", spaeter(7)).as_str()}))
            .unwrap_err()
            .contains("Überschrift")
    );
    assert!(termin(json!({"summary": "A"}))
        .unwrap_err()
        .contains("start"));
    // Ein Datum allein reicht für einen Termin mit Uhrzeit nicht.
    assert!(
        termin(json!({"summary": "A", "start": spaeter(7).as_str()}))
            .unwrap_err()
            .contains("Uhrzeit")
    );
    assert!(termin(
        json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str(), "all_day": "ja"})
    )
    .unwrap_err()
    .contains("all_day"));
}

#[test]
fn the_end_has_to_be_after_the_start() {
    let fehler = termin(json!({
        "summary": "A",
        "start": format!("{}T10:00", spaeter(7)).as_str(),
        "end": format!("{}T09:00", spaeter(7)).as_str()
    }))
    .unwrap_err();

    assert!(fehler.contains("Ende"), "{}", fehler);
}

#[test]
fn far_away_and_absurd_times_are_refused() {
    assert!(termin(json!({"summary": "A", "start": "1700-01-01T09:00"}))
        .unwrap_err()
        .contains("1700"));
    assert!(termin(json!({"summary": "A", "start": "2999-01-01T09:00"}))
        .unwrap_err()
        .contains("2999"));
    // Auch Ganztagestermine werden geprüft.
    assert!(
        termin(json!({"summary": "A", "start": "1700-01-01", "all_day": true}))
            .unwrap_err()
            .contains("1700")
    );
    // Mehr als ein Jahr: fast immer ein Irrtum beim Rechnen.
    assert!(termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)), "end": format!("{}T09:00", spaeter(21))}))
        .unwrap_err()
        .contains("7 Tage"));
    // Ein Termin mit Uhrzeit ist eine Besprechung; Urlaub darf länger dauern.
    let ende = Local::now().date_naive() + chrono::Duration::days(44);
    let urlaub = termin(json!({
        "summary": "Urlaub",
        "start": spaeter(30).as_str(),
        "end": ende.format("%Y-%m-%d").to_string(),
        "all_day": true
    }))
    .unwrap();
    assert!(
        entfalten(&urlaub.ics).contains(&format!("DTEND;VALUE=DATE:{}", ende.format("%Y%m%d"))),
        "{}",
        urlaub.ics
    );
}

#[test]
fn seconds_and_an_explicit_offset_are_accepted() {
    let tag = Local::now().date_naive() + chrono::Duration::days(7);
    let mit_zeitzone = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00:00+02:00", tag.format("%Y-%m-%d")),
        "end": format!("{}T10:00:00+02:00", tag.format("%Y-%m-%d"))
    }))
    .unwrap();
    let ics = entfalten(&mit_zeitzone.ics);

    // 09:00 in einer Zone zwei Stunden vor UTC sind 07:00 UTC, ganz ohne
    // Rücksicht auf die Sommerzeit des Rechners.
    assert!(
        ics.contains(&format!("DTSTART:{}T070000Z", tag.format("%Y%m%d"))),
        "{}",
        ics
    );
    assert!(
        ics.contains(&format!("DTEND:{}T080000Z", tag.format("%Y%m%d"))),
        "{}",
        ics
    );
}

#[test]
fn summer_and_winter_keep_the_time_the_user_named() {
    // Zwei weit auseinanderliegende Termine, beide in der Zukunft: Die Stunde an
    // der Wand muss in beiden 12 Uhr bleiben.
    let sommer =
        termin(json!({"summary": "A", "start": format!("{}T12:00", spaeter(60))})).unwrap();
    let winter =
        termin(json!({"summary": "A", "start": format!("{}T12:00", spaeter(200))})).unwrap();

    for plan in [&sommer, &winter] {
        let start = Local.timestamp_opt(plan_start(plan), 0).unwrap();

        // 12 Uhr bleibt 12 Uhr, egal ob Sommer- oder Winterzeit gilt.
        assert_eq!(start.hour(), 12, "{}", plan.when);
        assert_eq!(start.minute(), 0, "{}", plan.when);
    }
}

#[test]
fn long_text_is_folded_so_nextcloud_can_read_it() {
    let ort = "L".repeat(MAX_ORTS_CHARS);
    let beschreibung = format!("{} ä", "B".repeat(MAX_BESCHREIBUNG_CHARS - 2));
    let plan = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "location": ort,
        "description": beschreibung
    }))
    .unwrap();

    for zeile in plan.ics.split("\r\n") {
        assert!(zeile.len() <= ICS_LINE_LIMIT, "Zeile zu breit: {zeile}");
    }

    let ics = entfalten(&plan.ics);
    assert!(ics.contains(&format!("LOCATION:{}", "L".repeat(MAX_ORTS_CHARS))));
    assert!(
        ics.contains(&format!(
            "DESCRIPTION:{} ä",
            "B".repeat(MAX_BESCHREIBUNG_CHARS - 2)
        )),
        "Die Beschreibung muss vollständig ankommen: {}",
        ics.len()
    );
}

#[test]
fn folding_does_not_split_a_character() {
    let zeile = format!("SUMMARY:{}", "ä".repeat(120));
    let ics = fold(&zeile);
    let zusammen = ics.replace("\r\n ", "");

    assert_eq!(zusammen, zeile, "Der Text muss unverändert ankommen");
    assert!(ics.contains("\r\n "), "Es muss gefaltet worden sein");
}

#[test]
fn short_lines_stay_separated() {
    let ics = fold("SUMMARY:A\r\nDESCRIPTION:B");

    assert_eq!(ics, "SUMMARY:A\r\nDESCRIPTION:B");
}

#[test]
fn the_text_is_escaped_so_no_line_can_be_smuggled_in() {
    let plan = termin(json!({
        "summary": "Feier\r\nEND:VEVENT\r\nSUMMARY:Falsch",
        "start": format!("{}T09:00", spaeter(7)).as_str()
    }))
    .unwrap();
    let ics = &plan.ics;

    // Genau eine Zeile je Feld: der Umbruch im Text ist ein Textzeichen.
    assert_eq!(ics.matches("\r\nBEGIN:VEVENT").count(), 1, "{}", ics);
    assert_eq!(ics.matches("\r\nEND:VEVENT\r\n").count(), 1, "{}", ics);
    assert!(
        ics.contains("SUMMARY:Feier\\nEND:VEVENT\\nSUMMARY:Falsch\r\n"),
        "{}",
        ics
    );
    // Und der echte Umbruch steckt nur noch im Dateiformat, nicht im Text.
    assert_eq!(
        ics.matches("\r\n").count(),
        ics.lines().count() - 1,
        "{}",
        ics
    );
}

#[test]
fn separators_in_the_text_are_escaped() {
    let plan = termin(json!({
        "summary": "Koch,\\Nudeln; Termin",
        "start": format!("{}T09:00", spaeter(7)).as_str()
    }))
    .unwrap();
    let ics = entfalten(&plan.ics);

    assert!(
        ics.contains("SUMMARY:Koch\\,\\\\Nudeln\\; Termin\r\n"),
        "{}",
        ics
    );
}

#[test]
fn the_first_selected_calendar_is_used_when_nothing_is_named() {
    let verfuegbar = vec![kalender("privat", "Privat"), kalender("arbeit", "Arbeit")];
    let mut config = config();
    config.calendars = vec!["arbeit".to_string()];

    let plan = plan_event(
        &config,
        &verfuegbar,
        &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()}),
        "",
    )
    .unwrap();

    assert_eq!(plan.calendar_href, "arbeit");
    assert_eq!(plan.calendar_display, "Arbeit");
}

#[test]
fn a_named_calendar_wins_over_the_order() {
    let verfuegbar = vec![kalender("privat", "Privat"), kalender("arbeit", "Arbeit")];
    let mut config = config();
    config.calendars = vec!["privat".to_string(), "arbeit".to_string()];

    let plan = plan_event(&config, &verfuegbar, &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str(), "calendar": "Arbeit"}), "")
    .unwrap();

    assert_eq!(plan.calendar_href, "arbeit");
}

#[test]
fn a_calendar_can_also_be_named_by_its_path() {
    let verfuegbar = vec![kalender("persoenlich", "Persönlich")];

    let plan = plan_event(
        &config(),
        &verfuegbar,
        &json!({
            "summary": "A",
            "start": format!("{}T09:00", spaeter(7)).as_str(),
            "calendar": "persoenlich"
        }),
        "",
    )
    .unwrap();

    assert_eq!(plan.calendar_href, "persoenlich");
    assert_eq!(plan.calendar_display, "Persönlich");
}

#[test]
fn an_unknown_calendar_lists_the_ones_that_exist() {
    let verfuegbar = vec![kalender("privat", "Privat"), kalender("arbeit", "Arbeit")];
    let mut config = config();
    config.calendars = vec!["privat".to_string(), "arbeit".to_string()];

    let fehler = plan_event(&config, &verfuegbar, &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str(), "calendar": "Kanzlei"}), "")
    .unwrap_err();

    assert!(fehler.contains("Privat"), "{}", fehler);
    assert!(fehler.contains("Arbeit"), "{}", fehler);
}

#[test]
fn a_calendar_name_is_needed_when_several_are_selected() {
    let verfuegbar = vec![kalender("privat", "Privat"), kalender("arbeit", "Arbeit")];
    let mut config = config();
    config.calendars = vec!["privat".to_string(), "arbeit".to_string()];

    let fehler = plan_event(
        &config,
        &verfuegbar,
        &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()}),
        "",
    )
    .unwrap_err();

    // Der Text ist eine Frage an den Benutzer, keine Anweisung an das Modell:
    // Er kommt als Werkzeugergebnis zurück und soll weitergegeben werden.
    assert!(fehler.contains("mehrere Kalender"), "{}", fehler);
    assert!(fehler.contains("In welchen Kalender"), "{}", fehler);
    assert!(!fehler.contains("Nenne einen davon"), "{}", fehler);
}

#[test]
fn one_calendar_needs_no_name() {
    let verfuegbar = vec![kalender("privat", "Privat")];
    let mut config = config();
    config.calendars = vec!["privat".to_string()];

    let plan = plan_event(
        &config,
        &verfuegbar,
        &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()}),
        "",
    )
    .unwrap();

    assert_eq!(plan.calendar_href, "privat");
}

#[test]
fn without_a_login_there_is_nothing_to_write_to() {
    let mut config = config();
    config.calendars.clear();

    let fehler = plan_event(
        &config,
        &[],
        &serde_json::json!({ "summary": "A", "start": "morgen 14:00" }),
        "",
    )
    .unwrap_err();

    assert!(fehler.contains("angemeldet"), "{}", fehler);
}

#[test]
fn an_empty_selection_means_every_calendar_the_server_knows() {
    // Genau das war der Fehler im Betrieb: In der Leiste bedeutet eine leere
    // Auswahl „alle Kalender". Beim Anlegen wurde sie als „kein Kalender"
    // gelesen, und der Benutzer bekam die Meldung, er sei nicht angemeldet.
    let verfuegbar = vec![kalender("persoenlich", "Persönlich")];
    let mut config = config();
    config.calendars.clear();

    let plan = plan_event(
        &config,
        &verfuegbar,
        &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()}),
        "",
    )
    .unwrap();

    assert_eq!(plan.calendar_href, "persoenlich");
    assert_eq!(plan.calendar_display, "Persönlich");
}

#[test]
fn an_empty_selection_with_several_calendars_asks_for_the_name() {
    let verfuegbar = vec![
        kalender("persoenlich", "Persönlich"),
        kalender("arbeit", "Arbeit"),
    ];
    let mut config = config();
    config.calendars.clear();

    let fehler = plan_event(
        &config,
        &verfuegbar,
        &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()}),
        "",
    )
    .unwrap_err();

    assert!(fehler.contains("mehrere Kalender"), "{}", fehler);
    assert!(fehler.contains("In welchen Kalender"), "{}", fehler);
    assert!(fehler.contains("Persönlich"), "{}", fehler);
    assert!(fehler.contains("Arbeit"), "{}", fehler);
}

#[test]
fn a_calendar_can_be_named_by_its_path() {
    // Modelle greifen gern auf den Pfad aus der Kalenderliste zurück.
    let verfuegbar = vec![kalender("persoenlich", "Persönlich")];
    let mut config = config();
    config.calendars.clear();

    let plan = plan_event(&config, &verfuegbar, &json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str(), "calendar": "persoenlich"}), "")
    .unwrap();

    assert_eq!(plan.calendar_href, "persoenlich");
}

#[test]
fn the_text_is_trimmed_and_a_path_in_the_text_cannot_escape() {
    let plan = termin(json!({
        "summary": "  Spaziergang  ",
        "start": format!("{}T09:00", spaeter(7)).as_str(),
        "location": "  Park  "
    }))
    .unwrap();
    let ics = entfalten(&plan.ics);

    assert!(ics.contains("SUMMARY:Spaziergang\r\n"), "{}", ics);
    assert!(ics.contains("LOCATION:Park\r\n"), "{}", ics);
    assert!(
        plan.summary.starts_with("Neuer Termin: Spaziergang"),
        "{}",
        plan.summary
    );
}

#[test]
fn a_calendar_name_cannot_break_out_of_the_collection() {
    // Der Name landet als Pfadabschnitt in der Adresse. Ein Schrägstrich oder
    // ein Sprung nach oben dürfte nicht aus dem Sammelpfad hinausführen.
    assert!(escape_calendar_segment("../andere").is_err());
    assert!(escape_calendar_segment("privat/../..").is_err());
    assert!(escape_calendar_segment("").is_err());
    assert!(escape_calendar_segment("   ").is_err());
}

#[test]
fn a_calendar_name_with_a_space_stays_reachable() {
    assert_eq!(
        escape_calendar_segment("Meine Termine").unwrap(),
        "Meine%20Termine"
    );
    assert_eq!(escape_calendar_segment("100%").unwrap(), "100%25");
}

#[test]
fn the_address_of_a_termin_stays_inside_the_dav_area() {
    let config = config();
    let plan =
        termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()})).unwrap();

    let url = crate::calendar::client::event_url_for_test(&config, &plan);

    assert!(
        url.starts_with("https://cloud.example.org/remote.php/dav/calendars/kai/persoenlich/"),
        "{}",
        url
    );
    assert!(url.ends_with(&format!("/{}.ics", plan.uid)), "{}", url);
    assert!(!url.contains(".."), "{}", url);
    assert!(!url.contains(".org//"), "Doppelter Schrägstrich: {}", url);
}

#[test]
fn the_tool_name_is_stable_because_the_model_uses_it() {
    assert_eq!(EVENT_TOOL, "create_calendar_event");
}

#[test]
fn the_plan_carries_only_what_the_calendar_needs() {
    let plan =
        termin(json!({"summary": "A", "start": format!("{}T09:00", spaeter(7)).as_str()})).unwrap();
    let ics = &plan.ics;

    // Kein Alarm, keine Kategorie, kein Anhang: dafür gibt es in Mimir keine
    // Oberfläche, und stillschweigend gesetzte Felder wären später verwirrend.
    for verboten in [
        "ALARM",
        "CATEGORIES",
        "ATTACH",
        "CLASS",
        "PRIORITY",
        "SEQUENCE",
    ] {
        assert!(
            !ics.to_uppercase().contains(verboten),
            "{verboten} in {ics}"
        );
    }
}

/// Die UTC-Schreibweise einer Ortszeit am Tag von `spaeter(7)`.
fn utc_stunde(stunde: u32) -> String {
    let tag = Local::now().date_naive() + chrono::Duration::days(7);

    Local
        .with_ymd_and_hms(tag.year(), tag.month(), tag.day(), stunde, 0, 0)
        .unwrap()
        .with_timezone(&Utc)
        .format("%Y%m%dT%H%M%SZ")
        .to_string()
}

/// Die Zeitzeilen einer Datei, ohne Kennung und Zeitstempel.
fn zeiten(ics: &str) -> Vec<String> {
    entfalten(ics)
        .lines()
        .filter(|zeile| zeile.starts_with("DTSTART") || zeile.starts_with("DTEND"))
        .map(|zeile| zeile.to_string())
        .collect()
}

#[test]
fn the_session_knows_the_calendars_it_has_seen() {
    let session = crate::calendar::CalendarSession::default();
    session.set_calendars(
        vec![CalendarInfo {
            href: "privat".to_string(),
            display_name: "Privat".to_string(),
            ctag: "ctag-1".to_string(),
            color: "#3366ff".to_string(),
        }],
        Some("35.0.0.10".to_string()),
    );

    let kalender = session.calendars();

    assert_eq!(kalender.len(), 1);
    assert_eq!(kalender[0].display_name, "Privat");
    assert_eq!(kalender[0].href, "privat");
}

#[test]
fn after_a_write_the_panel_fetches_again() {
    // Ohne das bliebe der neue Termin bis zum nächsten Abruf unsichtbar, und
    // der Benutzer hielte das Anlegen für fehlgeschlagen.
    let session = crate::calendar::CalendarSession::default();
    session.mark_success(1_700_000_000);

    session.mark_calendar_write();

    assert_eq!(
        session.last_success(),
        None,
        "Der Stand muss als veraltet gelten, sonst wartet die Leiste"
    );
}

// --------------------------------------------------------- Angaben in Worten
// Das ist die Reaktion auf einen Fehlgriff aus dem Betrieb: Auf „heute 14 Uhr“
// legte das Modell einen Termin im Jahr 2023 an, weil es die Uhrzeit nicht
// kennt und ein Datum rät. Seitdem löst Mimir die Worte selbst auf.

/// Wie viele Tage liegt `was` von heute entfernt?
fn tage_bis(plan: &EventPlan) -> i64 {
    // In der Ortszeit vergleichen: Ein Termin um 00:30 UTC ist hier schon der
    // nächste Tag.
    let start = Local.timestamp_opt(plan_start(plan), 0).unwrap();

    start
        .date_naive()
        .signed_duration_since(Local::now().date_naive())
        .num_days()
}

fn plan_start(plan: &EventPlan) -> i64 {
    plan_start_fuer(&plan.ics)
}

/// Liest DTSTART aus einer entfalteten ICS-Datei.
fn plan_start_fuer(ics: &str) -> i64 {
    entfalten(ics)
        .lines()
        .find_map(|zeile| {
            let wert = zeile.trim_start_matches("DTSTART:").trim_end_matches('Z');
            let jahr: i32 = wert[0..4].parse().ok()?;
            let monat: u32 = wert[4..6].parse().ok()?;
            let tag: u32 = wert[6..8].parse().ok()?;
            let stunde: u32 = wert[9..11].parse().ok()?;
            let minute: u32 = wert[11..13].parse().ok()?;
            // In der Datei steht UTC; die Felder gehören auch so gelesen.
            Utc.with_ymd_and_hms(jahr, monat, tag, stunde, minute, 0)
                .single()
                .map(|zeit| zeit.timestamp())
        })
        .expect("DTSTART lesbar")
}

#[test]
fn heute_und_morgen_worten_werden_vom_werkzeug_aufgeloest() {
    let heute = termin(json!({"summary": "A", "start": "heute 14:00"})).unwrap();
    let morgen = termin(json!({"summary": "A", "start": "morgen 9:00"})).unwrap();
    let uebermorgen = termin(json!({"summary": "A", "start": "übermorgen"}));

    assert_eq!(tage_bis(&heute), 0, "{}", heute.when);
    assert_eq!(tage_bis(&morgen), 1, "{}", morgen.when);
    assert!(
        entfalten(&morgen.ics).contains("DTSTART:"),
        "{}",
        morgen.ics
    );
    // Ohne Uhrzeit gibt es keinen Ganztagsersatz: Das Modell soll nachfragen.
    assert!(uebermorgen.is_err(), "{:?}", uebermorgen.err());
}

#[test]
fn die_uhrzeit_bleibt_so_stehen_wie_gesagt() {
    for (angabe, stunde, minute) in [
        ("heute 14:00", 14u32, 0u32),
        ("heute um 14 Uhr", 14, 0),
        ("morgen 9:30", 9, 30),
        ("morgen 07.15", 7, 15),
        ("morgen um 8 Uhr", 8, 0),
    ] {
        let plan = termin(json!({"summary": "A", "start": angabe}))
            .unwrap_or_else(|fehler| panic!("{angabe}: {fehler}"));
        let start = Local.timestamp_opt(plan_start(&plan), 0).unwrap();

        assert_eq!(start.hour(), stunde, "{angabe} -> {}", plan.when);
        assert_eq!(start.minute(), minute, "{angabe} -> {}", plan.when);
    }
}

#[test]
fn tage_und_wochentage_werden_aufgeloest() {
    let in_drei = termin(json!({"summary": "A", "start": "in 3 Tagen 15:00"})).unwrap();
    assert_eq!(tage_bis(&in_drei), 3, "{}", in_drei.when);

    // „montag 10:00“ meint den nächsten Montag, nicht den heutigen.
    for angabe in ["montag 10:00", "mo 10:00"] {
        let plan = termin(json!({"summary": "A", "start": angabe})).unwrap();
        let start = Local.timestamp_opt(plan_start(&plan), 0).unwrap();
        let differenz = (start.weekday().num_days_from_monday() as i64
            - Local::now().weekday().num_days_from_monday() as i64)
            .rem_euclid(7);

        assert!(
            differenz > 0,
            "{angabe} liegt nicht in der Zukunft: {}",
            plan.when
        );
        assert!(differenz <= 7, "{angabe}: {differenz} Tage");
    }
}

#[test]
fn ein_geschriebenes_datum_wird_gelesen() {
    let tag = Local::now().date_naive() + chrono::Duration::days(7);
    let iso = tag.format("%Y-%m-%d");
    let kurz = tag.format("%e.%m.");
    let lang = tag.format("%d.%m.%Y");

    for angabe in [
        format!("{kurz} 14:00"),
        format!("{lang} 14:00"),
        format!("{iso} 14:00"),
    ] {
        let plan = termin(json!({"summary": "A", "start": angabe.as_str()}))
            .unwrap_or_else(|fehler| panic!("{angabe}: {fehler}"));
        let start = Local.timestamp_opt(plan_start(&plan), 0).unwrap();

        assert_eq!(start.date_naive(), tag, "{angabe}: {}", plan.when);
        assert_eq!(start.hour(), 14, "{angabe}: {}", plan.when);
    }
}

#[test]
fn monatsnamen_werden_gelesen() {
    for angabe in ["5. Oktober 14:00", "5.10. 14:00", "okt 5 14:00"] {
        let plan = termin(json!({"summary": "A", "start": angabe}))
            .unwrap_or_else(|fehler| panic!("{angabe}: {fehler}"));
        let start = Local.timestamp_opt(plan_start(&plan), 0).unwrap();

        assert_eq!(start.day(), 5, "{angabe}: {}", plan.when);
        assert_eq!(start.month(), 10, "{angabe}: {}", plan.when);
    }
}

#[test]
fn ein_datum_ohne_jahr_meint_das_naechste() {
    // Am 30. September ist „1.1.“ der nächste 1. Januar, nicht der vergangene.
    let plan = termin(json!({"summary": "A", "start": "1.1. 12:00"})).unwrap();
    let start = Local.timestamp_opt(plan_start(&plan), 0).unwrap();

    assert_eq!(start.day(), 1, "{}", plan.when);
    assert_eq!(start.month(), 1, "{}", plan.when);
    assert!(start > Local::now(), "{}", plan.when);
}

#[test]
fn das_absolute_format_mit_versatz_bleibt_genau() {
    // Diese Schreibweise muss weiterhin ohne Umweg über die Worte funktionieren.
    let tag = Local::now().date_naive() + chrono::Duration::days(7);
    let plan = termin(json!({
        "summary": "A",
        "start": format!("{}T09:00:00+02:00", tag.format("%Y-%m-%d")),
        "end": format!("{}T10:00:00+02:00", tag.format("%Y-%m-%d"))
    }))
    .unwrap();

    // Die Datei nennt UTC, unabhängig von der Zone des Rechners.
    assert!(
        entfalten(&plan.ics).contains(&format!("DTSTART:{}T070000Z", tag.format("%Y%m%d"))),
        "{}",
        plan.ics
    );
    assert_eq!(tage_bis(&plan), 7, "{}", plan.when);
}

#[test]
fn ein_weit_zurueckliegender_termin_wird_abgelehnt() {
    // Genau der Fehlgriff aus dem Betrieb: ein geratenes Datum Jahre zurück.
    let fehler = termin(json!({"summary": "test", "start": "2023-10-05T14:00"}))
        .expect_err("ein Termin von 2023 darf nicht angelegt werden");

    assert!(fehler.contains("Vergangenheit"), "{fehler}");
    assert!(fehler.contains("2023"), "{fehler}");
    // Und die Meldung sagt, wie es besser geht.
    assert!(fehler.contains("morgen"), "{fehler}");
}

#[test]
fn gestern_darf_noch_nachtragen_werden() {
    // Wer eine Einteilung vergessen hat, soll sie nachholen können.
    let gestern = Local::now() - chrono::Duration::days(1);

    let plan = termin(json!({
        "summary": "Nachgetragen",
        "start": gestern.format("%Y-%m-%dT%H:%M").to_string()
    }))
    .unwrap_or_else(|fehler| panic!("gestern muss möglich sein: {fehler}"));

    assert!(entfalten(&plan.ics).contains("DTSTART:"), "{}", plan.ics);
}

#[test]
fn ohne_uhrzeit_fragt_nach_statt_zu_raten() {
    let fehler = termin(json!({"summary": "A", "start": "morgen"}))
        .expect_err("ohne Uhrzeit wird nachgefragt");

    assert!(fehler.contains("Uhrzeit"), "{fehler}");
    assert!(
        fehler.contains("morgen 14:00"),
        "die Meldung nennt ein Beispiel: {fehler}"
    );
}

#[test]
fn unsinn_bleibt_ein_fehler_und_wird_nicht_zum_heutigen_tag() {
    // Eine Jahreszahl weit weg von jeder Plausibilität darf nicht stillschweigend
    // auf heute fallen.
    assert!(termin(json!({"summary": "A", "start": "1700-01-01T09:00"})).is_err());
    assert!(termin(json!({"summary": "A", "start": "2999-01-01T09:00"})).is_err());
}

/// Die Auflösung relativer Angaben an **jedem** Tag eines Jahres.
///
/// Die bisherigen Tests prüfen „morgen“ an dem Tag, an dem sie laufen. Damit
/// bleiben genau die Fälle ungeprüft, die im Betrieb schon wehgetan haben: der
/// erste eines Monats, der letzte Dezembertag, ein Schaltjahr. Ein Fehler dort
/// fällt nur einmal im Jahr auf und wird dann als seltsamer Einzelfall abgetan.
///
/// Der Startpunkt ist ein Parameter, nicht die Systemuhr. Sonst hinge die
/// Prüfung an der Tageszeit und der Zeitzone des Rechners, auf dem `cargo test`
/// gerade läuft.
#[test]
fn morgen_ist_ueber_ganzjahr_der_folgetag() {
    for tage in 0..366 {
        // Start am 1. Januar, damit Monats- und Jahreswechsel im Lauf liegen.
        let heute = NaiveDate::from_ymd_opt(2026, 1, 1)
            .expect("Startdatum")
            .checked_add_signed(chrono::Duration::days(tage))
            .expect("Datum bleibt im Jahr");

        let angabe = Angabe::neu_am("morgen 09:00", heute);
        let ermittelt = ermittle_tag(&angabe);

        assert_eq!(
            ermittelt,
            heute.succ_opt().expect("Folgetag"),
            "„morgen“ am {heute} ergab {ermittelt}"
        );
    }
}

#[test]
fn uebermorgen_und_gestern_sind_symmetrisch() {
    for tage in 0..366 {
        // 2028 ist ein Schaltjahr, der 29. Februar liegt mitten im Lauf.
        let heute = NaiveDate::from_ymd_opt(2028, 1, 1)
            .expect("Startdatum")
            .checked_add_signed(chrono::Duration::days(tage))
            .expect("Datum bleibt im Jahr");

        assert_eq!(
            ermittle_tag(&Angabe::neu_am("übermorgen", heute)),
            heute
                .succ_opt()
                .and_then(|t| t.succ_opt())
                .expect("übermorgen"),
            "„übermorgen“ am {heute}"
        );
        assert_eq!(
            ermittle_tag(&Angabe::neu_am("gestern", heute)),
            heute.pred_opt().expect("gestern"),
            "„gestern“ am {heute}"
        );
    }
}

/// Ein Wochentagsname meint den nächsten, nicht den laufenden.
///
/// Der Nullpunkt der Differenz geht auf sieben Tage, nicht auf null: „montags“
/// nennt einen Termin in der Zukunft, und der heutige Montag ist vorbei, sobald
/// man ihn ausspricht. Über ein ganzes Jahr geprüft, weil ein Fehler in der
/// Wochenrechnung an sechs von sieben Tagen unauffällig bleibt und nur an einem
/// einzigen auffällt – was dann als seltsamer Einzelfall abgetan wird.
#[test]
fn jeder_wochentagsname_landet_auf_seinem_wochentag() {
    const WOCHENTAGE: [&str; 7] = [
        "montag",
        "dienstag",
        "mittwoch",
        "donnerstag",
        "freitag",
        "samstag",
        "sonntag",
    ];
    // Die Kurzformen stehen in derselben Liste, und die beiden fehlenden
    // Langformen („mittwoch“, „sonnabend“) haben einen ganzen Wochentag lahm
    // gemacht. Beides wird hier über das ganze Jahr nachgeprüft.

    for tage in 0..366 {
        // Start am Montag, damit über 366 Tage jeder Wochentag als „heute“ vorkommt.
        let heute = NaiveDate::from_ymd_opt(2027, 1, 4)
            .expect("Startdatum")
            .checked_add_signed(chrono::Duration::days(tage))
            .expect("Datum bleibt im Jahr");

        for (nummer, name) in WOCHENTAGE.iter().enumerate() {
            let gesucht = nummer as u8;
            // Ohne Uhrzeit: „montag 14:00“ liest die 14 als Tag im Monat, was
            // bei einem ausgeschriebenen Datum („am 14. Oktober“) richtig ist und
            // hier nur den Wochentag verdecken würde.
            let ermittelt = ermittle_tag(&Angabe::neu_am(name, heute));

            assert_eq!(
                ermittelt.weekday().num_days_from_monday() as u8,
                gesucht,
                "„{name}“ am {heute} ergab {ermittelt}"
            );
            // Und immer in der Zukunft, auch wenn heute derselbe Wochentag ist.
            assert!(
                ermittelt > heute,
                "„{name}“ am {heute} ergab {ermittelt}, liegt also nicht in der Zukunft"
            );
            // Und nie mehr als eine Woche entfernt.
            assert!(
                ermittelt - heute <= chrono::Duration::days(7),
                "„{name}“ am {heute} sprang bis {ermittelt}"
            );
        }
    }
}

/// Jede Schreibweise eines Wochentags findet denselben Tag.
///
/// In der Liste standen zwei Tippfehler – „mitwoch“ statt „mittwoch“ und
/// „sonnabend“ statt „sonnabend“ – und eine fehlende Kurzform. Wer „mittwoch“
/// sagte, bekam den heutigen Tag: Der Wochentag war nicht zu finden, also blieb
/// nichts übrig als heute. Beides fällt nur auf, wenn jedes Wort über ein ganzes
/// Jahr an jedem Wochentag geprüft wird.
#[test]
fn jede_schreibweise_findet_den_wochentag() {
    const ALLE: [(&str, u8); 16] = [
        ("montag", 0),
        ("mo", 0),
        ("dienstag", 1),
        ("di", 1),
        ("mittwoch", 2),
        ("mitwoch", 2),
        ("mi", 2),
        ("donnerstag", 3),
        ("do", 3),
        ("freitag", 4),
        ("fr", 4),
        ("samstag", 5),
        ("sonnabend", 5),
        ("sa", 5),
        ("sonntag", 6),
        ("so", 6),
    ];

    // Die Liste wird unabhängig vom Datum gelesen; für alle Wörter gilt
    // dieselbe Zuordnung.
    for (wort, nummer) in ALLE {
        assert_eq!(
            wochentag_aus(&[wort.to_string()]),
            Some(nummer),
            "„{wort}“ wird nicht als Wochentag {nummer} gelesen"
        );
    }

    // Und die Liste deckt jeden Wochentag in beiden Längen ab: sonst fehlt einer.
    for nummer in 0..7u8 {
        assert!(
            ALLE.iter().any(|(_, n)| *n == nummer),
            "Wochentag {nummer} fehlt ganz in der Liste"
        );
        let kurze: Vec<&str> = ALLE
            .iter()
            .filter(|(_, n)| *n == nummer && n.to_string().len() <= 2)
            .map(|(w, _)| *w)
            .collect();
        assert!(!kurze.is_empty(), "Wochentag {nummer} hat keine Kurzform");
    }
}

/// Die Zählweise der Woche ist überall dieselbe: Montag ist null.
///
/// Chrono kennt `number_from_monday()`, das bei **eins** beginnt, und
/// `num_days_from_monday()`, das bei **null** beginnt. `WOCHENTAGE` ist
/// null-basiert. Mit der falschen Mischung in der Formel landete jeder Wochentag
/// einen Tag zu früh: „montag“ an einem Montag ergab den Sonntag.
#[test]
fn montag_ist_null_in_der_wochenzaehlung() {
    let montag = NaiveDate::from_ymd_opt(2027, 1, 4).expect("4.1.2027 ist ein Montag");

    for (versatz, erwartet) in [(0i64, 0u8), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)] {
        let tag = montag + chrono::Duration::days(versatz);
        assert_eq!(
            tag.weekday().num_days_from_monday() as u8,
            erwartet,
            "{tag} hat die falsche Nummer"
        );
        assert_eq!(
            naechster_wochentag(montag, erwartet),
            if versatz == 0 {
                montag + chrono::Duration::days(7)
            } else {
                tag
            },
            "„{erwartet}“ am Montag"
        );
    }
}

/// Das lesende Werkzeug liest dieselben Worte wie das schreibende.
///
/// Der Befund aus dem Betrieb: Das Modell schickte `from: übermorgen`, obwohl
/// das Schema `JJJJ-MM-TT` verlangte. Ein Parser, der nur Datumsgrenzen annimmt,
/// lässt das still ins Leere laufen und antwortet mit 30 Tagen. Die Antwort sah
/// damit nach einer leeren Agenda aus und war in Wahrheit eine Abfrage nach einem
/// völlig anderen Zeitraum.
#[test]
fn woerter_werden_auch_im_zeitraum_gelesen() {
    let heute = NaiveDate::from_ymd_opt(2026, 10, 1).expect("1.10.2026");

    let faelle = [
        ("2026-10-03", "übermorgen"),
        ("2026-10-03", "uebermorgen"),
        ("2026-10-02", "morgen"),
        ("2026-10-01", "heute"),
        ("2026-09-30", "gestern"),
        ("2026-09-29", "vorgestern"),
        ("2026-10-04", "sonntag"),
        ("2026-10-10", "in 9 Tagen"),
        ("2026-10-03", "03.10."),
        ("2026-10-03", "3.10.2026"),
        ("2026-10-03", "3. Oktober"),
        ("2026-10-03", "Oktober 3"),
    ];

    for (erwartet, wort) in faelle {
        assert_eq!(
            parse_zeitraum_tag(wort, heute).expect(wort),
            NaiveDate::parse_from_str(erwartet, "%Y-%m-%d").expect("erwartetes Datum"),
            "„{wort}“ am {heute}"
        );
    }
}

/// Über ein ganzes Jahr, mit einem Startdatum je Jahr statt mit der Systemuhr.
///
/// Die Worte sind nicht an einem Tag im Jahr erfunden, aber „in drei Tagen“ und
/// die Monatsnamen sind es: Der 31. Januar plus drei Tage ist nicht der 3. März.
#[test]
fn woerter_bleiben_ueber_ganzjahr_stabil() {
    for jahr in [2026, 2027, 2028] {
        for tage in 0..365 {
            let heute = NaiveDate::from_ymd_opt(jahr, 1, 1)
                .expect("Startdatum")
                .checked_add_signed(chrono::Duration::days(tage))
                .expect("Datum bleibt im Jahr");

            assert_eq!(
                parse_zeitraum_tag("morgen", heute).expect("morgen"),
                heute.succ_opt().expect("Folgetag"),
                "„morgen“ am {heute}"
            );
            assert_eq!(
                parse_zeitraum_tag("übermorgen", heute).expect("übermorgen"),
                heute
                    .succ_opt()
                    .and_then(|t| t.succ_opt())
                    .expect("übermorgen"),
                "„übermorgen“ am {heute}"
            );
            assert_eq!(
                parse_zeitraum_tag("in 3 Tagen", heute).expect("in 3 Tagen"),
                heute + chrono::Duration::days(3),
                "„in 3 Tagen“ am {heute}"
            );
        }
    }
}

/// Eine Angabe ohne verwertbaren Tag ist ein Fehler, nicht der heutige Tag.
///
/// Sonst hieße eine leere Angabe stillschweigend „heute“, und die Antwort auf
/// „welche Termine gibt es am Irgendwas“ wäre der heutige Tag.
#[test]
fn eine_angabe_ohne_tag_wird_abgelehnt() {
    let heute = NaiveDate::from_ymd_opt(2026, 10, 1).expect("1.10.2026");

    for unbrauchbar in ["irgendwann", "bald", "", "   ", "asdf"] {
        let fehler =
            parse_zeitraum_tag(unbrauchbar, heute).expect_err("muss eine Fehlermeldung geben");

        assert!(fehler.contains("JJJJ-MM-TT"), "{unbrauchbar:?}: {fehler}");
    }
}

/// Ein ISO-Datum geht auch im Zeitraum – es ist der Normalfall.
#[test]
fn ein_iso_datum_bleibt_ein_iso_datum() {
    let heute = NaiveDate::from_ymd_opt(2026, 10, 1).expect("1.10.2026");

    assert_eq!(
        parse_zeitraum_tag("2026-12-24", heute).expect("Heiligabend"),
        NaiveDate::from_ymd_opt(2026, 12, 24).expect("24.12.")
    );
}

/// Ein genannter Wochentag meint den **nächsten**.
///
/// Und ausdrücklich nicht den Tag danach: Null Differenz bedeutet, dass der
/// Wochentag heute ist – und die sind 7 Tage, nicht 1. Mit einer Tage-Verschiebung
/// ergab „montag“ an einem Montag den Dienstag.
#[test]
fn ein_wochentag_meint_den_naechsten_derselben_woche() {
    // 1.10.2026 ist ein Donnerstag.
    let heute = NaiveDate::from_ymd_opt(2026, 10, 1).expect("1.10.2026");

    // Donnerstag, 1.10.2026. Der **nächste** Freitag ist der 2.10. und nicht der
    // 9.10. – „nächster“ heißt nicht „volle Woche voraus“.
    let faelle = [
        ("montag", "2026-10-05"),
        ("dienstag", "2026-10-06"),
        ("mittwoch", "2026-10-07"),
        ("donnerstag", "2026-10-08"),
        ("freitag", "2026-10-02"),
        ("samstag", "2026-10-03"),
        ("sonntag", "2026-10-04"),
    ];

    for (wort, erwartet) in faelle {
        assert_eq!(
            parse_zeitraum_tag(wort, heute).expect(wort),
            NaiveDate::parse_from_str(erwartet, "%Y-%m-%d").expect("erwartetes Datum"),
            "„{wort}“ am Donnerstag {heute}"
        );
    }
}

/// Derselbe Wochentag an demselben Wochentag: eine Woche später.
///
/// Sonst käme der heutige Tag zurück, und „montag“ an einem Montag hieße heute.
#[test]
fn derselbe_wochentag_heute_ist_eine_woche_spater() {
    // 4.1.2027 ist ein Montag.
    let montag = NaiveDate::from_ymd_opt(2027, 1, 4).expect("4.1.2027 ist ein Montag");

    assert_eq!(
        parse_zeitraum_tag("montag", montag).expect("montag"),
        NaiveDate::from_ymd_opt(2027, 1, 11).expect("11.1.2027"),
        "der nächste Montag nach dem 4.1. ist der 11.1."
    );
    // Und das gilt für jeden Wochentag an sich selbst.
    for versatz in 0..7 {
        let heute = montag + chrono::Duration::days(versatz);
        let eigener = parse_zeitraum_tag("montag", heute).expect("montag");

        assert!(
            eigener > heute,
            "„montag“ am {heute} (Montag) ergab {eigener}"
        );
        assert_eq!(
            eigener.weekday().num_days_from_monday(),
            0,
            "„montag“ am {heute} ergab {eigener}, das ist kein Montag"
        );
    }
}

/// „Übernächst“ heißt eine ganze Woche weiter, nicht derselbe Tag.
///
/// Das „über“ wurde nicht gelesen: „übernächster montag“ landete auf demselben Tag
/// wie „montag“. Und weil „übernächst“ das „nächst“ enthält, muss es vor der
/// allgemeinen Wochentagsregel geprüft werden – sonst gewinnt der nächste.
#[test]
fn uebernaechster_wochentag_ist_eine_woche_weiter() {
    let heute = NaiveDate::from_ymd_opt(2026, 10, 1).expect("1.10.2026");

    for (wort, erwartet) in [
        ("übernächster montag", "2026-10-12"),
        // Auch ohne Umlaut: Wer kein „ü“ hat, schreibt „ue“.
        ("uebernachster montag", "2026-10-12"),
        ("übernächsten montag", "2026-10-12"),
        // Nächster Sonntag am 1.10. ist der 4.10., übernächst der 11.10.
        ("übernächster sonntag", "2026-10-11"),
    ] {
        assert_eq!(
            parse_zeitraum_tag(wort, heute).expect(wort),
            NaiveDate::parse_from_str(erwartet, "%Y-%m-%d").expect("erwartetes Datum"),
            "„{wort}“ am {heute}"
        );
    }
}

/// „Diese Woche“ meint die laufende, Montag bis Sonntag – auch wenn vorbei.
///
/// Der Wochentag wird hier **nicht** nach vorn gerutscht: „montag diese woche“ an
/// einem Donnerstag meint den Montag dieser Woche, auch wenn das Tage zurück
/// liegt. Das ist beim **Lesen** richtig; beim Schlegen fängt die Prüfung zur
/// Vergangenheit solche Angaben ab.
#[test]
fn diese_woche_ist_die_laufende_von_montag_bis_sonntag() {
    let donnerstag = NaiveDate::from_ymd_opt(2026, 10, 1).expect("1.10.2026 ist ein Donnerstag");

    for (wort, erwartet) in [
        ("montag diese woche", "2026-09-28"),
        ("montag", "2026-10-05"),
        ("montag nächste woche", "2026-10-12"),
        ("montag nächste woche", "2026-10-12"),
        // Auch mit dem Zusatz davor.
        ("nächste woche montag", "2026-10-12"),
        ("kommende woche montag", "2026-10-12"),
    ] {
        assert_eq!(
            parse_zeitraum_tag(wort, donnerstag).expect(wort),
            NaiveDate::parse_from_str(erwartet, "%Y-%m-%d").expect("erwartetes Datum"),
            "„{wort}“ am Donnerstag {donnerstag}"
        );
    }

    // Am Montag selbst meint „diese Woche“ den heutigen Montag.
    let montag = NaiveDate::from_ymd_opt(2026, 10, 5).expect("5.10.2026 ist ein Montag");
    assert_eq!(
        parse_zeitraum_tag("montag diese woche", montag).expect("montag"),
        montag,
        "am Montag meint „diese Woche“ den heutigen"
    );
}

/// Die Wochentagswoche stimmt an **jedem** Tag über mehrere Jahre.
///
/// Der erste Befund aus dem Betrieb war ein falsches „übernächste Woche" an einem
/// Sonntag, der in der laufenden Woche lag. Solche Rechnungen sehen an einem Tag
/// richtig aus und an drei others falsch – deshalb wird hier jeder Tag als
/// „heute“ durchlaufen, nicht einer ausgewählt.
///
/// Erwartet wird unabhängig nachgerechnet: Über den **Montag** der Woche, weil
/// der Vergleich der Tage sonst danebenliegt. Der Sonntag derselben Woche ist vom
/// Donnerstag aus sieben Tage entfernt und läge so in der nächsten.
#[test]
fn die_woche_eines_wochentags_stimmt_an_iedem_tag() {
    for jahr in [2026, 2027, 2028] {
        for tage in 0..365 {
            let heute = NaiveDate::from_ymd_opt(jahr, 1, 1)
                .expect("Startdatum")
                .checked_add_signed(chrono::Duration::days(tage))
                .expect("Datum bleibt im Jahr");
            let montag_dieser_woche = heute
                - chrono::Duration::days(
                    chrono::Datelike::weekday(&heute).num_days_from_monday() as i64
                );

            for name in ["montag", "freitag", "sonntag"] {
                let naechster = parse_zeitraum_tag(name, heute).expect(name);
                let mon = naechster
                    - chrono::Duration::days(
                        chrono::Datelike::weekday(&naechster).num_days_from_monday() as i64,
                    );
                // Der nächste Wochentag liegt in derselben Woche oder in der
                // kommenden – nie weiter. Am Donnerstag 1.1.2026 ist der nächste
                // Freitag der 2.1., und der gehört noch zur laufenden Woche. Die
                // Erwartung „immer die kommende Woche“ wäre falsch; geprüft wird
                // die Zahl der Wochen, nicht ein fester Wert.
                let wochen = (mon - montag_dieser_woche).num_days() / 7;
                assert!(
                    (0..=1).contains(&wochen),
                    "„{name}“ am {heute} (Wochentag {}) landet auf {naechster} und damit {wochen} Wochen weiter statt 0 oder 1",
                    chrono::Datelike::weekday(&heute).num_days_from_monday()
                );
                // Und er liegt stets in der Zukunft, auch wenn heute derselbe
                // Wochentag ist – dann sieben Tage, nicht null.
                assert!(
                    naechster > heute,
                    "„{name}“ am {heute} (Wochentag {}) ergab {naechster}, also nicht in der Zukunft",
                    chrono::Datelike::weekday(&heute).num_days_from_monday()
                );

                // Und „übernächst“ ist genau die Woche weiter, nicht dieselbe.
                let ueber = parse_zeitraum_tag(&format!("übernächster {name}"), heute)
                    .expect("übernächster");
                assert_eq!(
                    ueber - naechster,
                    chrono::Duration::days(7),
                    "„übernächster {name}“ am {heute} ist nicht eine Woche weiter"
                );
            }
        }
    }
}
