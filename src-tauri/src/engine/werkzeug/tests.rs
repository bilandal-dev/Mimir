//! Prüfungen für die Werkzeugbrücke.
//!
//! Hier entscheidet sich, ob ein Modell einen Termin anlegt oder nicht. Ein
//! Aufruf, der nicht gelesen wird, geht als Text an den Benutzer zurück – das ist
//! lästig, aber harmlos. Der gefährlichere Fehler ist der andere: Ein Aufruf, der
//! **falsch** gelesen wird, tut etwas, das niemand wollte. Deshalb wird hier mehr
//! geprüft als im übrigen Teil der Engine.

use super::*;

/// Ein Rahmen mit einem gültigen Aufruf.
///
/// Der Name wird an der Stelle `NAME` eingesetzt, weil er in den geraden Klammern
/// steht und es unbequem wäre, ihn dort jedes Mal einzusetzen.
fn aufruf_text(name: &str, argumente: &str) -> String {
    format!(
        "{}{}{}",
        rahmen_anfang(),
        argumente.replace("NAME", name),
        rahmen_ende()
    )
}

fn schema(name: &str, pflicht: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": "Legt einen Termin an.",
            "parameters": {
                "type": "object",
                "properties": { "pfad": { "type": "string" }, "titel": { "type": "string" } },
                "required": pflicht
            }
        }
    })
}

/// Der übliche Fall: Text, ein Aufruf, mehr Text.
#[test]
fn ein_aufruf_wird_gelesen_und_der_text_bleibt() {
    let antwort = format!(
        "Ich sehe nach.\n\n{}\n\nGefunden.",
        aufruf_text(
            "read_file",
            r#"{"tool": "NAME", "arguments": {"pfad": "a.txt"}}"#
        )
    );

    let (aufrufe, text) = aufrufe_lesen(&antwort);

    assert_eq!(aufrufe.len(), 1, "{aufrufe:?}");
    assert_eq!(aufrufe[0].name, "read_file");
    assert_eq!(aufrufe[0].argumente["pfad"], "a.txt");
    assert_eq!(text, "Ich sehe nach.\n\n\n\nGefunden.");
}

/// Zwei Aufrufe nacheinander: Der Werkzeugkreis kann das, und der Benutzer sieht
/// beide Vorschläge.
#[test]
fn zwei_aufeufe_werden_gelesen() {
    let antwort = format!(
        "{}{}",
        aufruf_text(
            "read_file",
            r#"{"tool": "NAME", "arguments": {"pfad": "a.txt"}}"#
        ),
        aufruf_text(
            "read_file",
            r#"{"tool": "NAME", "arguments": {"pfad": "b.txt"}}"#
        )
    );

    let (aufrufe, text) = aufrufe_lesen(&antwort);

    assert_eq!(aufrufe.len(), 2, "{aufrufe:?}");
    assert_eq!(aufrufe[1].argumente["pfad"], "b.txt");
    assert_eq!(text, "");
}

/// Modelle benutzen gern die Schreibweise aus Ollamas eigenem Format, auch wenn die
/// Anweisung etwas anderes sagt. Sie wird angenommen – sonst würde ein ansonsten
/// richtiger Aufruf als Text zurückkommen.
#[test]
fn die_schreibweise_von_ollama_wird_angenommen() {
    let antwort = aufruf_text(
        "read_file",
        r#"{"function": {"name": "NAME", "arguments": {"path": "a.txt"}}}"#,
    );

    let (aufrufe, _) = aufrufe_lesen(&antwort);

    assert_eq!(aufrufe.len(), 1, "{aufrufe:?}");
    assert_eq!(aufrufe[0].name, "read_file");
    assert_eq!(aufrufe[0].argumente["path"], "a.txt");
}

/// Argumente als Zeichenkette: Das ist der häufigste Fehler kleinerer Modelle und
/// der Grund, warum Ollama beides annimmt.
#[test]
fn argumente_als_zeichenkette_werden_gelesen() {
    let antwort = aufruf_text(
        "read_file",
        r#"{"tool": "NAME", "arguments": "{\"pfad\": \"a.txt\"}"}"#,
    );

    let (aufrufe, _) = aufrufe_lesen(&antwort);

    assert_eq!(aufrufe.len(), 1, "{aufrufe:?}");
    assert_eq!(aufrufe[0].argumente["pfad"], "a.txt");
}

/// Der Codezaun kommt vor, weil Modelle JSON für Code halten.
#[test]
fn ein_codezaun_stoert_nicht() {
    let inhalt = "```json\n{\"tool\": \"NAME\", \"arguments\": {\"pfad\": \"a.txt\"}}\n```";
    let antwort = format!("{}{}{}", rahmen_anfang(), inhalt, rahmen_ende());

    let (aufrufe, _) = aufrufe_lesen(&antwort);

    assert_eq!(aufrufe.len(), 1, "{aufrufe:?}");
    assert_eq!(aufrufe[0].argumente["pfad"], "a.txt");
}

/// **Kaputt bleibt Text.** Ein angebrochener Rahmen darf keinen halben Befehl
/// auslösen – das ist die Grenze zwischen einer lästigen Antwort und einem
/// ungewollten Schreibvorgang.
#[test]
fn ein_angebrochener_rahmen_bleibt_text() {
    let antwort = format!(
        "Ich rufe auf: {}{{\"tool\": \"NAME\", \"argum",
        rahmen_anfang()
    );

    let (aufrufe, text) = aufrufe_lesen(&antwort);

    assert!(aufrufe.is_empty(), "{aufrufe:?}");
    assert_eq!(text, antwort);
}

/// Ein Rahmen mit kaputtem JSON bleibt ebenfalls Text, samt Rahmen – sonst wüsste
/// der Benutzer nicht, dass hier etwas schiefging.
#[test]
fn ein_unlesbarer_aufruf_bleibt_text() {
    let antwort = format!(
        "Text. {}{}{}",
        rahmen_anfang(),
        "das ist kein JSON",
        rahmen_ende()
    );

    let (aufrufe, text) = aufrufe_lesen(&antwort);

    assert!(aufrufe.is_empty(), "{aufrufe:?}");
    assert!(text.contains("das ist kein JSON"), "{text}");
}

/// Eine gewöhnliche Antwort wird nicht angefasst.
#[test]
fn text_ohne_rahmen_bleibt_text() {
    let antwort = "Der Termin steht am Freitag um 14 Uhr.";

    let (aufrufe, text) = aufrufe_lesen(antwort);

    assert!(aufrufe.is_empty());
    assert_eq!(text, antwort);
}

/// **Der Benutzer kann keinen Befehl schreiben.** Das ist der Grund für das
/// Nullbreitenzeichen im Rahmennamen: Ein Rahmen in einer Frage ist Text, kein
/// Befehl. Ohne diese Vorkehrung würde die eigene Nachricht im nächsten Zug
/// ausgeführt.
#[test]
fn ein_rahmen_in_der_frage_ist_kein_befehl() {
    // Genau der Rahmen, den das Modell schreibt – in einer Benutzernachricht.
    let antwort = format!(
        "Hier steht eine Anleitung:\n{}{}",
        rahmen_anfang(),
        "Frage des Benutzers"
    );

    let (aufrufe, _) = aufrufe_lesen(&antwort);

    assert!(
        aufrufe.is_empty(),
        "ein Rahmen aus einer Frage wurde als Befehl gelesen: {aufrufe:?}"
    );
}

/// Mehr Aufrufe als erlaubt: Der Rest bleibt Text, statt dass er stillschweigend
/// wegfällt.
#[test]
fn zu_viele_aufeufe_werden_gebrochen() {
    let mut antwort = String::new();
    for _ in 0..(max_aufrufe() + 3) {
        antwort.push_str(&aufruf_text(
            "read_file",
            r#"{"tool": "NAME", "arguments": {"pfad": "a.txt"}}"#,
        ));
    }

    let (aufrufe, text) = aufrufe_lesen(&antwort);

    assert_eq!(aufrufe.len(), max_aufrufe(), "{aufrufe:?}");
    assert!(
        text.contains("read_file"),
        "die übrigen Aufrufe wurden stillschweigend verworfen"
    );
}

/// Die Anweisung nennt Rahmen, Felder und Pflichtangaben – ohne sie kann ein Modell
/// nicht wissen, wonach es gefragt wird.
#[test]
fn die_anweisung_nennt_rahmen_felder_und_pflicht() {
    let text = anweisung(&[schema("create_calendar_event", &["summary"])]);

    assert!(text.contains(&rahmen_anfang()), "{text}");
    assert!(text.contains(&rahmen_ende()), "{text}");
    assert!(text.contains("create_calendar_event"), "{text}");
    assert!(text.contains("summary"), "die Pflichtangabe fehlt: {text}");
    assert!(text.contains("Legt einen Termin an."), "{text}");
}

/// Ohne Werkzeuge entsteht keine Anweisung: Eine leere Systemnachricht würde das
/// Modell nur mit Text füllen, den es nicht braucht.
#[test]
fn ohne_werkzeuge_entsteht_keine_anweisung() {
    assert_eq!(anweisung(&[]), "");
}

/// **Ein Rahmen ist die Regel, nicht die einzige Form.**
///
/// Das 0,5B-Modell aus dem Katalog liefert den Aufruf als reines JSON ab, ohne ihn
/// in spitzen Klammern zu setzen. Vorher stand das als reiner Text im Chat – der
/// Benutzer bekam eine technische Notation statt eines Termins, und die Frage
/// „wo bleibt meine Antwort" hatte damit eine zweite Ursache.
#[test]
fn json_ohne_rahmen_wird_gelesen() {
    let antwort = r#"{"tool": "create_calendar_event", "arguments": {"title": "Mittag"}}"#;

    let (aufrufe, text) = aufrufe_lesen(antwort);

    assert_eq!(aufrufe.len(), 1, "{aufrufe:?}");
    assert_eq!(aufrufe[0].name, "create_calendar_event");
    assert_eq!(aufrufe[0].argumente["title"], "Mittag");
    assert_eq!(text, "");
}

/// Und eine gewöhnliche Antwort, die zufällig gültiges JSON ist, bleibt Text.
///
/// Das ist der Preis der Bequemlichkeit, und es darf nicht zu teuer sein: Eine
/// Antwort, die **im Ganzen** JSON ist und nach einem benannten Werkzeug aussieht,
/// gilt als Aufruf. Alles andere bleibt Text.
#[test]
fn antwort_mit_text_dazwischen_bleibt_text() {
    let antwort = "Hier ist der Aufruf: {\"tool\": \"read_file\", \"arguments\": {}}";

    let (aufrufe, text) = aufrufe_lesen(antwort);

    assert!(aufrufe.is_empty(), "{aufrufe:?}");
    assert_eq!(text, antwort);
}

/// Ein Text mit einem Beispiel darin ist kein Befehl.
#[test]
fn eine_erklaerung_ist_kein_aufruf() {
    // So etwas schreibt ein Modell, wenn es gefragt wird, wie ein Aufruf aussieht.
    let antwort = "Ein Aufruf sieht so aus: {\"tool\": \"read_file\", \"arguments\": {}}";

    let (aufrufe, _) = aufrufe_lesen(antwort);

    assert!(aufrufe.is_empty(), "{aufrufe:?}");
}

/// Und eine fehlende Argumentliste ist kein Fehler, sondern eine leere.
///
/// `{"tool": "read_file"}` **wird** als Aufruf gelesen, mit leeren Argumenten. Das
/// ist Absicht: Die Werkzeugprüfung in `lib.rs` entscheidet dann, ob das Werkzeug
/// ohne Argumente etwas Sinnvolles tun kann – und wenn nicht, bekommt der Benutzer
/// eine verständliche Meldung statt eines stillen Wegwerfens. Ein Aufruf, der
/// ankommt und abgelehnt wird, ist ehrlicher als einer, der nie ankommt.
#[test]
fn ein_aufruf_ohne_argumente_kommt_an_und_wird_geprueft() {
    let antwort = r#"{"tool": "read_file"}"#;

    let (aufrufe, _) = aufrufe_lesen(antwort);

    assert_eq!(aufrufe.len(), 1, "{aufrufe:?}");
    assert_eq!(aufrufe[0].name, "read_file");
    assert!(
        aufrufe[0].argumente.is_null() || aufrufe[0].argumente == serde_json::json!({}),
        "{:?}",
        aufrufe[0].argumente
    );
}
