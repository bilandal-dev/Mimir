//! Tests für das Ändern und Löschen.
//!
//! Zwei Dinge sind hier wichtiger als der glückliche Fall: dass fremde Felder
//! erhalten bleiben und dass die Fälle abgelehnt werden, bei denen Mimir mehr
//! tun würde als der Benutzer glaubt.

use super::*;
use chrono::Datelike;
use serde_json::json;

/// Ein Termin, wie Nextcloud ihn schreibt: mit Zeitzonenblock, Erinnerung und
/// Kategorie.
const AUS_NEXTCLOUD: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Nextcloud//Davik//DE\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Europe/Berlin\r\n\
BEGIN:STANDARD\r\n\
DTSTART:19701025T030000\r\n\
TZOFFSETFROM:+0200\r\n\
TZOFFSETTO:+0100\r\n\
END:STANDARD\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
UID:termin-1@nextcloud\r\n\
DTSTAMP:20260901T120000Z\r\n\
LAST-MODIFIED:20260910T080000Z\r\n\
SEQUENCE:3\r\n\
DTSTART;TZID=Europe/Berlin:20260920T090000\r\n\
DTEND;TZID=Europe/Berlin:20260920T103000\r\n\
SUMMARY:Teammeeting\r\n\
LOCATION:Raum 2\r\n\
DESCRIPTION:Kurze Vorbereitung\r\n\
CATEGORIES:Arbeit\r\n\
BEGIN:VALARM\r\n\
TRIGGER:-PT15M\r\n\
ACTION:DISPLAY\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

/// Liest `20261001T120000Z` als Sekunden seit 1970.
fn zeitpunkt(wert: &str) -> i64 {
    let text = wert.trim_end_matches('Z');
    let jahr: i32 = text[0..4].parse().unwrap();
    let monat: u32 = text[4..6].parse().unwrap();
    let tag: u32 = text[6..8].parse().unwrap();
    let stunde: u32 = text[9..11].parse().unwrap();
    let minute: u32 = text[11..13].parse().unwrap();

    Utc.with_ymd_and_hms(jahr, monat, tag, stunde, minute, 0)
        .unwrap()
        .timestamp()
}

fn config() -> CalendarConfig {
    CalendarConfig {
        server_url: "https://cloud.example.org".to_string(),
        username: "kai".to_string(),
        calendars: vec!["arbeit".to_string()],
        server_certificate: None,
    }
}

fn kalender() -> Vec<(String, String)> {
    vec![("arbeit".to_string(), "Arbeit".to_string())]
}

fn aendern(arguments: serde_json::Value, bestehend: &str) -> Result<UpdatePlan, String> {
    plan_update(
        &config(),
        &kalender(),
        &arguments,
        bestehend,
        "termin-1@nextcloud",
    )
}

fn loeschen(arguments: serde_json::Value, bestehend: &str) -> Result<DeletePlan, String> {
    plan_delete(
        &config(),
        &kalender(),
        &arguments,
        bestehend,
        "termin-1@nextcloud",
    )
}

#[test]
fn nur_der_titel_aendert_sich_und_der_rest_bleibt_unangetastet() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Teammeeting Montag"}),
        AUS_NEXTCLOUD,
    )
    .expect("der Plan muss sich bilden lassen");
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(neu.get("SUMMARY").as_deref(), Some("Teammeeting Montag"));
    // Alles, was Mimir nicht ändern sollte, ist noch da.
    assert_eq!(neu.get("LOCATION").as_deref(), Some("Raum 2"));
    assert_eq!(
        neu.get("DESCRIPTION").as_deref(),
        Some("Kurze Vorbereitung")
    );
    assert!(neu.has("CATEGORIES"), "die Kategorie ging verloren");
    assert!(neu.has_component("VALARM"), "die Erinnerung ging verloren");
    assert!(!neu.has("RRULE"), "eine Serie darf nicht entstehen");
    assert_eq!(neu.get("UID").as_deref(), Some("termin-1@nextcloud"));
    assert_eq!(neu.get("DTSTART").as_deref(), Some("20260920T090000"));
    // Der Zeitzonenblock ist unberührt.
    assert!(
        plan.nachher.contains("TZID:Europe/Berlin"),
        "{}",
        plan.nachher
    );
    assert!(
        plan.nachher.contains("TZOFFSETFROM:+0200"),
        "{}",
        plan.nachher
    );
}

#[test]
fn zeitstempel_und_fortlaufnummer_gehen_auf_den_neuen_stand() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Neu"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(
        neu.get("SEQUENCE").as_deref(),
        Some("4"),
        "die Fortlaufnummer blieb"
    );
    assert_ne!(neu.get("DTSTAMP").as_deref(), Some("20260901T120000Z"));
}

#[test]
fn ein_neuer_beginn_behaelt_die_dauer() {
    // „Schieb auf morgen 14 Uhr“ darf den Termin nicht auf eine Stunde
    // einkürzen, weil das alte Ende nicht mitgeschickt wurde.
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "start": "morgen 14:00"}),
        AUS_NEXTCLOUD,
    )
    .expect("Verschieben muss gehen");
    let neu = Ics::parse(&plan.nachher).unwrap();

    let beginn = neu.get("DTSTART").expect("DTSTART");
    let ende = neu.get("DTEND").expect("DTEND");
    assert_eq!(
        zeitpunkt(&ende) - zeitpunkt(&beginn),
        90 * 60,
        "die Dauer wurde nicht gehalten: {beginn} bis {ende}"
    );

    // Und die Zone ist weg, weil der Wert jetzt UTC ist.
    assert!(!plan.nachher.contains("DTSTART;TZID"), "{}", plan.nachher);
    assert!(plan.nachher.contains("DTSTART:20"), "{}", plan.nachher);
}

#[test]
fn ein_bestimmtes_ende_wird_gesetzt() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "start": "morgen 14:00", "end": "morgen 16:00"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    let beginn = neu.get("DTSTART").unwrap();
    let ende = neu.get("DTEND").unwrap();
    assert!(
        beginn.ends_with('Z') && ende.ends_with('Z'),
        "{beginn} / {ende}"
    );
    assert_eq!(
        neu.get("DTEND").unwrap().len(),
        neu.get("DTSTART").unwrap().len(),
        "die Zeiten haben verschiedene Länge: {beginn} / {ende}"
    );
}

#[test]
fn ein_ort_und_eine_beschreibung_werden_ersetzt_oder_entfernt() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "location": "Raum 5", "description": "Mitbringen: Notizbuch"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(neu.get("LOCATION").as_deref(), Some("Raum 5"));
    assert_eq!(
        neu.get("DESCRIPTION").as_deref(),
        Some("Mitbringen: Notizbuch")
    );

    // Und mit einem leeren Ort bzw. leerer Beschreibung verschwinden sie.
    let leer = aendern(
        json!({"uid": "termin-1@nextcloud", "location": "", "description": ""}),
        AUS_NEXTCLOUD,
    );
    let plan = leer.expect("leerer Ort ist erlaubt");
    let neu = Ics::parse(&plan.nachher).unwrap();
    assert!(!neu.has("LOCATION"), "der Ort blieb: {}", plan.nachher);
    assert!(
        !neu.has("DESCRIPTION"),
        "die Beschreibung blieb: {}",
        plan.nachher
    );

    // `null` dagegen heißt: nichts angefasst. Sonst würde ein Aufruf, der nur den
    // Titel nennt und aus Gewohnheit `description: null` mitschickt, die
    // Beschreibung verlieren.
    let nur_titel = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Neu", "description": null}),
        AUS_NEXTCLOUD,
    )
    .expect("nur der Titel wird geändert");
    let neu = Ics::parse(&nur_titel.nachher).unwrap();
    assert_eq!(
        neu.get("DESCRIPTION").as_deref(),
        Some("Kurze Vorbereitung")
    );
}

#[test]
fn eine_serie_wird_nicht_geaendert() {
    let serie = AUS_NEXTCLOUD.replace(
        "CATEGORIES:Arbeit",
        "CATEGORIES:Arbeit\r\nRRULE:FREQ=WEEKLY",
    );
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "start": "morgen 14:00"}),
        &serie,
    )
    .expect_err("eine Serie bleibt unberührt");

    assert!(fehler.contains("Serie"), "{fehler}");
    assert!(fehler.contains("Nextcloud"), "{fehler}");
}

#[test]
fn ein_termin_mit_teilnehmern_wird_weder_geaendert_noch_geloescht() {
    let mit_einladung = AUS_NEXTCLOUD.replace(
        "SUMMARY:Teammeeting",
        "ATTENDEE;CN=Kai:mailto:kai@example.org\r\nSUMMARY:Teammeeting",
    );

    let beim_aendern = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Anderes"}),
        &mit_einladung,
    )
    .expect_err("ein Termin mit Beteiligten wird nicht geändert");
    assert!(beim_aendern.contains("Absage"), "{beim_aendern}");

    let beim_loeschen = loeschen(json!({"uid": "termin-1@nextcloud"}), &mit_einladung)
        .expect_err("ein Termin mit Beteiligten wird nicht gelöscht");
    assert!(beim_loeschen.contains("Absage"), "{beim_loeschen}");
}

#[test]
fn ein_löschen_braucht_die_kennung_und_zeigt_den_termin() {
    let plan = loeschen(json!({"uid": "termin-1@nextcloud"}), AUS_NEXTCLOUD)
        .expect("das Löschen muss vorbereitet werden können");

    assert_eq!(plan.calendar_display, "Arbeit");
    assert!(plan.summary.contains("Teammeeting"), "{}", plan.summary);
    assert!(
        plan.summary.contains("nicht zurücknehmen"),
        "{}",
        plan.summary
    );
    // Die Vorschau zeigt die Datei, die verschwindet.
    assert!(plan.vorher.contains("SUMMARY:Teammeeting"));
    assert!(
        plan.vorher.contains("BEGIN:VALARM"),
        "was verschwindet, muss sichtbar sein"
    );
}

#[test]
fn ein_termin_lässt_sich_über_kennung_oder_titel_benennen() {
    // Der Titel ist der Fall aus dem Betrieb: Das Modell kannte nur den Titel und
    // schickte ihn als Kennung. Mit beiden Wegen kommt es an.
    assert_eq!(
        kennung_aus(&json!({"uid": " a@b "})).as_deref(),
        Some("a@b")
    );
    assert!(kennung_aus(&json!({"uid": ""})).is_none());
    assert!(kennung_aus(&json!({"uid": "   "})).is_none());
    assert!(kennung_aus(&json!({"title": "angelegt durch KI"})).is_none());

    assert!(pruefe_auswahl(&json!({"uid": "a@b"})).is_ok());
    assert!(pruefe_auswahl(&json!({"title": "angelegt durch KI"})).is_ok());

    // Wird gar nichts genannt, fragt Mimir nach – mit beiden Wegen im Text.
    let fehler = pruefe_auswahl(&json!({})).unwrap_err();
    assert!(fehler.contains("title"), "{fehler}");
    assert!(fehler.contains("list_calendar_events"), "{fehler}");
}

#[test]
fn ein_titel_statt_einer_kennung_gewinnt_beim_als_kennung_geschickten_titel() {
    // Genau das ist schiefgegangen: „angelegt durch KI“ war der Titel und stand
    // im Feld uid. Solange die Zeichen kein Steuerzeichen sind, wird es als
    // Kennung genommen – und der Aufruf scheitert mit einer verständlichen
    // Meldung statt still zu raten.
    let plan = aendern(
        json!({"uid": "angelegt durch KI", "title": "Neu"}),
        AUS_NEXTCLOUD,
    )
    .expect_err("eine fremde Kennung schlägt fehl");

    assert!(plan.contains("Kennung"), "{plan}");
}

#[test]
fn ein_datum_als_bei_mehreren_gleichnamigen_terminen() {
    // „morgen“ und „2026-10-01“ müssen denselben Tag ergeben.
    let morgen = (Local::now() + Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    assert_eq!(
        tag_aus(&morgen),
        Some(Local::now().date_naive() + Duration::days(1))
    );
    assert_eq!(
        tag_aus("morgen"),
        Some(Local::now().date_naive() + Duration::days(1))
    );
    assert_eq!(
        tag_aus("übermorgen"),
        Some(Local::now().date_naive() + Duration::days(2))
    );
    assert!(tag_aus("irgendwann mal").is_none());
}

#[test]
fn ein_titel_ohne_datum_wird_zur_eindeutigen_kennung() {
    // Der Fall, der vorher scheiterte: Es gibt genau einen Termin mit diesem
    // Titel, also braucht es keine Kennung.
    let plan = aendern(
        json!({"title": "Teammeeting", "summary": "Montag"}),
        AUS_NEXTCLOUD,
    );
    let plan = plan.expect("der Titel allein genügt bei einem eindeutigen Treffer");

    assert!(plan.summary.contains("Montag"), "{}", plan.summary);
}

#[test]
fn eine_kennung_mit_pfdadzeichen_der_caldav_nicht_verträgt_wird_kodiert() {
    assert_eq!(datei_name("a b@x"), "a%20b@x");
    assert_eq!(datei_name("a/b"), "a%2Fb");
    assert_eq!(datei_name("a?b#c%d"), "a%3Fb%23c%25d");
}

#[test]
fn eine_fremde_kennung_im_auftrag_wird_abgelehnt() {
    // Der Aufruf nennt eine andere Kennung als der geladene Termin: Das darf
    // nicht stillschweigend übersehen werden.
    let fehler = aendern(json!({"uid": "anderer@x", "summary": "Neu"}), AUS_NEXTCLOUD).unwrap_err();
    assert!(fehler.contains("Kennung"), "{fehler}");

    let fehler = loeschen(json!({"uid": "anderer@x"}), AUS_NEXTCLOUD).unwrap_err();
    assert!(fehler.contains("Kennung"), "{fehler}");
}

#[test]
fn unbekannte_felder_beim_aendern_werden_namentlich_abgelehnt() {
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "titel": "Tippfehler"}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("titel"), "{fehler}");
    assert!(
        fehler.contains("uid"),
        "die erlaubten Felder werden genannt: {fehler}"
    );
}

#[test]
fn ohne_eine_eingennbare_aenderung_wird_nichts_geschrieben() {
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "calendar": "Arbeit"}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("nichts zum Ändern"), "{fehler}");
}

#[test]
fn ein_vergleiches_feld_zaehlt_als_keine_aenderung() {
    // Dasselbe noch einmal zu senden ist keine Änderung: Es soll nicht als
    // Erfolg durchgehen und einen Schreibvorgang vortäuschen.
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Teammeeting", "location": "Raum 2"}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("nichts zum Ändern"), "{fehler}");
}

#[test]
fn ein_termin_in_der_vergangenheit_wird_nicht_verschoben() {
    let vor_drei_wochen = (Local::now() - Duration::days(21))
        .format("%Y-%m-%dT09:00")
        .to_string();
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "start": vor_drei_wochen}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("Vergangenheit"), "{fehler}");

    // Und ein Jahr weit weg von jeder Plausibilität fällt schon bei der
    // Jahreszahl durch, nicht erst bei der Vergangenheitsprüfung.
    assert!(aendern(
        json!({"uid": "termin-1@nextcloud", "start": "2020-03-04T09:00"}),
        AUS_NEXTCLOUD
    )
    .is_err());
}

#[test]
fn die_zusammenfassung_nennt_was_sich_aendert() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Teammeeting Montag", "location": "Raum 5"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();

    // Die Zusammenfassung ersetzt die Felder Zeile für Zeile: So sieht der
    // Benutzer im Bestätigungsfenster, was sich ändert, ohne die Datei zu lesen.
    assert!(
        plan.summary.contains("Titel: Teammeeting Montag"),
        "{}",
        plan.summary
    );
    assert!(plan.summary.contains("Ort: Raum 5"), "{}", plan.summary);
    assert!(plan.summary.contains("Kalender Arbeit"), "{}", plan.summary);
    assert!(
        plan.summary.contains("nicht zurücknehmen"),
        "{}",
        plan.summary
    );
}

#[test]
fn bei_mehreren_kalendern_wird_der_kalender_verlangt() {
    let zwei = vec![
        ("arbeit".to_string(), "Arbeit".to_string()),
        ("privat".to_string(), "Privat".to_string()),
    ];
    let mut config = config();
    config.calendars = vec!["arbeit".to_string(), "privat".to_string()];

    let ohne = plan_update(
        &config,
        &zwei,
        &json!({"title": "Teammeeting", "summary": "Neu"}),
        AUS_NEXTCLOUD,
        "termin-1@nextcloud",
    )
    .unwrap_err();
    assert!(ohne.contains("Arbeit") && ohne.contains("Privat"), "{ohne}");

    let mit = plan_update(
        &config,
        &zwei,
        &json!({"uid": "termin-1@nextcloud", "summary": "Neu", "calendar": "Privat"}),
        AUS_NEXTCLOUD,
        "termin-1@nextcloud",
    )
    .expect("mit Angabe des Kalenders geht es");
    assert_eq!(mit.calendar_display, "Privat");
    assert!(mit.summary.contains("Kalender Privat"), "{}", mit.summary);
}

#[test]
fn ein_unbekannter_kalender_wird_abgelehnt() {
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Neu", "calendar": "Kanzlei"}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("Kanzlei"), "{fehler}");
    assert!(
        fehler.contains("Arbeit"),
        "die vorhandenen Kalender werden genannt: {fehler}"
    );
}

#[test]
fn ohne_kalender_wird_nichts_angefasst() {
    let mut config = config();
    config.calendars.clear();

    let fehler = plan_update(
        &config,
        &[],
        &json!({"uid": "termin-1@nextcloud", "summary": "Neu"}),
        AUS_NEXTCLOUD,
        "termin-1@nextcloud",
    )
    .unwrap_err();

    assert!(fehler.contains("/calendar"), "{fehler}");
}

#[test]
fn ein_ganztagestermin_behaelt_sein_aussehen() {
    let ganztag = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:g1\r\nDTSTAMP:20260901T120000Z\r\nDTSTART;VALUE=DATE:20260922\r\nDTEND;VALUE=DATE:20260923\r\nSUMMARY:Urlaub\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    let plan = plan_update(
        &config(),
        &kalender(),
        &json!({"uid": "g1", "start": "2026-10-05"}),
        ganztag,
        "g1",
    )
    .expect("ein Ganztagestermin lässt sich verschieben");

    let neu = Ics::parse(&plan.nachher).unwrap();
    assert_eq!(neu.get("DTSTART").as_deref(), Some("20261005"));
    assert_eq!(
        neu.get("DTEND").as_deref(),
        Some("20261006"),
        "die Dauer ging verloren"
    );
    assert!(
        plan.nachher.contains("DTSTART;VALUE=DATE:20261005"),
        "{}",
        plan.nachher
    );
    // Der Wert darf kein `VALUE=DATE` enthalten: Das ist ein Parameter.
    assert!(
        !plan.nachher.contains("DTSTART:VALUE=DATE"),
        "{}",
        plan.nachher
    );
    assert!(Ics::parse(&plan.nachher).unwrap().ist_datum("DTSTART"));
}

/// Baut einen Termin für die Auswahl-Tests, mit einer festen Ortszeit.
fn termin(
    uid: &str,
    titel: &str,
    kalender: &str,
    jahr: i32,
    monat: u32,
    tag: u32,
    stunde: u32,
) -> crate::calendar::CalendarEvent {
    let lokal = chrono::Local
        .with_ymd_and_hms(jahr, monat, tag, stunde, 0, 0)
        .single()
        .expect("Ortszeit muss bildbar sein");

    crate::calendar::CalendarEvent {
        uid: uid.to_string(),
        summary: titel.to_string(),
        location: String::new(),
        start: lokal.with_timezone(&chrono::Utc).timestamp(),
        end: (lokal + chrono::Duration::hours(1))
            .with_timezone(&chrono::Utc)
            .timestamp(),
        all_day: false,
        floating: false,
        calendar: kalender.to_string(),
        calendar_href: "/calendars/kai/arbeit/".to_string(),
        reminder: None,
        reminder_text: String::new(),
        categories: Vec::new(),
        canceled: false,
    }
}

#[test]
fn eine_uhrzeit_aus_den_worten_des_benutzers_wird_gelesen() {
    let gestern = auswahl_aus("gestern 14:00");
    assert_eq!(
        gestern.uhrzeit,
        Some(chrono::NaiveTime::from_hms_opt(14, 0, 0).unwrap())
    );

    let mit_uhr = auswahl_aus("morgen um 9 Uhr");
    assert_eq!(
        mit_uhr.uhrzeit,
        Some(chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap())
    );

    // Ein Punkt kann eine Uhrzeit sein …
    let punkt = auswahl_aus("30. September 14.30");
    assert_eq!(
        punkt.uhrzeit,
        Some(chrono::NaiveTime::from_hms_opt(14, 30, 0).unwrap())
    );
}

#[test]
fn ein_reines_datum_ist_keine_uhrzeit() {
    // „14. September“ ist ein Tag, keine Uhr um 14. Würde das als Uhrzeit
    // gelesen, fiele der Termin um 14 Uhr aus der Auswahl heraus.
    assert!(uhrzeit_aus("14. September").is_none());
    assert!(uhrzeit_aus("morgen").is_none());
    assert!(uhrzeit_aus("2026-09-14").is_none());
    assert!(uhrzeit_aus("gestern").is_none());
}

#[test]
fn titel_und_uhrzeit_finden_den_termin_des_benutzers() {
    // Zwei Termine namens „test“ am selben Tag: Der Benutzer nennt die
    // Uhrzeit, und genau der 14-Uhr-Termin ist gemeint.
    let frueh = termin("frueh", "test", "Privat", 2026, 9, 30, 9);
    let mittag = termin("mittag", "test", "Privat", 2026, 9, 30, 14);
    let spaet = termin("spaet", "test", "Privat", 2026, 9, 30, 16);

    let auswahl = auswahl_aus("30. September 14:00");
    let treffer: Vec<_> = [&frueh, &mittag, &spaet]
        .into_iter()
        .filter(|t| passt_zu_termin(t, "test", &auswahl, None))
        .collect();

    assert_eq!(treffer.len(), 1, "es muss genau einer sein");
    assert_eq!(treffer[0].uid, "mittag");
}

#[test]
fn ein_gesagter_kalender_grenzt_die_suche_ein() {
    // Derselbe Titel und dieselbe Uhrzeit in zwei Kalendern. Ohne den
    // Kalender des Benutzers wäre das ein Raten.
    let arbeit = termin("a", "test", "Arbeit", 2026, 9, 30, 14);
    let privat = termin("p", "test", "Privat", 2026, 9, 30, 14);

    let auswahl = auswahl_aus("30. September 14:00");

    assert!(passt_zu_termin(&privat, "test", &auswahl, Some("Privat")));
    assert!(!passt_zu_termin(&arbeit, "test", &auswahl, Some("Privat")));
    // Und beide zusammen wären eben mehrdeutig.
    let ohne = [&arbeit, &privat]
        .into_iter()
        .filter(|t| passt_zu_termin(t, "test", &auswahl, None))
        .count();
    assert_eq!(ohne, 2, "ohne Kalenderangabe bleiben beide stehen");
}

#[test]
fn der_kalendername_passt_auch_als_pfad() {
    assert!(kalender_passt("privat", "Privat"));
    assert!(kalender_passt("Privat", "privat"));
    // Aus einem href das letzte Pfadglied nehmen.
    assert!(kalender_passt(
        "personal",
        "/remote.php/dav/calendars/kai/personal/"
    ));
    // Und ein Teil des Namens genügt.
    assert!(kalender_passt("privat", "Privatkalender"));
    assert!(!kalender_passt("arbeit", "Privat"));
    assert!(!kalender_passt("", "Privat"));
}

#[test]
fn ein_absgesagter_termin_ist_nie_ein_treffer() {
    let mut t = termin("weg", "test", "Privat", 2026, 9, 30, 14);
    t.canceled = true;
    assert!(!passt_zu_termin(
        &t,
        "test",
        &Auswahl {
            tag: None,
            uhrzeit: None
        },
        None
    ));
}

/// Der Vorgang aus dem Betrieb, unveraendert: Das Modell schickte Titel, Uhrzeit
/// und Kalender, weil der Benutzer nur das sagen kann. An „gestern“ scheiterte
/// die Aufloesung, weil das Wort nirgends aufgeloest wurde – der Termin wurde
/// auf den heutigen Tag gesucht und nicht gefunden.
#[test]
fn die_angaben_des_benutzers_finden_ihren_termin() {
    let eure_angabe = json!({
        "title": "test",
        "on_date": "gestern 14:00",
        "calendar": "Privat",
    });

    let gesucht = eure_angabe["title"].as_str().unwrap().to_lowercase();
    let auswahl = auswahl_aus(eure_angabe["on_date"].as_str().unwrap());
    let kalender = eure_angabe["calendar"].as_str();

    // Der Tag kommt aus „gestern“, nicht aus dem heutigen.
    assert_eq!(
        auswahl.tag,
        Some(Local::now().date_naive() - chrono::Duration::days(1))
    );
    assert_eq!(
        auswahl.uhrzeit,
        Some(chrono::NaiveTime::from_hms_opt(14, 0, 0).unwrap())
    );

    let gestern = Local::now().date_naive() - chrono::Duration::days(1);
    let treffer = termin(
        "t1",
        "test",
        "Privat",
        gestern.year(),
        gestern.month(),
        gestern.day(),
        14,
    );

    assert!(
        passt_zu_termin(&treffer, &gesucht, &auswahl, kalender),
        "Titel, Uhrzeit und Kalender müssen den Termin finden"
    );
}

/// Ein Datum ohne Jahr darf nicht in die Zukunft springen. Aus „30. September“
/// wurde der 30. September des Folgejahres, worauf im Kalender nichts lag.
#[test]
fn ein_datum_ohne_jahr_bleibt_in_diesem_jahr() {
    let jahr = Local::now().year();
    let monat = Local::now().month();
    // Ein Tag, der in jedem Fall in der Vergangenheit liegt.
    let tag = if Local::now().day() > 1 { 1 } else { 28 };

    let aufloesung = tag_aus(&format!("{tag}. {}", monatsname(monat)));

    if let Some(datum) = aufloesung {
        assert_eq!(
            datum.year(),
            jahr,
            "„{tag}. {monat}“ darf nicht in ein anderes Jahr zeigen"
        );
    }
}

fn monatsname(monat: u32) -> &'static str {
    const NAMEN: [&str; 12] = [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ];

    NAMEN[(monat - 1) as usize]
}

/// Der genannte Kalendername muss den Zielkalender finden, sonst scheitert der
/// Vorgang nach der erfolgreichen Titelsuche an einem Namen, den der Benutzer
/// nie gesehen hat.
#[test]
fn der_genannte_kalender_findet_den_zielkalender() {
    let mit_pfad = vec![
        (
            "/remote.php/dav/calendars/kai/personal/".to_string(),
            "Persönlich".to_string(),
        ),
        (
            "/remote.php/dav/calendars/kai/arbeit/".to_string(),
            "Arbeit".to_string(),
        ),
    ];
    let mut config = config();
    config.calendars = mit_pfad.iter().map(|(href, _)| href.clone()).collect();

    // „Personal“ steht nirgends – der Anwender sagt es so.
    let (href, name) = zielkalender(&config, &mit_pfad, Some("Personal"), "t1").unwrap();
    assert_eq!(name, "Persönlich");
    assert_eq!(href, "/remote.php/dav/calendars/kai/personal/");

    // Und über den echten Namen geht es auch.
    let (_, name) = zielkalender(&config, &mit_pfad, Some("Arbeit"), "t1").unwrap();
    assert_eq!(name, "Arbeit");
}

/// Der ganze Aufruf aus dem Screenshot muss durch die Planung laufen, ohne an
/// einem Feld zu scheitern, das das Werkzeug gar nicht kennt.
#[test]
fn der_auftrag_aus_dem_betrieb_laeuft_durch() {
    let kalender = vec![
        (
            "familienkalender".to_string(),
            "Familienkalender".to_string(),
        ),
        ("personal".to_string(), "Personal".to_string()),
    ];
    let mut config = config();
    config.calendars = vec!["familienkalender".to_string(), "personal".to_string()];

    let auftrag = json!({
        "title": "angelegt durch KI",
        "on_date": "heute",
        "calendar": "Familienkalender",
        "start": "heute 15 Uhr"
    });

    let plan = plan_update(&config, &kalender, &auftrag, AUS_NEXTCLOUD, "t1")
        .expect("der Auftrag muss sich planen lassen");

    let neu = Ics::parse(&plan.nachher).unwrap();
    // Das Datum wird gerechnet, nicht festgeschrieben. Mit einem festen Tag
    // schlug der Test um Mitternacht fehl – „heute 15 Uhr“ war am 1. Oktober
    // der 1. Oktober und am 2. Oktober der 2. Oktober, der Test behauptete
    // weiterhin den 1. Oktober. 15:00 Ortszeit ist in der Sommerzeit 13:00Z.
    let erwartet = (Local::now().date_naive() + chrono::Duration::days(0))
        .and_hms_opt(13, 0, 0)
        .expect("13:00")
        .and_utc()
        .format("%Y%m%dT%H%M%SZ")
        .to_string();
    assert_eq!(
        neu.get("DTSTART").as_deref(),
        Some(erwartet.as_str()),
        "15:00 Ortszeit ist 13:00Z"
    );
    assert!(
        plan.summary.contains("Familienkalender"),
        "{}",
        plan.summary
    );
}

// --- Erinnerung und Kategorie ----------------------------------------------

#[test]
fn die_erinnerung_laesst_sich_aendern_und_der_rest_bleibt() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "reminder": "eine Stunde vorher"}),
        AUS_NEXTCLOUD,
    )
    .expect("der Plan muss sich bilden lassen");
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(neu.trigger_minuten(), Some(60));
    assert_eq!(neu.get("SUMMARY").as_deref(), Some("Teammeeting"));
    assert_eq!(neu.get("LOCATION").as_deref(), Some("Raum 2"));
    assert_eq!(neu.get("CATEGORIES").as_deref(), Some("Arbeit"));
    assert!(
        plan.summary.contains("Erinnerung: 1 Stunde vorher"),
        "{}",
        plan.summary
    );
    // Geändert wurde die Erinnerung, sie kann also nicht als „bleibt" dastehen.
    assert!(
        !plan.summary.contains("Bleiben erhalten: die Erinnerung"),
        "{}",
        plan.summary
    );
    // Die Kategorie ist unangetastet und wird deshalb genannt.
    assert!(
        plan.summary.contains("Bleiben erhalten: die Kategorien"),
        "{}",
        plan.summary
    );
}

#[test]
fn eine_erinnerung_wird_angelegt_wo_keine_ist() {
    let ohne = AUS_NEXTCLOUD
        .split("BEGIN:VALARM\\r\\n")
        .next()
        .unwrap()
        .to_string()
        + "END:VEVENT\r\nEND:VCALENDAR\r\n";

    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "reminder": "5 Minuten vorher"}),
        &ohne,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(neu.trigger_minuten(), Some(5));
    assert!(plan.nachher.contains("BEGIN:VALARM"), "{}", plan.nachher);
    // Die Benachrichtigung nennt den Termin.
    assert!(
        plan.nachher.contains("DESCRIPTION:Teammeeting"),
        "{}",
        plan.nachher
    );
}

#[test]
fn die_erinnerung_der_einen_neuen_titel_erhaelt() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "summary": "Teammeeting Montag", "reminder": "5 Minuten vorher"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();

    assert!(
        plan.nachher.contains("DESCRIPTION:Teammeeting Montag"),
        "{}",
        plan.nachher
    );
    assert!(
        !plan.nachher.contains("DESCRIPTION:Teammeeting\r\n"),
        "{}",
        plan.nachher
    );
}

#[test]
fn die_erinnerung_weg_zu_sagen_entfernt_sie() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "reminder": "keine Erinnerung"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(neu.trigger_minuten(), None);
    assert!(!plan.nachher.contains("VALARM"), "{}", plan.nachher);
    assert!(
        plan.summary.contains("Erinnerung: keine"),
        "{}",
        plan.summary
    );
    assert_eq!(neu.get("SUMMARY").as_deref(), Some("Teammeeting"));
}

#[test]
fn ohne_reminder_bleibt_die_erinnerung_stehen() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "location": "Raum 4"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(
        neu.trigger_minuten(),
        Some(15),
        "die Erinnerung ging verloren"
    );
    assert!(
        plan.summary.contains("Bleiben erhalten: die Erinnerung"),
        "{}",
        plan.summary
    );
}

#[test]
fn eine_erinnerung_die_es_schon_gibt_ist_keine_aenderung() {
    // Sonst würde Mimir eine wirkungslose Änderung als Erfolg melden.
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "reminder": "15 Minuten vorher"}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("nichts zum Ändern"), "{fehler}");
}

#[test]
fn eine_erinnerung_der_nicht_gelesen_werden_kann_wird_nicht_geraten() {
    // Genau dieser Fall ist im Betrieb schiefgegangen: Das Modell hat eine
    // Erinnerung behauptet, die es nicht gab, und dafür den Termin verschoben.
    let fehler = aendern(
        json!({"uid": "termin-1@nextcloud", "reminder": "rechtzeitig", "start": "morgen 15:00"}),
        AUS_NEXTCLOUD,
    )
    .unwrap_err();

    assert!(fehler.contains("Erinnerung"), "{fehler}");
}

#[test]
fn die_kategorie_laesst_sich_aendern_und_werden() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "category": "Arbeit, Wichtig"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert_eq!(neu.get("CATEGORIES").as_deref(), Some("Arbeit;Wichtig"));
    assert_eq!(neu.trigger_minuten(), Some(15));
    assert!(
        plan.summary.contains("Kategorien: Arbeit, Wichtig"),
        "{}",
        plan.summary
    );
    assert!(
        !plan.summary.contains("Bleiben erhalten: die Kategorien"),
        "{}",
        plan.summary
    );
}

#[test]
fn ein_leerer_text_nimmt_die_kategorie_und_die_erinnerung_weg() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "category": "", "reminder": "keine"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();
    let neu = Ics::parse(&plan.nachher).unwrap();

    assert!(!neu.has("CATEGORIES"), "{}", plan.nachher);
    assert_eq!(neu.trigger_minuten(), None);
    assert_eq!(neu.get("SUMMARY").as_deref(), Some("Teammeeting"));
}

#[test]
fn erinnerung_und_kategorie_kommen_in_der_zusammenfassung_einzeln() {
    let plan = aendern(
        json!({"uid": "termin-1@nextcloud", "reminder": "am Vorabend", "category": "Termin"}),
        AUS_NEXTCLOUD,
    )
    .unwrap();

    assert!(
        plan.summary.contains("Erinnerung: am Vorabend"),
        "{}",
        plan.summary
    );
    assert!(
        plan.summary.contains("Kategorien: Termin"),
        "{}",
        plan.summary
    );
    assert!(
        !plan.summary.contains("Bleiben erhalten"),
        "es bleibt nichts übrig: {}",
        plan.summary
    );
}

// --- Fenster für einen Termin ----------------------------------------------

#[test]
fn die_felder_stehen_so_da_wie_im_kalender() {
    let details = termin_details(AUS_NEXTCLOUD, "Arbeit", "/arbeit/").unwrap();

    assert_eq!(details.summary, "Teammeeting");
    assert_eq!(details.location, "Raum 2");
    assert_eq!(details.description, "Kurze Vorbereitung");
    assert_eq!(details.categories, "Arbeit");
    assert_eq!(details.reminder, "15 Minuten vorher");
    assert_eq!(details.calendar, "Arbeit");
    assert_eq!(details.calendar_href, "/arbeit/");
    assert!(!details.all_day);
    assert!(
        details.gesperrt.is_empty(),
        "der Termin ist frei: {}",
        details.gesperrt
    );
}

#[test]
fn die_zeit_steht_in_der_zone_des_rechners() {
    let details = termin_details(AUS_NEXTCLOUD, "Arbeit", "/arbeit/").unwrap();

    // Der Kalender schreibt 09:00 in Europe/Berlin. Je nachdem, wo der Rechner
    // steht, ist das 07:00 oder 08:00 UTC – in der Zone des Rechners aber immer
    // 09:00. Genau das soll das Formular zeigen.
    let erwartet = super::liese_zeit(&Ics::parse(AUS_NEXTCLOUD).unwrap(), "DTSTART")
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%dT%H:%M")
        .to_string();

    assert_eq!(details.start, erwartet);
    assert!(details.start.ends_with("T09:00"), "{}", details.start);
}

#[test]
fn was_unveraendert_zurueckkommt_aendert_auch_nichts() {
    // Der wichtigste Fall: Wer das Fenster öffnet und nur auf „Abbrechen"
    // klickt, darf den Kalender nicht verändert haben. Und wer nur den Titel
    // ändert, darf keine Zeitänderung zu sehen bekommen.
    let details = termin_details(AUS_NEXTCLOUD, "Arbeit", "/arbeit/").unwrap();
    let plan = aendern(
        json!({
            "uid": details.uid,
            "summary": "Teammeeting Montag",
            "start": details.start,
            "end": details.end,
            "all_day": false,
            "location": details.location,
            "description": details.description,
            "category": details.categories,
            "reminder": details.reminder,
            "calendar": "/arbeit/",
        }),
        AUS_NEXTCLOUD,
    )
    .expect("die Übernahme muss klappen");

    assert!(!plan.summary.contains("Zeit:"), "{}", plan.summary);
    assert!(!plan.summary.contains("Erinnerung:"), "{}", plan.summary);
    assert!(!plan.summary.contains("Kategorien:"), "{}", plan.summary);
    assert_eq!(
        plan.summary.matches("Titel:").count(),
        1,
        "{}",
        plan.summary
    );

    // Und die Zeit steht unangetastet drin, inklusive des Zeitzonenbezugs.
    assert!(
        plan.nachher
            .contains("DTSTART;TZID=Europe/Berlin:20260920T090000"),
        "{}",
        plan.nachher
    );
}

#[test]
fn ein_ganztagestermin_zeigt_das_ende_als_datum() {
    let ganztag = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:g1\r\n\
                  DTSTAMP:20260901T120000Z\r\nDTSTART;VALUE=DATE:20260920\r\n\
                  DTEND;VALUE=DATE:20260923\r\nSUMMARY:Urlaub\r\nEND:VEVENT\r\n\
                  END:VCALENDAR\r\n";
    let details = termin_details(ganztag, "Arbeit", "/arbeit/").unwrap();

    assert!(details.all_day);
    assert_eq!(details.start, "2026-09-20");
    // Das Ende steht exklusiv in der Datei – das Formular soll aber zeigen, was
    // der Benutzer sieht, und das ist der letzte Tag. Deshalb wird hier nichts
    // gerechnet: Der Benutzer ändert das Feld auf 2026-09-23 und Mimir rechnet
    // es beim Speichern korrekt zurück.
    assert_eq!(details.end, "2026-09-23");
}

#[test]
fn ein_gesperrter_termin_sagt_warum() {
    let serie = AUS_NEXTCLOUD.replace("SUMMARY:Teammeeting", "SUMMARY:Serie\r\nRRULE:FREQ=WEEKLY");
    assert!(termin_details(&serie, "Arbeit", "/arbeit/")
        .unwrap()
        .gesperrt
        .contains("Serie"));

    let mit_teilnehmern = AUS_NEXTCLOUD.replace(
        "SUMMARY:Teammeeting",
        "SUMMARY:Vorstellungsgespräch\r\nATTENDEE:mailto:kai@example.org",
    );
    assert!(termin_details(&mit_teilnehmern, "Arbeit", "/arbeit/")
        .unwrap()
        .gesperrt
        .contains("beteiligt"));
}

#[test]
fn eine_erinnerung_die_nicht_gelesen_werden_kann_verschwindet_nicht_still() {
    // Sie steht in der Datei, aber ihr Wert ist keiner, den Mimir rechnen kann.
    // Beim Speichern dürfte sie nicht unbemerkt verloren gehen.
    let fremd = AUS_NEXTCLOUD.replace("TRIGGER:-PT15M", "TRIGGER:20260920T084500Z");
    let details = termin_details(&fremd, "Arbeit", "/arbeit/").unwrap();

    assert!(
        details.reminder.contains("nicht lesbar"),
        "{}",
        details.reminder
    );
}
