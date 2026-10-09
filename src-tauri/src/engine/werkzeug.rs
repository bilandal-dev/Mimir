//! Werkzeugaufrufe ohne Werkzeugsupport im Modell.
//!
//! Ollamas Modelle bekommen die Werkzeuge als Feld im Auftrag und antworten mit
//! einem eigenen Feld. Die eingebaute Engine kennt nur Text: Sie bekommt die
//! Werkzeuge als Anweisung in der Systemnachricht und antwortet mit einem
//! JSON-Abschnitt im Text.
//!
//! **Warum das für kleine Modelle gangbar ist.** Qwen2.5-Instruct ist darauf
//! trainiert, einer Anweisung im Text zu folgen und JSON zu schreiben. Der Weg ist
//! unzuverlässiger als ein Feld – deshalb sind hier drei Dinge, die ihn tragen:
//!
//! * Die Anweisung nennt das Format wörtlich und mit einem Beispiel.
//! * [`aufrufe_lesen`](super::werkzeug::aufrufe_lesen) ist nachsichtig beim Format
//!   (Feldnamen, Codezaun, ein Aufruf als Text oder als Liste) und streng beim Inhalt.
//! * Was nicht lesbar ist, bleibt Text. Ein kaputter Aufruf wird nie ausgeführt,
//!   sondern dem Benutzer als Text gezeigt – das ist ein umständlicher Satz und
//!   kein Werkzeug, das etwas tut, das niemand wollte.
//!
//! Der umgekehrte Weg – eine eigene Grammatik über llama.cpp – ist schneller und
//! würde die kleinen Modelle bessere Werkzeugargumente liefern lassen. Er ist hier
//! nicht genommen, weil die Katalogeinträge Qwen-Modelle ohne Grammatik sind und
//! weil eine Grammatik im Auftrag nicht zum vorhandenen Werkzeugkreis passt, ohne
//! dass er umgebaut wird.

/// Der Name des Rahmens, in dem ein Aufruf steht.
///
/// **Das Nullbreitenzeichen dahinter ist Absicht.** Modelle schreiben den Namen
/// gern ohne es, und ohne die Vorkehrung würde ein Rahmen, den der Benutzer in
/// seiner eigenen Frage schreiben könnte, beim nächsten Zug als Befehl gelesen. Ein
/// Nullbreitenzeichen sieht man nicht und verhindert genau das.
const NAME: &str = "tool_call";

/// Nullbreite: Ein Zeichen ohne Breite und ohne eigene Form.
const NULLBREITE: char = '\u{200b}';

/// Der Rahmen, in dem ein Aufruf steht.
pub fn rahmen_anfang() -> String {
    format!("<{NAME}{NULLBREITE}>")
}

/// Sein Ende.
pub fn rahmen_ende() -> String {
    format!("</{NAME}{NULLBREITE}>")
}

/// Wie viele Werkzeugaufrufe in einer Antwort gelesen werden.
///
/// Ollama lässt mehr zu, aber die Oberfläche und der Werkzeugkreis arbeiten mit
/// einer kleinen Zahl: Ein Modell, das drei Termine auf einmal anlegt, ist keiner
/// mehr, sondern ein Unfall. Hier ist die Grenze genau die aus `lib.rs`.
pub fn max_aufrufe() -> usize {
    crate::MAX_TOOL_CALLS_PER_MESSAGE
}

/// Die Anweisung, mit der ein Modell Werkzeuge benutzt.
///
/// Sie hängt an der Systemnachricht und nicht an der Frage: Sie beschreibt die
/// Bedienung der Anwendung, nicht den Auftrag.
pub fn anweisung(schemata: &[serde_json::Value]) -> String {
    if schemata.is_empty() {
        return String::new();
    }

    let mut texte = String::from(
        "Du hast Werkzeuge. Rufe sie auf, wenn du etwas wissen musst, was du nicht \
         weißt, und rede nicht über den Aufruf, sondern führe ihn aus.\n\n",
    );

    texte.push_str(&format!(
        "Schreibe einen Aufruf genau so:\n{}\n{{\"tool\": \"NAME\", \"arguments\": \
         {{\"FELD\": \"WERT\"}}}}\n{}\n\n",
        rahmen_anfang(),
        rahmen_ende(),
    ));

    texte.push_str("Ein Aufruf je Rahmen, mehrere Rahmen direkt nacheinander. ");
    texte.push_str("Alle Felder in \"arguments\" sind genau die hier genannten. ");
    texte.push_str("Text vor dem ersten Rahmen wird dem Benutzer gezeigt.\n\n");
    texte.push_str("Diese Werkzeuge gibt es:\n\n");

    for schema in schemata {
        let Some(funktion) = schema.get("function") else {
            continue;
        };

        let Some(name) = funktion.get("name").and_then(|wert| wert.as_str()) else {
            continue;
        };

        let beschreibung = funktion
            .get("description")
            .and_then(|wert| wert.as_str())
            .unwrap_or_default();

        texte.push_str(&format!("- {name}: {beschreibung}\n"));

        let parameter = funktion.get("parameters");

        if let Some(felder) = parameter.and_then(|parameter| parameter.get("properties")) {
            texte.push_str(&format!("  Felder: {}\n", felder_text(felder)));
        }

        if let Some(pflicht) = parameter
            .and_then(|parameter| parameter.get("required"))
            .and_then(|wert| wert.as_array())
        {
            let namen: Vec<&str> = pflicht.iter().filter_map(|wert| wert.as_str()).collect();

            if !namen.is_empty() {
                texte.push_str(&format!("  Pflicht: {}\n", namen.join(", ")));
            }
        }
    }

    texte.push_str(
        "\nRufe nur ein Werkzeug auf, wenn du es wirklich brauchst. Eine Antwort ohne \
         Aufruf ist richtig, wenn du etwas sagen kannst.\n",
    );

    texte
}

/// Die Felder eines Werkzeugs, als lesbare Liste.
fn felder_text(felder: &serde_json::Value) -> String {
    let Some(objekt) = felder.as_object() else {
        return String::new();
    };

    let mut namen: Vec<&str> = objekt.keys().map(String::as_str).collect();
    namen.sort_unstable();

    if namen.is_empty() {
        return "keine".to_string();
    }

    namen.join(", ")
}

/// Liest aus einer Antwort die angeforderten Werkzeugaufrufe und den übrigen Text.
///
/// Beides gehört zusammen: Der Aufruf steht als Block in der Nachricht, und genau
/// dieser Block darf nicht auch noch als Text erscheinen – sonst stünde im Chat eine
/// technische Notation, die der Benutzer weder gelesen hat noch lesen will.
///
/// **Was nicht zurückkommt.** Text ohne lesbaren Aufruf bleibt unangetastet, auch
/// wenn er wie ein Aufruf aussieht. Ein halb geschriebenes JSON, das ein Modell
/// nach vier Versuchen abbricht, ist eine Antwort über einen kaputten Aufruf und
/// keine Aktion.
pub fn aufrufe_lesen(antwort: &str) -> (Vec<Aufruf>, String) {
    // **Erst der Fall ohne Rahmen.** Ein Rahmen ist die Regel, nicht die einzige
    // Form: Ein 0,5B-Modell liefert den Aufruf als reines JSON ab, ohne ihn in die
    // spitzen Klammern zu setzen. Vorher stand das als reiner Text im Chat – der
    // Benutzer bekam eine technische Notation statt eines Termins.
    //
    // Der Aufwand dafür ist begrenzt: Es muss **die ganze Antwort** JSON sein
    // und die Form eines Aufrufs tragen. Ein Text, in dem irgendwo JSON steht,
    // wird nicht gelesen – sonst würde aus einem Beispiel in einer Erklärung ein
    // auszuführender Befehl. Und `aufruf_aus_wert` nimmt nur etwas an, das nach
    // einem benannten Werkzeug aussieht.
    let getrimmt = antwort.trim();
    if !getrimmt.is_empty() {
        if let Ok(wert) = serde_json::from_str::<serde_json::Value>(getrimmt) {
            if let Some(aufruf) = aufruf_aus_wert(&wert) {
                return (vec![aufruf], String::new());
            }
        }
    }

    let (anfang, ende) = (rahmen_anfang(), rahmen_ende());
    let mut aufrufe = Vec::new();
    let mut rest = String::new();
    let mut uebrig = antwort;
    let mut angebrochen = false;

    while let Some(beginn) = uebrig.find(&anfang) {
        let nach_start = &uebrig[beginn + anfang.len()..];

        let Some((inhalt, nach_ende)) = nach_start.split_once(&ende) else {
            // Kein Ende: der Rahmen ist angebrochen. Das ist ein Modellfehler, und
            // es wird als Text behandelt statt als halber Befehl.
            rest.push_str(&uebrig[..beginn + anfang.len()]);
            rest.push_str(nach_start);
            uebrig = "";
            angebrochen = true;
            break;
        };

        rest.push_str(&uebrig[..beginn]);

        match aufruf_lesen(inhalt) {
            Some(aufruf) if aufrufe.len() < max_aufrufe() => aufrufe.push(aufruf),
            _ => {
                // Ein Aufruf, der nicht lesbar ist, geht als Text zurück – mit
                // seinem Rahmen, damit erkennbar bleibt, dass hier etwas schiefging.
                rest.push_str(&anfang);
                rest.push_str(inhalt);
                rest.push_str(&ende);
            }
        }

        uebrig = nach_ende;
    }

    rest.push_str(uebrig);

    if !angebrochen {
        rest = rest.trim_end().to_string();
    }

    (aufrufe, rest)
}

/// Liest genau einen Aufruf aus dem Inhalt eines Rahmens.
///
/// Der Zaun wird **vor** dem JSON entfernt und nicht danach: Nachher stünde am Ende
/// noch ein Backtick, und genau daran scheitert `serde_json` – ein Aufruf, der
/// sonst richtig wäre, käme als Text zurück und der Benutzer sähe eine technische
/// Notation, die niemand verlangt hat.
fn aufruf_lesen(inhalt: &str) -> Option<Aufruf> {
    // `unwrap_or` und kein `?`: Fehlt ein umschließender Zaun, ist der Inhalt
    // genau so gut – nur ohne Zaun. Das Zurückgeben von `None` hieße hier, einen
    // völlig gültigen Aufruf zu verwerfen, nur weil ein Modell den Rahmen mit
    // Backticks umgeben hat.
    let text = ohne_zaun(inhalt.trim()).unwrap_or_else(|| inhalt.trim().to_string());
    let wert: serde_json::Value = serde_json::from_str(text.trim()).ok()?;

    aufruf_aus_wert(&wert)
}

/// Schneidet einen Codezaun ab, falls einer um das JSON steht.
///
/// Modelle schreiben JSON gern zwischen ```json und ```, weil sie das für Code
/// gelernt haben. Weg ist der Zaun am Anfang samt Sprachangabe und der am Ende –
/// **bevor** geparst wird, nicht danach: Ein stehengebliebener Backtick am Ende ist
/// der Grund, warum ein sonst richtiger Aufruf als Text zurückkäme.
///
/// Enthält der Text nach dem ersten Zaun keinen zweiten, wird er unangetastet
/// gelassen. Ein einzeiliges ```json {"a":1}``` ist zwar möglich, der Versuch es zu
/// erkennen ginge aber über eine Heuristik, und eine falsch erkannte Sprachangabe
/// zerstädt sonst gültiges JSON. Was nicht eindeutig ein Zaun ist, bleibt Text.
fn ohne_zaun(text: &str) -> Option<String> {
    let anfang = text.find("```")?;
    let hinter = &text[anfang + 3..];

    // Nach dem Zaun muss eine Sprachangabe und ein Zeilenumbruch kommen, sonst
    // ist das kein umschließender Zaun, sondern Text, der zufällig drei
    // Backticks enthält.
    let (sprache, inhalt) = hinter.split_once('\n')?;

    if sprache.is_empty() || sprache.contains('{') || sprache.contains('"') {
        return None;
    }

    Some(
        inhalt
            .trim_end()
            .strip_suffix("```")
            .unwrap_or(inhalt.trim_end())
            .trim_end()
            .to_string(),
    )
}

/// Ein Aufruf aus einem beliebigen JSON-Wert.
///
/// Es werden zwei Schreibweisen angenommen, weil Modelle beide benutzen: die
/// aus der Anweisung (`tool`/`arguments`) und die aus Ollamas Format
/// (`function.name`/`function.arguments`). Und die Argumente dürfen ein Objekt
/// **oder** ein Text mit JSON darin sein – das zweite ist der häufigere Fehler und
/// der Grund, warum die Fehlermeldung von Ollama früher daran hing.
fn aufruf_aus_wert(wert: &serde_json::Value) -> Option<Aufruf> {
    if let Some(liste) = wert.as_array() {
        if liste.len() == 1 {
            return aufruf_aus_wert(&liste[0]);
        }

        return None;
    }

    let funktion = wert.get("function").unwrap_or(wert);
    let name = funktion
        .get("name")
        .or_else(|| funktion.get("tool"))
        .and_then(|wert| wert.as_str())?;

    let argumente = funktion
        .get("arguments")
        .or_else(|| funktion.get("parameters"))
        .or_else(|| funktion.get("args"))
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));

    // Als Zeichenstring geschickte Argumente sind JSON im Text. Sie werden hier
    // einmal lesbar gemacht; schafft das nicht, bleibt der Text erhalten, damit die
    // Werkzeugprüfung eine verständliche Meldung daraus macht.
    let argumente = match argumente.as_str() {
        Some(text) => {
            serde_json::from_str(text).unwrap_or(serde_json::Value::String(text.to_string()))
        }
        None => argumente,
    };

    if name.trim().is_empty() {
        return None;
    }

    Some(Aufruf {
        name: name.trim().to_string(),
        argumente,
    })
}

/// Ein gelesener Werkzeugaufruf.
#[derive(Debug, Clone, PartialEq)]
pub struct Aufruf {
    pub name: String,
    pub argumente: serde_json::Value,
}

#[cfg(test)]
mod tests;
