// Prüft, ob Mimir die Wörter liest, die ein Modell tatsächlich zurückgibt –
// und was davon im Termin landen darf.
//
// Grund für beides: Das Modell soll die Worte des Benutzers unverändert
// weitergeben, weil Mimir sie selbst rechnet. Das setzt voraus, dass beide Seiten
// dieselbe Sprache sprechen – und ein Modell liefert „nächster Dienstag“, auch wenn
// der Benutzer „nächsten Dienstag“ gesagt hat. Diese Prüfung wirft genau die
// Formulierungen gegen den Parser, die in der Validierung mit einem echten
// Modell vorgekommen sind, statt gegen die, die man sich vorher ausgedacht hat.
//
// Geprüft wird über `parse_term`, weil das der Einstieg ist, den `plan_event`
// benutzt: Er probiert erst die Worte und dann das absolute Format. `parse_wann`
// allein käme beim absoluten Format an einer Stelle an, der Mimir nicht braucht –
// und der Test würde einen Fehler melden, den es in der Anwendung nicht gibt.
//
// Sie ist Teil des Repositorys, weil sie ohne Modell läuft: `parse_term` braucht
// nur einen String, und ein Modell liefert nicht immer dieselben Formen. Die
// Validierung mit dem Modell selbst bleibt die Benutzersache, die Modellauswahl
// nicht.
//
// Aufruf:  cargo test --manifest-path src-tauri/Cargo.toml modellausgaben

use chrono::{Datelike, Duration, Local, Weekday};

/// Wie viele Tage ab heute: eine Zahl, oder `Dienstag` für den nächsten noch
/// kommenden Dienstag.
#[derive(Clone, Copy)]
enum Versatz {
    Tage(i64),
    Dienstag,
}

/// Tageszeiten und die Uhrzeit, die daraus wird.
///
/// Nach dem ersten Validierungslauf: Das Modell gab „übermorgen früh" wörtlich
/// weiter und Mimir lehnte ab. Das ist jetzt behoben – der Termin entsteht. Die
/// Uhrzeiten stehen in `write.rs` an der Stelle, die sie verwendet, und werden
/// hier nicht wiederholt.
const TAGESZEITEN: &[(&str, &str)] = &[
    ("nachts", "22:00"),
    ("frühmorgens", "06:30"),
    ("früh", "08:00"),
    ("vormittag", "09:00"),
    ("vormittags", "09:00"),
    ("mittag", "12:00"),
    ("abend", "18:00"),
];

/// Was der Benutzer sagte, was ein Modell daraus machte, und was Mimir daraus
/// rechnen muss. `None` heißt: die Angabe darf nicht lesbar sein.
type Fall = (&'static str, &'static str, Option<(Versatz, &'static str)>);

const FAELLE: &[Fall] = &[
    // Unverändert durchgereicht – das soll der Normalfall sein.
    (
        "morgen um 14 Uhr",
        "morgen um 14 Uhr",
        Some((Versatz::Tage(1), "14:00")),
    ),
    // Aus dem ersten Validierungslauf: Das Modell lieferte „übermorgen früh" und
    // „Donnerstagmittag". Beides wird inzwischen gelesen – die Uhrzeit stand in
    // der Anweisung als fehlend drin und war es gar nicht.
    (
        "übermorgen früh",
        "übermorgen früh",
        Some((Versatz::Tage(2), "08:00")),
    ),
    (
        "morgen um 14 Uhr",
        "morgen um 14",
        Some((Versatz::Tage(1), "14:00")),
    ),
    (
        "nächsten Dienstag um 9:30",
        "nächster Dienstag um 9:30",
        Some((Versatz::Dienstag, "09:30")),
    ),
    // Und das Umgekehrte: Der Benutzer sagt etwas, das Mimir nicht kennt. Das
    // darf nicht geraten werden, es muss scheitern.
    ("irgendwann nächste Woche", "irgendwann nächste Woche", None),
];

#[test]
fn modellausgaben_liest_mimir() {
    let heute = Local::now().date_naive();
    let mut fehler = Vec::new();

    for (_gesagt, vom_modell, erwartung) in FAELLE {
        let ergebnis = crate::calendar::write::parse_term(vom_modell);

        match (*erwartung, ergebnis) {
            (None, Err(_)) => {}
            (None, Ok(wann)) => fehler.push(format!(
                "\"{vom_modell}\" hätte scheitern müssen, kam aber als {wann} heraus"
            )),
            (Some(_), Err(grund)) => {
                fehler.push(format!("\"{vom_modell}\" wurde nicht gelesen: {grund}"))
            }
            (Some((versatz, uhrzeit)), Ok(wann)) => {
                // `parse_term` liefert UTC, weil die Terminzeit über die
                // Kalenderschnittstelle läuft. Verglichen wird deshalb in der
                // Zeitzone des Rechners – sonst wiche jede Stunde um die
                // Sommerzeitverschiebung ab und der Test scheiterte im Sommer.
                let lokal = wann.with_timezone(&Local);
                let soll = heute + Duration::days(tage_bis(heute, versatz));
                if lokal.date_naive() != soll {
                    fehler.push(format!(
                        "\"{vom_modell}\" wurde auf {} gelesen, erwartet war {soll}",
                        lokal.date_naive()
                    ));
                }
                if lokal.format("%H:%M").to_string() != *uhrzeit {
                    fehler.push(format!(
                        "\"{vom_modell}\" wurde auf {} gelesen, erwartet war {uhrzeit}",
                        lokal.format("%H:%M")
                    ));
                }
            }
        }
    }

    assert!(
        fehler.is_empty(),
        "Mimir liest diese Modellausgaben nicht:\n{}",
        fehler.join("\n")
    );
}

/// Der nächste noch kommende Wochentag, wie der Parser ihn sucht.
fn tage_bis(heute: chrono::NaiveDate, versatz: Versatz) -> i64 {
    match versatz {
        Versatz::Tage(tage) => tage,
        Versatz::Dienstag => {
            // Der heute Wochentag selbst zählt nicht mit: „Montag" an einem
            // Montag meint den kommenden Montag, nicht den heutigen – so steht
            // es in `write.rs`. An einem Dienstag ist das deshalb eine Woche.
            let abstand = (Weekday::Tue.number_from_monday() as i64
                - heute.weekday().number_from_monday() as i64)
                .rem_euclid(7);
            if abstand == 0 {
                7
            } else {
                abstand
            }
        }
    }
}

/// Der Wochentag im Klartext ist die Form, die der Benutzer am häufigsten sagt.
///
/// Beide Fälle aus der Validierung stehen hier: die vom Benutzer gewünschte und
/// die, die ein Modell daraus machte. Sie müssen auf denselben Tag zeigen, sonst
/// legt das Modell Termine auf einen anderen Wochentag, als der Benutzer ihn
/// genannt hat – und das fällt erst in der Vorschau auf, wenn überhaupt.
#[test]
fn wochentagsformen_treffen_denselben_tag() {
    let heute = Local::now().date_naive();
    let erwartet = heute + Duration::days(tage_bis(heute, Versatz::Dienstag));

    for form in [
        "nächsten Dienstag um 9:30",
        "nächster Dienstag um 9:30",
        "Dienstag um 9:30",
        "dienstag 9:30",
    ] {
        let gelesen = crate::calendar::write::parse_term(form)
            .unwrap_or_else(|grund| panic!("„{form}“ wurde nicht gelesen: {grund}"))
            .with_timezone(&Local);

        assert_eq!(
            gelesen.date_naive(),
            erwartet,
            "„{form}“ muss den nächsten noch kommenden Dienstag meinen"
        );
        assert_eq!(
            gelesen.format("%H:%M").to_string(),
            "09:30",
            "„{form}“ verliert die Uhrzeit"
        );
    }
}

/// Jede Tageszeit wird zu der Uhrzeit, die dafür festgelegt ist.
///
/// Und nur in ganzen Wörtern: „Frühstück mit Anna" enthält „früh" und darf
/// daraus keinen Vormittagstermin machen. Das war der Grund für die
/// Wortsuche statt einer Teilstringsuche.
#[test]
fn tageszeiten_werden_zu_ihrer_uhrzeit() {
    for (wort, uhrzeit) in TAGESZEITEN {
        let satz = format!("morgen {wort}");

        for form in [satz.clone(), satz.replace(' ', "  "), format!("{satz} Uhr")] {
            let gelesen = crate::calendar::write::parse_term(&form)
                .unwrap_or_else(|grund| panic!("„{form}“ wurde nicht gelesen: {grund}"));
            assert_eq!(
                gelesen.with_timezone(&Local).format("%H:%M").to_string(),
                *uhrzeit,
                "„{form}“ ergibt nicht {uhrzeit}"
            );
        }
    }

    // Der Gegenfall: Ein Wort, das eine Tageszeit **enthält**, aber keine ist.
    for unlesbar in [
        "morgen Frühstück mit Anna",
        "morgen Mittagessen mit der Familie",
        "morgen Abendessen um 19 Uhr",
    ] {
        // "um 19 Uhr" trägt die Uhrzeit selbst; die übrigen beiden nicht.
        let erwartet_lesbar = unlesbar.contains("19");
        assert_eq!(
            crate::calendar::write::parse_term(unlesbar).is_ok(),
            erwartet_lesbar,
            "„{unlesbar}“ wurde falsch gelesen"
        );
    }
}

/// Absolute Angaben liest Mimir ebenfalls.
///
/// Gegenprobe zu den Wortformen: Ein Modell, das umrechnet und
/// `2026-10-13T09:30` liefert, ist damit nicht verloren – nur langsamer, weil der
/// Benutzer in der Vorschau nicht mehr den Satz sieht, den er gesagt hat.
#[test]
fn absolute_angaben_liest_mimir_auch() {
    // Die Form, die in den Schemata als erlaubt genannt wird: `JJJJ-MM-TTThh:mm`.
    let absolut = crate::calendar::write::parse_term("2026-10-13T09:30")
        .expect("das absolute Format wird gelesen")
        .with_timezone(&Local);
    assert_eq!(
        absolut.format("%H:%M").to_string(),
        "09:30",
        "die Uhrzeit geht beim Umrechnen verloren"
    );
    assert_eq!(
        absolut.date_naive(),
        chrono::NaiveDate::from_ymd_opt(2026, 10, 13).expect("gültiges Datum"),
        "das absolute Datum geht beim Umrechnen verloren"
    );

    // Und was nicht lesbar ist, muss als Fehler kommen und nicht als irgendein
    // Tag. Ein geratener Termin ist schlimmer als keiner.
    for unlesbar in ["", "irgendwann", "bald", "irgendwann nächste Woche"] {
        assert!(
            crate::calendar::write::parse_term(unlesbar).is_err(),
            "\"{unlesbar}\" wurde als Zeitpunkt gelesen"
        );
    }
}

/// Ein Ende ohne Tagesangabe gehört zum Tag des Beginns.
///
/// Aus dem zweiten Validierungslauf: Das Modell lieferte für „Freitagmittag"
/// `start: "Freitag 14:00"` und `end: "15:00"`. „15:00" ist für sich allein der
/// heutige Tag, der Termin lag damit in der Vergangenheit und wurde abgelehnt.
#[test]
fn ein_ende_ohne_tag_gehoert_zum_termin() {
    let (href, name) = ("persoenlich", "Persönlich");
    let kalender = vec![href.to_string()];
    let config = crate::calendar::CalendarConfig {
        server_url: "https://kalender.example.org".to_string(),
        username: "benutzer".to_string(),
        calendars: kalender,
        server_certificate: None,
    };
    let verfuegbar = vec![(href.to_string(), name.to_string())];

    let plan = crate::calendar::write::plan_event(
        &config,
        &verfuegbar,
        &serde_json::json!({
            "summary": "Mittagstermin",
            "start": "Freitag 14:00",
            "end": "15:00"
        }),
        "",
    )
    .expect("ein Ende ohne Tag gehört zum Termin");

    // Und derselbe Aufruf ohne `end` muss dasselbe ergeben: Die Vorgabedauer ist
    // eine Stunde, das genannte Ende war es zufällig auch – die Korrektur darf
    // den Ablauf also nicht verändern.
    let ohne_ende = crate::calendar::write::plan_event(
        &config,
        &verfuegbar,
        &serde_json::json!({ "summary": "Mittagstermin", "start": "Freitag 14:00" }),
        "",
    )
    .expect("ohne Ende gilt die Vorgabedauer");
    assert_eq!(plan.when, ohne_ende.when);

    // Ein Ende **mit** Tag bleibt, wie es ist: „morgen 10 Uhr" zu einem Termin
    // am Freitag ist der nächste Tag und wird nicht auf den Freitag umgedeutet.
    let spaeter = crate::calendar::write::plan_event(
        &config,
        &verfuegbar,
        &serde_json::json!({
            "summary": "Termin",
            "start": "Freitag 14:00",
            "end": "Samstag 10:00"
        }),
        "",
    )
    .expect("ein Ende mit Tag bleibt, wie es ist");
    assert_ne!(spaeter.when, ohne_ende.when);
}

/// Ein erfundener Ort landet nicht im Termin.
///
/// Aus dem zweiten Validierungslauf: Zu „Ich brauche morgen einen Termin mit der
/// Bank" schickte das Modell `location: "Online"`. Der Ort stand danach im
/// Kalender, und der Benutzer sah ihn erst in der Vorschau – also zu spät.
#[test]
fn ein_erfundener_ort_landet_nicht_im_termin() {
    let (config, verfuegbar, start) = aufbau();

    // Der Ort, den der Benutzer gesagt hat, steht im Termin.
    let gesagt = crate::calendar::write::plan_event(
        &config,
        &verfuegbar,
        &serde_json::json!({
            "summary": "Hausärztin",
            "start": start,
            "location": "Talstraße 8"
        }),
        "Trag morgen um 14 Uhr einen Termin mit der Hausärztin ein, Ort Talstraße 8.",
    )
    .expect("der Termin bildet sich");
    assert!(
        gesagt.ics.contains("Talstraße"),
        "der genannte Ort fehlt:\n{}",
        gesagt.ics
    );
    assert!(gesagt.verworfen.is_empty(), "{:?}", gesagt.verworfen);

    // Der erfundene nicht – und er wird gemeldet, damit das Weglassen auffällt.
    let erfunden = crate::calendar::write::plan_event(
        &config,
        &verfuegbar,
        &serde_json::json!({
            "summary": "Bank",
            "start": start,
            "location": "Online"
        }),
        "Ich brauche morgen zwischen 15 und 16 Uhr einen Termin mit der Bank.",
    )
    .expect("der Termin bildet sich");
    assert!(
        !erfunden.ics.contains("Online"),
        "der erfundene Ort steht im Termin:\n{}",
        erfunden.ics
    );
    assert_eq!(erfunden.verworfen, vec!["Ort".to_string()]);
    assert!(
        erfunden.summary.contains("Nicht übernommen"),
        "der Hinweis fehlt: {}",
        erfunden.summary
    );
}

/// Ohne die Worte des Benutzers wird nichts verworfen.
///
/// Die wichtigste Grenze der Prüfung: Sie darf nie etwas entfernen, weil sie ihre
/// Grundlage nicht kennt. Ein Aufruf aus einem anderen Weg – ohne Benutzertext –
/// behält deshalb jedes Feld.
#[test]
fn ohne_benutzertext_wird_nichts_verworfen() {
    let (config, verfuegbar, start) = aufbau();

    for text in ["", "   ", "15"] {
        let plan = crate::calendar::write::plan_event(
            &config,
            &verfuegbar,
            &serde_json::json!({
                "summary": "Termin",
                "start": start,
                "location": "Praxis Dr. Klein"
            }),
            text,
        )
        .expect("der Termin bildet sich");

        assert!(
            plan.ics.contains("Praxis"),
            "ohne Benutzertext wurde der Ort verworfen ({text:?})"
        );
        assert!(plan.verworfen.is_empty(), "{text:?}: {:?}", plan.verworfen);
    }
}

/// Der Wortabgleich selbst, unabhängig vom Termin.
///
/// Die Fälle, an denen eine naive Prüfung scheitert: Ein Ort, der sich im Text
/// wiederfindet, muss durchgehen; ein Wort, das nur **enthalten** ist, nicht.
#[test]
fn der_wortabgleich_kennt_seine_grenzen() {
    let gedeckt = crate::calendar::write::feld_gedeckt;

    // Genau so gesagt.
    assert!(gedeckt("Talstraße 8", "Ort Talstraße 8", &[]));
    assert!(gedeckt("Hausärztin", "Termin mit der Hausärztin", &[]));
    // Bindestriche und Großschreibung stören nicht.
    assert!(gedeckt("St.-Nikolaus", "in St. Nikolaus", &[]));
    assert!(gedeckt("TALSTRASSE", "Ort talstrasse", &[]));
    // Ein Wort aus einer Liste zählt als gesagt.
    assert!(gedeckt("Praxis", "irgendwo", &["praxis"]));

    // Erfunden.
    assert!(!gedeckt("Online", "Termin mit der Bank", &[]));
    assert!(!gedeckt(
        "Praxis Dr. Klein",
        "Termin mit der Hausärztin",
        &[]
    ));

    // **Grenze, die der Vergleich nicht schafft**: Ein Ort, in dem ein Teil des
    // Wortes steckt („Talstraße 8, Haus 12b" passt teilweise auf „Talstraße 8"),
    // wird behalten. Ein exakter Abgleich je Wort wäre strenger und würde
    // umgekehrt Fälle verlieren, in denen der Benutzer „Talstr. 8" sagt und das
    // Modell „Talstraße 8" – der Normalfall ist das umgekehrte Verhältnis:
    // stehen bleiben ist der harmlosere Fehler.
    //
    // Hier festgehalten, damit niemand die Prüfung für genauer hält, als sie ist.
    assert!(gedeckt("Talstraße 8, Haus 12b", "Ort Talstraße 8", &[]));
}

/// Zahlen und Kürzel sind keine Wörter, die man prüfen kann.
///
/// „15 Uhr" im Benutzertext und „15:00" beim Modell sind derselbe Wunsch, und ein
/// Ort wie „Raum 204" soll nicht daran scheitern, dass eine Ziffer keine
/// Buchstaben hat.
#[test]
fn eine_uhrzeit_versperrt_den_ort_nicht() {
    let gedeckt = crate::calendar::write::feld_gedeckt;

    assert!(gedeckt("Raum 204", "Bitte um 16 Uhr, Raum 204", &[]));
    assert!(gedeckt("12b", "Haus 12b", &[]));
    assert!(gedeckt("Stock 2", "im zweiten Stock", &[]));
}

/// Die Konfiguration für die Termintests: ein Kalender, ein Termin weit weg.
fn aufbau() -> (
    crate::calendar::CalendarConfig,
    Vec<(String, String)>,
    String,
) {
    let config = crate::calendar::CalendarConfig {
        server_url: "https://kalender.example.org".to_string(),
        username: "benutzer".to_string(),
        calendars: vec!["persoenlich".to_string()],
        server_certificate: None,
    };
    let verfuegbar = vec![("persoenlich".to_string(), "Persönlich".to_string())];
    // In der Zukunft: Die Prüfung gegen die Vergangenheit soll hier nicht das
    // eigentliche Thema der Tests verdecken.
    let start = (Local::now() + Duration::days(7))
        .format("%Y-%m-%dT09:00")
        .to_string();

    (config, verfuegbar, start)
}
