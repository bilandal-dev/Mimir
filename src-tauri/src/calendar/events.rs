//! Aufbereiten der Termine aus einer ICS-Datei.
//!
//! Gelesen wird alles, was für eine Anzeige brauchbar ist: Zeitpunkt, Titel, Ort
//! und Wiederholungen. Geschrieben wird hier nichts.
//!
//! Bewusst unterstützt ist der Teil von RFC 5545 und RFC 5545-RRULE, den die
//! Oberfläche von Nextcloud erzeugt: Frequenz, Intervall, Zähler, Ende, Wochentage
//! mit vorangestellter Zahl, Monatstage, Monate, Ausnahmen und verschobene
//! Termine. Was nicht darstellbar ist, wird nicht geraten: Solche Termine
//! erscheinen einmalig, statt falsch zu liegen.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use std::collections::HashMap;
use std::str::FromStr;

use super::{
    CalendarEvent, MAX_EVENT_CATEGORIES, MAX_EVENT_LOCATION_BYTES, MAX_EVENT_SUMMARY_BYTES,
};

/// Zeitraum, für den Termine ausgegeben werden.
#[derive(Clone, Copy)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Höchstes Alter eines Startdatums, das noch durchlaufen wird. Eine
/// Wiederholung ohne Ende darf nicht unbegrenzt Rechenzeit binden; 20 Jahre
/// decken jeden Kalender ab, auch alte Archivtermine.
const MAX_SCAN_DAYS: i64 = 365 * 20;
/// Grenze für erzeugte Vorkommen je Serie, unabhängig vom Zeitraum.
///
/// Beide Grenzen betreffen nur die Anzeige, nicht die Kalenderdatei: Eine Serie
/// mit mehr als 500 Vorkommen endet in der Leiste stillschweigend, und für
/// Kalender weit in der Zukunft gibt es keinen Anzeigepfad.
const MAX_OCCURRENCES: usize = 500;

struct Property {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Property {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Zeitzone eines Termins: mit Zone, als lokale Zeit der Maschine oder ohne
/// Angabe („floating"). Ohne Angabe bleibt die Uhrzeit so stehen, wie sie in der
/// Datei steht.
#[derive(Clone)]
enum Zone {
    Named(Tz),
    Machine,
    Floating,
}

/// Setzt Punkte und Kommas aus dem Fließtext zurück.
fn unescape_text(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }

        match characters.next() {
            Some('n') | Some('N') => result.push('\n'),
            Some(',') => result.push(','),
            Some(';') => result.push(';'),
            Some('\\') => result.push('\\'),
            Some(other) => result.push(other),
            None => break,
        }
    }

    result
}

/// Die Kategorien eines Termins, aus `CATEGORIES`.
///
/// Die Norm trennt sie mit Semikolon; ein maskiertes `\;` bleibt Teil des Namens
/// und darf deshalb nicht als Trenner gelten – sonst hieße „Müller\; Meier" zwei
/// Kategorien. Leere Einträge fallen weg, damit ein `;;` aus einer anderen
/// Software keine leere Kategorie in der Leiste erzeugt.
fn kategorien(items: &[&Property]) -> Vec<String> {
    let Some(wert) = property(items, "CATEGORIES").map(|item| item.value.as_str()) else {
        return Vec::new();
    };

    let mut teile: Vec<String> = Vec::new();
    let mut aktuell = String::new();
    let mut zeichen = wert.chars();

    while let Some(naechster) = zeichen.next() {
        match naechster {
            // Ein maskierender Backslash gehört zum nächsten Zeichen, nicht zum
            // Trennen.
            '\\' => {
                aktuell.push('\\');
                if let Some(nachste) = zeichen.next() {
                    aktuell.push(nachste);
                }
            }
            ';' => {
                teile.push(unescape_text(&aktuell));
                aktuell.clear();
            }
            other => aktuell.push(other),
        }
    }

    teile.push(unescape_text(&aktuell));
    teile
        .into_iter()
        .map(|kategorie| kategorie.trim().to_string())
        .filter(|kategorie| !kategorie.is_empty())
        .take(MAX_EVENT_CATEGORIES)
        .collect()
}

fn trimmed_summary(value: &str) -> String {
    let text = unescape_text(value);

    if text.chars().count() > MAX_EVENT_SUMMARY_BYTES {
        return text.chars().take(MAX_EVENT_SUMMARY_BYTES).collect();
    }

    text
}

fn trimmed_location(value: &str) -> String {
    let text = unescape_text(value).replace(['\n', '\r', '\t'], " ");
    // Wortweise gekürzt, damit kein halbes Wort stehen bleibt. Die Grenze wird
    // nach jedem angehängten Wort geprüft, deshalb kann das letzte Wort sie
    // überschreiten – unkritisch, denn gewollt ist nur: nicht endlos lang.
    let mut result = String::new();

    for word in text.split_whitespace() {
        if !result.is_empty() {
            result.push(' ');
        }

        result.push_str(word);

        if result.len() >= MAX_EVENT_LOCATION_BYTES {
            break;
        }
    }

    result
}

/// Entfaltet die Zeilenumbrüche innerhalb einer ICS-Zeile: Eine Zeile, die mit
/// Leerzeichen oder Tab beginnt, ist die Fortsetzung der vorangehenden.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');

        if let Some(previous) = lines.last_mut() {
            if let Some(rest) = line.strip_prefix(' ') {
                previous.push_str(rest);
                continue;
            }

            if let Some(rest) = line.strip_prefix('\t') {
                previous.push_str(rest);
                continue;
            }
        }

        lines.push(line.to_string());
    }

    lines
}

fn parse_line(line: &str) -> Option<Property> {
    // NAME;PARAM=WERT:INHALT
    let colon = line.find(':')?;
    let (head, value) = line.split_at(colon);
    let value = &value[1..];

    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();

    if name.is_empty() {
        return None;
    }

    let mut params = Vec::new();

    for part in parts {
        let part = part.trim();

        if part.is_empty() {
            continue;
        }

        match part.split_once('=') {
            Some((key, raw)) => {
                params.push((
                    key.trim().to_ascii_uppercase(),
                    raw.trim().trim_matches('"').to_string(),
                ));
            }
            // Wertlose Parameter, etwa `VALUE=DATE;X` sind harmlos.
            None => params.push((part.to_ascii_uppercase(), String::new())),
        }
    }

    Some(Property {
        name,
        params,
        value: value.to_string(),
    })
}

/// Liest alle VEVENT-Blöcke einer ICS-Datei.
fn collect_events(text: &str) -> Vec<Vec<Property>> {
    let mut events: Vec<Vec<Property>> = Vec::new();
    let mut current: Option<Vec<Property>> = None;

    for line in unfold(text) {
        let trimmed = line.trim();

        if trimmed.eq_ignore_ascii_case("BEGIN:VEVENT") {
            current = Some(Vec::new());
            continue;
        }

        if trimmed.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(properties) = current.take() {
                events.push(properties);
            }

            continue;
        }

        if let (Some(properties), Some(property)) = (current.as_mut(), parse_line(trimmed)) {
            properties.push(property);
        }
    }

    events
}

fn property<'a>(properties: &'a [&'a Property], name: &str) -> Option<&'a Property> {
    properties
        .iter()
        .find(|property| property.name.eq_ignore_ascii_case(name))
        .copied()
}

fn properties<'a>(properties: &'a [&'a Property], name: &str) -> Vec<&'a Property> {
    properties
        .iter()
        .filter(|property| property.name.eq_ignore_ascii_case(name))
        .copied()
        .collect()
}

/// Steht diese Zeile auf `BEGIN:VALARM` oder `END:VALARM`?
fn ist_grenze_der_erinnerung(property: &Property) -> Option<bool> {
    (property.name.eq_ignore_ascii_case("BEGIN") || property.name.eq_ignore_ascii_case("END"))
        .then_some(property.name.eq_ignore_ascii_case("BEGIN"))
}

/// Teilt die Zeilen eines Termins in seine eigenen und die der Erinnerung.
///
/// Der `VALARM`-Block steht **im** Termin, bringt aber eigene Zeilen mit – eine
/// `DESCRIPTION` vor allem, die es auch am Termin geben kann. Ohne diese
/// Trennung nähme ein Termin die Beschreibung aus seiner Erinnerung. Umgekehrt
/// gilt das für `TRIGGER`: Das steht ausschließlich in der Erinnerung.
///
/// Zeilen außerhalb einesBlocks gehören zum Termin. Ein `BEGIN:VALARM` ohne
/// zugehöriges `END:VALARM` macht den Rest des Termins zur Erinnerung – das ist
/// die sichere Deutung, denn so entsteht keine Zeile doppelt.
fn trenne_erinnerung(items: &[Property]) -> (Vec<&Property>, Vec<&Property>) {
    let mut termin = Vec::new();
    let mut erinnerung = Vec::new();
    let mut in_alarm = false;

    for item in items {
        if let Some(beginn) = ist_grenze_der_erinnerung(item) {
            in_alarm = beginn;
            continue;
        }

        if in_alarm {
            erinnerung.push(item);
        } else {
            termin.push(item);
        }
    }

    (termin, erinnerung)
}

/// Liest eine iCalendar-Zeitangabe. Das abschließende `Z` steht für UTC und
/// wird hier nur abgeschnitten: Welche Zone gilt, entscheidet der Aufrufer über
/// die Eigenschaften der Zeile.
fn parse_naive(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim();
    let without_zone = value.strip_suffix('Z').unwrap_or(value);

    NaiveDateTime::parse_from_str(without_zone, "%Y%m%dT%H%M%S")
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(without_zone, "%Y%m%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
}

/// Trägt eine Zeitangabe nur ein Datum, wie es Ganztagestermine tun?
fn is_date_only(value: &str) -> bool {
    let value = value.trim();

    value.len() == 8 && value.chars().all(|character| character.is_ascii_digit())
}

/// Löst die Zone einer Zeitangabe auf. Bekannte Zonen kommen aus der
/// Zeitzonendatenbank von chrono-tz, damit Sommerzeit stimmt; unbekannte Namen
/// bleiben stehen, statt geraten zu werden.
fn resolve_zone(property: &Property) -> Zone {
    if property
        .param("VALUE")
        .is_some_and(|value| value.eq_ignore_ascii_case("DATE"))
    {
        return Zone::Machine;
    }

    if let Some(tzid) = property.param("TZID") {
        return match Tz::from_str(tzid) {
            Ok(zone) => Zone::Named(zone),
            Err(_) => Zone::Floating,
        };
    }

    if property.value.trim().ends_with('Z') {
        return Zone::Named(Tz::UTC);
    }

    Zone::Floating
}

/// Rechnet eine lokale Wandzeit in einen Zeitpunkt um.
fn to_utc(naive: NaiveDateTime, zone: &Zone) -> Option<DateTime<Utc>> {
    match zone {
        Zone::Named(zone) => match zone.from_local_datetime(&naive) {
            chrono::LocalResult::Single(value) => Some(value.with_timezone(&Utc)),
            // Zur Zeitumstellung gibt es zwei mögliche Zeiten; genommen wird die
            // frühere, so wie es Kalenderprogramme tun.
            chrono::LocalResult::Ambiguous(earlier, _) => Some(earlier.with_timezone(&Utc)),
            chrono::LocalResult::None => None,
        },
        Zone::Machine | Zone::Floating => Local
            .from_local_datetime(&naive)
            .earliest()
            .map(|value| value.with_timezone(&Utc)),
    }
}

/// Liest eine Dauer nach RFC 5545, also `P1DT2H30M` oder `PT45M`.
fn duration_of(value: &str) -> Option<Duration> {
    let value = value.trim().to_ascii_uppercase();
    let mut total = Duration::zero();
    let mut in_time = false;
    let mut number = String::new();
    let mut sign = 1i64;

    for character in value.chars() {
        match character {
            'P' => in_time = false,
            'T' => in_time = true,
            '-' => sign = -1,
            '+' => sign = 1,
            '0'..='9' => number.push(character),
            'W' | 'D' if !in_time => {
                let amount: i64 = number.parse().ok()?;
                total += if character == 'W' {
                    Duration::weeks(sign * amount)
                } else {
                    Duration::days(sign * amount)
                };
                number.clear();
            }
            'H' if in_time => {
                let amount: i64 = number.parse().ok()?;
                total += Duration::hours(sign * amount);
                number.clear();
            }
            'M' if in_time => {
                let amount: i64 = number.parse().ok()?;
                total += Duration::minutes(sign * amount);
                number.clear();
            }
            'S' => {
                let amount: i64 = number.parse().ok()?;
                total += Duration::seconds(sign * amount);
                number.clear();
            }
            _ => return None,
        }
    }

    (total != Duration::zero()).then_some(total)
}

#[derive(Clone, Copy, PartialEq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, PartialEq)]
enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "MO" => Some(Weekday::Monday),
            "TU" => Some(Weekday::Tuesday),
            "WE" => Some(Weekday::Wednesday),
            "TH" => Some(Weekday::Thursday),
            "FR" => Some(Weekday::Friday),
            "SA" => Some(Weekday::Saturday),
            "SU" => Some(Weekday::Sunday),
            _ => None,
        }
    }
}

/// Ein BYDAY-Eintrag, gegebenenfalls mit vorangestellter Zahl wie `1MO` oder
/// `-1FR` für den letzten Freitag des Monats.
#[derive(Clone, Copy)]
struct ByDay {
    weekday: Weekday,
    ordinal: Option<i32>,
}

/// Die Teile einer Wiederholungsregel, die hier eine Rolle spienen.
#[derive(Clone)]
struct Rule {
    frequency: Frequency,
    interval: i64,
    count: Option<usize>,
    until: Option<NaiveDateTime>,
    by_day: Vec<ByDay>,
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
    /// TRUE, wenn die Regel einen Teil benutzt, den diese Umsetzung nicht
    /// abbildet. Solche Termine erscheinen nur einmalig.
    unsupported: bool,
}

/// Zerlegt `1MO` in Zahl und Wochentag.
fn split_ordinal(entry: &str) -> (Option<i32>, &str) {
    let bytes = entry.as_bytes();
    let mut index = 0;
    let mut sign = 1i32;

    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        sign = if bytes[index] == b'-' { -1 } else { 1 };
        index += 1;
    }

    let digits_start = index;

    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }

    if index == digits_start {
        return (None, entry);
    }

    (
        entry[digits_start..index]
            .parse::<i32>()
            .ok()
            .map(|value| sign * value),
        &entry[index..],
    )
}

fn parse_rule(value: &str) -> Option<Rule> {
    let value = value.trim().trim_start_matches("RRULE:");
    let mut rule = Rule {
        frequency: Frequency::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        unsupported: false,
    };
    let mut frequency_seen = false;

    for part in value.split(';') {
        let Some((key, raw)) = part.split_once('=') else {
            continue;
        };

        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                frequency_seen = true;
                rule.frequency = match raw.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Frequency::Daily,
                    "WEEKLY" => Frequency::Weekly,
                    "MONTHLY" => Frequency::Monthly,
                    "YEARLY" => Frequency::Yearly,
                    _ => {
                        rule.unsupported = true;
                        Frequency::Daily
                    }
                };
            }
            "INTERVAL" => rule.interval = raw.trim().parse().unwrap_or(1).max(1),
            "COUNT" => rule.count = raw.trim().parse().ok(),
            "UNTIL" => rule.until = parse_naive(raw),
            "BYDAY" => {
                for entry in raw.split(',') {
                    let (ordinal, name) = split_ordinal(entry);

                    match Weekday::from_name(name) {
                        Some(weekday) => rule.by_day.push(ByDay { weekday, ordinal }),
                        None => rule.unsupported = true,
                    }
                }
            }
            "BYMONTHDAY" => {
                for entry in raw.split(',') {
                    match entry.trim().parse() {
                        Ok(day) => rule.by_month_day.push(day),
                        Err(_) => rule.unsupported = true,
                    }
                }
            }
            "BYMONTH" => {
                for entry in raw.split(',') {
                    match entry.trim().parse() {
                        Ok(month) => rule.by_month.push(month),
                        Err(_) => rule.unsupported = true,
                    }
                }
            }
            // WKST verschiebt nur den Bezugspunkt bei Intervallen, die hier
            // über Tage laufen, und damit nichts am Ergebnis.
            "WKST" => {}
            // Diese Regeln werden nicht aufgefaltet, sondern als nicht abgebildet
            // gemeldet: Der Termin erscheint dann nur einmal. `BYYEARWEEK` ist
            // absichtlich mitgeschrieben, obwohl die Norm `BYYEARWEEKNO` sagt –
            // beides landet ohnehin unten im `_`-Zweig.
            "BYSETPOS" | "BYYEARDAY" | "BYWEEKNO" | "BYYEARWEEK" => rule.unsupported = true,
            _ => rule.unsupported = true,
        }
    }

    frequency_seen.then_some(rule)
}

fn weekday_of(date: NaiveDate) -> Weekday {
    match date.weekday() {
        chrono::Weekday::Mon => Weekday::Monday,
        chrono::Weekday::Tue => Weekday::Tuesday,
        chrono::Weekday::Wed => Weekday::Wednesday,
        chrono::Weekday::Thu => Weekday::Thursday,
        chrono::Weekday::Fri => Weekday::Friday,
        chrono::Weekday::Sat => Weekday::Saturday,
        chrono::Weekday::Sun => Weekday::Sunday,
    }
}

fn month_length(year: i32, month: u32) -> i32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_default();
    let next = NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap_or_default();

    (next - first).num_days() as i32
}

/// Trifft ein Wochentag mit vorangestellter Zahl auf diesen Tag zu?
fn ordinal_matches(day: NaiveDate, position: i32) -> bool {
    let length = month_length(day.year(), day.month());

    if position > 0 {
        (day.day() as i32 - 1) / 7 + 1 == position
    } else {
        // -1 heißt letzter: der Tag liegt in den letzten sieben Tagen.
        day.day() as i32 + 7 > length
    }
}

/// Trifft ein Monatstag wie `15` oder `-1` auf diesen Tag zu?
fn month_day_matches(day: NaiveDate, expected: i32) -> bool {
    if expected > 0 {
        day.day() as i32 == expected
    } else {
        month_length(day.year(), day.month()) - day.day() as i32 + 1 == -expected
    }
}

/// Gehört dieser Kalendertag zur Regel? Der Vergleich läuft Tag für Tag ab dem
/// Startdatum, statt Monats- und Jahressprünge getrennt zu behandeln. Für die
/// Kalendergrößen, um die es hier geht, ist das unkritisch und deutlich
/// schwerer falsch zu bekommen.
///
/// Achtung, was je Frequenz **nicht** ausgewertet wird: `BYDAY` und `BYMONTHDAY`
/// gelten nur für `MONTHLY` und `YEARLY`. `FREQ=DAILY;BYDAY=MO,WE` erzeugt
/// deshalb jeden Intervalltag und nicht nur Montage und Mittwoche.
/// `BYMONTH` gilt dagegen für alle Frequenzen.
fn rule_matches(rule: &Rule, start: NaiveDate, day: NaiveDate) -> bool {
    if (day - start).num_days() < 0 {
        return false;
    }

    if !rule.by_month.is_empty() && !rule.by_month.contains(&day.month()) {
        return false;
    }

    let weekday = weekday_of(day);

    match rule.frequency {
        Frequency::Daily => (day - start).num_days() % rule.interval == 0,
        Frequency::Weekly => {
            let start_week = start.weekday().num_days_from_monday() as i64;
            let day_week = day.weekday().num_days_from_monday() as i64;

            if (day_week - start_week).rem_euclid(rule.interval) != 0 {
                return false;
            }

            if rule.by_day.is_empty() {
                weekday == weekday_of(start)
            } else {
                // Nur Wochentage ohne vorangestellte Zahl werden hier
                // ausgewertet. Eine wöchentliche Regel, die ausschließlich
                // ordinale Einträge nennt (`BYDAY=1MO`), trifft damit auf keinen
                // Tag – der Termin verschwindet dann ganz, statt einmalig zu
                // erscheinen. Ordinale Wochentage sind in
                // `template_from_properties` nur für `YEARLY` als nicht
                // abgebildet markiert.
                rule.by_day
                    .iter()
                    .any(|entry| entry.weekday == weekday && entry.ordinal.is_none())
            }
        }
        Frequency::Monthly => {
            let months = (day.year() as i64 - start.year() as i64) * 12
                + (day.month() as i64 - start.month() as i64);

            if months < 0 || months % rule.interval != 0 {
                return false;
            }

            if !rule.by_day.is_empty() {
                rule.by_day.iter().any(|entry| {
                    entry.weekday == weekday
                        && entry
                            .ordinal
                            .is_none_or(|position| ordinal_matches(day, position))
                })
            } else if !rule.by_month_day.is_empty() {
                rule.by_month_day
                    .iter()
                    .any(|expected| month_day_matches(day, *expected))
            } else {
                day.day() == start.day()
            }
        }
        Frequency::Yearly => {
            let years = day.year() as i64 - start.year() as i64;

            if years < 0 || years % rule.interval != 0 {
                return false;
            }

            if !rule.by_day.is_empty() {
                // Wie viele Male dieser Wochentag im Jahr liegt, wird hier nicht
                // ausgezählt. Solche Regeln kommen in der Praxis nicht vor und
                // werden deshalb gar nicht erst zugelassen.
                rule.by_day
                    .iter()
                    .any(|entry| entry.weekday == weekday && entry.ordinal.is_none())
            } else if !rule.by_month_day.is_empty() {
                rule.by_month_day
                    .iter()
                    .any(|expected| month_day_matches(day, *expected))
            } else if rule.by_month.is_empty() {
                day.month() == start.month() && day.day() == start.day()
            } else {
                true
            }
        }
    }
}

#[derive(Clone)]
struct EventTemplate {
    uid: String,
    summary: String,
    location: String,
    calendar: String,
    calendar_href: String,
    start: NaiveDateTime,
    end_offset: Duration,
    all_day: bool,
    zone: Zone,
    /// Wie viele Minuten vor dem Beginn die Erinnerung klingelt, wenn der Termin
    /// eine hat. Der Wert wird nicht geraten: Steht kein lesbarer `TRIGGER` in
    /// der Erinnerung, bleibt es `None`.
    reminder: Option<i64>,
    /// Die Kategorien, wie sie in der Datei stehen.
    categories: Vec<String>,

    rules: Vec<Rule>,
    excluded: Vec<NaiveDateTime>,
    recurrence_id: Option<NaiveDateTime>,
    canceled: bool,
}

fn template_from_properties(
    items: &[Property],
    calendar: &str,
    calendar_href: &str,
) -> Option<EventTemplate> {
    // Ab hier ist `items` der Termin ohne die Zeilen seiner Erinnerung. Wer die
    // Trennung überspringt, liest die falsche `DESCRIPTION` und findet keinen
    // `TRIGGER`.
    let (items, alarm) = trenne_erinnerung(items);
    let start_property = property(&items, "DTSTART")?;
    let start = parse_naive(&start_property.value)?;

    if start_property.value.trim().ends_with('Z') && start_property.param("TZID").is_some() {
        // Widersprüchliche Angabe: lieber die ausdrückliche Zone als das Z.
        return None;
    }

    let all_day = is_date_only(&start_property.value)
        || start_property
            .param("VALUE")
            .is_some_and(|value| value.eq_ignore_ascii_case("DATE"));
    let zone = if all_day {
        Zone::Machine
    } else {
        resolve_zone(start_property)
    };

    // Ohne DTEND und ohne DURATION gilt ein Ganztagestermin als ein Tag, ein
    // Termin mit Uhrzeit als punktgenau.
    let end_offset = match property(&items, "DTEND").and_then(|item| parse_naive(&item.value)) {
        Some(end) => end.signed_duration_since(start),
        None => match property(&items, "DURATION").and_then(|item| duration_of(&item.value)) {
            Some(duration) => duration,
            None if all_day => Duration::days(1),
            None => Duration::zero(),
        },
    };

    let rules = properties(&items, "RRULE")
        .iter()
        .filter_map(|item| parse_rule(&item.value))
        // Ordinale Wochentage in Jahresregeln werden nicht ausgezählt.
        .map(|mut rule| {
            if rule.frequency == Frequency::Yearly
                && rule.by_day.iter().any(|e| e.ordinal.is_some())
            {
                rule.unsupported = true;
            }

            rule
        })
        .collect();

    let mut excluded = Vec::new();

    for item in properties(&items, "EXDATE") {
        for entry in item.value.split(',') {
            if let Some(value) = parse_naive(entry) {
                excluded.push(value);
            }
        }
    }

    Some(EventTemplate {
        uid: property(&items, "UID")
            .map(|item| item.value.trim().to_string())
            .unwrap_or_default(),
        summary: property(&items, "SUMMARY")
            .map(|item| trimmed_summary(&item.value))
            .unwrap_or_default(),
        location: property(&items, "LOCATION")
            .map(|item| trimmed_location(&item.value))
            .unwrap_or_default(),
        calendar: calendar.to_string(),
        calendar_href: calendar_href.to_string(),
        start,
        end_offset,
        all_day,
        zone,
        reminder: property(&alarm, "TRIGGER")
            .and_then(|item| super::ics::trigger_zu_minuten(&item.value)),
        categories: kategorien(&items),
        rules,
        excluded,
        recurrence_id: property(&items, "RECURRENCE-ID").and_then(|item| parse_naive(&item.value)),
        canceled: property(&items, "STATUS")
            .is_some_and(|item| item.value.trim().eq_ignore_ascii_case("CANCELED")),
    })
}

fn occurrence_to_event(template: &EventTemplate, start: NaiveDateTime) -> Option<CalendarEvent> {
    let start_utc = to_utc(start, &template.zone)?;
    let end_utc = to_utc(start + template.end_offset, &template.zone)?;

    Some(CalendarEvent {
        uid: template.uid.clone(),
        summary: template.summary.clone(),
        location: template.location.clone(),
        start: start_utc.timestamp(),
        end: end_utc.timestamp(),
        all_day: template.all_day,
        floating: matches!(template.zone, Zone::Floating),
        calendar: template.calendar.clone(),
        calendar_href: template.calendar_href.clone(),
        reminder: template.reminder,
        reminder_text: template
            .reminder
            .map(|minuten| format!("Erinnerung {}", super::write::erinnerung_text(minuten)))
            .unwrap_or_default(),
        categories: template.categories.clone(),
        canceled: template.canceled,
    })
}

/// Erzeugt alle Vorkommen einer Serie im Zeitraum.
fn occurrences(template: &EventTemplate, window: &Window) -> Vec<CalendarEvent> {
    let usable: Vec<&Rule> = template
        .rules
        .iter()
        .filter(|rule| !rule.unsupported)
        .collect();
    let mut starts: Vec<NaiveDateTime> = Vec::new();

    if usable.is_empty() {
        starts.push(template.start);
    } else {
        let start_day = template.start.date();
        let time = template.start.time();
        // Gezählt wird ab dem Startdatum, weil COUNT alle Vorkommen seit Beginn
        // der Serie meint. Ausgegeben wird nur, was im Zeitraum liegt.
        let from = std::cmp::max(
            start_day,
            window.start.with_timezone(&Utc).date_naive() - Duration::days(2),
        );
        let to = std::cmp::min(
            start_day + Duration::days(MAX_SCAN_DAYS),
            window.end.with_timezone(&Utc).date_naive() + Duration::days(2),
        );
        let mut produced = 0usize;

        for rule in usable {
            let mut counted = 0usize;
            let mut day = start_day;

            while day <= to {
                if rule_matches(rule, start_day, day) {
                    let candidate = day.and_time(time);

                    if !rule.until.is_some_and(|until| candidate > until) {
                        // COUNT nennt alle Vorkommen seit Beginn der Serie. Ein
                        // ausgenommenes Tag zaehlt mit, sonst waere eine Serie
                        // mit Ausnahmen laenger als ihre eigene Angabe.
                        counted += 1;

                        if let Some(limit) = rule.count {
                            if counted > limit {
                                break;
                            }
                        }

                        if day >= from && !template.excluded.contains(&candidate) {
                            starts.push(candidate);
                            produced += 1;
                        }
                    }
                }

                day += Duration::days(1);
            }

            if produced >= MAX_OCCURRENCES {
                break;
            }
        }
    }

    let mut events: Vec<CalendarEvent> = starts
        .iter()
        .filter_map(|start| occurrence_to_event(template, *start))
        .collect();

    events.sort_by_key(|event| event.start);
    events.dedup_by_key(|event| event.start);

    events
}

/// Löst die Serie auf: verschobene Termine ersetzen ihr Vorkommen, gelöschte
/// verschwinden.
fn resolve_overrides(
    base: &EventTemplate,
    overrides: &[(NaiveDateTime, EventTemplate)],
    window: &Window,
) -> Vec<CalendarEvent> {
    let generated = occurrences(base, window);
    let mut result: Vec<CalendarEvent> = Vec::new();
    let mut replaced = vec![false; generated.len()];

    for (recurrence_id, replacement) in overrides {
        let Some(position) = to_utc(*recurrence_id, &base.zone).and_then(|value| {
            generated
                .iter()
                .position(|event| event.start == value.timestamp())
        }) else {
            // Verschobener Termin außerhalb des Zeitraums: nichts zu tun.
            continue;
        };

        replaced[position] = true;

        if replacement.canceled {
            continue;
        }

        if let Some(event) = occurrence_to_event(replacement, replacement.start) {
            result.push(event);
        }
    }

    if !base.canceled {
        for (position, event) in generated.iter().enumerate() {
            if !replaced[position] {
                result.push(event.clone());
            }
        }
    }

    result.sort_by_key(|event| event.start);
    result
}

/// Liest eine ICS-Datei und liefert die Termine im Zeitraum, nach Zeit sortiert.
///
/// `calendar` ist der Anzeigename, `calendar_href` der Pfad dahinter. Beide
/// werden mitgenommen: Die Leiste zeigt den Namen, und für das Öffnen eines
/// Termins im Fenster braucht es den Pfad.
pub fn parse_events(
    text: &str,
    calendar: &str,
    calendar_href: &str,
    window: &Window,
) -> Vec<CalendarEvent> {
    let mut series: HashMap<String, (EventTemplate, Vec<(NaiveDateTime, EventTemplate)>)> =
        HashMap::new();
    let mut result: Vec<CalendarEvent> = Vec::new();

    for properties in collect_events(text) {
        let Some(template) = template_from_properties(&properties, calendar, calendar_href) else {
            continue;
        };

        match template.recurrence_id {
            Some(recurrence_id) => {
                // Verschobener Termin. Fehlt in diesem Fenster der ursprüngliche
                // Termin, dient der Termin selbst als eigene Vorlage.
                let key = if template.uid.is_empty() {
                    format!("{calendar}:{recurrence_id}")
                } else {
                    template.uid.clone()
                };

                match series.get_mut(&key) {
                    Some((_, list)) => list.push((recurrence_id, template)),
                    None => {
                        let mut base = template.clone();
                        base.recurrence_id = None;
                        base.canceled = false;
                        base.rules.clear();
                        series.insert(key, (base, vec![(recurrence_id, template)]));
                    }
                }
            }
            None if template.rules.is_empty() => {
                if let Some(event) = occurrence_to_event(&template, template.start) {
                    result.push(event);
                }
            }
            None => {
                let key = if template.uid.is_empty() {
                    format!("{calendar}:{}", template.start)
                } else {
                    template.uid.clone()
                };

                series.entry(key).or_insert_with(|| (template, Vec::new()));
            }
        }
    }

    for (_, (base, overrides)) in series {
        for event in resolve_overrides(&base, &overrides, window) {
            if !event.canceled {
                result.push(event);
            }
        }
    }

    let (from, to) = (window.start.timestamp(), window.end.timestamp());
    result.retain(|event| event.end > from && event.start < to);
    result.sort_by_key(|event| event.start);
    result
}

/// Zeitraum für die Leiste: ein Tag zurück, damit laufende Termine sichtbar
/// bleiben, und 92 Tage voraus. Das ist der ganze Abfragezeitraum der Leiste,
/// keine Seitengröße – weiter hinten liegende Termine sieht nur `/termine` mit
/// eigenem Zeitraum.
pub fn default_window(now: DateTime<Utc>) -> Window {
    Window {
        start: now - Duration::days(1),
        end: now + Duration::days(92),
    }
}

/// Ein Zeitraum für das Auflisten von Terminen auf Zuruf.
///
/// Anders als die Leiste läuft hier kein fester Zeitraum: Das Modell nennt, was
/// der Benutzer genannt hat. Ein Tag zurück, damit ein laufender Termin
/// auftaucht, und der genannte Abstand danach.
pub fn window_from_now(now: DateTime<Utc>, tage: i64) -> Window {
    let tage = tage.clamp(1, 366);

    Window {
        start: now - Duration::days(1),
        end: now + Duration::days(tage),
    }
}

/// Der Zeitraum für einen von Hand genannten Zeitraum: `[von, bis]` in Tagen.
///
/// Das Werkzeug kennt zwei Arten, einen Zeitraum zu bekommen: eine Zahl Tage ab
/// jetzt, oder zwei ausgeschriebene Datumsgrenzen. Die zweite Art ist keine
/// Bequemlichkeit, sondern die einzige, mit der sich „morgen" überhaupt genau
/// treffen lässt. Über `range` ist der früheste mögliche Start „jetzt minus einen
/// Tag"; wer den nächsten Tag meint, bekommt mit einer Zahl unweigerlich den
/// heutigen Tag mitgeliefert und nennt daraufhin den falschen.
///
/// `von` ist der erste Tag, `bis` der letzte, beide **einschließlich**. Fehlt
/// `bis`, ist es genau ein Tag. Das Fenster beginnt intern einen Tag früher, damit
/// eine über den Fensterrand laufende Serie nicht fehlt – der Aufrufer filtert
/// diesen Tag beim Antworten wieder heraus.
///
/// Ein Datum, das sich nicht darstellen lässt, führt nicht zu einem leeren
/// Fenster, sondern zu einem brauchbaren um jetzt: Ein stillschweigend leeres
/// Ergebnis sähe aus wie „keine Termine" aus.
pub fn window_from_days(now: DateTime<Utc>, von: NaiveDate, bis: Option<NaiveDate>) -> Window {
    // Der Tag vor `von` als UTC-Mitternacht.
    let vor = von
        .pred_opt()
        .and_then(|tag| tag.and_hms_opt(0, 0, 0))
        .map(|t| DateTime::<Utc>::from_naive_utc_and_offset(t, Utc));
    // Die Mitte der Nacht **nach** `bis`, als exklusive Grenze.
    let nach = bis
        .or(Some(von))
        .and_then(|tag| tag.succ_opt())
        .and_then(|tag| tag.and_hms_opt(0, 0, 0))
        .map(|t| DateTime::<Utc>::from_naive_utc_and_offset(t, Utc));

    let (Ok(anfang), Ok(ende)) = (vor.ok_or(()), nach.ok_or(())) else {
        return window_from_now(now, 1);
    };

    // Mindestens ein Tag: Ein `bis` vor `von` ist ein Fehler im Aufruf, und ein
    // leeres Fenster läge näher beim Wegsehen als beim Korrigieren.
    Window {
        start: anfang,
        end: ende.max(anfang + Duration::days(1)),
    }
}

#[cfg(test)]
mod tests;
