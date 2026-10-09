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

fn main() {
    // Der Ort, an dem llama-cpp-sys-2 sein C++ ablegt. Er entsteht beim ersten
    // Bau und ist danach stabil; ohne diesen Hinweis baut Cargo den C++-Teil nicht
    // neu, wenn sich dort etwas ändert.
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

    tauri_build::build()
}
