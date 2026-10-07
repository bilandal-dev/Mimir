//! Lässt ein echtes Modell den Terminumfang bedienen.
//!
//! Zweck: Vor der Frage, ob Mimir ein eigenes Modell mitbringen soll, muss
//! bekannt sein, ob ein kleines Modell die Kalenderwerkzeuge zuverlässig wählt.
//! Das ist eine Eigenschaft des Modells und lässt sich nur mit einem Modell
//! messen – kein Rust-Test kann sie beantworten.
//!
//! Drei Dinge sind bewusst so gebaut, dass die Messung ehrlich bleibt:
//!
//! * **Schemata und Anweisung kommen aus `termine_toolset_for`**, also aus
//!   derselben Funktion, aus der auch `list_tools` sie holt. Eine Kopie wäre eine
//!   zweite Wahrheit, die genau dann stimmt, wenn man sie pflegt.
//! * **Die Anfrage geht durch dieselben Prüfungen** wie im Betrieb: Nachricht,
//!   Systemanweisung und Schemata werden vor dem Absenden begrenzt und
//!   abgewiesen. Sonst wäre die Messung an einem Aufbau vorbei, den Mimir gar
//!   nicht zulässt.
//! * **Das Werkzeug wird über `validated()` gelesen**, also mit derselben
//!   Namensprüfung wie im Betrieb. Ein Modell, das einen Unsinnnamen liefert,
//!   fällt hier genauso auf wie dort.
//!
//! Der Rest ist bewusst schmal: Streaming aus, `think` aus. Beides ändert nichts
//! daran, ob das Modell das richtige Werkzeug wählt, und beides macht den Lauf
//! schneller und auswertbarer.

use std::time::Instant;

use serde_json::json;

use crate::{
    termine_validierung_pruefungen::datumsangabe, validate_chat_input, validate_tool_schemas,
    ChatMessage, OllamaRequest,
};

/// Was ein Satz mit sich bringen soll: das erwartete Werkzeug und eine kurze
/// Bezeichnung, damit die Ausgabe lesbar bleibt und beim nächsten Lauf
/// erkennbar ist, welcher Fall wieder aufgetreten ist.
///
/// Die Auswahl deckt ab, was im Kalender am häufigsten scheitert: Uhrzeit im
/// Klartext, Wochentag ohne Artikel, „früh" und „mittags", eine Erinnerung, ein
/// Ort, eine Kategorie, mehrere Angaben in einem Satz, ein Ganztagestermin, zwei
/// Termine in einem Satz, ein Ändern, ein Löschen und drei Fragen nach dem
/// Bestand.
pub struct Erwartung {
    pub werkzeug: &'static str,
    pub tag: &'static str,
}

/// Kein Werkzeug, sondern eine Frage: Bei diesen Sätzen fehlt eine Angabe, die
/// Mimir nicht erfinden darf.
///
/// Als Wert im `werkzeug`-Feld, damit die Liste geschlossen bleibt und ein
/// versehentlich nicht gesetztes Feld sofort auffällt statt still zu einem
/// Vergleich gegen einen Namen zu führen, den es nie gab.
pub const NACHFRAGE: &str = "nachfrage";

macro_rules! saetze {
    ($(($werkzeug:literal, $tag:literal, $satz:literal)),* $(,)?) => {
        pub const ERWARTUNGEN: &[Erwartung] = &[$(
            Erwartung { werkzeug: $werkzeug, tag: $tag },
        )*];

        pub const SAETZE: &[&str] = &[$($satz),*];
    };
}

saetze![
    (
        "create_calendar_event",
        "Uhrzeit im Klartext",
        "Trag morgen um 14 Uhr einen Termin mit der Hausärztin ein, Ort Talstraße 8."
    ),
    (
        "create_calendar_event",
        "früh und Erinnerung",
        "Mach mir übermorgen früh einen Termin fürs Zahnarzt, 15 Minuten vorher erinnern."
    ),
    (
        "create_calendar_event",
        "Wochentag ohne Artikel",
        "Setz nächsten Dienstag um 9:30 einen Termin mit Marc ein."
    ),
    // Absagen ist kein Anlegen: Erwartet wird das Ändern oder Löschen. Ein
    // „Vergiss" als `create` hätte den Termin verdoppelt statt entfernt.
    (
        "delete_calendar_event",
        "absagen, nicht anlegen",
        "Vergiss bitte das Fitnessstudio am Donnerstag."
    ),
    // Nachfragen statt raten: Bei diesen vier fehlt je etwas, das Mimir nicht
    // erfinden kann. Erwartet wird deshalb **kein** Werkzeug, sondern eine
    // Frage – das ist das Verhalten, das die Nachfrageregel erzwingen soll.
    //
    // Aus dem zweiten Lauf: Ohne die Regel hat qwen2.5:7b aus „irgendwann mal
    // was mit Sarah" einen Termin mit erfundener Uhrzeit gemacht. Mit der Regel
    // soll es fragen.
    ("nachfrage", "Zeit fehlt", "Trag irgendwann mal was mit Sarah ein."),
    (
        "nachfrage",
        "Titel fehlt",
        "Mach mir morgen um 15 Uhr einen Termin."
    ),
    (
        "nachfrage",
        "kein Kalender genannt",
        "Trag morgen um 14 Uhr einen Termin mit der Hausärztin ein."
    ),
    (
        "nachfrage",
        "Welcher Termin gemeint ist",
        "Verschieb den Termin auf morgen um 16 Uhr."
    ),
    (
        "create_calendar_event",
        "Uhrzeit als Zeitspanne",
        "Ich brauche morgen zwischen 15 und 16 Uhr einen Termin mit der Bank."
    ),
    (
        "create_calendar_event",
        "mittags ohne Zahl",
        "Leg einen Termin für Freitagmittag rein."
    ),
    (
        "create_calendar_event",
        "Kategorie",
        "Mach heute noch einen Anruf bei der IT, Kategorie Arbeit."
    ),
    (
        "create_calendar_event",
        "Freitag ohne Artikel",
        "Termin nächste Woche Dienstag um 11 Uhr mit Sarah."
    ),
    (
        "create_calendar_event",
        "Raum im Ort",
        "Bitte einen Termin übermorgen um 16 Uhr, Raum 204."
    ),
    // Ein Ganztag braucht `all_day`: Ohne das Feld legt das Werkzeug einen Termin
    // an, der um 14:00 beginnt – der Benutzer wollte den ganzen Tag frei.
    (
        "create_calendar_event",
        "Ganztag",
        "Trag am Freitag einen ganzen Arbeitstag Urlaub ein."
    ),
    (
        "create_calendar_event",
        "Uhr und früh zusammen",
        "Mach einen Termin morgen um 8 Uhr früh zum Bahnhof."
    ),
    (
        "create_calendar_event",
        "mittags und Erinnerung",
        "Ich muss Donnerstagmittag zum Amtsgericht, 20 Minuten vorher erinnern."
    ),
    (
        "update_calendar_event",
        "ändern",
        "Verschieb den Termin mit der Hausärztin auf morgen um 16 Uhr."
    ),
    (
        "delete_calendar_event",
        "löschen",
        "Lass den Zahnarzttermin nächste Woche weg."
    ),
    (
        "create_calendar_event",
        "viele Felder",
        "Steh morgen um 14 Uhr einen Termin mit der Hausärztin ein, Ort Talstraße 8, Kategorie Gesundheit, 30 Minuten vorher erinnern."
    ),
    (
        "create_calendar_event",
        "zwei Termine, der erste",
        "Zwei Termine: morgen um 9 Uhr Frühstück mit Anna und übermorgen um 18 Uhr Kino."
    ),
    (
        "list_calendar_events",
        "Woche fragen",
        "Wie sieht meine Woche aus?"
    ),
    (
        "list_calendar_events",
        "Woche fragen, kurz",
        "Was steht diese Woche an?"
    ),
    (
        "list_calendar_events",
        "bestimmter Tag",
        "Hast du am Freitag noch was?"
    ),
    (
        "create_calendar_event",
        "unvollständig",
        "Leg noch einen Termin an, morgen um 11 Uhr, Lena."
    ),
    (
        "create_calendar_event",
        "Dienstag mit Kategorie",
        "Mach einen Termin Dienstag um 14 Uhr mit dem Steuerberater, Kategorie Arbeit."
    ),
    (
        "create_calendar_event",
        "sehr früh",
        "Bitte morgen um 7 Uhr einen Termin, early start."
    ),
    (
        "create_calendar_event",
        "Mittag",
        "Trag übermorgen um 12:30 einen Mittagstermin mit der Mama ein."
    ),
    (
        "create_calendar_event",
        "kommenden Freitag",
        "Setz einen Termin am kommenden Freitag auf 17 Uhr."
    ),
];

/// Die Antwort des Modells, so weit sie für die Beurteilung zählt.
pub struct Antwort {
    /// Name des aufgerufenen Werkzeugs, `None` wenn keines kam.
    werkzeug: Option<String>,
    /// Argumente, so wie das Modell sie lieferte.
    pub argumente: serde_json::Value,
    /// Antworttext, wenn das Modell statt eines Werkzeugs geantwortet hat.
    pub text: String,
    /// Dauer der Anfrage in Sekunden.
    pub sekunden: f64,
    /// Name, den das Modell schickte, auch wenn er die Prüfung nicht besteht.
    /// Sonst wäre ein Unsinnname stillschweigend weggefallen und der Lauf
    /// säume besser aus, als er ist.
    roh_werkzeug: Option<String>,
}

impl Antwort {
    /// Das Werkzeug nach der Namensprüfung, wie im Betrieb gelesen.
    pub fn werkzeug(&self) -> Option<&str> {
        self.werkzeug.as_deref()
    }

    /// Trägt der Aufruf überhaupt etwas, das Mimir bauen kann?
    ///
    /// Ein leerer Aufruf sieht in der Ausgabe wie ein Erfolg aus – der
    /// Werkzeugname steht da, nur ohne Argumente – und ist es nicht. Das kam
    /// vor, als die Nachfrageregel das Modell dazu brachte, das Werkzeug zu
    /// nennen und dann nachzufragen: Es lieferte den Namen und die leere
    /// Argumentliste. Für die Messung zählt das als Fehler, nicht als Teilerfolg.
    pub fn trägt_angaben(&self) -> bool {
        match self.werkzeug.as_deref() {
            Some(name) if name == crate::calendar::write::EVENT_TOOL => {
                let hat_titel = self
                    .argumente
                    .get("summary")
                    .is_some_and(|wert| !wert.is_null());
                let hat_zeit = self
                    .argumente
                    .get("start")
                    .is_some_and(|wert| !wert.is_null());
                // `all_day` genügt: Ein Ganztagestermin braucht nur ein Datum.
                let ist_ganztag = self
                    .argumente
                    .get("all_day")
                    .and_then(|wert| wert.as_bool())
                    .unwrap_or(false);

                (hat_titel && hat_zeit) || (ist_ganztag && hat_zeit)
            }
            Some(name)
                if name == crate::calendar::edit::UPDATE_TOOL
                    || name == crate::calendar::edit::DELETE_TOOL =>
            {
                self.argumente
                    .get("uid")
                    .is_some_and(|wert| !wert.is_null())
                    || self
                        .argumente
                        .get("title")
                        .is_some_and(|wert| !wert.is_null())
            }
            // Beim Lesen zählt schon der Aufruf: `list_calendar_events` braucht
            // außer einem Zeitraum nichts.
            _ => true,
        }
    }

    /// Die Argumente in Kurzform, für die Ausgabe.
    pub fn angaben_kurz(&self) -> String {
        let text = self.argumente.to_string();
        if text.len() > 120 {
            format!("{}…", &text[..117])
        } else {
            text
        }
    }

    /// Der Name, wie er ankam – auch wenn er die Prüfung nicht besteht.
    pub fn roh_werkzeug(&self) -> Option<&str> {
        self.roh_werkzeug.as_deref()
    }

    /// Kann Mimir mit diesem Werkzeugaufruf etwas anfangen?
    ///
    /// Das ist die Frage, an der der ganze Entwurf hängt: Das Modell soll die
    /// Worte des Benutzers weitergeben, weil Mimir sie rechnet. Eine Antwort, die
    /// das richtige Werkzeug wählt und Worte liefert, die Mimir nicht liest, ist
    /// im Chat nicht besser als gar keine – der Benutzer sieht dann eine
    /// Fehlermeldung statt eines Termins.
    ///
    /// Der Satz des Benutzers wird mitgegeben, weil die Planung Ort und Kalender
    /// gegen seine Worte prüft. Ohne ihn würde die Validierung einen Weg messen,
    /// den es im Betrieb nicht gibt.
    ///
    /// Geprüft wird die Planung, nicht der Schreibvorgang: `plan_event` berührt den
    /// Kalender nicht, und genau dieselbe Funktion liefert die Vorschau. Was hier
    /// als „geht nicht" gemeldet wird, scheitert im Betrieb an derselben Stelle.
    pub fn planbar(&self, satz: &str) -> Result<(), String> {
        match self.werkzeug.as_deref() {
            Some(name) if name == crate::calendar::write::EVENT_TOOL => {
                // Zwei Kalender, weil der Anwendungsfall der ist, in dem der
                // Benutzer mehrere ausgewählt hat. Mit nur einem wäre ein
                // erfundener Kalendername unauffällig, weil er nie geprüft würde.
                crate::calendar::write::plan_event(
                    &crate::calendar::CalendarConfig {
                        server_url: "https://kalender.example.org".to_string(),
                        username: "benutzer".to_string(),
                        calendars: vec!["persoenlich".to_string(), "arbeit".to_string()],
                        server_certificate: None,
                    },
                    &[
                        ("persoenlich".to_string(), "Persönlich".to_string()),
                        ("arbeit".to_string(), "Arbeit".to_string()),
                    ],
                    &self.argumente,
                    satz,
                )
                .map(|_| ())
                .map_err(|grund| format!("Mimir kann daraus keinen Termin bauen: {grund}"))
            }
            Some(name)
                if name == crate::calendar::edit::UPDATE_TOOL
                    || name == crate::calendar::edit::DELETE_TOOL =>
            {
                // Für Ändern und Löschen braucht es den Termin aus Nextcloud. Das
                // ist hier nicht erreichbar, geprüft wird deshalb nur, ob die
                // Argumente die Form haben, die das Werkzeug erwartet: `title`
                // und `on_date` benennen den Termin, `uid` wäre der Weg darüber.
                let hat_bezeichnung = ["title", "uid"].iter().any(|feld| {
                    self.argumente
                        .get(*feld)
                        .and_then(|wert| wert.as_str())
                        .is_some_and(|text| !text.trim().is_empty())
                });
                let hat_zeit = self
                    .argumente
                    .get("on_date")
                    .and_then(|wert| wert.as_str())
                    .is_some_and(|text| !text.trim().is_empty());

                if !hat_bezeichnung || !hat_zeit {
                    return Err(
                        "Der Termin ist nicht benannt: Es braucht title und on_date oder eine uid."
                            .to_string(),
                    );
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[derive(serde::Deserialize)]
struct AntwortRoh {
    #[serde(default)]
    message: Option<ChatMessage>,
    #[serde(default)]
    total_duration: Option<u64>,
}

#[derive(serde::Deserialize)]
struct ModelleRoh {
    #[serde(default)]
    models: Vec<ModellRoh>,
}

#[derive(serde::Deserialize)]
struct ModellRoh {
    name: String,
}

/// Die Namen der Modelle auf dem Server.
///
/// Wird vor dem Lauf aufgerufen, damit ein Tippfehler im Modellnamen nicht erst
/// nach zehn Minuten Laufzeit auffällt.
pub async fn hole_modell(server: &str) -> Result<Vec<String>, String> {
    // Ohne den Anbieter bricht schon das Bauen jedes Clients ab – auch für
    // reine Klartext-Verbindungen. Die Anwendung installiert ihn beim Start;
    // dieses Programm muss es selbst tun, sonst käme der Fehler als Absturz
    // statt als Meldung.
    crate::install_crypto_provider();

    let client = crate::build_ollama_chat_client()?;
    let antwort = client
        .get(format!("{server}/api/tags"))
        .send()
        .await
        .map_err(|error| {
            crate::connection_error_message(
                &format!("{server}/api/tags"),
                &crate::format_reqwest_error(&error),
                error.is_connect(),
                1,
            )
        })?;

    // Der Status wird vor dem Lesen des Rumpfes geprüft: Sonst wäre ein
    // Fehlerstatus als unlesbare Antwort gemeldet und die Ursache läge woanders
    // als sie liegt.
    if !antwort.status().is_success() {
        return Err(format!("HTTP {} bei der Modelliste", antwort.status()));
    }

    let text = antwort
        .text()
        .await
        .map_err(|grund| format!("Die Modelliste ist nicht lesbar: {grund}"))?;

    let roh: ModelleRoh = serde_json::from_str(&text)
        .map_err(|grund| format!("Die Modelliste ist nicht lesbar: {grund}"))?;

    Ok(roh.models.into_iter().map(|modell| modell.name).collect())
}

/// Stellt dem Modell eine Aufgabe aus dem Terminumfang.
///
/// Die Anfrage geht durch dieselben Prüfungen wie im Betrieb: Nachricht,
/// Systemanweisung und Schemata werden vor dem Absenden begrenzt und
/// abgewiesen. Sonst würde ein Lauf mit einem zu langen Satz nicht die
/// tatsächliche Anweisung prüfen, sondern eine gekürzte.
pub async fn frage(server: &str, modell: &str, satz: &str) -> Result<Antwort, String> {
    // Aus derselben Funktion wie `list_tools`, mit denselben Kalendernamen.
    // Ohne Anmeldung gäbe es im Terminumfang kein Werkzeug, und es gäbe nichts
    // zu messen – deshalb die zwei festen Kalender. Zwei, weil der Fall mit
    // mehreren der ist, in dem das Modell raten musste.
    let kalender = vec!["Persönlich".to_string(), "Arbeit".to_string()];
    let werkzeuge = crate::termine_toolset_for(&kalender)?.tools;

    let system = format!(
        "{}\n\n{}",
        crate::termine_anweisung(&kalender),
        datumsangabe()
    );

    let nachricht = ChatMessage {
        role: "user".to_string(),
        content: satz.to_string(),
        ..Default::default()
    };

    validate_chat_input(modell, std::slice::from_ref(&nachricht))?;
    crate::normalize_system_prompt(&system)?;

    let anfrage = OllamaRequest {
        model: modell.to_string(),
        messages: vec![nachricht],
        stream: false,
        system: Some(system),
        tools: validate_tool_schemas(Some(werkzeuge))?,
        options: None,
    };

    // Das Modell bleibt zwischen den Sätzen im Speicher. Ohne das lädt Ollama
    // es bei jedem Satz neu, und ein Lauf über 24 Sätze lädt es vierundzwanzigmal
    // – auf einem Rechner, der das über LAN von einer Platte tun muss. Das ist
    // der Grund, an dem ein solcher Lauf in einen Zusammenbruch läuft und nicht
    // an der Fähigkeit des Modells.
    //
    // `keep_alive` steht in keiner Mimir-Anfrage, weil die Anwendung den Server
    // nicht belasten will: Sie fragt einmal und lässt ihn danach wieder frei.
    // Ein Durchlauf dagegen will das Gegenteil.
    let mut koerper = serde_json::to_value(&anfrage)
        .map_err(|grund| format!("Die Anfrage ist nicht baubar: {grund}"))?;
    let felder = koerper
        .as_object_mut()
        .ok_or_else(|| "Die Anfrage ist kein Objekt".to_string())?;
    felder.insert("keep_alive".to_string(), json!(30));

    let client = crate::build_ollama_chat_client()?;
    let start = Instant::now();

    let antwort = client
        .post(format!("{server}/api/chat"))
        .json(&koerper)
        .send()
        .await
        .map_err(|error| {
            crate::connection_error_message(
                &format!("{server}/api/chat"),
                &crate::format_reqwest_error(&error),
                error.is_connect(),
                1,
            )
        })?;

    let status = antwort.status();
    let text = antwort
        .text()
        .await
        .map_err(|grund| format!("Die Antwort ist nicht lesbar: {grund}"))?;

    if !status.is_success() {
        // Ollamas eigene Fehlermeldung ist verständlicher als ein HTTP-Code, und
        // die steht im Feld `error`.
        if let Ok(fehler) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(meldung) = fehler.get("error").and_then(|wert| wert.as_str()) {
                return Err(format!("HTTP {}: {}", status.as_u16(), meldung));
            }
        }
        return Err(format!("HTTP {}", status.as_u16()));
    }

    let sekunden = start.elapsed().as_secs_f64();

    let roh: AntwortRoh = serde_json::from_str(&text)
        .map_err(|grund| format!("Die Antwort ist nicht lesbar: {grund}"))?;

    let nachricht = roh.message.unwrap_or_default();
    let aufruf = nachricht.tool_calls.first();

    // Über `validated()`, damit ein Unsinnname hier genauso auffällt wie im
    // Betrieb. Der rohe Name bleibt erhalten, damit die Ausgabe zeigt, was kam.
    let (werkzeug, argumente) = match aufruf.map(|aufruf| aufruf.function.validated()) {
        Some(Ok((name, argumente))) => (Some(name), argumente),
        _ => (None, json!({})),
    };

    Ok(Antwort {
        roh_werkzeug: aufruf.map(|aufruf| aufruf.function.name.clone()),
        werkzeug,
        argumente,
        text: nachricht.content,
        // Die vom Server gemeldete Dauer ist genauer: Sie deckt nur die
        // Verarbeitung ab, nicht den Verbindungsaufbau übers WLAN.
        sekunden: roh
            .total_duration
            .map(|dauer| dauer as f64 / 1e9)
            .unwrap_or(sekunden),
    })
}

impl Antwort {
    /// Welche Felder hat der Aufruf gefüllt?
    ///
    /// Für die Ausgabe der Nachfrage-Fälle: Ein leerer Aufruf und ein Aufruf mit
    /// drei Feldern sehen in der Zusammenfassung gleich aus, sind aber
    /// unterschiedliche Fehler.
    pub fn gefuellte_felder(&self) -> Vec<&'static str> {
        const FELDER: [&str; 9] = [
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

        if self.werkzeug.is_some() {
            return FELDER
                .iter()
                .copied()
                .filter(|feld| {
                    self.argumente
                        .get(*feld)
                        .is_some_and(|wert| !wert.is_null())
                })
                .collect();
        }

        FELDER.to_vec()
    }
}
