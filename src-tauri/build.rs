// Mimir wird aus dem Quelltext gebaut und nicht als Binary verteilt. Deshalb
// gibt es hier nichts zu tun: Es wird kein Pfad des Baurechners versteckt, weil
// es keinen gibt, der etwas verrät. Wer selbst baut, baut auf seinem eigenen
// Rechner, und der Pfad, den Tauri als Asset-Präfix einbettet, ist seiner.
//
// Auch die eingebaute Engine ändert daran nichts: Deren C++-Teil baut das Paket
// `llama-cpp-sys-2` über sein eigenes Build-Skript, und das weiß besser, wann es
// neu bauen muss, als eine Vermutung von hier. Auch die Prozessoranbindung der
// Engine steht nicht hier, sondern in `.cargo/config.toml` – sie muss gesetzt sein,
// **bevor** dieses Paket übersetzt, und Build-Skripte der Abhängigkeiten laufen
// vor dem eigenen.

use std::path::Path;

/// Prüft **alle** bereits übersetzten Engine-Verzeichnisse, nicht eines.
///
/// Gesucht wird von `OUT_DIR` aus, weil `llama-cpp-sys-2` keinen `cargo:root`
/// mitgibt: Es bleibt nichts übrig, als die Geschwisterverzeichnisse selbst
/// abzulaufen. Gerade die alten sind die gefährlichen, denn sie enthalten
/// gerade die Anweisungen für eine fremde CPU.
fn pruefe_engine_flags(baupfad: &Path) {
    // Die Schalter, die wir selbst setzen. `ON` ist der gefährliche Wert: Genau
    // ihn darf kein Cache mehr enthalten.
    let gesetzt: &[&str] = &[
        "GGML_AVX",
        "GGML_AVX2",
        "GGML_FMA",
        "GGML_F16C",
        "GGML_BMI2",
        "GGML_AVX512",
    ];

    let Ok(eintraege) = std::fs::read_dir(baupfad) else {
        return;
    };

    let mut widersprueche: Vec<String> = Vec::new();

    for eintrag in eintraege.flatten() {
        let name = eintrag.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("llama-cpp-sys-2-") {
            continue;
        }

        let cache = eintrag
            .path()
            .join("out")
            .join("build")
            .join("CMakeCache.txt");
        let Ok(inhalt) = std::fs::read_to_string(&cache) else {
            continue;
        };

        for schalter in gesetzt {
            // Nur ein **ausdrücklich gesetzter** Wert auf etwas anderes als `OFF`
            // ist ein Widerspruch. Ein Schlüssel, den der Cache gar nicht kennt,
            // ist keiner: Dort wurde die Funktion nie eingeschaltet, und genau
            // darum geht es.
            let widerspruch = inhalt.lines().any(|zeile| {
                zeile.starts_with(&format!("{schalter}:"))
                    && !zeile.contains("=OFF")
                    && !zeile.contains("=FALSE")
            });

            if widerspruch {
                widersprueche.push(format!("{schalter} in {name}"));
            }
        }
    }

    if widersprueche.is_empty() {
        return;
    }

    panic!(
        "Die Engine wurde mit Anweisungen gebaut, die diese CPU nicht kennt:\n  {}\n\
         Grund: Ein Build-Verzeichnis ist älter als ../.cargo/config.toml, und CMake\n\
         behält eine einmal konfigurierte Prozessoranbindung bei. Cargo führt das\n\
         Buildskript dann nicht erneut aus, und jedes Bauen linkt weiter dagegen.\n\
         Ein solcher Bau endet später mit SIGILL beim Laden des Modells.\n\
         Abhilfe:  rm -rf src-tauri/target/{{debug,release}}/build/llama-cpp-sys-2-*",
        widersprueche.join("\n  ")
    );
}

fn main() {
    let out = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    let engine = Path::new(&out)
        .join("build")
        .join("llama-cpp-sys-2-out")
        .join("build");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../.cargo/config.toml");

    // Nur prüfen, wenn es keinen Cache gibt: Nach dem ersten Bau ist `cmake`
    // nicht mehr nötig, und ein erneuter Aufruf ohne es – etwa beim Bauen auf
    // einem Rechner, der die Engine längst gebaut hat – würde grundlos abbrechen.
    if Path::new(&engine).is_dir() {
        println!("cargo:rerun-if-changed={}", engine.display());
    }

    // `OUT_DIR` endet auf `.../build/mimir-<hash>/out`; der Ordner `build` darunter
    // ist der, in dem alle Pakete ihre Übersetzung ablegen.
    if let Some(baupfad) = Path::new(&out).ancestors().nth(2) {
        pruefe_engine_flags(baupfad);
    }

    tauri_build::build()
}
