//! Die Datumsangabe, wie das Frontend sie baut.
//!
//! Bewusst eine zweite Umsetzung derselben Angabe, und nicht die aus
//! `main.js` übernommen: Das Frontend ist JavaScript im Anwendungsordner und
//! ohne Fenster nicht ausführbar. Der Aufwand wäre größer als der Nutzen, und
//! beide Fassungen werden gegen dieselbe Erwartung geprüft – `datumsangabe.test.mjs`
//! hier, dieselbe Logik dort.
//!
//! Die Angabe wird in **jede** Anfrage an das Modell getan, weil ein Modell keine
//! Uhr hat. Sie nennt den heutigen Tag mit Wochentag und die folgenden sieben
//! Tage mit ihrer Woche, damit „nächsten Dienstag" und „übermorgen" auflösbar
//! sind, ohne dass das Modell selbst zählt.
//!
//! Sie steht hier, weil die Validierung sie braucht: Ein Satz wie „morgen um 14
//! Uhr" ergibt nur dann einen Termin, wenn das Modell weiß, welcher Tag heute
//! ist. Ohne diese Angabe würde die Messung eine Fähigkeit prüfen, die es in der
//! Anwendung nicht gibt.

use chrono::{Datelike, Duration, Local};

const WOCHENTAGE: [&str; 7] = [
    "Sonntag",
    "Montag",
    "Dienstag",
    "Mittwoch",
    "Donnerstag",
    "Freitag",
    "Samstag",
];

/// Wie der Montag im Verhältnis zum heutigen Tag genannt wird.
///
/// „In dieser Woche" heißt ab Montag, „in der laufenden" in der Woche davor und
/// „in der nächsten" danach. Genau diese drei Wörter braucht das Modell, um
/// „diese Woche" und „nächsten Montag" richtig zu lesen.
fn wochenname(tag: chrono::NaiveDate, heute: chrono::NaiveDate) -> &'static str {
    // Montag als Wochenanfang. `num_days_from_monday()` beginnt bei null,
    // deshalb das `+ 6 % 7` – sonst wäre der Sonntag der Wochenanfang.
    let wochen_start =
        heute - Duration::days((heute.weekday().num_days_from_monday() as i64 + 6) % 7);
    let woche = (tag - wochen_start).num_days().div_euclid(7);
    match woche {
        w if w < 0 => "vergangenen",
        0 => "laufenden",
        1 => "nächsten",
        _ => "späteren",
    }
}

/// Die Angabe, wie sie in das Systemfeld jeder Anfrage geht.
pub fn datumsangabe() -> String {
    let jetzt = Local::now();
    let heute = jetzt.date_naive();

    let mut zeilen = vec![
        format!("HEUTE IST: {}", heute.format("%Y-%m-%d")),
        format!(
            "Das ist {}, {} Uhr {}.",
            WOCHENTAGE[heute.weekday().num_days_from_sunday() as usize],
            jetzt.format("%H:%M"),
            // Die Zeitzone nennt der Browser; hier kommt die des Rechners. Ohne
            // Versatzangabe rechnet das Werkzeug in der Zeit des Rechners, und
            // genau die soll das Modell auch vorfinden.
            jetzt.format("%Z")
        ),
        "Weitere Angaben: „heute“, „morgen“, „übermorgen“, „in drei Tagen“ und die Wochentage. \
         Für „diese Woche“ gilt der Montag als erster Tag."
            .to_string(),
    ];

    for tage in 1..=7 {
        let tag = heute + Duration::days(tage);
        zeilen.push(format!(
            "  {} ist ein {} in der {} Woche.",
            tag.format("%Y-%m-%d"),
            WOCHENTAGE[tag.weekday().num_days_from_sunday() as usize],
            wochenname(tag, heute)
        ));
    }

    zeilen.join("\n")
}
