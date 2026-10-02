// Setzt `CARGO_MANIFEST_DIR` auf einen neutralen Wert, bevor `tauri_build` läuft.
//
// **Dieser Ansatz ist nicht brauchbar und wird nicht angewendet.** Er ist hier
// festgehalten, damit niemand ihn erneut ausprobiert.
//
// Die Idee: Tauri verbindet einen relativen `frontendDist` mit
// `CARGO_MANIFEST_DIR` und bettet das Ergebnis als **absoluten** Pfad in das
// Binary – als Präfix der Asset-Schlüssel. Dort steht der vollständige Pfad des
// Rechners, an dem Mimir gebaut wurde, und damit der Benutzername.
//
// Warum es nicht geht: Dieselbe Variable braucht Tauri auch, um
// `tauri.conf.json` zu finden. Setzt man sie auf einen neutralen Wert, meldet
// `tauri::generate_context!` beim Kompilieren:
//
//     error: unable to read Tauri config file at /mimir/tauri.conf.json
//
// Der Pfad zum Manifest und der im Binary sind nicht trennbar. `--remap-path-prefix`
// in `.cargo/config.toml` greift ebenfalls nicht, weil der Pfad zur Bauzeit als
// Zeichenkette entsteht und nicht aus dem Quelltext stammt.
//
// Was bleibt: Der Pfad steht an **einer** Stelle im Binary und wird von
// `src/tests/binary-pruefen.mjs` gemeldet. Enthalten sind der Benutzername des
// Baurechners und sein Projektverzeichnis – beides verrät nichts, was nicht schon
// im Namen der App steht, und beides ist beim Bauen auf einem fremden Rechner
// ohnehin anders.

fn main() {
    tauri_build::build()
}
