use super::*;

/// Der wichtigste Test im ganzen Modul: **Der Text einer Mail darf nur aus
/// `text/plain` stammen.** HTML wird verworfen, weil dort Anhang im Unsichtbaren
/// und Links hinter Beschriftungen liegen – beides Formen, in denen ein
/// Absender mitliest, was der Empfänger bestätigen soll.
#[test]
fn nimmt_nur_text_und_nie_html() {
    let roh = "From: a@example.org\r\n\
Subject: Rechnung\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><body><p>Rechnung <b>500 Euro</b></p>\
<img src=\"https://evil.example/spion.png\"><a href=\"https://evil.example\">Jetzt zahlen</a>\
</body></html>";

    let (aufgeteilt, anhaenge) = zerlege_mime(roh.as_bytes());

    assert!(
        !aufgeteilt.text.contains("evil.example"),
        "HTML ist durchgekommen"
    );
    assert!(!aufgeteilt.text.contains("<html"));
    assert!(
        aufgeteilt.text.is_empty(),
        "HTML-Teil wurde als Text genommen"
    );
    assert!(anhaenge.is_empty());
}

#[test]
fn nimmt_aus_einem_mehrteil_den_text_und_listet_anhaenge() {
    let roh = "From: a@example.org\r\n\
Subject: Bericht\r\n\
Content-Type: multipart/mixed; boundary=\"WAND\"\r\n\
\r\n\
--WAND\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hier ist der eigentliche Text.\r\n\
--WAND\r\n\
Content-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"Rechnung.pdf\"\r\n\
\r\n\
%PDF-1.4\r\n\
--WAND--\r\n";

    let (aufgeteilt, anhaenge) = zerlege_mime(roh.as_bytes());

    assert_eq!(aufgeteilt.text.trim(), "Hier ist der eigentliche Text.");
    assert_eq!(anhaenge, vec!["Rechnung.pdf"]);
    assert_eq!(aufgeteilt.kopf.absender, "a@example.org");
    assert_eq!(aufgeteilt.kopf.betreff, "Bericht");
}

/// Ein Anhang wird **nicht** in den Text aufgenommen – sonst landet ein
/// executables Dokument im Kontext des Modells und in der Zusammenfassung.
#[test]
fn laesst_anhaenge_aus_dem_text() {
    let roh = "From: a@example.org\r\n\
Subject: Anhang\r\n\
Content-Type: multipart/mixed; boundary=\"WAND\"\r\n\
\r\n\
--WAND\r\n\
Content-Type: text/plain\r\n\
\r\n\
Bitte pruefen.\r\n\
--WAND\r\n\
Content-Type: text/plain; name=\"notiz.txt\"\r\n\
Content-Disposition: attachment; filename=\"notiz.txt\"\r\n\
\r\n\
DIESER TEXT DARF NICHT IM BODY STEHEN\r\n\
--WAND--\r\n";

    let (aufgeteilt, anhaenge) = zerlege_mime(roh.as_bytes());

    assert!(!aufgeteilt.text.contains("DARF NICHT"), "Anhang im Text");
    assert_eq!(aufgeteilt.text.trim(), "Bitte pruefen.");
    assert_eq!(anhaenge, vec!["notiz.txt"]);
}

#[test]
fn dekodiert_base64_und_quoted_printable() {
    let base64 = base64_decode(b"SGVsbG8gV2VsdA==");
    assert_eq!(String::from_utf8_lossy(&base64), "Hello Welt");

    let quoted = dekodiere_quoted_printable(b"Gr=C3=BC=C3=9Fe");
    assert_eq!(String::from_utf8_lossy(&quoted), "Grüße");
}

/// Der weiche Umbruch: `=` am Zeilenende bedeutet, dass die Zeile weitergeht.
/// Ohne das zerfällt ein Text, der lang umbrochen wurde, mitten im Wort.
#[test]
fn loest_den_weichen_umbruch_auf() {
    let daten = dekodiere_quoted_printable(b"Hallo=\r\nWelt");
    assert_eq!(String::from_utf8_lossy(&daten), "HalloWelt");
}

/// **`=?UTF-8?Q?…?=` muss lesbar werden.** Ohne diese Stufe stünde für jede
/// Mail mit Umlauten eine Zeichenkette in der Leiste, die nichts bedeutet.
#[test]
fn dekodiert_kopfzeilen() {
    assert_eq!(dekodiere_kopf(b"=?UTF-8?Q?Gr=C3=BC=C3=9Fe?="), "Grüße");
    assert_eq!(dekodiere_kopf(b"Normaler Betreff"), "Normaler Betreff");
    assert_eq!(dekodiere_kopf(b"=?UTF-8?B?R3LDvMOfZQ==?="), "Grüße");

    // **Mehrere kodierte Felder in einer Zeile.** Sie entstehen, wenn der
    // Betreff zu lang zum Kodieren war; das Leerzeichen zwischen ihnen ist der
    // Umbruch und gehört nicht zum Betreff (RFC 2047 §6.2).
    assert_eq!(
        dekodiere_kopf(b"=?UTF-8?Q?Rechnung?= =?UTF-8?Q?f=C3=BCr_Oktober?="),
        "Rechnung für Oktober"
    );

    // Zwischen gewöhnlichem Text und kodiertem Feld gehört das Leerzeichen
    // dagegen zum Betreff – dort wird es nicht entfernt.
    assert_eq!(
        dekodiere_kopf(b"Betreff =?UTF-8?Q?Gr=C3=BC=C3=9Fe?="),
        "Betreff Grüße"
    );

    // Ein kaputter Wert darf den Rest der Zeile nicht verschlucken.
    assert!(dekodiere_kopf(b"Betreff =?kaputt").contains("Betreff"));
}

#[test]
fn trennt_kopfzeilen_vom_rumpf() {
    let roh = b"From: a@example.org\r\nSubject: Test\r\n\r\nDer Rumpf";
    let (kopf, rumpf) = trenne_kopf(roh);

    assert_eq!(value(&kopf, "from").as_deref(), Some("a@example.org"));
    assert_eq!(value(&kopf, "subject").as_deref(), Some("Test"));
    assert_eq!(rumpf, b"Der Rumpf");
}

/// Kopfzeilen dürfen umgebrochen werden; beide Teile gehören demselben Feld.
#[test]
fn fuegt_fortsetzungszeilen_zusammen() {
    let roh = b"Subject: Ein sehr langer\r\n Betreff der umbrochen wurde\r\n\r\nText";
    let (kopf, _) = trenne_kopf(roh);

    assert_eq!(
        value(&kopf, "subject").as_deref(),
        Some("Ein sehr langer Betreff der umbrochen wurde")
    );
}

#[test]
fn base64_kodiert_korrekt() {
    // Der Weg hier ist: PLAIN-Antwort und Base64-Inhalt brauchen dieselbe
    // Kodierung. Ein Fehler darin ergäbe ein falsches Passwort – der Fehler
    // sähe aus wie ein falsches Passwort.
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    assert_eq!(base64_encode(b"Kai"), "S2Fp");

    let passwort = "\0kai@example.org\0app-passwort";
    assert_eq!(
        base64_decode(base64_encode(passwort.as_bytes()).as_bytes()),
        passwort.as_bytes()
    );
}

#[test]
fn zerlegt_server_adresse() {
    assert_eq!(
        zerlege_server("imaps://imap.example.org:993").unwrap(),
        ("imap.example.org".to_string(), 993)
    );
    assert!(zerlege_server("imap.example.org").is_err());
    assert!(zerlege_server("imaps://:993").is_err());
    assert!(zerlege_server("imaps://imap.example.org:abc").is_err());
}

#[test]
fn loest_absendernamen_zurueck() {
    assert_eq!(
        aufloese_namen("Kai <kai@example.org>"),
        "Kai (kai@example.org)"
    );
    assert_eq!(aufloese_namen("kai@example.org"), "kai@example.org");
    assert_eq!(
        aufloese_namen("\"Kai Weber\" <kai@example.org>, Andere <a@example.org>"),
        "Kai Weber (kai@example.org), Andere (a@example.org)"
    );
}
