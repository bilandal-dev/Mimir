//! Tests für das Bearbeiten einer Termin-Datei.
//!
//! Der wichtigste Fall ist der letzte: Wer einen Termin ändert, darf nicht
//! dabei fremde Felder verlieren. Erinnerungen, Kategorien, Anlagen und
//! Teilnehmer gehören dem Kalender, nicht Mimir.

use super::*;

const TERMIN: &str = "BEGIN:VCALENDAR\r\n\
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
UID:abc-123@mimir\r\n\
DTSTAMP:20260901T120000Z\r\n\
CREATED:20260901T120000Z\r\n\
LAST-MODIFIED:20260901T120000Z\r\n\
SEQUENCE:0\r\n\
DTSTART;TZID=Europe/Berlin:20260914T090000\r\n\
DTEND;TZID=Europe/Berlin:20260914T100000\r\n\
SUMMARY:Zahnarzt\r\n\
LOCATION:Praxis Dr. Klein\r\n\
CATEGORIES:Gesundheit,Termin\r\n\
RRULE:FREQ=WEEKLY;COUNT=6\r\n\
BEGIN:VALARM\r\n\
TRIGGER:-PT30M\r\n\
ACTION:DISPLAY\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn ein_termin_wird_gelesen_und_nicht_verdreht() {
    let ics = Ics::parse(TERMIN).expect("die Datei ist lesbar");

    assert_eq!(ics.get("UID").as_deref(), Some("abc-123@mimir"));
    assert_eq!(ics.get("SUMMARY").as_deref(), Some("Zahnarzt"));
    assert_eq!(ics.get("LOCATION").as_deref(), Some("Praxis Dr. Klein"));
    assert!(ics.has("RRULE"), "die Serie wurde nicht erkannt");
    assert!(
        ics.has_component("VALARM"),
        "die Erinnerung wurde nicht erkannt"
    );
    assert!(!ics.has("ATTENDEE"), "es sind niemanden eingeladen");
}

#[test]
fn ein_eingesetzter_wert_ersetzt_den_alten_und_der_rest_bleibt() {
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.set("SUMMARY", "Zahnarzt Dr. Klein");
    let text = ics.to_text();

    assert!(text.contains("SUMMARY:Zahnarzt Dr. Klein"), "{text}");
    assert!(
        !text.contains("SUMMARY:Zahnarzt\r\n"),
        "der alte Wert blieb: {text}"
    );
    // Alles, was Mimir nichts angeht, bleibt unverändert erhalten.
    assert!(text.contains("UID:abc-123@mimir"), "{text}");
    assert!(text.contains("CATEGORIES:Gesundheit,Termin"), "{text}");
    assert!(text.contains("RRULE:FREQ=WEEKLY;COUNT=6"), "{text}");
    assert!(text.contains("BEGIN:VALARM"), "{text}");
    assert!(text.contains("TRIGGER:-PT30M"), "{text}");
    assert!(text.contains("CREATED:20260901T120000Z"), "{text}");
}

#[test]
fn der_zeitzonenbezug_bleibt_beim_aendern_der_zeit_erhalten() {
    // Ohne den Parameter TZID wandert der Termin: 09:00 in der Zonenzeit des
    // Rechners statt in der des Kalenders. Deshalb bleiben Parameter stehen.
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.set("DTSTART", "20260914T140000");
    let text = ics.to_text();

    assert!(
        text.contains("DTSTART;TZID=Europe/Berlin:20260914T140000"),
        "{text}"
    );
    assert!(
        text.contains("DTEND;TZID=Europe/Berlin:20260914T100000"),
        "{text}"
    );
}

#[test]
fn eine_fehlende_eigenschaft_wird_eingesetzt() {
    let einfach = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:x\r\nDTSTART:20260914T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut ics = Ics::parse(einfach).unwrap();
    ics.set("LOCATION", "Rathaus");
    let text = ics.to_text();

    assert!(text.contains("LOCATION:Rathaus"), "{text}");
    // Und zwar vor END:VEVENT, sonst stünde es außerhalb des Termins.
    let lage = text.find("LOCATION:Rathaus").unwrap();
    let ende = text.find("END:VEVENT").unwrap();
    assert!(lage < ende, "die Zeile steht hinter dem Termin: {text}");
}

#[test]
fn eine_eigenschaft_laesst_sich_entfernen() {
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.remove("RRULE");
    let text = ics.to_text();

    assert!(!text.contains("RRULE"), "{text}");
    // Der Rest bleibt unangetastet.
    assert!(text.contains("SUMMARY:Zahnarzt"), "{text}");
    assert!(text.contains("BEGIN:VALARM"), "{text}");
    assert!(text.contains("END:VEVENT"), "{text}");
    // Und der Block bleibt zusammenpassend.
    let neu = Ics::parse(&text).expect("die Datei bleibt lesbar");
    assert!(!neu.has("RRULE"));
}

#[test]
fn gefaltete_zeilen_werden_zuerst_entfaltet() {
    // Nextcloud und andere Kalender falten lange Zeilen. Wer sie nicht
    // entfaltet, schreibt eine kaputte Datei.
    // Die Fortsetzungszeichen stehen bewusst als "\r\n " in einer Zeile: Ein
    // Zeilenumbruch im Quelltext würde den führenden Leerraum der Fortsetzung
    // verschlucken, und dann wäre die Datei nicht wirklich gefaltet.
    let gefaltet = concat!(
        "BEGIN:VCALENDAR\r\n",
        "VERSION:2.0\r\n",
        "BEGIN:VEVENT\r\n",
        "UID:x\r\n",
        "DESCRIPTION:Das ist ein sehr langer Text, der in der Datei über mehrere physi",
        "\r\n sche Zeilen fortgesetzt wurde und trotzdem ein einziges Feld ist.\r\n",
        "SUMMARY:Kurz\r\n",
        "END:VEVENT\r\n",
        "END:VCALENDAR\r\n",
    );

    assert!(
        gefaltet.contains("\r\n sche"),
        "die Vorlage ist nicht gefaltet"
    );
    let mut ics = Ics::parse(gefaltet).unwrap();
    let lang = ics.get("DESCRIPTION").expect("Beschreibung");
    assert!(lang.starts_with("Das ist ein sehr langer Text"), "{lang}");
    assert!(lang.ends_with("Feld ist."), "{lang}");
    assert!(!lang.contains("\r"), "die Fortsetzung blieb drin: {lang}");

    ics.set("SUMMARY", "Neu");
    let text = ics.to_text();
    assert!(
        !text.contains("\r\n  sche"),
        "die Faltung wurde nicht zurückgenommen: {text}"
    );
    let neu = Ics::parse(&text).unwrap();
    assert_eq!(neu.get("SUMMARY").as_deref(), Some("Neu"));
    assert_eq!(neu.get("DESCRIPTION").as_deref(), Some(lang.as_str()));
}

#[test]
fn das_zurueckschreiben_haelt_die_zeilengrenze_ein() {
    let lang = "B".repeat(300);
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.set("DESCRIPTION", &lang);
    let text = ics.to_text();

    for zeile in text.split("\r\n") {
        assert!(
            zeile.len() <= 75,
            "Zeile zu breit ({} Zeichen): {zeile}",
            zeile.len()
        );
    }

    // Und der Inhalt kommt vollständig an.
    let neu = Ics::parse(&text).unwrap();
    assert_eq!(neu.get("DESCRIPTION").as_deref(), Some(lang.as_str()));
}

#[test]
fn ein_umlaut_wird_nicht_zerrissen() {
    let text = format!("SUMMARY:{}", "ä".repeat(60));
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.set("SUMMARY", &text[8..]);
    let neu = Ics::parse(&ics.to_text()).unwrap();

    assert_eq!(neu.get("SUMMARY").as_deref(), Some("ä".repeat(60).as_str()));
}

#[test]
fn ein_zeitzonenblock_bleibt_unangetastet() {
    // Der VTIMEZONE steht vor dem Termin. Ein Werkzeug, das die erste Zeile mit
    // BEGIN: greift, würde den Kalender in Stücke reißen.
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.set("SUMMARY", "Anderer Titel");
    let text = ics.to_text();

    assert!(text.contains("TZID:Europe/Berlin"), "{text}");
    assert!(text.contains("TZOFFSETFROM:+0200"), "{text}");
    assert!(
        text.find("BEGIN:VTIMEZONE") < text.find("BEGIN:VEVENT"),
        "{text}"
    );
    // Der Zeitzonenblock darf keinen Termin bekommen: Genau darüber ist dieser
    // Test entstanden.
    assert!(
        text.contains("DTSTART:19701025T030000"),
        "der Zeitzonenblock wurde verändert: {text}"
    );
    assert!(
        text.contains("DTSTART;TZID=Europe/Berlin:20260914T090000"),
        "{text}"
    );
}

#[test]
fn eine_datei_ohne_termin_wird_abgelehnt() {
    // Eine kaputte Datei stillschweigend zu „reparieren“ wäre schlimmer als ein
    // Fehler: Der Benutzer wüsste nicht, was passiert.
    assert!(Ics::parse("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n").is_err());
    assert!(Ics::parse("").is_err());
    assert!(Ics::parse("BEGIN:VEVENT\r\nUID:x\r\n").is_err());
}

// --- Erinnerung -------------------------------------------------------------

#[test]
fn die_erinnerung_wird_gelesen_und_nicht_die_des_termins() {
    // Zwei gleichnamige Zeilen: `DESCRIPTION` steht am Termin und in der
    // Erinnerung. Ohne den Umweg über den Block träfe der Zugriff die falsche.
    let ics = Ics::parse(TERMIN).unwrap();

    assert_eq!(ics.trigger_minuten(), Some(30));
    assert_eq!(
        ics.get("DESCRIPTION"),
        None,
        "der Termin hat keine Beschreibung"
    );
}

#[test]
fn die_erinnerung_wird_ersetzt_und_der_rest_des_blockes_bleibt() {
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.setze_erinnerung("-PT15M", "Zahnarzt");
    let text = ics.to_text();

    assert!(text.contains("TRIGGER:-PT15M"), "{text}");
    assert!(
        !text.contains("TRIGGER:-PT30M"),
        "der alte Wert blieb: {text}"
    );
    assert!(
        text.contains("ACTION:DISPLAY"),
        "das wurde vorher nie gelesen: {text}"
    );
    // Der Termin selbst bleibt unangetastet.
    assert!(text.contains("SUMMARY:Zahnarzt"), "{text}");
    assert!(
        text.contains("DTSTART;TZID=Europe/Berlin:20260914T090000"),
        "{text}"
    );
    assert!(text.contains("CATEGORIES:Gesundheit,Termin"), "{text}");
}

#[test]
fn eine_erinnerung_wird_angelegt_wo_noch_keine_ist() {
    let ohne = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\n\
                SUMMARY:Zahnarzt\r\nDTSTART:20260914T090000Z\r\nDTEND:20260914T100000Z\r\n\
                END:VEVENT\r\nEND:VCALENDAR\r\n";

    let mut ics = Ics::parse(ohne).unwrap();
    assert_eq!(ics.trigger_minuten(), None);

    ics.setze_erinnerung("-PT1H", "Zahnarzt");
    let text = ics.to_text();

    assert_eq!(ics.trigger_minuten(), Some(60));
    assert!(text.contains("BEGIN:VALARM"), "{text}");
    assert!(text.contains("TRIGGER:-PT1H"), "{text}");
    assert!(text.contains("DESCRIPTION:Zahnarzt"), "{text}");
    // Der Block gehört in den Termin.
    assert!(
        text.find("BEGIN:VEVENT") < text.find("BEGIN:VALARM"),
        "{text}"
    );
    assert!(text.find("END:VALARM") < text.find("END:VEVENT"), "{text}");
    // Und genau eine Erinnerung, keine zwei.
    assert_eq!(text.matches("BEGIN:VALARM").count(), 1, "{text}");
}

#[test]
fn eine_erinnerung_wird_entfernt_und_nichts_dannt_so_herum() {
    let mut ics = Ics::parse(TERMIN).unwrap();
    assert!(ics.entferne_erinnerung());
    let text = ics.to_text();

    assert!(!text.contains("VALARM"), "{text}");
    assert!(!text.contains("TRIGGER"), "{text}");
    assert!(text.contains("SUMMARY:Zahnarzt"), "{text}");
    assert!(text.contains("CATEGORIES:Gesundheit,Termin"), "{text}");
    assert_eq!(ics.trigger_minuten(), None);
    // Ein zweites Entfernen ist kein Fehler, aber auch kein Erfolg.
    assert!(!ics.entferne_erinnerung());
}

#[test]
fn eine_halbe_erinnerung_wird_beim_anfassen_geschlossen() {
    // Nextcloud kann eine Datei mit BEGIN:VALARM ohne END:VALARM hinterlassen.
    // Eine zweite daneben einzufügen wäre stiller Datenverlust.
    let kaputt = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:A\r\n\
                  DTSTART:20260914T090000Z\r\nBEGIN:VALARM\r\nTRIGGER:-PT30M\r\n\
                  END:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut ics = Ics::parse(kaputt).unwrap();
    ics.setze_erinnerung("-PT10M", "A");
    let text = ics.to_text();

    assert_eq!(text.matches("BEGIN:VALARM").count(), 1, "{text}");
    assert_eq!(text.matches("END:VALARM").count(), 1, "{text}");
    assert_eq!(ics.trigger_minuten(), Some(10));
}

#[test]
fn ein_trigger_wert_der_nicht_gelesen_wird_ist_ein_fehler_statt_eine_annahme() {
    // Mimir schreibt nur Dauern. Ein fremder Wert darf nicht als „irgendwann
    // vorher" gelesen werden, sonst meldete Mimir eine wirkungslose Änderung.
    for wert in ["20260914T083000Z", "-P1M", "bald", ""] {
        assert_eq!(trigger_zu_minuten(wert), None, "„{wert}“ wurde gelesen");
    }

    assert_eq!(trigger_zu_minuten("-PT15M"), Some(15));
    assert_eq!(trigger_zu_minuten("-PT1H30M"), Some(90));
    assert_eq!(trigger_zu_minuten("-P1D"), Some(1440));
    assert_eq!(trigger_zu_minuten("-P2W"), Some(20160));
    assert_eq!(trigger_zu_minuten("-PT45S"), Some(0));
}

#[test]
fn eine_beschreibung_des_termins_lands_in_die_und_nicht_in_die_erinnerung() {
    // Die Erinnerung trägt eine eigene DESCRIPTION. Ohne die Trennung träfe ein
    // set() auf DESCRIPTION die Erinnerung, und der Termin bekäme keine.
    let datei = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\n\
                SUMMARY:Zahnarzt\r\nDTSTART:20260914T090000Z\r\n\
                BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Erinnerungstext\r\n\
                TRIGGER:-PT30M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    let mut ics = Ics::parse(datei).unwrap();
    assert_eq!(
        ics.get("DESCRIPTION"),
        None,
        "der Termin hat noch keine Beschreibung"
    );

    ics.set("DESCRIPTION", "Kurze Vorbereitung");
    let text = ics.to_text();

    assert_eq!(
        ics.get("DESCRIPTION").as_deref(),
        Some("Kurze Vorbereitung")
    );
    assert!(
        text.contains("DESCRIPTION:Erinnerungstext"),
        "die Erinnerung wurde überschrieben: {text}"
    );
    assert_eq!(ics.trigger_minuten(), Some(30), "{text}");
}

#[test]
fn eine_zeile_der_erinnerung_wird_beim_entfernen_nicht_erfasst() {
    // `remove` sucht im ganzen Termin. Es darf die VALARM-Zeilen nicht mit
    //nehmen, sonst verschwände die Erinnerung beim Leeren einer Eigenschaft.
    let mut ics = Ics::parse(TERMIN).unwrap();
    ics.remove("DESCRIPTION");
    let text = ics.to_text();

    assert!(text.contains("BEGIN:VALARM"), "{text}");
    assert!(!text.contains("DESCRIPTION:"), "{text}");
}
