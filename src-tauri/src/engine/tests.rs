//! Prüfungen für die eingebaute Engine.
//!
//! Geprüft wird, was ohne Modell entschieden wird: die Threadzahl und die
//! Schleifenerkennung. Beides sind Entscheidungen, die man in einem Fenster mit
//! zwei Gigabyte Speicher nicht sieht – ein Rechner, der sich selbst festsetzt,
//! und eine Antwort, die endlos weiterläuft.
//!
//! Das Rechnen selbst braucht ein Modell und ist deshalb nicht hier: Es läuft über
//! den Weg, den ein Benutzer geht – ein Katalogeintrag herunterladen und fragen.

use super::*;

/// Die Engine lässt dem WebView einen Kern.
///
/// Sie rechnet auf allen Kernen, die das Betriebssystem anbietet, und der WebView
/// hängt an denselben. Ohne Abzug hätte eine Antwort auf einem Vierkerner drei
/// Kerne, und die Oberfläche liefe sichtbar ruckelnd nebenher.
#[test]
fn die_threadzahl_laesst_platz_fuer_die_oberflaeche() {
    let threads = standard_threads();

    assert!(threads >= 1, "ohne Threads rechnet gar nichts");
    assert!(
        threads <= 32,
        "{} Threads sind mehr, als sinnvoll sind",
        threads
    );
}

/// Die Schleifenerkennung greift bei einer echten Wiederholung.
#[test]
fn eine_wiederholte_antwort_wird_erkannt() {
    let satz = "Dies ist ein wiederholter Abschnitt der Antwort. ";
    let text = format!("{satz}{satz}{satz}");

    assert!(
        wiederholt(&text),
        "dreimal derselbe Abschnitt gilt als Schleife"
    );
}

/// Und sie greift nicht bei einer langen Antwort aus verschiedenen Sätzen.
///
/// Der Gegenfall der Schleife: viel Text, aber jeder Abschnitt kommt nur einmal vor.
/// Ohne diese Grenze würde jede ausführliche Antwort abgeschnitten – und der
/// Benutzer bekäme eine scheinbar vollständige Antwort, die unvollständig ist.
#[test]
fn eine_lange_antwort_gilt_nicht_als_schleife() {
    let text = "Der Termin am Freitag steht im Kalender. ".repeat(3)
        + "Ich habe ihn auf den 14. November gelegt. "
        + "Der Wecker klingelt eine halbe Stunde vorher. "
        + "Eine Erinnerung per Mail ist nicht eingestellt, weil die Adresse fehlt. "
        + "Soll ich sie aus dem Kalender holen? Das geht, wenn du es sagst.";

    assert!(!wiederholt(&text), "verschiedene Sätze sind keine Schleife");
}

/// Kurze Antworten können gar keine Schleife haben.
///
/// Der Vergleich braucht mindestens einen Abschnitt und dessen Wiederholung. Ohne
/// diese Grenze würde jede Antwort abgeschnitten, die zufällig kurz genug ist.
#[test]
fn eine_kurze_antwort_gilt_nicht_als_schleife() {
    assert!(!wiederholt("hallo"));
    assert!(!wiederholt(""));
}

/// **Jedes normale Fenster muss eine Antwort zulassen.**
///
/// Vorher stand in `erzeugen` die Bedingung `auftrag + MAX_ANTWORT_TOKEN <= fenster`.
/// Die ist bei jedem Fenster unter der Obergrenze von 8192 Token immer falsch –
/// also bei jedem Fenster, das ein Mensch einstellt. Die Engine hat daraufhin
/// jede Anfrage abgelehnt, noch bevor sie gerechnet hat, und der Benutzer hätte
/// „Fenster zu klein" bekommen bei einem Fenster, das genau richtig war.
#[test]
fn ein_normales_fenster_laesst_eine_antwort_zu() {
    // Das ist der Vorgabewert, und es ist der Wert aus der Oberfläche.
    for (fenster, auftrag) in [
        (2048, 100),  // ein kurzer Chat
        (2048, 1500), // ein langer Verlauf
        (4096, 3800), // ein sehr langer Verlauf
        (512, 200),   // ein absichtlich kleines Fenster
    ] {
        let (_, antwort) = super::fenster_aufteilen(fenster, auftrag);

        assert!(
            antwort >= super::MIN_ANTWORT_TOKEN,
            "bei {} Token Fenster und {} Token Auftrag blieben nur {} für die Antwort",
            fenster,
            auftrag,
            antwort
        );
    }
}

/// Ein Auftrag, der das Fenster sprengt, wird abgeschnitten – und was dann noch
/// übrig ist, muss für eine Antwort reichen, sonst hilft auch das Kürzen nichts.
#[test]
fn ein_zu_grosser_auftrag_wird_gekuerzt_und_lässt_platz() {
    let (auftrag, antwort) = super::fenster_aufteilen(2048, 50_000);

    // Nicht 2048, sondern 2048 minus das, was für eine Antwort bleiben muss. Ein
    // Auftrag, der das ganze Fenster belegt, wäre durch das Fenster geschützt –
    // und die Antwort hätte keinen Platz mehr.
    assert_eq!(
        auftrag,
        2048 - (super::MIN_ANTWORT_TOKEN + 8),
        "der Auftrag durfte das ganze Fenster fressen"
    );
    assert!(
        antwort >= super::MIN_ANTWORT_TOKEN,
        "es blieb nichts für die Antwort"
    );
    assert!(
        auftrag + antwort <= 2048,
        "Auftrag und Antwort passen nicht zusammen in das Fenster"
    );
}

/// Und die Obergrenze gilt weiterhin: Ein sehr großes Fenster erzeugt keine
/// unbegrenzte Antwort.
#[test]
fn ein_sehr_grosses_fenster_bleibt_begrenzt() {
    let (_, antwort) = super::fenster_aufteilen(super::MAX_FENSTER, 100);

    assert_eq!(antwort, super::MAX_ANTWORT_TOKEN);
}

/// Wie viele Durchgänge ein Auftrag braucht.
///
/// **Ohne diese Zerlegung beendet llama.cpp das Programm.** Es verlangt für einen
/// Durchgang höchstens `n_batch` Token und ruft eine Zusicherung auf, wenn mehr
/// kommen. Ein Auftrag mit Systemanweisung und Werkzeugbeschreibung – im lokalen
/// Betrieb **jeder** Auftrag, weil der Umfang dort `termine` ist – hat gut 1400
/// Token und damit bei 512 mehr als zwei Durchgänge nötig.
///
/// Der Absturz war die Ursache dafür, dass die Oberfläche gar keine Antwort
/// bekam: Die Engine brach ab, bevor sie den ersten Token erzeugt hatte.
#[test]
fn ein_langer_auftrag_wird_zerlegt() {
    let stapel = 512usize;

    for (auftrag, durchgaenge) in [
        (100usize, 1usize), // ein kurzer Satz
        (512, 1),           // genau ein Stapel
        (513, 2),           // einer darüber
        (1400, 3),          // der Fall aus dem lokalen Betrieb
        (2048, 4),          // ein ganzes Fenster
    ] {
        let berechnet = super::durchgaenge(auftrag, stapel);

        assert_eq!(berechnet, durchgaenge, "{auftrag} Token");
        assert!(
            berechnet * stapel >= auftrag,
            "{auftrag} Token passen nicht in {durchgaenge} Durchgänge"
        );
    }
}

/// Und ein Auftrag von null Token braucht gar keinen Durchgang.
#[test]
fn ein_leerer_auftrag_braucht_nichts() {
    assert_eq!(super::durchgaenge(0, 512), 0);
}
