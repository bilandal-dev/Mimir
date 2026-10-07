//! Termine anlegen.
//!
//! Geplant wird hier, ausgeführt in `client.rs`. Die Vorschau im Chat entsteht
//! aus derselben Planung wie der tatsächliche Vorgang, kann also nicht von der
//! Wirkung abweichen – dasselbe gilt für die Schreibwerkzeuge des Agentenmodus.
//!
//! Bewusst nicht in **diesem** Modul: Termine ändern und löschen stehen in
//! `edit.rs`. Teilnehmer werden nirgends geschrieben – eine Einladung verlässt den
//! Rechner und lässt sich nicht zurücknehmen; das soll der Benutzer selbst
//! entscheiden, in Nextcloud.

use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
};
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};

use super::CalendarConfig;

/// Der Name des Werkzeugs. Er steht hier, weil Schema, Ausführung und
/// Freigabeprüfung denselben Namen brauchen.
pub const EVENT_TOOL: &str = "create_calendar_event";

/// Obergrenzen der Werkzeugfelder. Sie stehen bewusst niedriger als die Kürzung
/// der Leiste (`MAX_EVENT_SUMMARY_BYTES`/`MAX_EVENT_LOCATION_BYTES`), damit die
/// Vorschau den vollständigen Termin zeigt.
pub const MAX_UEBERSCHRIFT_CHARS: usize = 200;
pub const MAX_ORTS_CHARS: usize = 200;
pub const MAX_BESCHREIBUNG_CHARS: usize = 2000;
/// Obergrenze für die Kategorien eines Termins.
pub const MAX_KATEGORIE_CHARS: usize = 100;
/// Ohne `end` angenommene Dauer.
const DEFAULT_DURATION_MINUTES: i64 = 60;
/// Längeres als eine Woche ist mit hoher Wahrscheinlichkeit ein Irrtum des
/// Modells, etwa ein Jahrestag ohne Jahreswechsel. Ganztagestermine dürfen
/// länger sein: Urlaub besteht nicht aus einem einzigen Tag.
pub const MAX_DAUER_TAGE: i64 = 7;
/// So weit darf ein Termin zurückliegen, ohne abgelehnt zu werden.
pub const MAX_RUECKLIEGEND_TAGE: i64 = 7;
const MAX_DAUER_TAGE_GANZTAG: i64 = 366;
/// Ab hier werden Zeilen gefaltet. RFC 5545 erlaubt 75 Oktett ohne den
/// Zeilenumbruch; hier wird mit 74 konservativ gefaltet, jede Fortsetzung beginnt
/// mit einem Leerzeichen und bringt es zusammen wieder auf 74. `ics.rs::falten`
/// nutzt die 75 – beide Wege sind gültig, die Differenz ist gewollt.
const ICS_LINE_LIMIT: usize = 74;
/// Frühestes und spätestes Jahr, das Mimir ohne Nachfrage akzeptiert. Alles
/// andere wird abgelehnt, statt stillschweigend korrigiert zu werden.
const MIN_JAHR: i32 = 1970;
const MAX_JAHR: i32 = 2200;
const ZEIT_HINWEIS: &str = "Das Feld \"start\" oder \"end\" wird nicht verstanden. Zeiten als \
                          JJJJ-MM-TTThh:mm, etwa 2026-09-14T09:00; Sekunden, ein Z oder ein \
                          Versatz wie +02:00 sind erlaubt.";

static UID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Die Argumente, wie das Backend sie liest.
///
/// `deny_unknown_fields` ist hier wichtig: Ein Tippfehler des Modells wie
/// `titel` würde sonst stillschweigend ignoriert, und der Termin entstünde
/// ohne Überschrift. So bekommt das Modell stattdessen eine Meldung, die ihm
/// sagt, welche Felder es verwenden soll.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRequest {
    pub summary: String,
    pub start: String,
    pub end: Option<String>,
    #[serde(default)]
    pub all_day: bool,
    pub location: Option<String>,
    pub description: Option<String>,
    pub calendar: Option<String>,
    /// Wie weit vor dem Beginn es erinnern soll, in den Worten des Benutzers.
    /// `keine Erinnerung` heißt: keine.
    pub reminder: Option<String>,
    /// Die Kategorien, etwa „Arbeit“. Mehrere mit Komma trennen.
    pub category: Option<String>,
}

impl EventRequest {
    /// Liest die Argumente eines Werkzeugaufrufs und benennt jeden Fehler.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, String> {
        if value.get("attendees").is_some() {
            return Err(
                "Das Feld \"attendees\" wird nicht angenommen: Mimir verschickt keine \
                 Einladungen. Lege den Termin ohne Teilnehmer an und lade die Beteiligten in \
                 Nextcloud ein."
                    .to_string(),
            );
        }

        if value.get("attachments").is_some() || value.get("attach").is_some() {
            return Err(
                "Das Feld für Anlagen wird nicht angenommen: Eine Datei als Anlage gehört in \
                 Nextcloud hinein, sonst wäre sie nur an eine Kopie des Termins gehängt."
                    .to_string(),
            );
        }

        if let Some(fehler) = unbekannte_felder(value) {
            return Err(format!(
                "Diese Felder kennt der Termin nicht: {fehler}. Erlaubt sind \"summary\", \
                 \"start\", \"end\", \"all_day\", \"location\", \"description\", \"calendar\", \
                 \"reminder\" und \"category\"."
            ));
        }

        serde_json::from_value::<EventRequest>(value.clone()).map_err(|_| {
            "Der Termin braucht \"summary\" und \"start\", dazu dürfen \"end\", \
             \"all_day\", \"location\", \"description\", \"calendar\", \"reminder\" und \
             \"category\" kommen. Weitere Felder werden nicht hingenommen."
                .to_string()
        })
    }
}

/// Geplanter Termin, vollständig und ohne Netz.
#[derive(Clone, Debug)]
pub struct EventPlan {
    /// Pfadbestandteil des Kalenders im Sammelpfad.
    pub calendar_href: String,
    /// Anzeigename des Kalenders, wie er in der Vorschau steht.
    pub calendar_display: String,
    /// Eindeutige Kennung; zugleich Grundlage des Dateinamens im Kalender.
    pub uid: String,
    pub file_name: String,
    /// Der Inhalt, der geschrieben wird. Genau dieser Text steht auch in der
    /// Vorschau, damit der Benutzer die tatsächliche Wirkung sieht.
    pub ics: String,
    /// Ein Satz für die Zusammenfassung im Bestätigungsfenster.
    pub summary: String,
    /// Beginn und Ende in der Zeitzone des Rechners.
    pub when: String,
    /// Welche Felder das Modell gefüllt hat, obwohl der Benutzer sie nicht
    /// genannt hat, und die deshalb nicht im Termin stehen.
    ///
    /// Leer heißt: nichts verworfen. Das ist der Normalfall, und ein leeres Feld
    /// wird in der Zusammenfassung **nicht** erwähnt – der Benutzer soll nicht
    /// jedes Mal lesen, dass nichts ausgelassen wurde.
    pub verworfen: Vec<String>,
}

/// Liest ein reines Datum für Ganztagestermine.
///
/// Eine Uhrzeit darf mitgeschickt werden und wird ignoriert; ein fehlendes
/// Datum ist ein Fehler, weil ein Ganztagestermin sonst auf den falschen Tag fiele.
pub fn parse_datum(value: &str) -> Result<NaiveDate, String> {
    let text = clean(value);
    let hinweis = "Ein Ganztagestermin braucht ein Datum als JJJJ-MM-TT, etwa 2026-09-14.";

    if text.len() < 10 {
        return Err(hinweis.to_string());
    }

    let jahr: i32 = text[0..4].parse().map_err(|_| hinweis.to_string())?;
    let monat: u32 = text[5..7].parse().map_err(|_| hinweis.to_string())?;
    let tag: u32 = text[8..10].parse().map_err(|_| hinweis.to_string())?;

    if text.as_bytes()[4] != b'-' || text.as_bytes()[7] != b'-' {
        return Err(hinweis.to_string());
    }

    if !jahr_plausibel(jahr) {
        return Err(jahr_fehler(jahr));
    }

    NaiveDate::from_ymd_opt(jahr, monat, tag).ok_or_else(|| hinweis.to_string())
}

/// Liest eine Terminzeit, in welcher Schreibweise auch immer.
///
/// Zuerst die Worte des Benutzers, dann das absolute Format mit Versatz. Wer
/// „morgen 14 Uhr“ sagt, muss kein Datum rechnen; wer „2026-09-14T09:00+02:00“
/// liefert, bekommt genau diesen Zeitpunkt.
pub fn parse_term(raw: &str) -> Result<DateTime<Utc>, String> {
    match parse_wann(raw) {
        Ok(ort) => Ok(ort.with_timezone(&Utc)),
        Err(ort_fehler) => parse_zeit(raw).map_err(|_| ort_fehler),
    }
}

/// Rechnet die Worte des Benutzers in einen Zeitpunkt um.
///
/// Das ist die eigentliche Aufgabe: Modelle rechnen schlecht mit Daten, und der
/// Benutzer sagt „morgen um neun“, nicht „2026-10-01T09:00“. Wer die Worte
/// hier auflöst, muss das Modell nicht raten lassen – und genau daran ist es
/// gescheitert: Auf „heute 14 Uhr“ kam ein Termin im Jahr 2023 heraus.
///
/// Verstanden werden:
/// - Tage: `heute`, `morgen`, `übermorgen`, `in N Tagen`, `diese Woche`
/// - Wochentage: `montag` bis `sonntag`, auch abgekürzt, samt `nächsten`,
///   `kommenden`, `nächster` – gesucht wird die nächste noch kommende
///   Gelegenheit
/// - Datum: `2026-09-14`, `14.9.`, `14.09.2026`, `14. September`
/// - Uhrzeit: `14:00`, `14.00`, `14 Uhr`, `um 14 Uhr`, `14`
pub fn parse_wann(raw: &str) -> Result<DateTime<Local>, String> {
    let angabe = Angabe::neu(raw);
    let tag = ermittle_tag(&angabe);

    mit_uhrzeit(tag, &angabe.worte, &angabe.original)
}

/// Die aufbereitete Angabe des Benutzers.
///
/// Sie wird an zwei Stellen gebraucht – für Termine mit Uhrzeit (`parse_wann`)
/// und für die Auswahl „welchen Termin meinst du“ (`parse_tag`). Ganztagestermine
/// laufen an `parse_datum` vorbei: dort zählen nur die ersten zehn Zeichen, Worte
/// wie „morgen“ werden nicht gelesen. Eine eigene Auflösung je Stelle hieße
/// zweimal dieselbe Regel, und die zwei geraten irgendwann auseinander.
struct Angabe {
    original: String,
    /// Klein geschrieben, damit „Übermorgen“ wie „übermorgen“ gilt.
    klein: String,
    worte: Vec<String>,
    heute: NaiveDate,
}

impl Angabe {
    fn neu(raw: &str) -> Self {
        Self::neu_am(raw, Local::now().date_naive())
    }

    /// Dieselbe Angabe, aber mit einem vorgegebenen heutigen Tag.
    ///
    /// Nur für die Prüfung erreichbar. Die Auflösung hängt am heutigen Tag, und
    /// der einzige Weg, sie an einem 31. Dezember oder im Schaltjahr zu prüfen,
    /// ist, ihr diesen Tag zu geben – die Systemuhr des Rechners lässt sich nicht
    /// auf einen bestimmten Tag stellen. Im Betrieb führt `neu` an die Uhr.
    fn neu_am(raw: &str, heute: NaiveDate) -> Self {
        let klein = raw.trim().to_lowercase();

        Self {
            original: raw.trim().to_string(),
            worte: klein
                .split_whitespace()
                .map(|wort| wort.to_string())
                .collect(),
            klein,
            heute,
        }
    }
}

/// Löst die Worte des Benutzers in einen Tag auf.
///
/// Ein ausgeschriebenes Datum hat Vorrang: „am 5. Oktober 14 Uhr“ darf nicht über
/// die Worte „am“ stolpern. Ohne jedes Wort bleibt der heutige Tag – „14 Uhr“
/// heißt heute um 14 Uhr.
fn ermittle_tag(angabe: &Angabe) -> NaiveDate {
    match datum_aus(&angabe.worte, angabe.heute) {
        Some(datum) => datum,
        // Die Rückwärtswörter stehen vor „morgen“, sonst fiele „gestern“ über
        // das „morgen“ darin auf den heutigen Tag. Sie fehlten hier und machten
        // aus „lösche den Termin von gestern 14 Uhr“ eine Suche nach dem
        // heutigen Tag – ohne Treffer, mit einer rätselhaften Fehlermeldung.
        None if angabe.klein.contains("übermorgen") || angabe.klein.contains("uebermorgen") => {
            angabe.heute + Duration::days(2)
        }
        None if angabe.klein.contains("vorgestern") => angabe.heute - Duration::days(2),
        None if angabe.klein.contains("gestern") => angabe.heute - Duration::days(1),
        None if angabe.klein.contains("morgen") => angabe.heute + Duration::days(1),
        None if angabe.klein.contains("heute")
            || angabe.klein.contains("jetzt")
            || angabe.klein.contains("woche") =>
        {
            angabe.heute
        }
        None => match wochentag_datum(&angabe.klein, angabe.heute) {
            Some(datum) => datum,
            None => match tage_in_aus(&angabe.worte) {
                Some(tage) => angabe.heute + Duration::days(tage),
                None => angabe.heute,
            },
        },
    }
}

/// Ein Datum aus den Worten des Benutzers, auch **ohne** Uhrzeit.
///
/// „morgen“ genügt. Wird gebraucht, wenn aus einer Titelangabe ein einzelner
/// Termin werden soll: „das Teammeeting morgen“ nennt keinen Zeitpunkt.
pub fn parse_tag(raw: &str) -> Result<NaiveDate, String> {
    let angabe = Angabe::neu(raw);

    // Anders als bei einer Uhrzeit darf hier nichts auf den heutigen Tag
    // hinauslaufen: „irgendwann mal“ ist kein Datum. Sonst würde „welches Teammeeting
    // meinst du, irgendwann mal“ auf den heutigen Tag zeigen und damit auf einen
    // ganz anderen Termin.
    let nennt_einen_tag = !angabe.worte.is_empty()
        && (datum_aus(&angabe.worte, angabe.heute).is_some()
            || [
                "heute",
                "jetzt",
                "morgen",
                "übermorgen",
                "uebermorgen",
                "gestern",
                "vorgestern",
                "woche",
                "tag",
                "tage",
            ]
            .iter()
            .any(|wort| angabe.klein.contains(wort))
            || wochentag_aus(&angabe.worte).is_some()
            || tage_in_aus(&angabe.worte).is_some());

    if !nennt_einen_tag {
        return Err(format!(
            "„{}“ nennt keinen Tag. Schreib es als JJJJ-MM-TT oder in den Worten des \
             Benutzers, etwa „morgen“.",
            angabe.original
        ));
    }

    let tag = ermittle_tag(&angabe);

    if !jahr_plausibel(tag.year()) {
        return Err(jahr_fehler(tag.year()));
    }

    Ok(tag)
}

/// Setzt Datum und Uhrzeit zusammen und prüft die Plausibilität.
fn mit_uhrzeit(
    tag: NaiveDate,
    worte: &[String],
    original: &str,
) -> Result<DateTime<Local>, String> {
    if !jahr_plausibel(tag.year()) {
        return Err(jahr_fehler(tag.year()));
    }

    let Some((stunde, minute)) = stunde_aus(worte) else {
        // Eine Tageszeit wie „früh" oder „mittags" ist eine echte Angabe und
        // keine Lücke: Der Benutzer meint den Vormittag, nicht irgendeine
        // Uhrzeit. Sie wird hier festgelegt, damit aus „übermorgen früh" ein
        // Termin wird statt einer Rückfrage.
        //
        // Aus dem ersten Validierungslauf: Das Modell gab „übermorgen früh" und
        // „Donnerstagmittag" wörtlich weiter, wie es die Anweisung verlangt, und
        // Mimir lehnte beide ab. Zurückgewiesen wird hier nichts – nur ergänzt.
        if let Some((stunde, minute)) = tageszeit_aus(worte) {
            return Local
                .from_local_datetime(&tag.and_hms_opt(stunde, minute, 0).expect("Stunde geprüft"))
                .earliest()
                .ok_or_else(|| ZEIT_HINWEIS.to_string());
        }

        return Err(format!(
            "Zu „{original}“ fehlt die Uhrzeit. Nenne sie mit, etwa „morgen 14:00“."
        ));
    };

    Local
        .from_local_datetime(&tag.and_hms_opt(stunde, minute, 0).expect("Stunde geprüft"))
        .earliest()
        .ok_or_else(|| ZEIT_HINWEIS.to_string())
}

/// Die deutschen Monatsnamen und ihre üblichen Abkürzungen.
const MONATE: [(&str, u32); 25] = [
    ("januar", 1),
    ("jan", 1),
    ("februar", 2),
    ("feb", 2),
    ("märz", 3),
    ("maerz", 3),
    ("mrz", 3),
    ("mar", 3),
    ("april", 4),
    ("apr", 4),
    ("mai", 5),
    ("juni", 6),
    ("jun", 6),
    ("juli", 7),
    ("jul", 7),
    ("august", 8),
    ("aug", 8),
    ("september", 9),
    ("sep", 9),
    ("oktober", 10),
    ("okt", 10),
    ("november", 11),
    ("nov", 11),
    ("dezember", 12),
    ("dez", 12),
];

/// Sucht ein ausgeschriebenes Datum in der Angabe.
///
/// Gelesen werden `2026-09-14` mit jedem üblichen Trenner, `14.9.`, `14.09.2026`
/// sowie `5. Oktober` und `Oktober 5`. Fehlt das Jahr, gilt das laufende –
/// aber nur, wenn der Tag noch nicht vorbei ist. „14.9.“ bedeutet im Oktober
/// also den nächsten 14. September und nicht den vergangenen.
fn datum_aus(worte: &[String], heute: NaiveDate) -> Option<NaiveDate> {
    for (position, wort) in worte.iter().enumerate() {
        let teile: Vec<&str> = wort
            .split(['-', '.', '/'])
            .filter(|t| !t.is_empty())
            .collect();
        let alles_ziffern = !teile.is_empty()
            && teile
                .iter()
                .all(|teil| teil.chars().all(|c| c.is_ascii_digit()));

        if alles_ziffern {
            match teile.len() {
                3 if teile[0].len() == 4 => {
                    if let Some(datum) = as_date(
                        teile[0].parse().ok()?,
                        teile[1].parse().ok()?,
                        teile[2].parse().ok()?,
                    ) {
                        return Some(datum);
                    }
                }
                3 => {
                    if let Some(datum) = as_date(
                        teile[2].parse().ok()?,
                        teile[1].parse().ok()?,
                        teile[0].parse().ok()?,
                    ) {
                        return Some(datum);
                    }
                }
                2 => {
                    // Tag.Monat ohne Jahr.
                    let tag: u32 = teile[0].parse().ok()?;
                    let monat: u32 = teile[1].parse().ok()?;

                    if let Some(datum) = as_date(heute.year(), monat, tag) {
                        return Some(if datum < heute {
                            match as_date(heute.year() + 1, monat, tag) {
                                Some(folges) => folges,
                                None => datum,
                            }
                        } else {
                            datum
                        });
                    }
                }
                _ => {}
            }
        }

        // „5. Oktober“ und „Oktober 5“
        if let Some(monat) = monat_aus(wort) {
            for nachbar in [position.checked_sub(1), Some(position + 1)] {
                let Some(nachbar) = nachbar.filter(|i| *i < worte.len()) else {
                    continue;
                };
                let tag: Option<u32> = worte[nachbar]
                    .split(['.', ','])
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .parse()
                    .ok();

                let Some(tag) = tag else { continue };

                let Some(datum) = as_date(heute.year(), monat, tag) else {
                    continue;
                };

                // Ein Datum ohne Jahr darf nicht in die Zukunft springen, nur
                // weil es schon vorbei ist: „der Termin am 30. September“
                // meint den von diesem Jahr, nicht den nächsten. Der Sprung auf
                // das Folgejahr bleibt den reinen Wochentagen vorbehalten
                // („nächsten Montag“), wo die Zukunft die einzig lesbare Deutung
                // ist. Sonst landete die Suche auf einem 30. September, den es
                // noch gar nicht gibt.
                return Some(datum);
            }
        }
    }

    None
}

fn as_date(jahr: i32, monat: u32, tag: u32) -> Option<NaiveDate> {
    if !(1..=12).contains(&monat) || tag == 0 || tag > 31 {
        return None;
    }

    NaiveDate::from_ymd_opt(jahr, monat, tag)
}

fn monat_aus(wort: &str) -> Option<u32> {
    let klein = wort
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    if klein.is_empty() {
        return None;
    }

    MONATE
        .iter()
        .find(|(name, _)| *name == klein)
        .map(|(_, monat)| *monat)
}

/// Die Wochentage, wie der Benutzer sie sagt.
///
/// Zwei Schreibweisen fehlten hier, und beide haben einen Wochentag lahm gemacht:
/// „mitwoch“ statt „mittwoch“ und „sonnabend“ statt „sonnabend“. Wer „trage mich
/// mittwoch ein“ sagte, bekam den heutigen Tag – der Tag ließ sich nicht finden,
/// also blieb nichts übrig als heute.
const WOCHENTAGE: [(&str, u8); 16] = [
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

/// Sucht einen Wochentag in der Angabe.
fn wochentag_aus(worte: &[String]) -> Option<u8> {
    for wort in worte {
        let sauber = wort
            .trim_matches(|c: char| !c.is_alphabetic())
            .to_lowercase();
        if sauber.len() < 2 {
            continue;
        }

        if let Some((_, nummer)) = WOCHENTAGE.iter().find(|(name, _)| *name == sauber) {
            return Some(*nummer);
        }
    }

    None
}

/// Löst einen Zeitraum-Angabe in einen Tag auf – auch aus **Wörtern**.
///
/// Für das lesende Kalenderwerkzeug. Der Grund ist die Praxis: Nach dem neuen
/// Schema sollte das Modell `from` als `JJJJ-MM-TT` schicken, und es schickte
/// statt dessen `from: übermorgen`. Ein Parser, der nur Datumsgrenzen annimmt,
/// lässt das stillschweigend ins Leere laufen und antwortet mit 30 Tagen – „in
/// diesem Zeitraum von übermorgen keine Termine“, was nach einer leeren Agenda
/// aussieht und in Wahrheit eine falsche Abfrage ist.
///
/// Deshalb werden hier dieselben Worte gelesen wie beim Schreiben: `morgen`,
/// `übermorgen`, `gestern`, `montag`, `in drei Tagen`, `5. Oktober`. Die Regel
/// steht an einer Stelle; ein zweiter, abweichender Leser driftet auseinander.
///
/// Fehlt jeder verwertbare Tag, ist das ein Fehler und kein heute: Eine leere
/// Angabe ist kein Wunsch nach dem heutigen Tag.
pub fn parse_zeitraum_tag(raw: &str, heute: NaiveDate) -> Result<NaiveDate, String> {
    let angabe = Angabe::neu_am(raw, heute);
    let gefunden = datum_aus(&angabe.worte, heute)
        .or_else(|| {
            if angabe.klein.contains("übermorgen") || angabe.klein.contains("uebermorgen") {
                Some(heute + Duration::days(2))
            } else if angabe.klein.contains("vorgestern") {
                Some(heute - Duration::days(2))
            } else if angabe.klein.contains("gestern") {
                Some(heute - Duration::days(1))
            } else if angabe.klein.contains("morgen") {
                Some(heute + Duration::days(1))
            } else if angabe.klein.contains("heute") || angabe.klein.contains("jetzt") {
                Some(heute)
            } else {
                None
            }
        })
        .or_else(|| wochentag_datum(&angabe.klein, heute))
        .or_else(|| tage_in_aus(&angabe.worte).map(|t| heute + Duration::days(t)));

    gefunden.ok_or_else(|| {
        format!(
            "\"{raw}\" nennt keinen Tag. Schick ein Datum als JJJJ-MM-TT, \
oder ein Wort wie „morgen“, „montag“ oder „in drei Tagen“."
        )
    })
}

/// Die nächste noch kommende Gelegenheit für diesen Wochentag.
///
/// „Montag“ an einem Montag meint den nächsten Montag, nicht den heutigen: Ein
/// Termin, den man „montags“ nennt, liegt in der Zukunft. Steht ausdrücklich
/// „heute“ davor, greift die Tagesauflösung weiter oben.
///
/// Achtung bei der Zählung: Chrono hat zwei Zählweisen. `number_from_monday()`
/// beginnt bei **eins**, `num_days_from_monday()` und der Cast auf `u8` bei
/// **null**. `WOCHENTAGE` ist null-basiert, also muss der heutige Wochentag über
/// dieselbe Zählung laufen. Mit der falschen Mischung landete jeder Wochentag
/// einen Tag zu früh: „montag“ am Montag ergab den Sonntag.
fn naechster_wochentag(heute: NaiveDate, gesucht: u8) -> NaiveDate {
    // Der Schritt um eine Woche geht bei `rem_euclid(7)` von 6 auf 0, und ohne
    // den Sprung über 0 käme der heutige Wochentag zurück: „montag“ an einem
    // Montag ergäbe heute. Das soll den nächsten Montag treffen, denn ein
    // ausgesprochener Wochentag meint einen Termin in der Zukunft.
    let bis_wochentag =
        (gesucht as i64 - heute.weekday().num_days_from_monday() as i64).rem_euclid(7);
    // Null heißt: Der Wochentag ist **heute**. Dann sind es 7 Tage, nicht 0 –
    // ein ausgesprochener Wochentag meint einen Termin in der Zukunft. Und
    // ausdrücklich nicht 1: Ein Tag später ist der nächste Wochentag, nicht
    // derselbe, und „montag“ an einem Montag ergab so den Dienstag.
    let schritte = if bis_wochentag == 0 { 7 } else { bis_wochentag };

    heute + Duration::days(schritte)
}

/// Der Wochentag aus einer Angabe, mit „diese“, „nächste“ und „übernächste“ Woche.
///
/// Ohne Zusatz ist es der **nächste** Wochentag – auf derselben Seite wie heute
/// liegt, nie der vorige. Mit Zusatz verschiebt sich das um ganze Wochen:
///
/// - „montag“ am Donnerstag → der Montag in vier Tagen
/// - „übernächster montag“ → der Montag in zwei Wochen
/// - „montag nächste woche“ → der Montag nach dem kommenden
/// - „montag diese woche“ → der Montag dieser Woche, auch wenn er schon vorbei war
///
/// Die Reihenfolge der Prüfungen ist wichtig: „übernächster“ enthält „nächst“,
/// würde also von einer allgemeinen „nächste“-Regel mitgenommen. Der
/// übernächste Fall kommt deshalb zuerst.
///
/// Beide Schreibweisen mit Umlaut und mit „ue“ stehen drin, weil ein Benutzer
/// „uebernachster“ schreibt, der kein „ü“ auf der Taste findet, und weil die
/// Angabe sonst als unbekanntes Wort durchfällt.
fn wochentag_datum(text: &str, heute: NaiveDate) -> Option<NaiveDate> {
    let klein = text.trim().to_lowercase();
    let worte: Vec<String> = klein.split_whitespace().map(|w| w.to_string()).collect();
    let gesucht = wochentag_aus(&worte)?;
    let naechster = naechster_wochentag(heute, gesucht);

    // „diese Woche“ geht vor: Sie meint die laufende, und die kann schon
    // vorbei sein. Ein Wochentag in der Vergangenheit wird hier nicht
    // nach vorn gerutscht – „montag diese woche“ an einem Donnerstag meint den
    // Montag dieser Woche, auch wenn das drei Tage zurück liegt.
    if klein.contains("diese woche") || klein.contains("dieser woche") {
        let eigener = Duration::days(heute.weekday().num_days_from_monday() as i64);

        return Some(heute - eigener + Duration::days(gesucht as i64));
    }

    // „übernächster“ zuerst: Das Wort enthält „nächst“ und würde sonst von der
    // allgemeinen Regel unten mitgenommen.
    if uebernaechst(&klein) {
        return Some(naechster + Duration::days(7));
    }

    if klein.contains("nächste woche")
        || klein.contains("naechste woche")
        || klein.contains("kommende woche")
    {
        return Some(naechster + Duration::days(7));
    }

    if klein.contains("letzte woche") || klein.contains("vorige woche") {
        return Some(naechster - Duration::days(7));
    }

    Some(naechster)
}

/// Steht in der Angabe ein „übernächst“ – mit Umlaut oder als „ue“?
///
/// Beide Schreibweisen, weil eine fehlende Taste kein Grund ist, den Wochentag
/// zu verlieren. Und weil „übernächst“ das „nächst“ enthält, muss es eigens
/// geprüft werden: Wer nur auf „nächst“ prüft, nimmt den **nächsten** und lässt
/// das „über“ weg – „übernächster Montag“ landete damit auf demselben Tag wie
/// „Montag“.
fn uebernaechst(klein: &str) -> bool {
    klein.contains("übernext")
        || klein.contains("uebernext")
        || klein.contains("übernächst")
        || klein.contains("uebernachst")
        || klein.contains("über nächsten")
        || klein.contains("ueber naechsten")
        || klein.contains("übernaechsten")
        || klein.contains("übernachster")
}

/// Liest „in drei Tagen“ und „in 2 Tagen“.
fn tage_in_aus(worte: &[String]) -> Option<i64> {
    let mut gefunden: Option<i64> = None;

    for (position, wort) in worte.iter().enumerate() {
        if !(wort.contains("tag") || wort.contains("tage")) {
            continue;
        }

        // Die Zahl steht meist davor („in 3 Tagen“), seltener danach.
        for nachbar in [position.checked_sub(1), Some(position + 1)] {
            let Some(nachbar) = nachbar.filter(|i| *i < worte.len()) else {
                continue;
            };
            if let Ok(zahl) = worte[nachbar]
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<i64>()
            {
                gefunden = Some(zahl.clamp(1, 366));
            }
        }
    }

    gefunden
}

/// Sucht die Uhrzeit in der Angabe.
///
/// Gelesen werden `14:00`, `14.00`, `14 Uhr` und ein nacktes `14`, aber nur
/// wenn daraus ausdrücklich eine Uhrzeit gemacht wird: „am 14.09.2026“ ist ein
/// Datum, keine 14 Uhr.
fn stunde_aus(worte: &[String]) -> Option<(u32, u32)> {
    for wort in worte {
        let sauber =
            wort.trim_matches(|c: char| !c.is_ascii_digit() && c != ':' && c != '.' && c != ':');

        if let Some((stunde, minute)) = mit_doppelpunkt(sauber) {
            return Some((stunde, minute));
        }

        // „14 Uhr“ und „14 Uhr 30“
        if sauber.len() <= 2 && sauber.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(stunde) = sauber.parse::<u32>() {
                return uhrzeit_pruefen(stunde, 0);
            }
        }
    }

    // Uhrzeit und Minute in getrennten Wörtern: „um 14 und 30“
    for (position, wort) in worte.iter().enumerate() {
        let sauber = wort.trim_matches(|c: char| !c.is_ascii_digit());
        if sauber.is_empty() || sauber.len() > 2 {
            continue;
        }

        let Ok(stunde) = sauber.parse::<u32>() else {
            continue;
        };

        let mut minute = 0;
        if let Some(nachbar) = worte.get(position + 1) {
            let n = nachbar.trim_matches(|c: char| !c.is_ascii_digit());

            if n.len() == 2 {
                if let Ok(wert) = n.parse::<u32>() {
                    minute = wert;
                }
            }
        }

        if let Some(zeit) = uhrzeit_pruefen(stunde, minute) {
            return Some(zeit);
        }
    }

    None
}

/// Die Tageszeiten, die Deutschland kennt.
///
/// Nicht geraten und nicht aus einer Skala errechnet: Für diese sechs Wörter
/// gibt es eine gebräuchliche Uhrzeit, und sie steht hier. Jede andere
/// Tageszeitangabe – „vormittags", „gegen Mittag", „am frühen Abend" – bleibt
/// unbehandelt und führt zur Nachfrage.
///
/// Der Sinn ist nicht Bequemlichkeit, sondern eine Fehlerquelle weniger. Ein
/// geratener Vormittag wäre ein Termin, den der Benutzer nicht bestellt hat,
/// und die Vorschau wäre die letzte Stelle, an der es auffällt.
const TAGESZEITEN: [(&str, u32, u32); 7] = [
    // "nachts" steht am Ende: Es geht bei den Terminen um Schlafenszeiten.
    ("nachts", 22, 0),
    ("frühmorgens", 6, 30),
    ("früh", 8, 0),
    ("vormittag", 9, 0),
    ("vormittags", 9, 0),
    ("mittag", 12, 0),
    ("abend", 18, 0),
];

/// Liest eine Tageszeit wie „früh" oder „mittags" als Uhrzeit.
///
/// Bewusst **kein** `contains`: „Frühstück" enthält „früh", und daraus 8 Uhr zu
/// machen wäre eine Fabel. Gesucht wird deshalb nur in ganzen Wörtern.
fn tageszeit_aus(worte: &[String]) -> Option<(u32, u32)> {
    for wort in worte {
        if let Some((_, stunde, minute)) = TAGESZEITEN
            .iter()
            .find(|(name, _, _)| *name == reiner(wort))
        {
            return Some((*stunde, *minute));
        }
    }

    None
}

/// Setzt ein Ende ohne Tagesangabe auf den Tag des Beginns.
///
/// Ein Ende, das nur eine Uhrzeit nennt, ist für sich allein **heute**: „15:00"
/// heißt nicht Freitag 15:00, sondern heute 15:00. Als Ende eines Termins, der
/// am Freitag beginnt, läge es damit in der Vergangenheit – und der Termin
/// würde abgelehnt, obwohl die Angabe des Benutzers genau eine war.
///
/// Deshalb wird es hier auf den Tag des Beginns gesetzt. Ein Ende **mit**
/// Tagesangabe bleibt, wie es ist: „morgen 10 Uhr“ zu einem Termin am Freitag
/// ist ein Tag später und wird nicht umgedeutet.
fn ende_am_tag(ende: DateTime<Utc>, start: DateTime<Utc>) -> DateTime<Utc> {
    if ende.date_naive() >= start.date_naive() {
        return ende;
    }

    // Die Uhrzeit kommt in der Zeitzone des Rechners, nicht in UTC:
    // `parse_term` liefert UTC, und „15:00" ist dort 13:00 – diese Zahl auf den
    // Tag des Beginns zu legen hübe den Termin eine Stunde verschoben und
    // wieder vor den Beginn.
    let start_ort = start.with_timezone(&Local);
    let ende_zeit = ende.with_timezone(&Local).time();

    Local
        .from_local_datetime(&start_ort.date_naive().and_time(ende_zeit))
        .earliest()
        .map(|lokal| lokal.with_timezone(&Utc))
        // Kann nicht ausfallen: ein gültiger Tag mit einer gültigen Uhrzeit.
        .unwrap_or(ende)
}

/// Zerlegt `14:00` und `14.00`.
fn mit_doppelpunkt(wort: &str) -> Option<(u32, u32)> {
    let teile: Vec<&str> = wort.split([':', '.']).collect();
    if teile.len() != 2 {
        return None;
    }

    let stunde: u32 = teile[0].parse().ok()?;
    let minute: u32 = teile[1].parse().ok()?;

    uhrzeit_pruefen(stunde, minute)
}

fn uhrzeit_pruefen(stunde: u32, minute: u32) -> Option<(u32, u32)> {
    if stunde > 23 || minute > 59 {
        return None;
    }

    Some((stunde, minute))
}

/// Zerlegt eine Zeitangabe in den Zeitpunkt, den sie bezeichnet.
///
/// Angenommen wird, was ein Modell üblicherweise liefert: `JJJJ-MM-TTThh:mm`,
/// mit Sekunden, mit `Z` oder mit einem Versatz. Ohne Angabe gilt die Zeit des
/// Rechners, weil ein Kalendertermin eine lokale Zeit ist. Mit Versatz ist die
/// Angabe ein Zeitpunkt und wird nicht noch einmal als Ortszeit gelesen – sonst
/// rutscht der Termin um die Zonenzeit des Rechners.
///
/// Geschrieben wird UTC, damit keine Zeitzonentabelle im Kalender landen muss.
fn parse_zeit(raw: &str) -> Result<DateTime<Utc>, String> {
    let wert = raw.trim();
    let hinweis = ZEIT_HINWEIS;

    // Datum, T und mindestens Stunde:Minute. Alles Kürzere ist keine Zeit.
    if wert.len() < 16 || wert.as_bytes().get(10) != Some(&b'T') {
        return Err(hinweis.to_string());
    }

    // Der Versatz steht hinter der Uhrzeit, nicht am Ende der ganzen Zeile:
    // Bei „09:00:00+02:00“ endet die Angabe auf einer Ziffer.
    let (zeit, versatz_minuten) = match wert[10..].find(['+', '-', 'Z', 'z']) {
        None => (wert, 0i64),
        Some(position) => {
            let zeit = &wert[..10 + position];
            let versatz = &wert[10 + position..];
            let minuten = if versatz.starts_with('Z') || versatz.starts_with('z') {
                0
            } else {
                lese_versatz(versatz)?
            };

            (zeit, minuten)
        }
    };

    for muster in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(zeit.trim(), muster) {
            if !jahr_plausibel(naive.year()) {
                return Err(jahr_fehler(naive.year()));
            }

            // Mit Versatz ist es ein Zeitpunkt, ohne Versatz eine Ortszeit.
            if versatz_minuten != 0 {
                return Ok(Utc.from_utc_datetime(&(naive - Duration::minutes(versatz_minuten))));
            }

            return Local
                .from_local_datetime(&naive)
                .earliest()
                .ok_or_else(|| ZEIT_HINWEIS.to_string())
                .map(|zeit| zeit.with_timezone(&Utc));
        }
    }

    Err(hinweis.to_string())
}

/// Liest einen Versatz wie `+02:00` in Minuten. Das Vorzeichen steht vorn.
fn lese_versatz(wert: &str) -> Result<i64, String> {
    let vorzeichen = match wert.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err(ZEIT_HINWEIS.to_string()),
    };

    if wert.len() != 6 || wert.as_bytes()[3] != b':' {
        return Err(ZEIT_HINWEIS.to_string());
    }

    let stunden: i64 = wert[1..3].parse().map_err(|_| ZEIT_HINWEIS.to_string())?;
    let minuten: i64 = wert[4..6].parse().map_err(|_| ZEIT_HINWEIS.to_string())?;

    if stunden > 14 || minuten > 59 {
        return Err(ZEIT_HINWEIS.to_string());
    }

    Ok(vorzeichen * (stunden * 60 + minuten))
}

/// Jahreszahlen, bei denen Mimir aufhört, nachzufragen.
///
/// Ein Termin im Jahr 1700 oder 2999 kommt von einem gerechneten Datum, nicht
/// von einem Wunsch. Es nachzufragen kostet eine Runde und verhindert einen
/// Termin, der zwanzig Jahre in der Zukunft liegt.
fn jahr_plausibel(jahr: i32) -> bool {
    (MIN_JAHR..=MAX_JAHR).contains(&jahr)
}

fn jahr_fehler(jahr: i32) -> String {
    format!(
        "Das Jahr {jahr} liegt außerhalb dessen, was Mimir anlegt ({MIN_JAHR} bis {MAX_JAHR}). \
         Nenne ein Jahr in dieser Spanne."
    )
}

/// Entfernt, was in einem Kalendertext nichts zu suchen hat, und schneidet die
/// Ränder ab.
///
/// Wagenrückläufe und Nullbytes fliegen raus: Sie würden die Datei auftrennen
/// oder als Steuerzeichen im Kalender landen. Zeilenumbrüche bleiben erhalten –
/// sie werden später zu `\n` escaped und stehen dann als Absatz in der
/// Beschreibung.
pub fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character != '\r' && *character != '\0')
        .collect::<String>()
        .trim()
        .to_string()
}

/// So viele Wörter von mindestens drei Zeichen braucht es, damit eine Prüfung
/// überhaupt etwas entscheidet.
///
/// Drei ist eine bewusst niedrige Schwelle. Sie steht gegen zwei Versuche
/// gleichzeitig: „Mach morgen einen Termin“ (zwei Wörter) soll nichts prüfen,
/// und „Trag morgen um 14 Uhr einen Termin ein“ (fünf) soll prüfen. Wer eine
/// Adresse prüfen will, nennt meistens mehr als zwei Wörter.
const MIN_WOERTER: usize = 3;

/// Kommt dieses Wort in den Worten des Benutzers vor?
///
/// Das ist die Grundlage für das Verwerfen erfundener Felder. Aus dem zweiten
/// Validierungslauf: Das Modell schickte zu „Mach heute noch einen Anruf bei der
/// IT, Kategorie Arbeit" zusätzlich einen Kalendernamen, den der Benutzer nie
/// genannt hatte, und zu „Ich brauche morgen einen Termin mit der Bank" den Ort
/// „Online". Beides steht danach im Kalender.
///
/// Der Vergleich ist bewusst **Wort für Wort**, nicht „ist der Wert ein
/// Teilstring": `Ort Talstraße 8` enthält nicht `straße`, und eine Adresse, die
/// sich im Text wiederfindet, soll stehen bleiben. Umgekehrt gilt dasselbe – „Praxis
/// am Talstraße 8" passt nicht auf „Ort Talstraße 8", weil „Praxis" fehlt.
///
/// Groß- und Kleinschreibung, Umlaute, Bindestriche und Punkte werden
/// überbrückt: „Talstraße" und „Talstrasse" gelten als dasselbe Wort, und
/// „St.-Nikolaus" soll an „Nikolaus" ankommen.
fn wort_gefunden(wert: &str, woerter: &[String]) -> bool {
    let gesucht: Vec<String> = wert
        .split_whitespace()
        .map(normalisiere)
        .filter(|wort| !wort.is_empty())
        .collect();

    if gesucht.is_empty() {
        return false;
    }

    // Gesucht wird in der **normalisierten Fassung des ganzen Satzes**, nicht in
    // einzelnen Wörtern. „St.-Nikolaus" wird dabei zu „stnikolaus" – würde man
    // es im Satz des Benutzers „in St. Nikolaus" suchen, stünden dort „st" und
    // „nikolaus" getrennt und der Vergleich schlüge fehl.
    //
    // „Enthalten" statt „gleich": „Stock 2" steckt in „im zweiten Stock" nicht,
    // das ist aber kein Fehler dieser Prüfung, sondern ein Fall, in dem das
    // Modell umgerechnet hat – und dafür sind die Zahlenfelder da.
    gesucht
        .iter()
        .all(|wort| woerter.iter().any(|kandidat| kandidat.contains(wort)))
}

/// Bringt ein Wort auf einen gemeinsamen Nenner, damit verglichen werden kann.
///
/// Kleinbuchstaben ohne alles, was kein Buchstabe oder keine Ziffer ist, und
/// `ß` wird zu `ss`. Ohne das wäre „Talstraße" nie als „Talstrasse" wiederzufinden,
/// und ein Punkt in einer Abkürzung würde jedes Wort dahinter zerreißen.
fn normalisiere(wort: &str) -> String {
    wort.chars()
        .flat_map(|character| {
            let klein = character.to_lowercase();
            if character == 'ß' {
                return vec!['s', 's'];
            }
            klein.filter(|zeichen| zeichen.is_alphanumeric()).collect()
        })
        .collect()
}

/// Die inhaltstragenden Wörter einer Nachricht.
///
/// Ohne Zahlen und ohne Einmaleins: Eine Uhrzeit soll nicht daran scheitern, dass
/// der Benutzer „15" sagte und das Modell „15:00" schrieb. Die Zahl steckt im
/// Zeitfeld und wird ohnehin strenger geprüft.
fn benutzerwoerter(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(normalisiere)
        .filter(|wort| !wort.is_empty())
        .collect()
}

/// Soll dieses Feld bleiben, weil der Benutzer es genannt hat?
///
/// **Ohne Benutzertext bleibt jedes Feld stehen.** Das ist die wichtige Grenze:
/// Die Prüfung darf nie etwas entfernen, weil sie ihre Grundlage nicht kennt. Wo
/// die Worte des Benutzers nicht mitkommen – ein Aufruf aus einem anderen Weg,
/// ein alter Verlauf –, wird nichts verworfen und nichts gemeldet.
pub fn feld_gedeckt(wert: &str, benutzertext: &str, felder: &[&str]) -> bool {
    let woerter = benutzerwoerter(benutzertext);

    // Zu wenig Ausgangstext heißt: nichts prüfen. Ein einzelnes „15" ist keine
    // Grundlage für eine Wortvergleichsprüfung – damit würde Mimir Orte
    // verwerfen, nur weil der Benutzer eine Uhrzeit genannt hat.
    //
    // Die Schwelle liegt bewusst tief. Sie zu hoch zu setzen hieße, dass ein
    // kurzer Satz „Termin morgen“ nichts prüfen dürfte, und genau dort erfand
    // das Modell am meisten. Der Preis der falschen Richtung ist nur ein
    // stehengebliebener Ort, der zweite ein falscher.
    if woerter.iter().filter(|wort| wort.len() >= 3).count() < MIN_WOERTER {
        return true;
    }

    wert.split_whitespace()
        .all(|wort| felder.contains(&wort) || wort_gefunden(wort, &woerter))
}

/// Maskiert die Zeichen, die in einem ICS-Wert eine Bedeutung haben.
///
/// Komma, Semikolon und Backslash trennen Felder, ein Zeilenumbruch würde die
/// Zeile zerlegen. Ohne diese Maskierung könnte ein harmloser Text wie
/// `a, b` die Beschreibung als zwei Felder lesbar machen.
pub fn escape_text(value: &str) -> String {
    let mut ergebnis = String::with_capacity(value.len());

    for (index, zeile) in value.split('\n').enumerate() {
        if index > 0 {
            ergebnis.push_str("\\n");
        }

        for character in zeile.chars() {
            match character {
                '\\' => ergebnis.push_str("\\\\"),
                ';' => ergebnis.push_str("\\;"),
                ',' => ergebnis.push_str("\\,"),
                other => ergebnis.push(other),
            }
        }
    }

    ergebnis
}

/// Faltet die Zeilen nach RFC 5545.
///
/// Eine Zeile wird höchstens 74 Oktett breit (`ICS_LINE_LIMIT`), jede Fortsetzung
/// beginnt mit einem Leerzeichen und bringt es zusammen wieder auf 74. Ohne das
/// Umbruchzeichen hängt Nextcloud lange Beschreibungen an das nächste Feld und der
/// Termin erscheint kaputt. Umlaute zählen als zwei Oktett, deshalb wird nie in der
/// Mitte eines Zeichens geschnitten.
fn fold(text: &str) -> String {
    let mut ergebnis = String::with_capacity(text.len() + 16);
    let mut zeilen = text.split("\r\n").peekable();
    let mut erste = true;

    for zeile in zeilen.by_ref() {
        // Das Trennzeichen gehört zwischen zwei Zeilen, nicht vor die erste.
        if !erste {
            ergebnis.push_str("\r\n");
        }
        erste = false;

        if zeile.len() <= ICS_LINE_LIMIT {
            ergebnis.push_str(zeile);
            continue;
        }

        let ende = grenze(zeile, ICS_LINE_LIMIT);
        ergebnis.push_str(&zeile[..ende]);
        let mut rest = &zeile[ende..];

        while !rest.is_empty() {
            ergebnis.push_str("\r\n ");
            let ende = grenze(rest, ICS_LINE_LIMIT - 1);
            ergebnis.push_str(&rest[..ende]);
            rest = &rest[ende..];
        }
    }

    ergebnis
}

/// Die größte Länge, bei der noch kein Zeichen zerrissen wird.
fn grenze(text: &str, max: usize) -> usize {
    let mut ende = text.len().min(max);
    while ende > 0 && !text.is_char_boundary(ende) {
        ende -= 1;
    }
    ende
}

// --- Erinnerung --------------------------------------------------------------
// Die Erinnerung ist die einzige Angabe, die nicht am Termin selbst steht, sondern
// an ihm hängt: „5 Minuten vorher“. Sie wird deshalb wie eine Uhrzeit gerechnet,
// aber in die andere Richtung.

/// So weit vor dem Termin darf die Erinnerung liegen. Alles darüber ist mit hoher
/// Wahrscheinlichkeit keine mehr, sondern eine gerechnete Jahreszahl.
const MAX_ERINNERUNG_TAGE: i64 = 90;

const ERINNERUNG_HINWEIS: &str = "Die Erinnerung wird nicht verstanden. Schreib sie so, wie \
    du sie sagen würdest: „5 Minuten vorher“, „eine halbe Stunde vorher“, „eine Stunde \
    vorher“, „eine Woche vorher“, „am Vorabend“ oder „keine Erinnerung“.";

/// Rechnet die Worte des Benutzers für die Erinnerung in Minuten um.
///
/// `None` heißt: Die Erinnerung soll weg. `Some(Minuten)` heißt: so viele Minuten
/// **vor** dem Beginn. Eine Erinnerung *nach* dem Termin gibt es nicht – die Norm
/// sieht dafür nichts vor, und Nextcloud würde sie ohnehin nicht anzeigen.
///
/// Verstanden werden Ziffern und die Zahlwörner bis sechzig, die Einheiten
/// Minute/Stunde/Tag/Woche in allen gebräuchlichen Schreibweisen samt
/// Abkürzungen, die Bruchformen halb/viertel/dreiviertel sowie die festen Wendungen
/// „am Vorabend" und „am Vortag".
pub fn parse_erinnerung(raw: &str) -> Result<Option<i64>, String> {
    let klein = raw.trim().to_lowercase();

    if klein.is_empty() {
        return Ok(None);
    }

    let worte: Vec<&str> = klein.split_whitespace().collect();

    // Die Abschalt-Worte zuerst: Sie stehen im Widerspruch zu jeder Zeitangabe
    // und müssen deshalb vor dem Rechnen weg.
    for wort in &worte {
        if ABSCHALTEN.contains(&reiner(wort)) {
            return Ok(None);
        }
    }

    for wort in &worte {
        if NACHTRUZLICH.contains(&reiner(wort)) {
            return Err(
                "Eine Erinnerung kann nur vorher klingeln, nicht nachher. Schreib sie \
                 um, etwa „5 Minuten vorher“."
                    .to_string(),
            );
        }
    }

    // Feste Wendungen, die nicht gerechnet werden können.
    if worte
        .iter()
        .any(|wort| matches!(reiner(wort), "vorabend" | "vortag" | "vorabtags"))
    {
        return Ok(Some(1440));
    }

    let mut anzahl: Option<f64> = None;
    let mut faktor: Option<i64> = None;

    for wort in &worte {
        // Wörter, die nur Füllung sind. „vor" wird entfernt, damit „5 min vor"
        // dasselbe ergibt wie „5 min vorher" – an der Richtung ändert das nichts.
        if FUELWORT.contains(&reiner(wort)) {
            continue;
        }

        let (ziffern, rest) = zahlteil(wort);

        if !ziffern.is_empty() {
            // „5,5" ist eine Zahl; ein Komma ohne Ziffern dahinter nicht.
            let text = ziffern.trim_end_matches(['.', ',']);
            let wert: f64 = text
                .replace(',', ".")
                .parse()
                .map_err(|_| ERINNERUNG_HINWEIS.to_string())?;

            anzahl = Some(anzahl.unwrap_or(1.0) * wert);
        }

        if rest.is_empty() {
            continue;
        }

        // Der Rest kann eine Einheit sein („min"), ein Zahlwort („halb") oder
        // beides („halbe stunde" steht als zwei Wörter, „viertelstunde" als eines).
        let (wortteil, gefunden) = match einheit(rest) {
            Some((suffix, wert)) => (rest.trim_end_matches(suffix).trim(), Some(wert)),
            None => (rest, None),
        };

        if !wortteil.is_empty() {
            let wert = zahl_wort(wortteil).ok_or_else(|| ERINNERUNG_HINWEIS.to_string())?;
            anzahl = Some(anzahl.unwrap_or(1.0) * wert);
        }

        if let Some(wert) = gefunden {
            if faktor.is_some() {
                return Err(ERINNERUNG_HINWEIS.to_string());
            }

            faktor = Some(wert);
        }
    }

    let faktor = faktor.ok_or_else(|| ERINNERUNG_HINWEIS.to_string())?;
    let minuten = ((anzahl.unwrap_or(1.0) * faktor as f64).round() as i64).max(0);

    if minuten == 0 {
        return Err(
            "Eine Erinnerung genau zum Beginn ergibt keinen Sinn. Schreib eine Spanne, \
             etwa „5 Minuten vorher“, oder „keine Erinnerung“."
                .to_string(),
        );
    }

    if minuten > MAX_ERINNERUNG_TAGE * 1440 {
        return Err(format!(
            "Eine Erinnerung mehr als {MAX_ERINNERUNG_TAGE} Tage vorher passt zu keinem \
             Termin. {ERINNERUNG_HINWEIS}"
        ));
    }

    Ok(Some(minuten))
}

/// Die Wörter, mit denen die Erinnerung abgeschaltet wird.
const ABSCHALTEN: [&str; 9] = [
    "keine",
    "keiner",
    "keinen",
    "ohne",
    "weg",
    "aus",
    "nicht",
    "abschalten",
    "deaktivieren",
];

/// Wörter, die eine Erinnerung *nach* dem Termin verlangen würden.
const NACHTRUZLICH: [&str; 5] = ["nachher", "nach", "spaeter", "danach", "folgenden"];

/// Füllwörter, die für die Rechnung nichts bedeuten.
const FUELWORT: [&str; 12] = [
    "erinnerung",
    "erinnerungen",
    "erinnern",
    "vorher",
    "vor",
    "davor",
    "am",
    "der",
    "den",
    "terminbeginn",
    "terminstart",
    "beginn",
];

/// Die Zahl am Anfang eines Wortes, getrennt vom Rest.
///
/// Damit wird aus `5min`, `5 min` und `5` dieselbe Angabe, ohne dass die drei
/// Schreibweisen einzeln behandelt werden müssen.
fn zahlteil(wort: &str) -> (&str, &str) {
    let ende = wort
        .char_indices()
        .take_while(|(_, zeichen)| zeichen.is_ascii_digit() || *zeichen == ',' || *zeichen == '.')
        .map(|(index, zeichen)| index + zeichen.len_utf8())
        .last()
        .unwrap_or(0);

    wort.split_at(ende)
}

/// Die Einheit am Ende eines Wortes: der passende Suffix und sein Wert in Minuten.
///
/// Der Suffix wird zurückgegeben, damit ihn der Aufrufer vom Wort abschneiden
/// kann – sonst bliebe bei `viertelstunde` ein `viertel` zurück, das als Zahlwort
/// gelesen würde.
fn einheit(wort: &str) -> Option<(&'static str, i64)> {
    // Vollständige Wörter, auch als Suffix: „zwei wochen" ebenso wie „wochen".
    const WOERTER: [(&str, i64); 12] = [
        ("minuten", 1),
        ("minute", 1),
        ("stunden", 60),
        ("stunde", 60),
        ("tagen", 1440),
        ("tage", 1440),
        ("tag", 1440),
        ("wochen", 10080),
        ("woche", 10080),
        ("std", 60),
        ("min", 1),
        ("mins", 1),
    ];

    // Abkürzungen gelten nur als ganzes Wort. Als Suffix wären sie falsch: Das
    // „t" in „montag" wäre eine Tagesangabe, „w" in „zweiwöchentlich" eine Woche.
    const ABKURZUNGEN: [(&str, i64); 4] = [("m", 1), ("h", 60), ("d", 1440), ("w", 10080)];

    if let Some((name, wert)) = ABKURZUNGEN.iter().find(|(name, _)| *name == wort) {
        return Some((*name, *wert));
    }

    // Längste Schreibweise zuerst, sonst gewinnt bei „minuten" das kürzere „min".
    WOERTER
        .iter()
        .filter(|(name, _)| wort.ends_with(name))
        .max_by_key(|(name, _)| name.len())
        .map(|(name, wert)| (*name, *wert))
}

/// Zahlwörter, die ein Benutzer statt einer Ziffer sagen könnte.
fn zahl_wort(wort: &str) -> Option<f64> {
    Some(match wort {
        "ein" | "eine" | "einen" | "einem" | "einer" | "eins" | "1" => 1.0,
        "zwei" | "zwo" => 2.0,
        "drei" => 3.0,
        "vier" => 4.0,
        "fünf" | "fuenf" => 5.0,
        "sechs" => 6.0,
        "sieben" => 7.0,
        "acht" => 8.0,
        "neun" => 9.0,
        "zehn" => 10.0,
        "fünfzehn" | "fuenfzehn" => 15.0,
        "zwanzig" => 20.0,
        "dreißig" | "dreissig" => 30.0,
        "fünfzig" | "fuenfzig" => 50.0,
        "sechzig" => 60.0,
        "halb" | "halbe" | "halben" | "halber" => 0.5,
        "viertel" | "viertelstunde" | "viertelstunden" => 0.25,
        "dreiviertel" => 0.75,
        _ => return None,
    })
}

/// Das Wort ohne Satzzeichen, damit „keine," wie „keine“ gilt.
fn reiner(wort: &str) -> &str {
    wort.trim_matches(|zeichen: char| !zeichen.is_alphanumeric() && zeichen != '-')
}

/// Der Wert der `TRIGGER`-Zeile für eine Erinnerung, etwa `-PT5M` oder `-P1D`.
///
/// `minuten` ist der Abstand **vor** dem Beginn. Das Minus steht deshalb fest im
/// Wert und nicht in einem Parameter – genau so schreibt es die Norm für einen
/// Zeitpunkt vor dem Ereignis vor. Eine Angabe „nach dem Termin“ gibt es nicht,
/// entsprechend gibt es auch kein Vorzeichen im Aufruf.
pub fn trigger_text(minuten: i64) -> String {
    let betrag = minuten.abs();

    if betrag == 0 {
        return "-PT0M".to_string();
    }

    if betrag % 10080 == 0 {
        return format!("-P{}W", betrag / 10080);
    }

    if betrag % 1440 == 0 {
        return format!("-P{}D", betrag / 1440);
    }

    let stunden = betrag / 60;
    let reste = betrag % 60;

    if reste == 0 {
        format!("-PT{stunden}H")
    } else if stunden == 0 {
        format!("-PT{reste}M")
    } else {
        format!("-PT{stunden}H{reste}M")
    }
}

/// Dieselbe Angabe für den Menschen, im Bestätigungsfenster.
pub fn erinnerung_text(minuten: i64) -> String {
    let betrag = minuten.abs();

    if betrag == 0 {
        return "beim Beginn".to_string();
    }

    if betrag >= 1440 {
        let tage = betrag / 1440;

        if tage == 1 {
            return "am Vorabend".to_string();
        }

        return format!("{tage} Tage vorher");
    }

    let stunden = betrag / 60;
    let minuten = betrag % 60;

    match (stunden, minuten) {
        (0, _) => format!("{minuten} Minuten vorher"),
        (_, 0) => format!(
            "{stunden} Stunde{} vorher",
            if stunden == 1 { "" } else { "n" }
        ),
        _ => format!("{stunden} Stunden {minuten} Minuten vorher"),
    }
}

/// Formatiert einen Zeitpunkt für die Anzeige, auf Deutsch.
fn display_time(moment: DateTime<Utc>) -> String {
    let lokal = moment.with_timezone(&Local);
    let heute = Local::now().date_naive();
    let datum = if lokal.date_naive() == heute {
        "heute".to_string()
    } else if lokal.date_naive() == heute + Duration::days(1) {
        "morgen".to_string()
    } else {
        lokal.format("%d.%m.%Y").to_string()
    };

    format!("{datum}, {:02}:{:02} Uhr", lokal.hour(), lokal.minute())
}

/// Baut die ICS-Datei. Die Zeiten stehen als UTC drin, damit der Kalender ohne
/// Zeitzonentabelle auskommt; die Anzeige rechnet sie lokal zurück.
fn build_ics(
    uid: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    request: &EventRequest,
    erinnerung: Option<i64>,
    kategorie: Option<&str>,
) -> String {
    let mut vevent = vec![
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", super::client::now().format("%Y%m%dT%H%M%SZ")),
    ];

    if request.all_day {
        // Ganztagestermine tragen ein reines Datum, und das Ende ist der erste
        // Tag danach – so ist es in der Norm festgelegt und Nextcloud zeigt den
        // Termin nicht um einen Tag zu kurz an.
        vevent.push(format!("DTSTART;VALUE=DATE:{}", start.format("%Y%m%d")));
        vevent.push(format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")));
    } else {
        vevent.push(format!("DTSTART:{}", start.format("%Y%m%dT%H%M%SZ")));
        vevent.push(format!("DTEND:{}", end.format("%Y%m%dT%H%M%SZ")));
    }

    vevent.push(format!("SUMMARY:{}", escape_text(&request.summary)));

    if let Some(location) = &request.location {
        vevent.push(format!("LOCATION:{}", escape_text(location)));
    }

    if let Some(description) = &request.description {
        vevent.push(format!("DESCRIPTION:{}", escape_text(description)));
    }

    // Die Kategorien stehen als eine Zeile mit Semikolon als Trenner, weil das
    // so die Norm vorsieht. Jeder einzelne Wert wird maskiert – sonst würde ein
    // Komma im Namen zwei Kategorien ergeben.
    if let Some(kategorien) = kategorie.map(kategorien_text) {
        vevent.push(format!("CATEGORIES:{kategorien}"));
    }

    // Ohne Teilnehmer bleibt es: Eine Einladung verlässt den Rechner. Die
    // Erinnerung wird weiter unten angehängt, weil sie ein eigener Block ist.
    vevent.push("STATUS:CONFIRMED".to_string());
    vevent.push("TRANSP:OPAQUE".to_string());

    let mut zeilen = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//Mimir//Kalenderleiste//DE".to_string(),
        "CALSCALE:GREGORIAN".to_string(),
        "BEGIN:VEVENT".to_string(),
    ];
    zeilen.extend(vevent);

    if let Some(minuten) = erinnerung {
        // Die Benachrichtigung trägt den Titel des Termins; ohne Text würde
        // Nextcloud je nach Einstellung einen leeren Titel anzeigen.
        zeilen.push("BEGIN:VALARM".to_string());
        zeilen.push("ACTION:DISPLAY".to_string());
        zeilen.push(format!("DESCRIPTION:{}", escape_text(&request.summary)));
        zeilen.push(format!("TRIGGER:{}", trigger_text(minuten)));
        zeilen.push("END:VALARM".to_string());
    }

    zeilen.push("END:VEVENT".to_string());
    zeilen.push("END:VCALENDAR".to_string());

    fold(&zeilen.join("\r\n"))
}

/// Die Kategorien als eine ICS-Zeile, ohne `CATEGORIES:`.
pub fn kategorien_text(roh: &str) -> String {
    roh.split(',')
        .map(|kategorie| escape_text(kategorie.trim()))
        .filter(|kategorie| !kategorie.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

/// Listet Felder auf, die es im Termin nicht gibt.
///
/// Statt eines allgemeinen Fehlers wird der Name genannt: Ein Modell, das
/// erfährt, welches Feld nicht passt, korrigiert den Aufruf im nächsten Schritt
/// selbst, statt denselben Aufruf zu wiederholen.
fn unbekannte_felder(value: &serde_json::Value) -> Option<String> {
    const ERLAUBT: [&str; 9] = [
        "summary",
        "start",
        "end",
        "all_day",
        "location",
        "description",
        "calendar",
        "reminder",
        "category",
    ];

    let objekt = value.as_object()?;
    // Höchstens vier Namen, damit die Meldung lesbar bleibt. Das Modell darf
    // `unbekannte_felder` auch wiederholt aufrufen und sieht dann den Rest.
    let unbekannt: Vec<&str> = objekt
        .keys()
        .map(|name| name.as_str())
        .filter(|name| !ERLAUBT.contains(name))
        .take(4)
        .collect();

    if unbekannt.is_empty() {
        return None;
    }

    Some(unbekannt.join(", "))
}

/// Wie weit ein Termin zurückliegen darf.
///
/// Sieben Tage decken „das habe ich gestern vergessen“ ab. Alles darüber ist
/// mit hoher Wahrscheinlichkeit ein geratenes Datum: Auf „heute 14 Uhr“ ist
/// dadurch ein Termin im Jahr 2023 entstanden, den niemand bemerkt hat, weil
/// im Bestätigungsfenster ein plausibel aussehender Termin stand.
fn zu_weit_hinten(start: DateTime<Utc>) -> bool {
    start < Local::now() - Duration::days(MAX_RUECKLIEGEND_TAGE)
}

/// Prüft alle Felder und baut den Termin, ohne ihn zu senden.
pub fn plan_event(
    config: &CalendarConfig,
    verfuegbare_kalender: &[(String, String)],
    arguments: &serde_json::Value,
    benutzertext: &str,
) -> Result<EventPlan, String> {
    let request = EventRequest::from_value(arguments)?;
    let summary = clean(&request.summary);

    if summary.is_empty() {
        return Err("Ein Termin braucht eine Überschrift".to_string());
    }

    if summary.chars().count() > MAX_UEBERSCHRIFT_CHARS {
        return Err(format!(
            "Die Überschrift ist zu lang (höchstens {} Zeichen).",
            MAX_UEBERSCHRIFT_CHARS
        ));
    }

    let location = request
        .location
        .as_deref()
        .map(clean)
        .filter(|v| !v.is_empty());
    let description = request
        .description
        .as_deref()
        .map(clean)
        .filter(|v| !v.is_empty());

    if let Some(location) = &location {
        if location.chars().count() > MAX_ORTS_CHARS {
            return Err(format!(
                "Der Ort ist zu lang (höchstens {} Zeichen).",
                MAX_ORTS_CHARS
            ));
        }
    }

    if let Some(description) = &description {
        if description.chars().count() > MAX_BESCHREIBUNG_CHARS {
            return Err(format!(
                "Die Beschreibung ist zu lang (höchstens {} Zeichen).",
                MAX_BESCHREIBUNG_CHARS
            ));
        }
    }

    // Die Erinnerung wird hier gerechnet, damit ein Fehler noch vor dem Kalender
    // auffällt und nicht erst nach der Rückfrage. `build_ics` rechnet nicht noch
    // einmal: Was hier scheitert, kommt gar nicht erst in die Datei.
    let erinnerung = match request
        .reminder
        .as_deref()
        .map(clean)
        .filter(|w| !w.is_empty())
    {
        Some(wort) => parse_erinnerung(&wort)?,
        None => None,
    };

    let kategorie = request
        .category
        .as_deref()
        .map(clean)
        .filter(|wert| !wert.is_empty());

    if let Some(kategorie) = &kategorie {
        if kategorie.chars().count() > MAX_KATEGORIE_CHARS {
            return Err(format!(
                "Die Kategorie ist zu lang (höchstens {MAX_KATEGORIE_CHARS} Zeichen)."
            ));
        }
    }

    if request.all_day {
        // Ein Ganztagestermin braucht nur ein Datum. Eine mitgeschickte Uhrzeit
        // ändert daran nichts und wird abgeschnitten, damit aus „Urlaub am
        // 14.09. um 15 Uhr“ nicht stillschweigend ein Nachmittagstermin wird.
        let start_tag = parse_datum(&request.start)?;
        let end_tag = match &request.end {
            Some(wert) => parse_datum(wert)?,
            None => start_tag + Duration::days(1),
        };

        if end_tag <= start_tag {
            return Err("Das Ende des Termins liegt nicht nach dem Beginn".to_string());
        }

        if (end_tag - start_tag).num_days() > MAX_DAUER_TAGE_GANZTAG {
            return Err(format!(
                "Ein Ganztagestermin darf höchstens {MAX_DAUER_TAGE_GANZTAG} Tage dauern. Für \
                 einen längeren Zeitraum einen Kalendereintrag anlegen.",
            ));
        }

        if start_tag < Local::now().date_naive() - Duration::days(MAX_RUECKLIEGEND_TAGE) {
            return Err(format!(
                "Der Termin liegt in der Vergangenheit: {} war vor {} Tagen. Heute ist {}. \
                 Frage den Benutzer, ob er wirklich dieses Datum meint, oder übernimm seine \
                 Worte wie „morgen“ unverändert.",
                start_tag.format("%d.%m.%Y"),
                Local::now()
                    .date_naive()
                    .signed_duration_since(start_tag)
                    .num_days(),
                Local::now().format("%d.%m.%Y")
            ));
        }

        // Ein Ganztagestermin hat keine Uhrzeit; die Norm sieht dafür
        // UTC-Mitternacht vor, und genau die landet auch in der Datei.
        let start = Utc.from_utc_datetime(
            &start_tag
                .and_hms_opt(0, 0, 0)
                .expect("Mitternacht geht immer"),
        );
        let end = Utc.from_utc_datetime(
            &end_tag
                .and_hms_opt(0, 0, 0)
                .expect("Mitternacht geht immer"),
        );
        let mut plan = finish_plan(
            config,
            verfuegbare_kalender,
            &Ausgang {
                request,
                start,
                ende: end,
                erinnerung,
                kategorie: &kategorie,
                benutzertext,
            },
        )?;

        // Bei mehreren Tagen steht die Spanne da, nicht nur der erste Tag.
        let tage = (end_tag - start_tag).num_days();
        let zeit = if tage == 1 {
            format!("{} ganztägig", start_tag.format("%d.%m.%Y"))
        } else {
            format!(
                "{} bis {} ganztägig",
                start_tag.format("%d.%m.%Y"),
                (end_tag - Duration::days(1)).format("%d.%m.%Y")
            )
        };

        plan.when = zeit.clone();
        plan.summary = format!(
            "Neuer Ganztagestermin: {summary}\n{zeit}\nKalender {}{}",
            plan.calendar_display,
            zusaetze(erinnerung, kategorie.as_deref())
        );

        return Ok(plan);
    }

    // Termin mit Uhrzeit. Erst die Worte des Benutzers, dann das absolute
    // Format: Ein Modell rechnet Datumsangaben schlecht, also soll es sie gar
    // nicht erst umrechnen müssen.
    let start = parse_term(&request.start)?;

    // Ein Ende ohne Tagesangabe gehört zum selben Tag wie der Beginn.
    //
    // Aus dem zweiten Validierungslauf: Für „Leg einen Termin für Freitagmittag
    // rein" lieferte qwen2.5:7b `start: "Freitag 14:00"` mit `end: "15:00"`.
    // „15:00" ist für sich allein der heutige Tag – der Termin lag damit in der
    // Vergangenheit und wurde abgelehnt, obwohl der Benutzer genau eine Angabe
    // gemacht hatte. Siehe `ende_am_tag`.
    //
    // Ein Ende **mit** Tagesangabe bleibt, wie es ist: „morgen 10 Uhr" zu einem
    // Termin am Freitag ist der nächste Tag und wird nicht umgedeutet.
    let ende = match &request.end {
        Some(wert) => ende_am_tag(parse_term(wert)?, start),
        // Ohne eigenes Ende die Vorgabedauer. „morgen 10 Uhr“ heißt ohne
        // Angabe bis morgen 10 Uhr.
        None => start + Duration::minutes(DEFAULT_DURATION_MINUTES),
    };

    if ende <= start {
        return Err("Das Ende des Termins liegt nicht nach dem Beginn".to_string());
    }

    if zu_weit_hinten(start) {
        return Err(format!(
            "Der Termin liegt in der Vergangenheit: {} war vor {} Tagen. Heute ist {}. \
             Frage den Benutzer, ob er wirklich dieses Datum meint, oder übernimm seine \
             Worte wie „morgen 14 Uhr“ unverändert.",
            start.with_timezone(&Local).format("%d.%m.%Y"),
            Local::now().signed_duration_since(start).num_days(),
            Local::now().format("%d.%m.%Y")
        ));
    }

    if (ende - start) > Duration::days(MAX_DAUER_TAGE) {
        return Err(format!(
            "Ein Termin darf höchstens {MAX_DAUER_TAGE} Tage dauern. Für einen längeren \
             Zeitraum einen Kalendereintrag anlegen.",
        ));
    }

    finish_plan(
        config,
        verfuegbare_kalender,
        &Ausgang {
            request,
            start,
            ende,
            erinnerung,
            kategorie: &kategorie,
            benutzertext,
        },
    )
}

/// Die Zeilen für Erinnerung und Kategorie im Bestätigungsfenster.
fn zusaetze(erinnerung: Option<i64>, kategorie: Option<&str>) -> String {
    let mut teile: Vec<String> = Vec::new();

    if let Some(minuten) = erinnerung {
        teile.push(format!("\nErinnerung {}", erinnerung_text(minuten)));
    }

    if let Some(kategorie) = kategorie {
        teile.push(format!("\nKategorien {}", kategorie));
    }

    teile.concat()
}

/// Setzt Kalender, Kennung und Texte zusammen.
/// Was die Planung aus dem Auftrag und aus dem Termin selbst macht.
///
/// Zusammengefasst, weil `finish_plan` sonst acht Parameter bekäme. Der
/// Benutzertext steht darin, weil Ort und Beschreibung daran gemessen werden,
/// ob der Benutzer sie genannt hat – siehe `finish_plan`.
struct Ausgang<'a> {
    request: EventRequest,
    start: DateTime<Utc>,
    ende: DateTime<Utc>,
    erinnerung: Option<i64>,
    kategorie: &'a Option<String>,
    benutzertext: &'a str,
}

fn finish_plan(
    config: &CalendarConfig,
    verfuegbar: &[(String, String)],
    ausgang: &Ausgang<'_>,
) -> Result<EventPlan, String> {
    let Ausgang {
        request,
        start,
        ende: end,
        erinnerung,
        kategorie,
        benutzertext,
    } = ausgang;
    let (calendar_href, calendar_display) =
        waehle_kalender(config, verfuegbar, request.calendar.as_deref())?;

    // Ort und Kalender fallen weg, wenn der Benutzer sie nicht genannt hat. Sie
    // stehen beide im Termin, und beide erfindet das Modell gern: Der Ort wird
    // aus dem Anlass abgeleitet („mit der Bank“ wird zu „Online"), und der
    // Kalender aus geratenen Namen wie „Arbeitskalender".
    //
    // Was der Benutzer **umgerechnet** hat, bleibt: `end` und `reminder` tragen
    // durch, weil dort das Wegrechnen der Normalfall ist und die Zeit ohnehin
    // strenger geprüft wird als ein Ort.
    let mut verworfen: Vec<String> = Vec::new();

    let ort = request
        .location
        .as_deref()
        .map(clean)
        .filter(|wert| !wert.is_empty())
        .filter(|wert| feld_gedeckt(wert, benutzertext, &[]))
        .or_else(|| {
            if request
                .location
                .as_deref()
                .map(clean)
                .is_some_and(|wert| !wert.is_empty())
            {
                verworfen.push("Ort".to_string());
            }
            None
        });

    let beschreibung = request
        .description
        .as_deref()
        .map(clean)
        .filter(|wert| !wert.is_empty())
        .filter(|wert| feld_gedeckt(wert, benutzertext, &[]))
        .or_else(|| {
            if request
                .description
                .as_deref()
                .map(clean)
                .is_some_and(|wert| !wert.is_empty())
            {
                verworfen.push("Beschreibung".to_string());
            }
            None
        });

    let bereinigt = EventRequest {
        summary: clean(&request.summary),
        start: request.start.clone(),
        end: request.end.clone(),
        all_day: request.all_day,
        location: ort,
        description: beschreibung,
        calendar: request.calendar.clone(),
        reminder: request.reminder.clone(),
        category: kategorie.as_ref().map(|wert| wert.to_string()),
    };
    let request = &bereinigt;

    let uid = format!(
        "{}-{}@mimir",
        super::client::now().timestamp_millis(),
        UID_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let ics = build_ics(
        &uid,
        *start,
        *end,
        request,
        *erinnerung,
        kategorie.as_deref(),
    );
    let when = format!("{} bis {}", display_time(*start), display_time(*end));

    // Was verworfen wurde, steht in der Zusammenfassung – dort, wo der Benutzer
    // ohnehin hinsieht, bevor er zustimmt. Ohne diesen Hinweis wäre ein
    // verschwundener Ort nur schwer zu bemerken.
    let hinweis = if verworfen.is_empty() {
        String::new()
    } else {
        format!(
            "\nNicht übernommen, weil du es nicht genannt hast: {}",
            verworfen.join(", ")
        )
    };

    Ok(EventPlan {
        calendar_href,
        summary: format!(
            "Neuer Termin: {}\n{when}\nKalender {calendar_display}{}{hinweis}",
            bereinigt.summary,
            zusaetze(*erinnerung, kategorie.as_deref())
        ),
        file_name: format!("{uid}.ics"),
        uid,
        ics,
        when,
        calendar_display,
        verworfen,
    })
}

/// Prüft den Kalendernamen, der als letzter Pfadabschnitt in die Adresse kommt.
///
/// Der Name stammt aus der Antwort des Servers und wird trotzdem geprüft: Ein
/// „..“ darin dürfte nicht aus dem Sammelpfad herausführen, und Steuerzeichen
/// gehören nicht in eine Adresse. Leerzeichen werden kodiert, damit ein
/// Kalender mit Leerzeichen im Namen erreichbar bleibt.
pub fn escape_calendar_segment(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("Der Kalendername ist leer".to_string());
    }

    if value.contains('/') || value.contains("..") {
        return Err("Der Kalendername ist ungültig".to_string());
    }

    let escaped: String = value
        .chars()
        .filter(|character| !character.is_control())
        .map(|character| match character {
            ' ' => "%20".to_string(),
            '%' => "%25".to_string(),
            '?' | '#' => format!("%{:02X}", character as u32),
            other => other.to_string(),
        })
        .collect();

    if escaped.is_empty() {
        return Err("Der Kalendername ist leer".to_string());
    }

    Ok(escaped)
}

/// Sucht den Zielkalender.
///
/// Maßgeblich ist die Auswahl des Benutzers, und die bedeutet dasselbe wie im
/// Lesepfad: Nichts ausgewählt heißt alle lesbaren Kalender. Bei mehreren
/// ausgewählten Kalendern darf Mimir nicht stillschweigend den ersten nehmen –
/// der Benutzer soll wissen, in welchem Kalender der Termin landet.
fn waehle_kalender(
    config: &CalendarConfig,
    verfuegbar: &[(String, String)],
    wunsch: Option<&str>,
) -> Result<(String, String), String> {
    let gewaehlt: Vec<&(String, String)> = if config.calendars.is_empty() {
        verfuegbar.iter().collect()
    } else {
        verfuegbar
            .iter()
            .filter(|(href, _)| config.calendars.iter().any(|wahl| wahl == href))
            .collect()
    };

    if gewaehlt.is_empty() {
        return Err(
            "Für Termine muss Mimir angemeldet und mit mindestens einem Kalender verbunden \
             sein. In der Leiste unter Auswahl einen Kalender wählen."
                .to_string(),
        );
    }

    if let Some(wunsch) = wunsch.map(clean).filter(|wert| !wert.is_empty()) {
        for (href, name) in &gewaehlt {
            // Hier wird **exakt** verglichen, absichtlich ohne unscharfen
            // Vergleich: Der Schreibpfad entscheidet, in welchem Kalender
            // geschrieben wird, und ein Treffer auf Verdacht wäre stiller
            // Datenverlust. Beim Ändern und Löschen wird dagegen unscharf
            // verglichen (`edit::kalender_passt`), weil dort der Termin vorher
            // gefunden und gelesen wird. Für das Anlegen sollte der Name daher
            // aus der Kalenderliste stammen.
            //
            // Sowohl der Anzeigename als auch der Pfad aus der Kalenderliste
            // sind möglich; Modelle greifen gern auf den Pfad zurück.
            if *name == wunsch || href == &wunsch || href.ends_with(&format!("/{wunsch}")) {
                return Ok((href.clone(), name.clone()));
            }
        }

        return Err(format!(
            "Den Kalender „{wunsch}“ gibt es nicht. Vorhanden sind: {}.",
            gewaehlt
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if gewaehlt.len() == 1 {
        return Ok(gewaehlt[0].clone());
    }

    // Der Text ist eine **Frage an den Benutzer**, nicht eine Anweisung an das
    // Modell. Er kommt als Werkzeugergebnis zurück, und das Modell soll ihn dem
    // Benutzer weitergeben – „Nenne einen davon" lädt zum Raten ein, die Frage
    // nicht.
    Err(format!(
        "In welchen Kalender soll der Termin? Es sind mehrere Kalender ausgewählt: {}.",
        gewaehlt
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(test)]
mod tests;
