// Mimir wird aus dem Quelltext gebaut und nicht als Binary verteilt. Deshalb
// gibt es hier nichts zu tun: Es wird kein Pfad des Baurechners versteckt, weil
// es keinen gibt, der etwas verrät. Wer selbst baut, baut auf seinem eigenen
// Rechner, und der Pfad, den Tauri als Asset-Präfix einbettet, ist seiner.
fn main() {
    tauri_build::build()
}