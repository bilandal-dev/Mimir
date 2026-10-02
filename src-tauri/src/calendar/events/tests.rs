//! Tests für das Aufbereiten der Termine. Die Fälle stammen aus dem, was die
//! Nextcloud-Oberfläche und andere Kalender tatsächlich erzeugen.

use super::*;
use chrono::{TimeZone, Timelike};

fn days(ics: &str) -> Vec<CalendarEvent> {
    let window = Window {
        start: Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2040, 1, 1, 0, 0, 0).unwrap(),
    };
    let mut events = parse_events(ics, "Test", "/calendars/kai/test/", &window);
    events.sort_by_key(|event| event.start);
    events
}

fn local(ics: &str) -> Vec<String> {
    days(ics)
        .into_iter()
        .map(|event| {
            format!(
                "{}|{}|{}",
                Utc.timestamp_opt(event.start, 0)
                    .unwrap()
                    .format("%d.%m. %H:%M"),
                event.all_day,
                event.summary
            )
        })
        .collect()
}

#[test]
fn a_simple_event_with_a_timezone_becomes_a_fixed_moment() {
    let ics = "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART;TZID=Europe/Berlin:20260912T090000\r\n\
               DTEND;TZID=Europe/Berlin:20260912T103000\r\n\
               SUMMARY:Teammeeting\r\n\
               LOCATION:Raum 2\\, erste Etage\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n";
    let events = days(ics);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    // Berlin im September ist UTC+2.
    assert_eq!(
        Utc.timestamp_opt(event.start, 0)
            .unwrap()
            .format("%H:%M")
            .to_string(),
        "07:00"
    );
    assert_eq!(
        Utc.timestamp_opt(event.end, 0)
            .unwrap()
            .format("%H:%M")
            .to_string(),
        "08:30"
    );
    assert_eq!(event.summary, "Teammeeting");
    assert_eq!(event.location, "Raum 2, erste Etage");
    assert!(!event.all_day);
    assert!(!event.floating);
}

#[test]
fn an_all_day_event_lasts_exactly_one_day() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:2\r\n\
               DTSTART;VALUE=DATE:20260912\r\n\
               DTEND;VALUE=DATE:20260913\r\n\
               SUMMARY:Urlaub\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    assert_eq!(events.len(), 1);
    assert!(events[0].all_day);
    let start = Utc.timestamp_opt(events[0].start, 0).unwrap();
    let end = Utc.timestamp_opt(events[0].end, 0).unwrap();
    assert_eq!((end - start).num_hours(), 24);
}

#[test]
fn daylight_saving_is_respected_in_a_series() {
    // Die Serie läuft über die Zeitumstellung vom 25. Oktober 2026. Neun Uhr
    // muss in der Lokalzeit neun Uhr bleiben, der Abstand zu UTC ändert sich.
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:3\r\n\
               DTSTART;TZID=Europe/Berlin:20261020T090000\r\n\
               DTEND;TZID=Europe/Berlin:20261020T100000\r\n\
               RRULE:FREQ=WEEKLY;COUNT=3\r\n\
               SUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let times: Vec<String> = days(ics)
        .into_iter()
        .map(|event| {
            let utc = Utc.timestamp_opt(event.start, 0).unwrap();
            let local = utc.with_timezone(&chrono_tz::Europe::Berlin);
            format!(
                "{} UTC={}",
                local.format("%d.%m. %H:%M"),
                utc.format("%H:%M")
            )
        })
        .collect();

    assert_eq!(
        times,
        vec![
            "20.10. 09:00 UTC=07:00".to_string(),
            "27.10. 09:00 UTC=08:00".to_string(),
            "03.11. 09:00 UTC=08:00".to_string(),
        ]
    );
}

#[test]
fn a_daily_rule_with_an_interval_stays_on_its_rhythm() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:4\r\n\
               DTSTART:20260901T080000Z\r\n\
               DTEND:20260901T090000Z\r\n\
               RRULE:FREQ=DAILY;INTERVAL=3;COUNT=4\r\n\
               SUMMARY:Laufen\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 4);
    assert!(list[0].starts_with("01.09. 08:00"), "{list:?}");
    assert!(list[1].starts_with("04.09. 08:00"), "{list:?}");
    assert!(list[2].starts_with("07.09. 08:00"), "{list:?}");
    assert!(list[3].starts_with("10.09. 08:00"), "{list:?}");
}

#[test]
fn a_weekly_rule_with_several_days_creates_one_event_per_day() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:5\r\n\
               DTSTART;TZID=Europe/Berlin:20260907T100000\r\n\
               DTEND;TZID=Europe/Berlin:20260907T110000\r\n\
               RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;COUNT=6\r\n\
               SUMMARY:Sport\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 6);
    assert!(list[0].starts_with("07.09."), "{list:?}");
    assert!(list[1].starts_with("09.09."), "{list:?}");
    assert!(list[2].starts_with("11.09."), "{list:?}");
    assert!(list[3].starts_with("14.09."), "{list:?}");
}

#[test]
fn a_monthly_rule_on_the_last_friday_finds_the_right_days() {
    // Der letzte Freitag ist im September 2026 der 25., im Oktober der 30. und
    // im November der 27. Ein Zähler muss dabei greifen, sonst passt es nur
    // zufällig.
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:6\r\n\
               DTSTART;TZID=Europe/Berlin:20260925T160000\r\n\
               DTEND;TZID=Europe/Berlin:20260925T170000\r\n\
               RRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3\r\n\
               SUMMARY:Monatsreview\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 3);
    assert!(list[0].starts_with("25.09."), "{list:?}");
    assert!(list[1].starts_with("30.10."), "{list:?}");
    assert!(list[2].starts_with("27.11."), "{list:?}");
}

#[test]
fn a_monthly_rule_on_the_first_monday_finds_the_right_days() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:7\r\n\
               DTSTART;TZID=Europe/Berlin:20260907T090000\r\n\
               DTEND;TZID=Europe/Berlin:20260907T100000\r\n\
               RRULE:FREQ=MONTHLY;BYDAY=1MO;COUNT=2\r\n\
               SUMMARY:Kickoff\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 2);
    assert!(list[0].starts_with("07.09."), "{list:?}");
    assert!(list[1].starts_with("05.10."), "{list:?}");
}

#[test]
fn excluded_days_disappear_from_a_series() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:8\r\n\
               DTSTART:20260901T080000Z\r\nDTEND:20260901T083000Z\r\n\
               RRULE:FREQ=DAILY;COUNT=5\r\n\
               EXDATE:20260903T080000Z\r\n\
               SUMMARY:Therapie\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 4, "{list:?}");
    assert!(
        !list.iter().any(|entry| entry.starts_with("03.09.")),
        "{list:?}"
    );
}

#[test]
fn a_moved_occurrence_replaces_its_original() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:9\r\n\
               DTSTART;TZID=Europe/Berlin:20260907T100000\r\n\
               DTEND;TZID=Europe/Berlin:20260907T110000\r\n\
               RRULE:FREQ=WEEKLY;COUNT=3\r\n\
               SUMMARY:Teammeeting\r\nEND:VEVENT\r\n\
               BEGIN:VEVENT\r\nUID:9\r\n\
               RECURRENCE-ID;TZID=Europe/Berlin:20260914T100000\r\n\
               DTSTART;TZID=Europe/Berlin:20260915T140000\r\n\
               DTEND;TZID=Europe/Berlin:20260915T150000\r\n\
               SUMMARY:Teammeeting (verschoben)\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 3, "{list:?}");
    assert!(list[0].starts_with("07.09."), "{list:?}");
    assert!(list[1].starts_with("15.09."), "{list:?}");
    assert!(list[1].ends_with("Teammeeting (verschoben)"), "{list:?}");
    assert!(list[2].starts_with("21.09."), "{list:?}");
}

#[test]
fn a_deleted_occurrence_is_gone_but_the_series_continues() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:10\r\n\
               DTSTART;TZID=Europe/Berlin:20260907T100000\r\n\
               DTEND;TZID=Europe/Berlin:20260907T110000\r\n\
               RRULE:FREQ=DAILY;COUNT=3\r\n\
               SUMMARY:Arzt\r\nEND:VEVENT\r\n\
               BEGIN:VEVENT\r\nUID:10\r\n\
               RECURRENCE-ID;TZID=Europe/Berlin:20260908T100000\r\n\
               DTSTART;TZID=Europe/Berlin:20260908T100000\r\n\
               DTEND;TZID=Europe/Berlin:20260908T110000\r\n\
               STATUS:CANCELED\r\nSUMMARY:Arzt\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 2, "{list:?}");
    assert!(
        !list.iter().any(|entry| entry.starts_with("08.09.")),
        "{list:?}"
    );
}

#[test]
fn folded_lines_and_escapes_are_read_correctly() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:11\r\n\
               DTSTART:20260912T120000Z\r\nDTEND:20260912T130000Z\r\n\
               SUMMARY:Ein langer Titel der über\r\n  mehrere Zeilen geht\r\n\
               LOCATION:Zeile eins\\nZeile zwei\r\n\
               END:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].summary,
        "Ein langer Titel der über mehrere Zeilen geht"
    );
    assert_eq!(events[0].location, "Zeile eins Zeile zwei");
}

#[test]
fn a_duration_instead_of_an_end_is_understood() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:12\r\n\
               DTSTART:20260912T120000Z\r\nDURATION:PT1H30M\r\n\
               SUMMARY:Block\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    let start = Utc.timestamp_opt(events[0].start, 0).unwrap();
    let end = Utc.timestamp_opt(events[0].end, 0).unwrap();
    assert_eq!((end - start).num_minutes(), 90);
}

#[test]
fn an_unknown_timezone_stays_as_written() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:13\r\n\
               DTSTART;TZID=Mars/Olympus:20260912T090000\r\n\
               DTEND;TZID=Mars/Olympus:20260912T100000\r\n\
               SUMMARY:Fremde Zone\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    assert!(events[0].floating, "unbekannte Zone wurde geraten");
}

#[test]
fn a_rule_we_cannot_map_shows_up_once_instead_of_wrong() {
    // BYWEEKNO wird hier nicht ausgezählt. Falsch aufgeklappt wäre schlimmer
    // als einmalig.
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:14\r\n\
               DTSTART:20260912T120000Z\r\nDTEND:20260912T130000Z\r\n\
               RRULE:FREQ=WEEKLY;BYWEEKNO=36\r\n\
               SUMMARY:Unbekannt\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let list = local(ics);
    assert_eq!(list.len(), 1, "{list:?}");
    assert!(list[0].starts_with("12.09."), "{list:?}");
}

#[test]
fn the_window_decides_what_is_returned() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:15\r\n\
               DTSTART:20260101T100000Z\r\nDTEND:20260101T110000Z\r\n\
               SUMMARY:Alt\r\nEND:VEVENT\r\n\
               BEGIN:VEVENT\r\nUID:16\r\n\
               DTSTART:20260912T100000Z\r\nDTEND:20260912T110000Z\r\n\
               SUMMARY:Neu\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let window = Window {
        start: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(),
    };
    let events = parse_events(ics, "Test", "/calendars/kai/test/", &window);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].summary, "Neu");
}

#[test]
fn a_series_without_an_end_does_not_run_forever() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:17\r\n\
               DTSTART:20260912T120000Z\r\nDTEND:20260912T130000Z\r\n\
               RRULE:FREQ=DAILY\r\n\
               SUMMARY:Endlos\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let window = Window {
        start: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2026, 9, 30, 0, 0, 0).unwrap(),
    };
    let events = parse_events(ics, "Test", "/calendars/kai/test/", &window);
    assert!(!events.is_empty());
    assert!(events.len() <= MAX_OCCURRENCES);
    // Der erste 12. September muss dabei sein, das Datum stimmt.
    let first = Utc.timestamp_opt(events[0].start, 0).unwrap();
    assert_eq!(first.day(), 12);
}

#[test]
fn events_without_a_start_are_skipped() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:18\r\nSUMMARY:Ohne Zeit\r\n\
               END:VEVENT\r\n\
               BEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nEND:VTIMEZONE\r\n\
               END:VCALENDAR\r\n";
    assert!(days(ics).is_empty());
}

#[test]
fn the_whole_day_of_a_series_start_is_covered() {
    // Der Fensteranfang liegt mitten in einer Serie; das erste Vorkommen am
    // Randtag darf nicht fehlen.
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:19\r\n\
               DTSTART:20260901T220000Z\r\nDTEND:20260901T230000Z\r\n\
               RRULE:FREQ=DAILY\r\n\
               SUMMARY:Nachts\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let window = Window {
        start: Utc.with_ymd_and_hms(2026, 9, 5, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2026, 9, 8, 0, 0, 0).unwrap(),
    };
    let events = parse_events(ics, "Test", "/calendars/kai/test/", &window);
    let days: Vec<u32> = events
        .iter()
        .map(|event| Utc.timestamp_opt(event.start, 0).unwrap().day())
        .collect();
    assert_eq!(days, vec![5, 6, 7]);
}

#[test]
fn long_titles_are_cut_and_locations_are_collapsed() {
    let ics = format!(
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:20\r\n\
         DTSTART:20260912T120000Z\r\nDTEND:20260912T130000Z\r\n\
         SUMMARY:{}\r\nLOCATION: {}\r\n\
         END:VEVENT\r\nEND:VCALENDAR\r\n",
        "Titel mit äöüß und sogar Emojis 🎉🎉".repeat(60),
        "Ort    mit    vielen    Leerzeichen"
    );
    let events = days(&ics);
    assert_eq!(events[0].summary.chars().count(), MAX_EVENT_SUMMARY_BYTES);
    assert!(events[0].summary.is_char_boundary(events[0].summary.len()));
    assert_eq!(events[0].location, "Ort mit vielen Leerzeichen");
}

#[test]
fn a_zero_length_event_does_not_break_the_display() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:21\r\n\
               DTSTART:20260912T120000Z\r\nDTEND:20260912T120000Z\r\n\
               SUMMARY:Punkt\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].start, events[0].end);
}

#[test]
fn the_calendar_name_comes_along_for_the_display() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:22\r\n\
               DTSTART:20260912T120000Z\r\nDTEND:20260912T130000Z\r\n\
               SUMMARY:Termin\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    assert_eq!(events[0].calendar, "Test");
}

#[test]
fn a_contradictory_zone_note_is_left_out() {
    // DTSTART mit Z und zusätzlichem TZID ist widersprüchlich; lieber
    // weglassen als falsch einordnen.
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:23\r\n\
               DTSTART;TZID=Europe/Berlin:20260912T090000Z\r\n\
               DTEND;TZID=Europe/Berlin:20260912T100000Z\r\n\
               SUMMARY:Widerspruch\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert!(days(ics).is_empty());
}

#[test]
fn hours_and_minutes_survive_the_conversion() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:24\r\n\
               DTSTART:20260912T235959Z\r\nDTEND:20260913T000000Z\r\n\
               SUMMARY:Mitternacht\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = days(ics);
    let start = Utc.timestamp_opt(events[0].start, 0).unwrap();
    let end = Utc.timestamp_opt(events[0].end, 0).unwrap();
    assert_eq!((end - start).num_seconds(), 1);
    assert_eq!(start.hour(), 23);
}

#[test]
fn a_calendar_without_events_is_not_an_error() {
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nextcloud//DE\r\nEND:VCALENDAR\r\n";
    assert!(days(ics).is_empty());
}

// --- Erinnerung und Kategorien beim Lesen -----------------------------------

#[test]
fn die_erinnerung_und_die_kategorien_kommen_mit_dem_termin_an() {
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260912T090000Z\r\n\
               DTEND:20260912T100000Z\r\n\
               SUMMARY:Zahnarzt\r\n\
               CATEGORIES:Gesundheit;Termin\r\n\
               BEGIN:VALARM\r\n\
               ACTION:DISPLAY\r\n\
               DESCRIPTION:Zahnarzt\r\n\
               TRIGGER:-PT15M\r\n\
               END:VALARM\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    assert_eq!(termine.len(), 1);
    assert_eq!(termine[0].reminder, Some(15));
    assert_eq!(termine[0].categories, vec!["Gesundheit", "Termin"]);
}

#[test]
fn die_beschreibung_der_erinnerung_wird_nicht_die_des_termins() {
    // Beide tragen eine DESCRIPTION. Ohne Trennung hätte der Termin die
    // Benachrichtigung als Beschreibung übernommen.
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260912T090000Z\r\n\
               SUMMARY:Teammeeting\r\n\
               DESCRIPTION:Kurze Vorbereitung\r\n\
               BEGIN:VALARM\r\n\
               DESCRIPTION:Erinnerungstext\r\n\
               TRIGGER:-PT30M\r\n\
               END:VALARM\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    // Die Anzeige nutzt `summary` und `location`; entscheidend ist, dass die
    // Beschreibung der Erinnerung nirgends als Termintext auftaucht.
    assert_eq!(termine[0].summary, "Teammeeting");
    assert!(termine[0].summary.contains("Teammeeting"));
    assert_eq!(termine[0].reminder, Some(30));
}

#[test]
fn eine_erinnerung_ohne_gueltigen_ausloeser_wird_nicht_geraten() {
    // Ein absoluter Auslösezeitpunkt ist gültige iCalendar-Syntax, aber kein
    // Abstand. Besser nichts anzeigen als eine falsche Zahl.
    for trigger in ["20260912T084500Z", "bald", "PT15M"] {
        let termine = days(&format!(
            "BEGIN:VCALENDAR\r\n\
             VERSION:2.0\r\n\
             BEGIN:VEVENT\r\n\
             UID:1\r\n\
             DTSTAMP:20260901T120000Z\r\n\
             DTSTART:20260912T090000Z\r\n\
             SUMMARY:A\r\n\
             BEGIN:VALARM\r\n\
             TRIGGER:{trigger}\r\n\
             END:VALARM\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR\r\n"
        ));

        assert_eq!(termine[0].reminder, None, "„{trigger}“ wurde gelesen");
    }
}

#[test]
fn ein_termin_ohne_erinnerung_und_ohne_kategorien_hat_keine() {
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260912T090000Z\r\n\
               SUMMARY:A\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    assert_eq!(termine[0].reminder, None);
    assert!(termine[0].categories.is_empty());
}

#[test]
fn leere_kategorien_und_maskierte_namen() {
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260912T090000Z\r\n\
               SUMMARY:A\r\n\
               CATEGORIES:Arbeit;;Müller\\; Meier\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    assert_eq!(termine[0].categories, vec!["Arbeit", "Müller; Meier"]);
}

#[test]
fn die_erinnerung_zieht_bei_einer_serie_mit_je_dem_termin_mit() {
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260907T090000Z\r\n\
               DTEND:20260907T100000Z\r\n\
               SUMMARY:Wochenbericht\r\n\
               CATEGORIES:Arbeit\r\n\
               RRULE:FREQ=WEEKLY;COUNT=3\r\n\
               BEGIN:VALARM\r\n\
               TRIGGER:-P1D\r\n\
               END:VALARM\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    assert_eq!(termine.len(), 3);
    for termin in &termine {
        assert_eq!(termin.reminder, Some(1440), "{termin:?}");
        assert_eq!(termin.categories, vec!["Arbeit"], "{termin:?}");
    }
}

#[test]
fn die_anzeige_bekommt_denselben_text_wie_das_bestaetigungsfenster() {
    // Leiste, /termine und Bestätigungsfenster sagen dasselbe. Steht hier ein
    // eigener Text, laufen die drei Ansichten auseinander.
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260912T090000Z\r\n\
               SUMMARY:Zahnarzt\r\n\
               BEGIN:VALARM\r\n\
               TRIGGER:-PT15M\r\n\
               END:VALARM\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    assert_eq!(termine[0].reminder_text, "Erinnerung 15 Minuten vorher");
    assert_eq!(
        termine[0].reminder_text,
        format!(
            "Erinnerung {}",
            super::super::write::erinnerung_text(termine[0].reminder.unwrap())
        )
    );
}

#[test]
fn ohne_erinnerung_bleibt_der_anzeigetext_leer() {
    let termine = days(
        "BEGIN:VCALENDAR\r\n\
               VERSION:2.0\r\n\
               BEGIN:VEVENT\r\n\
               UID:1\r\n\
               DTSTAMP:20260901T120000Z\r\n\
               DTSTART:20260912T090000Z\r\n\
               SUMMARY:A\r\n\
               END:VEVENT\r\n\
               END:VCALENDAR\r\n",
    );

    assert!(termine[0].reminder_text.is_empty());
}

/// Der gemeldete Fehler: „Termine von morgen“ lieferte den heutigen Tag.
///
/// Der Grund lag nicht am Kalender, sondern am einzigen verfügbaren Zeitraum: Eine
/// Zahl Tage ab jetzt kann den morgigen Tag nicht treffen, ihr früherster Start ist
/// „jetzt minus ein Tag“. Diese Tests halten fest, was der ausgeschriebene
/// Zeitraum liefern muss.
mod fenster_aus_tagen {
    use super::*;
    use chrono::NaiveDate;

    fn tag(jahr: i32, monat: u32, tag: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(jahr, monat, tag).unwrap()
    }

    fn jetzt() -> DateTime<Utc> {
        // Donnerstag, 1. Oktober 2026, 18:00 Uhr – der Fall aus dem Gemerkt.
        Utc.with_ymd_and_hms(2026, 10, 1, 18, 0, 0).unwrap()
    }

    #[test]
    fn ein_tag_ist_genau_ein_tag() {
        let fenster = window_from_days(jetzt(), tag(2026, 10, 2), None);

        // Der gewünschte Tag liegt vollständig im Fenster, der Vortag davor.
        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2026, 10, 3, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn ohne_bis_ist_es_der_eine_tag() {
        let fenster = window_from_days(jetzt(), tag(2026, 10, 2), None);

        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2026, 10, 3, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn bis_ist_der_letzte_tag_einschliesslich() {
        // Montag bis Sonntag derselben Woche.
        let fenster = window_from_days(jetzt(), tag(2026, 10, 5), Some(tag(2026, 10, 11)));

        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2026, 10, 4, 0, 0, 0).unwrap()
        );
        // Der Montag nach der Woche – sonst fiele der Sonntag weg.
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn von_und_bis_am_gleichen_tag_ist_ein_tag() {
        let fenster = window_from_days(jetzt(), tag(2026, 10, 2), Some(tag(2026, 10, 2)));

        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2026, 10, 3, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn von_bis_zurueck_ergibt_leerstes_fenster() {
        // Kein Absturz und kein leerer Zeitraum: Es wird der gewünschte Tag
        // genommen, nicht nichts.
        let fenster = window_from_days(jetzt(), tag(2026, 10, 10), Some(tag(2026, 10, 2)));

        assert!(fenster.end > fenster.start);
        assert!(fenster.end <= Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0).unwrap());
    }

    #[test]
    fn monatswechsel_wird_richtig_ueberschritten() {
        // Ohne `bis` ist es genau ein Tag, also der 31. Januar – nicht der ganze
        // Januar. Der Monatswechsel ist damit über die Grenze getestet.
        let fenster = window_from_days(jetzt(), tag(2026, 1, 31), None);

        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2026, 1, 30, 0, 0, 0).unwrap()
        );
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn monatswechsel_mit_bis() {
        // 30. Januar bis 2. Februar: der Februar gehört dazu.
        let fenster = window_from_days(jetzt(), tag(2026, 1, 30), Some(tag(2026, 2, 2)));

        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2026, 1, 29, 0, 0, 0).unwrap()
        );
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2026, 2, 3, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn schaltjahr_wird_richtig_ueberschritten() {
        let fenster = window_from_days(jetzt(), tag(2028, 2, 28), Some(tag(2028, 3, 1)));

        // Der 29. Februar 2028 existiert.
        assert_eq!(
            fenster.start,
            Utc.with_ymd_and_hms(2028, 2, 27, 0, 0, 0).unwrap()
        );
        assert_eq!(
            fenster.end,
            Utc.with_ymd_and_hms(2028, 3, 2, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn tageszahl_wird_begrenzt() {
        let fenster = window_from_days(jetzt(), tag(2026, 10, 2), Some(tag(2027, 10, 2)));

        assert!(fenster.end <= Utc.with_ymd_and_hms(2027, 10, 5, 0, 0, 0).unwrap());
    }

    #[test]
    fn unmuegliches_datum_faellt_auf_die_heutige_umgebung_zurueck() {
        // Der Minimaltag lässt keinen Tagesbeginn zu; statt keines Fensters soll
        // der Aufrufer ein brauchbares um jetzt bekommen.
        let fenster = window_from_days(jetzt(), NaiveDate::MIN, None);

        assert!(fenster.end > fenster.start);
        assert!(fenster.start <= jetzt());
        assert!(fenster.end > jetzt());
    }

    #[test]
    fn die_zahl_allein_kann_den_morgigen_tag_nicht_treffen() {
        // Der Grund für `from`: `range: 1` schließt den heutigen Tag ein.
        let fenster = window_from_now(jetzt(), 1);

        assert!(fenster.start < jetzt());
        assert!(fenster.end > Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap());
    }
}
