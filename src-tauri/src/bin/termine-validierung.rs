// Gibt den Werkzeugsatz des Terminumfangs aus, damit die Validierung mit einem
// echten Modell gegen **dieselben** Schemata und Anweisungen läuft, die Mimir
// im Betrieb schickt.
//
// Warum das ein Skript ist und nicht in `termine-validierung.mjs` steht: Die
// Schemata sind Rust. Sie in JavaScript nachzubauen hieße, sie zu pflegen, und
// der Fehler fällt erst auf, wenn ein Modell in der Anwendung scheitert. Hier
// kommt stattdessen das Original heraus, aus derselben Funktion, aus der auch
// `list_tools` sie holt.
//
// Aufruf:  cargo run --manifest-path src-tauri/Cargo.toml --bin termine-validierung -- <server> <modell> [sätze]

use mimir_lib::termine_validierung::{frage, hole_modell, Antwort, ERWARTUNGEN, NACHFRAGE, SAETZE};

#[tokio::main]
async fn main() {
    let argumente: Vec<String> = std::env::args().skip(1).collect();

    if argumente.len() < 2 {
        eprintln!(
            "Verwendung: termine-validierung <server> <modell> [anzahl sätze]\n\n\
             Beispiel: termine-validierung http://localhost:11434 qwen3:4b"
        );
        std::process::exit(2);
    }

    let server = &argumente[0];
    let modell = argumente[1].clone();
    let anzahl: usize = argumente
        .get(2)
        .and_then(|wert| wert.parse().ok())
        .unwrap_or(SAETZE.len())
        .min(SAETZE.len());

    let modelle = match hole_modell(server).await {
        Ok(modelle) => modelle,
        Err(grund) => {
            // Ohne Server prüft dieses Programm nichts. Das wird laut gesagt und
            // nicht als leerer Erfolg gemeldet.
            eprintln!("Der Server {server} antwortet nicht: {grund}");
            eprintln!("Ohne Server ist nichts geprüft. Es wird kein Erfolg vorgetäuscht.");
            std::process::exit(2);
        }
    };

    if !modelle.contains(&modell) {
        eprintln!("„{modell}“ gibt es dort nicht.");
        eprintln!("Vorhanden: {}", modelle.join(", "));
        std::process::exit(2);
    }

    println!("— Der Terminumfang gegen ein Modell —");
    println!();
    println!("Server: {server}");
    println!("Auf dem Server: {}", modelle.join(", "));
    println!("Geprüft mit: {modell}");
    println!("Sätze: {anzahl} von {}", SAETZE.len());
    println!();

    let mut richtig = 0;
    let mut brauchbar = 0;
    let mut geprueft = 0;
    let mut befunde: Vec<String> = Vec::new();

    for (index, satz) in SAETZE.iter().take(anzahl).enumerate() {
        let erwartet = &ERWARTUNGEN[index];

        let antwort = match frage(server, &modell, satz).await {
            Ok(antwort) => antwort,
            Err(grund) => {
                println!("  {:2}. FEHLER  {grund}", index + 1);
                befunde.push(format!(
                    "„{satz}“ kam nicht durch: {grund} — {}",
                    erwartet.tag
                ));
                continue;
            }
        };

        geprueft += 1;

        // Bei diesen Sätzen ist die richtige Antwort eine **Frage**, kein
        // Werkzeugaufruf. Geprüft wird deshalb das Gegenteil: Dass nichts
        // aufgerufen wurde und der Text überhaupt eine Frage enthält. Ein
        // Werkzeugaufruf an dieser Stelle ist ein Fehler, kein Teilerfolg.
        if erwartet.werkzeug == NACHFRAGE {
            // Nach einem Fragezeichen zu suchen ist zu grob: Das Modell
            // formuliert auf Deutsch auch ohne Fragezeichen als Frage – „Um
            // einen Termin mit Sarah einzutragen, benötige ich mehr
            // Informationen“ ist eine Nachfrage und keine Behauptung. Gezählt
            // wird deshalb, dass **kein** Werkzeug aufgerufen wurde und die
            // Antwort überhaupt nach einem Fehlen fragt.
            let fragt_nach = antwort.text.contains('?')
                || [
                    "benötige",
                    "brauche",
                    "fehlt",
                    "welche",
                    "welchen",
                    "welcher",
                ]
                .iter()
                .any(|wort| antwort.text.to_lowercase().contains(wort));

            let gefragt = antwort.werkzeug().is_none() && fragt_nach;
            let vollstaendig = antwort.planbar(satz);

            println!(
                "  {:2}. {}  [{}]",
                index + 1,
                if gefragt { "richtig" } else { "FALSCH " },
                erwartet.tag
            );
            println!("      » {satz}");
            println!(
                "      {}",
                antwort.text.chars().take(200).collect::<String>()
            );
            println!("      {:.1}s", antwort.sekunden);
            println!();

            if gefragt {
                richtig += 1;
                brauchbar += 1;
                continue;
            }

            befunde.push(format!(
                "„{satz}“ hätte nachfragen müssen ({}), kam aber mit {}\n    gefüllte Felder: {}\n    Antwort: {}",
                erwartet.tag,
                aufruf_wort(&antwort),
                if antwort.gefuellte_felder().is_empty() {
                    "keine".to_string()
                } else {
                    antwort.gefuellte_felder().join(", ")
                },
                antwort.text.chars().take(120).collect::<String>()
            ));
            if let Err(mangel) = vollstaendig {
                befunde.push(format!(
                    "    und daraus wäre kein Termin geworden: {mangel}"
                ));
            }
            continue;
        }

        let aufruf = antwort.werkzeug().unwrap_or("kein Werkzeug");
        let passt = aufruf == erwartet.werkzeug;

        println!(
            "  {:2}. {}  [{}]",
            index + 1,
            if passt { "richtig" } else { "FALSCH " },
            erwartet.tag
        );
        println!("      » {satz}");

        if aufruf == "kein Werkzeug" {
            // Der rohe Name steht daneben, wenn das Modell eines rief, das die
            // Namensprüfung nicht besteht: Sonst sähe der Fall nach „gar kein
            // Werkzeug" aus, obwohl eines kam.
            match antwort.roh_werkzeug() {
                Some(roh) => println!("      (Name nicht bestanden: {roh})"),
                None => println!(
                    "      (kein Werkzeug) {}",
                    antwort.text.chars().take(160).collect::<String>()
                ),
            }
        } else {
            println!(
                "      {aufruf} {}",
                serde_json::to_string(&antwort.argumente).unwrap_or_default()
            );
        }

        println!("      {:.1}s", antwort.sekunden);
        println!();

        // Ein Aufruf ohne verwertbare Angaben ist das ernsteste Ergebnis: Das
        // Modell hat das Werkzeug **benannt**, aber nichts eingetragen, was Mimir
        // bauen könnte. Das ist kein halber Erfolg, sondern ein Fehler mit
        // besonders irreführendem Aussehen – in der Ausgabe steht ein
        // vollständiger Werkzeugname und sonst nichts.
        //
        // Solche Fälle zählen deshalb nicht als „richtiges Werkzeug". Vor dem
        // Zählen kommt die Prüfung, ob überhaupt etwas eingetragen wurde.
        if antwort.werkzeug().is_some() && !antwort.trägt_angaben() {
            befunde.push(format!(
                "„{satz}“\n    rief {aufruf} ohne brauchbare Angaben auf (gefiillt: {})",
                if antwort.gefuellte_felder().is_empty() {
                    "nichts".to_string()
                } else {
                    antwort.gefuellte_felder().join(", ")
                }
            ));
            continue;
        }

        // Das Werkzeug kann richtig und das Ergebnis trotzdem unbrauchbar sein:
        // Mimir rechnet die gelieferten Worte selbst. Ein Lauf, der nur auf den
        // Namen sieht, würde einen Termin für gut halten, den die Anwendung
        // nicht bauen kann.
        let planbar = match antwort.planbar(satz) {
            Ok(()) => {
                println!("      Mimir kann daraus einen Termin bauen");
                true
            }
            Err(mangel) => {
                println!("      {mangel}");
                befunde.push(format!(
                    "„{satz}“\n    {mangel}\n    (Werkzeug {aufruf} war richtig)"
                ));
                false
            }
        };

        if passt {
            richtig += 1;
        }
        if passt && planbar {
            brauchbar += 1;
        }

        if !passt {
            befunde.push(format!(
                "„{satz}“\n    erwartet {} ({}), kam {}",
                erwartet.werkzeug, erwartet.tag, aufruf
            ));
        }
    }

    println!("— Zusammenfassung —");
    println!();
    println!("{richtig} von {geprueft} Sätzen mit dem erwarteten Werkzeug.");
    println!(
        "{brauchbar} von {geprueft} Sätzen vollständig brauchbar: richtiges Werkzeug und ein \
         Termin, den Mimir daraus bauen kann."
    );

    if !befunde.is_empty() {
        println!();
        println!("Nicht passend:");
        for befund in &befunde {
            println!("  {befund}");
        }
    }
}

/// Wie der Aufruf in einer Meldung genannt wird, wenn keiner erwartet war.
fn aufruf_wort(antwort: &Antwort) -> String {
    match antwort.werkzeug() {
        Some(name) => format!("dem Werkzeug {name}"),
        None => "keinem Werkzeug, aber ohne Frage".to_string(),
    }
}
