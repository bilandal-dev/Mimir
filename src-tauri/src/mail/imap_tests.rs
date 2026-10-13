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

    // **Kein `passwort` als Variablenname.** Der Wert hier ist das Format, in
    // dem AUTHENTICATE PLAIN seine Zugangsdaten erwartet – ein Beispiel, kein
    // Schlüssel. Als `passwort` benannt sieht er im Quelltext aber aus wie einer,
    // und `src/tests/keine-geheimnisse.mjs` meldet zu Recht einen Wert, der an
    // einer Stelle steht, die nach Geheimnis aussieht.
    let zugangsdaten = "\0kai@example.org\0app-passwort";
    assert_eq!(
        base64_decode(base64_encode(zugangsdaten.as_bytes()).as_bytes()),
        zugangsdaten.as_bytes()
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

    // **465 gehört zu SMTP.** Die Adresse entsteht beim Abschreiben aus einer
    // Gmail-Einstellung, und dort stehen beide Ports nebeneinander. Der Fehler
    // fiel lange nicht auf, weil die Verbindung dann nicht scheitert, sondern
    // schweigt – die Meldung muss ihn also beim Eintragen finden, nicht erst
    // nach einem Wartzeitversuch.
    let smtp_port = zerlege_server("imaps://imap.gmail.com:465").unwrap_err();
    assert!(
        smtp_port.contains("993"),
        "die Meldung nennt den richtigen Port, nicht nur den falschen: {smtp_port}"
    );
    assert!(
        smtp_port.contains("SMTP"),
        "die Meldung sagt, worum es geht: {smtp_port}"
    );
    assert!(zerlege_server("imaps://smtp.example.org:465").is_err());
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

// --- Die Vorschau in der Leiste ------------------------------------------------

/// **Die Vorschau zeigt den Text, nicht den Kopf.** Der begrenzte Abruf holt die
/// ersten Bytes der *ganzen* Nachricht – Kopfzeilen stehen mit drin. Blieben sie
/// stehen, stünde in der Leiste `Content-Type: multipart/mixed` als erster Satz
/// einer Mail.
#[test]
fn die_vorschau_ignoriert_die_kopfzeilen() {
    let chunk = "From: a@example.org\r\n\
Subject: Termin\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
X-Sehr-Lange-Kopfzeile: mit vielen woerten die nichts zur vorschau beitragen\r\n\
\r\n\
Hallo Kai,\r\n\
\r\n\
Donnerstag passt.";

    let vorschau = vorschau_aus_chunk(chunk.as_bytes());

    assert_eq!(vorschau, "Hallo Kai,\n\nDonnerstag passt.");
    assert!(!vorschau.contains("Content-Type"), "{vorschau}");
    assert!(!vorschau.contains("Subject:"), "{vorschau}");
}

/// **Fehlt der Trenner, ist der ganze Anhang ein Kopf** – und wird nicht als Text
/// ausgegeben. Lieber keine Vorschau als eine erfundene.
#[test]
fn ohne_trenner_gibt_es_keine_vorschau() {
    let nur_kopf = "From: a@example.org\r\nSubject: Termin\r\nContent-Type: text/plain";

    assert_eq!(vorschau_aus_chunk(nur_kopf.as_bytes()), "");
}

/// **Zeilenumbrüche sind kein Absatz.** Eine Signatur aus fünf Leerzeilen oder ein
/// umgebrochener Betreff im Rumpf dürfen die Vorschau nicht in fünf Teile
/// zerschneiden, die mitten im Satz enden.
#[test]
fn zeilenumbrueche_werden_zu_einem_absatz() {
    let chunk = "Content-Type: text/plain; charset=utf-8\r\n\r\n\
Hallo Kai,\r\n\
wie geht es\r\n\
dir?\r\n\
\r\n\
Bis bald\r\n\
\r\n\
-- \r\n\
Kai Weber";

    let vorschau = vorschau_aus_chunk(chunk.as_bytes());

    assert_eq!(
        vorschau,
        "Hallo Kai, wie geht es dir?\n\nBis bald\n\n-- Kai Weber"
    );
}

/// **Die Grenze bricht am Absatz, nicht im Satz** – dieselbe Regel wie im Fenster.
#[test]
fn die_vorschau_wird_an_der_grenze_gekuerzt() {
    let chunk = format!(
        "Content-Type: text/plain; charset=utf-8\r\n\r\n{}",
        "Ein Absatz. ".repeat(200)
    );

    let vorschau = vorschau_aus_chunk(chunk.as_bytes());

    assert!(
        vorschau.len() <= MAX_MAIL_PREVIEW_BYTES,
        "zu lang: {} Zeichen",
        vorschau.len()
    );
    assert!(vorschau.ends_with("[…gekürzt…]"), "{vorschau}");
}

/// **Aus einem Mehrteil kommt der Textanteil, nicht die Dateiliste.** Eine Mail
/// mit PDF und Bild zeigt in der Liste den ersten Satz, nicht den Trenner.
#[test]
fn die_vorschau_zeigt_den_textteil_eines_mehrteils() {
    let chunk = "Content-Type: multipart/mixed; boundary=\"WAND\"\r\n\
\r\n\
--WAND\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Anbei die Rechnung fuer diesen Monat.\r\n\
--WAND\r\n\
Content-Type: application/pdf\r\n\
Content-Disposition: attachment; filename=\"Rechnung.pdf\"\r\n\
\r\n\
%PDF-1.4\r\n\
--WAND--\r\n";

    let vorschau = vorschau_aus_chunk(chunk.as_bytes());

    assert_eq!(vorschau, "Anbei die Rechnung fuer diesen Monat.");
    assert!(!vorschau.contains("WAND"), "{vorschau}");
    assert!(!vorschau.contains("PDF"), "{vorschau}");
}

/// **HTML bleibt auch in der Vorschau HTML.** Dieselbe Regel wie im Fenster: Der
/// Code wird nicht angezeigt, er verschwindet. Wer eine reine HTML-Mail bekommt,
/// sieht in der Leiste gar nichts – das ist korrekt und kein Fehler.
#[test]
fn eine_reine_html_mail_zeigt_keine_vorschau() {
    let chunk = "Content-Type: text/html; charset=utf-8\r\n\r\n\
<html><body><p>Rechnung <b>500 Euro</b></p></body></html>";

    assert_eq!(vorschau_aus_chunk(chunk.as_bytes()), "");
}

/// **Abgeschnittener MIME-Anhang.** So sieht ein begrenzter Abruf in der Praxis
/// aus: Der Text ist da, die abschließende Grenze fehlt. Das ist kein Fehler, an
/// dem die Vorschau scheitern darf.
#[test]
fn vertragt_einen_abgeschnittenen_mehrteil() {
    let chunk = "Content-Type: multipart/alternative; boundary=\"WAND\"\r\n\
\r\n\
--WAND\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Der Text ist abgeschnitten, die Grenze fehlt";

    let vorschau = vorschau_aus_chunk(chunk.as_bytes());

    assert!(
        vorschau.starts_with("Der Text ist abgeschnitten"),
        "{vorschau}"
    );
}

/// **Quoted-Printable.** Die häufigste Kodierung in ausgehenden Mails; ohne
/// Decodieren stünde in der Leiste `Hallo=20Kai` und im Postfach `Hallo Kai`.
#[test]
fn die_vorschau_dekodiert_quoted_printable() {
    let chunk = "Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Hallo Kai,\r\n\
\r\n\
das kosten=20Euro.\r\n\
Ein sehr langer Satz, der am Ende der Zei=\r\nle umbrochen wurde.";

    let vorschau = vorschau_aus_chunk(chunk.as_bytes());

    assert!(vorschau.contains("Hallo Kai"), "{vorschau}");
    assert!(!vorschau.contains("=20"), "{vorschau}");
    // Der harte Umbruch mitten im Wort muss wieder ein Wort sein.
    assert!(vorschau.contains("Zeile umbrochen"), "{vorschau}");
}

/// **Leere und winzige Abfälle** dürfen keine panik auslösen.
#[test]
fn ueberlebt_leere_und_kleine_abfaelle() {
    assert_eq!(vorschau_aus_chunk(b""), "");
    assert_eq!(vorschau_aus_chunk(b"\r\n\r\n"), "");
    assert_eq!(vorschau_aus_chunk(b"Content-Type: text/plain\r\n\r\n"), "");
    assert_eq!(
        vorschau_aus_chunk(b"Content-Type: text/plain\r\n\r\n   \r\n"),
        ""
    );
}

/// **Eine einfache Textmail ist keine Mehrteilmail.** Sie hat keine Grenze,
/// keinen zweiten Kopf und keinen `content-type` am Rumpf – der Rumpf *ist* der
/// Text. Dieser Test hält fest, dass die Zerlegung das auch so behandelt.
#[test]
fn eine_einfache_textmail_liefert_ihren_text() {
    let roh = "From: a@example.org\r\n\
Subject: Termin\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hallo Kai,\r\n\
\r\n\
Donnerstag passt.";

    let (aufgeteilt, anhaenge) = zerlege_mime(roh.as_bytes());

    assert!(
        aufgeteilt.text.contains("Donnerstag passt."),
        "der Text einer einfachen Textmail geht verloren: {:?}",
        aufgeteilt.text
    );
    assert!(anhaenge.is_empty());
}

// --- Die Leitung selbst --------------------------------------------------------

/// Ein Server, der die Verbindung annimmt und dann schweigt.
///
/// **Der Fehler, den diese Attrappe verhindert.** Eine Leitung ohne Antwort war
/// bis hierher kein Fehler, sondern ein Auftrag, der nie zurückkam: Die Leiste
/// stand auf „Postfach wird geladen", kein Klick half, und die Konsole blieb
/// leer. Genau daran ließ sich nicht absehen, ob Mimir oder der Server nicht
/// antwortet – und die Suche nach der Ursacle ging an der falschen Stelle weiter.
/// Eine Frist macht daraus eine Meldung, die benennt, was fehlt.
///
/// Die Zeit läuft hier nicht in echt: Der Test wartet 50 Millisekunden statt der
/// 45 Sekunden aus dem Betrieb, weil die Frist als Parameter übergeben wird.
#[tokio::test]
async fn ein_server_der_schweigt_bricht_ab_statt_zu_warten() {
    let (hier, _dort) = tokio::io::duplex(4096);

    // `_dort` bleibt hier liegen und wird nicht geschrieben: Die Leitung ist
    // offen, es kommt nichts. Genau der Zustand, der vorher endlos war.
    let mut verbindung = Verbindung::new(hier);

    let frist = std::time::Duration::from_millis(50);
    let ergebnis = verbindung.lies_roh_zeile_bis(frist).await;

    assert!(
        ergebnis.is_err(),
        "ein schweigender Server muss ein Fehler sein, nicht eine offene Leitung"
    );
    assert!(
        ergebnis.unwrap_err().to_string().contains("Sekunden"),
        "die Meldung nennt die Frist, sonst ist „hängt“ nicht von „langsam“ zu unterscheiden"
    );
    assert!(
        verbindung.beendet,
        "nach einer Frist gilt die Leitung als hinüber – sonst käme der nächste Befehl darauf"
    );
}

/// Fährt einen Befehl gegen eine Leitung, die eine feste Antwort schickt.
///
/// **So lässt sich prüfen, was die Verbindungsschicht versteht, ohne einen
/// Mailserver zu brauchen.** `duplex` ist ein Rohr im Speicher; die andere Seite
/// schickt die Antwort, als käme sie vom Server.
async fn befehl_gegen_antwort(
    antwort: &'static [u8],
) -> Result<Vec<imap_proto::Response<'static>>, String> {
    let (hier, mut dort) = tokio::io::duplex(256 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort.write_all(antwort).await;
        let _ = dort.flush().await;
        // Die Leitung offen halten, bis der Befehl gelesen hat.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut verbindung = Verbindung::new(hier);
    verbindung.befehl("B1 NOOP").await
}

#[tokio::test]
async fn eine_gewoehnliche_antwort_wird_gelesen() {
    let antwort = b"* CAPABILITY IMAP4rev1\r\nB1 OK NOOP done\r\n";

    let antworten = befehl_gegen_antwort(antwort)
        .await
        .expect("das ist eine gewöhnliche Antwort");

    assert_eq!(antworten.len(), 2);
    assert!(matches!(antworten[1], imap_proto::Response::Done { .. }));
}

/// **Ein Literal läuft über mehrere Zeilen.**
///
/// So schickt jeder Server einen `BODY[]`-Abruf zurück:
/// `* 1 FETCH (BODY[] {10}\r\nHallo Kai!)\r\n`. Die Längenangabe steht am
/// Ende der Zeile, der Inhalt kommt **danach** – wer nur bis zum ersten `\n`
/// liest, reicht dem Parser eine Zeile, die mitten im Literal abbricht.
///
/// Betrifft die Vorschau in der Leiste ebenso wie `RFC822`, den Abruf für den
/// Text einer geöffneten Nachricht.
#[tokio::test]
async fn eine_literal_antwort_wird_lesbar() {
    let antwort = b"* 1 FETCH (BODY[] {10}\r\nHallo Kai!)\r\nB1 OK done\r\n";

    let antworten = befehl_gegen_antwort(antwort)
        .await
        .expect("ein Literal ist eine ganz gewöhnliche Antwort");

    let imap_proto::Response::Fetch(nummer, werte) = &antworten[0] else {
        panic!("erwartet: eine FETCH-Antwort, {:?}", antworten[0]);
    };

    assert_eq!(*nummer, 1);

    let Some(imap_proto::AttributeValue::BodySection { data, .. }) = werte.first() else {
        panic!("erwartet: ein BODY[]-Anteil, {werte:?}");
    };

    assert_eq!(data.as_deref(), Some(&b"Hallo Kai!"[..]));
}

/// **Zwei Literale in einer Antwort** – ein Abruf, der Kopf und Text zugleich
/// holt, trägt beide in einer Zeile.
#[tokio::test]
async fn zwei_literale_in_einer_antwort_werden_lesbar() {
    let antwort = b"* 1 FETCH (RFC822.SIZE {7}\r\nHallo K RFC822 {6}\r\nHallo))\r\nB1 OK done\r\n";

    let antworten = befehl_gegen_antwort(antwort)
        .await
        .expect("zwei Literale sind eine ganz gewöhnliche Antwort");

    let imap_proto::Response::Fetch(_, werte) = &antworten[0] else {
        panic!("erwartet: eine FETCH-Antwort");
    };

    assert!(werte.len() >= 2, "erwartet: zwei Anteile, {werte:?}");
}

/// **Ein nicht synchronisierendes Literal `{n+}` ist kein Inhalt.** Es kündigt
/// Daten an, die der Server *irgendwann* schickt – sie stehen nicht im nächsten
/// Befehl. Würde man sie hier lesen, käme der Inhalt in die Auswertung der
/// folgenden Antwort hinein.
#[tokio::test]
async fn ein_nicht_synchronisierendes_literal_bleibt_stehen() {
    assert_eq!(literal_anteil(b"* 1 FETCH (UID 5)\r\n"), None);
    assert_eq!(literal_anteil(b"* 1 FETCH (RFC822.SIZE 4096)\r\n"), None);

    // `{10}` in `* 1 FETCH (BODY[] {10}` endet bei Zeichen 22.
    assert_eq!(
        literal_anteil(b"* 1 FETCH (BODY[] {10}\r\n"),
        Some((22, 10))
    );

    // **Nicht synchronisierend:** `{10+}` kündigt nichts für diese Antwort an.
    assert_eq!(literal_anteil(b"* 1 FETCH (BODY[] {10+}\r\n"), None);

    // Zwei Literale: das erste zählt, das zweite kommt im nächsten Durchlauf.
    assert_eq!(
        literal_anteil(b"* 1 FETCH (A {3}\r\nxxxB {4}\r\nyyyy)\r\n"),
        Some((16, 3))
    );

    // **Keine Panik auf Unsinn.** Eine geschweifte Klammer ohne Ziffern, eine
    // ohne schließende – der Server schickt, was er schickt.
    assert_eq!(literal_anteil(b"* 1 FETCH (BODY[] {}\r\n"), None);
    assert_eq!(literal_anteil(b"* 1 FETCH (BODY[] {10\r\n"), None);
    assert_eq!(literal_anteil(b"* 1 FETCH ({abc}\r\n"), None);
    assert_eq!(literal_anteil(b""), None);
}

// --- Die Anmeldung -----------------------------------------------------------

/// Spielt eine ganze IMAP-Anmeldung gegen eine Leitung, die feste Antworten
/// schickt – wie ein Server, aber ohne Netz.
async fn anmelden_gegen(antworten: &'static [&'static [u8]]) -> Result<(), String> {
    let (hier, mut dort) = tokio::io::duplex(64 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        for antwort in antworten {
            let _ = dort.write_all(antwort).await;
            let _ = dort.flush().await;
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut verbindung = Verbindung::new(hier);
    anmelden(&mut verbindung, "kai@example.org", "app-passwort").await
}

/// **Der übliche Weg:** Der Server verlangt die Zugangsdaten mit `+`, und nimmt
/// sie danach an.
#[tokio::test]
async fn meldet_sich_mit_app_passwort_an() {
    let antworten: &'static [&'static [u8]] =
        &[b"+ \r\n", b"A1 OK [CAPABILITY IMAP4rev1] Logged in\r\n"];

    anmelden_gegen(antworten)
        .await
        .expect("eine richtige Anmeldung ist keine Ausnahme");
}

/// **Abgelehnt wird abgelehnt gemeldet.**
///
/// Einst prüfte `anmelden` nur, ob eine `+`-Zeile *nach* den Zugangsdaten kam,
/// und übersah das Ergebnis davor. Meldete der Server die Anmeldung schon beim
/// Verfahren als abgelehnt, kam `A1 NO` zurück – und das stand nirgends geprüft.
#[tokio::test]
async fn meldet_eine_abgelehnte_anmeldung() {
    let antworten: &'static [&'static [u8]] =
        &[b"A1 NO [AUTHENTICATIONFAILED] Invalid credentials\r\n"];

    let fehler = anmelden_gegen(antworten)
        .await
        .expect_err("eine abgelehnte Anmeldung ist kein Erfolg");

    assert!(
        fehler.contains("abgelehnt"),
        "die Meldung soll den Grund nennen: {fehler}"
    );
}

/// **Ein Server, der `AUTHENTICATE` nicht kennt**, bekommt eine klare Meldung –
/// nicht eine Anmeldung, die nur danach aussieht.
#[tokio::test]
async fn meldet_einen_server_ohne_authenticate() {
    let antworten: &'static [&'static [u8]] = &[b"A1 BAD Unknown command\r\n"];

    let fehler = anmelden_gegen(antworten)
        .await
        .expect_err("BAD ist kein Erfolg");

    assert!(!fehler.contains("unverständlich"), "{fehler}");
}

// --- Die Befehle, die Mimir schickt ------------------------------------------

/// **Jeder `FETCH` braucht eine gültige Menge** – sonst kennt der Server den
/// Befehl nicht.
///
/// Nach RFC 3501 ist ein Element einer Menge entweder eine einzelne Nummer (`5`)
/// oder ein Bereich (`5:9`). `1:5:5` ist beides nicht; Gmail antwortet darauf
/// mit *„Unrecognized command"*, und Mimir zeigte die Antwort des Servers an,
/// statt zu sagen, welcher Befehl ihn entzückt.
#[test]
fn die_befehle_enthalten_keine_ungueltige_menge() {
    for (vorhanden, grenze) in [(10usize, 5usize), (3, 5), (1, 1), (50, 50), (100, 1)] {
        let menge = nachrichten_menge(Some(vorhanden), grenze);
        let mut teile = menge.split(':');

        let von: usize = teile
            .next()
            .unwrap_or_default()
            .parse()
            .unwrap_or_else(|_| panic!("keine Zahl am Anfang von {menge:?}"));
        let bis: usize = teile
            .next()
            .unwrap_or_default()
            .parse()
            .unwrap_or_else(|_| panic!("keine Zahl am Ende von {menge:?}"));

        assert_eq!(
            teile.next(),
            None,
            "{menge:?} hat mehr als zwei Teile – das ist keine Menge"
        );
        assert!(von >= 1, "eine Nachrichtennummer beginnt bei 1: {menge:?}");
        assert!(von <= bis, "die Menge läuft rückwärts: {menge:?}");
        assert!(bis <= vorhanden, "{menge:?} geht über den Ordner hinaus");
    }
}

/// **Und es kommen die neuesten.** Der Ordner nummeriert von 1; ein Postfach mit
/// 1000 Nachrichten und einer Leiste für 10 soll die 991 bis 1000 zeigen, nicht
/// die 1 bis 10.
#[test]
fn die_menge_nimmt_die_neuesten() {
    assert_eq!(nachrichten_menge(Some(1000), 10), "991:1000");
    assert_eq!(nachrichten_menge(Some(7), 10), "1:7");
    assert_eq!(nachrichten_menge(Some(1), 1), "1:1");
}

/// **Ein leerer Ordner ergibt keinen Befehl.** `FETCH 1:0` wäre ein Befehl, den
/// kein Server annimmt – und der Ordner ist kein Fehlerfall, sondern ein
/// Postfach ohne Nachrichten.
#[test]
fn ein_leerer_ordner_ergibt_keinen_befehl() {
    assert_eq!(nachrichten_menge(Some(0), 10), "");
    assert_eq!(nachrichten_menge(Some(5), 0), "");
}

/// **Ohne `EXISTS`-Antwort** bleibt nur das, was immer geht. `1:*` ist eine
/// gültige Menge und bedeutet „alle"; hinten wird ohnehin abgeschnitten.
#[test]
fn ohne_antwort_bleibt_es_bei_der_sicheren_menge() {
    assert_eq!(nachrichten_menge(None, 10), "1:*");
}

/// **Die Antwort, die Gmail auf ein `SELECT` schickt**, gelesen mit der
/// Verbindungslogik von Mimir – samt Literal, weil Gmail eine `LIST`-Antwort
/// gern als Literal schickt.
#[tokio::test]
async fn die_antwort_auf_select_ist_lesbar() {
    let antwort = concat!(
        "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n",
        "* OK [PERMANENTFLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft \\*)] Limited\r\n",
        "* 231 EXISTS\r\n",
        "* 0 RECENT\r\n",
        "* OK [UIDVALIDITY 1466002015] UIDs valid\r\n",
        "* OK [UIDNEXT 3952] Predicted next UID\r\n",
        "B1 OK [READ-WRITE] SELECT completed\r\n",
    );

    let antworten = befehl_gegen_antwort(antwort.as_bytes())
        .await
        .expect("eine SELECT-Antwort ist gewöhnlich");

    let vorhanden = antworten.iter().find_map(|antwort| match antwort {
        imap_proto::Response::MailboxData(imap_proto::MailboxDatum::Exists(anzahl)) => {
            Some(*anzahl)
        }
        _ => None,
    });

    assert_eq!(vorhanden, Some(231));
}

/// **Und die FETCH-Antwort dazu**, mit den Feldern, die die Leiste braucht.
///
/// So schickt Gmail sie – und an zwei Stellen steht mehr drin, als man auf den
/// ersten Blick erwartet:
///
///  - **Die Adresslisten haben doppelte Klammern.** `env-from = "(" 1*address ")"`
///    und `address = "(" … ")"` – eine Liste mit einer Adresse ist also
///    `((…))`, nicht `(…)`. Das ist kein Tippfehler des Servers, sondern die
///    Grammatik aus RFC 3501 §7.4.2.
///  - **Das Betreff steht an zweiter Stelle**, nicht am Ende: Die Reihenfolge ist
///    Datum, Betreff, Von, Absender, Antwort-An, An, Cc, Bcc, Bezug, Kennung.
#[tokio::test]
async fn die_antwort_auf_fetch_ist_lesbar() {
    let antwort = concat!(
        "* 231 FETCH (UID 3951 FLAGS (\\Seen) ",
        "RFC822.SIZE 12345 ",
        "ENVELOPE (\"Mon, 12 Oct 2026 09:14:00 +0200\" ",
        "\"Re: Termin am Donnerstag\" ",
        "((\"Sarah\" NIL \"sarah\" \"example.org\")) ",
        "((\"kai\" NIL \"kai\" \"privat.invalid\")) ",
        "((\"kai\" NIL \"kai\" \"privat.invalid\")) ",
        "((\"kai\" NIL \"kai\" \"privat.invalid\")) NIL NIL NIL ",
        "\"<abc@example.org>\"))\r\n",
        "B2 OK FETCH completed\r\n",
    );

    let antworten = befehl_gegen_antwort(antwort.as_bytes())
        .await
        .expect("eine FETCH-Antwort ist gewöhnlich");

    let imap_proto::Response::Fetch(231, werte) = &antworten[0] else {
        panic!("erwartet: FETCH 231, {:?}", antworten[0]);
    };

    assert!(werte
        .iter()
        .any(|w| matches!(w, imap_proto::AttributeValue::Uid(3951))));
    assert!(werte
        .iter()
        .any(|w| matches!(w, imap_proto::AttributeValue::Rfc822Size(12345))));
    assert!(werte
        .iter()
        .any(|w| matches!(w, imap_proto::AttributeValue::Envelope(_))));
}

/// **Und die Vorschau-Antwort** – der Fall, für den es die Literal-Behandlung
/// überhaupt gibt.
#[tokio::test]
async fn die_antwort_auf_die_vorschau_ist_lesbar() {
    let antwort = concat!(
        "* 231 FETCH (BODY[] {10}\r\n",
        "Hallo Kai!)\r\n",
        "B3 OK FETCH completed\r\n",
    );

    let antworten = befehl_gegen_antwort(antwort.as_bytes())
        .await
        .expect("ein Literal ist eine gewöhnliche Antwort");

    let imap_proto::Response::Fetch(_, werte) = &antworten[0] else {
        panic!("erwartet: eine FETCH-Antwort");
    };

    let Some(imap_proto::AttributeValue::BodySection { data, .. }) = werte.first() else {
        panic!("erwartet: einen BODY[]-Anteil, {werte:?}");
    };

    assert_eq!(data.as_deref(), Some(&b"Hallo Kai!"[..]));
}
// --- Die ganze Befehlsfolge gegen einen ---------------------------------

/// Spielt einen Server, der `FETCH` **nur in einem geöffneten Ordner** beantwortet.
///
/// **So verhält sich Gmail, und so verhält sich jeder IMAP-Server nach RFC
/// 3501.** Ein `FETCH` ohne vorheriges `SELECT` wird mit `BAD` abgelehnt. Vor
/// dem Ausbau von `hole_text` stand dort genau das: Die Verbindung war
/// angemeldet, aber nichts ausgewählt – und die Meldung sprach von einem
/// Befehl, den der Server nicht verstanden habe, während der Befehl völlig
/// richtig war. Es fehlte der Zustand davor, und genau den sieht kein Test von
/// der Befehlsfolge her, der beides am Ende prüft.
async fn gegen_server_mit_ordnerpflicht() -> Result<Vec<Vec<u8>>, String> {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gesendet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    tokio::spawn({
        let notiert = gesendet.clone();
        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (lesen, mut schreiben) = tokio::io::split(dort);
            let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));

            let mut puffer = vec![0u8; 8192];
            let mut geoeffnet = false;

            for _ in 0..4 {
                let mut befehl = Vec::new();
                while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                    if anzahl == 0 {
                        break;
                    }
                    befehl.extend_from_slice(&puffer[..anzahl]);
                    if befehl.ends_with(b"\r\n") {
                        break;
                    }
                }

                if befehl.is_empty() {
                    break;
                }

                notiert.lock().unwrap().push(befehl.clone());
                let text = String::from_utf8_lossy(&befehl).to_string();
                let tag = text.split_whitespace().next().unwrap_or("B1").to_string();

                let antwort = if text.contains("SELECT") {
                    geoeffnet = true;
                    format!("* 1 EXISTS\r\n{tag} OK [READ-WRITE] SELECT done\r\n").into_bytes()
                } else if text.contains("STORE") {
                    // `hole_text` markiert im Anschluss an seinen Abruf.
                    format!("* 1 FETCH (FLAGS (\\Seen))\r\n{tag} OK STORE completed\r\n").into_bytes()
                } else if text.contains("FETCH") && !geoeffnet {
                    format!("{tag} BAD Please select a mailbox first\r\n").into_bytes()
                } else if text.contains("FETCH") {
                    let roh = b"Subject: Hallo\r\nContent-Type: text/plain\r\n\r\nHallo!\r\n";
                    // **Auf `BODY.PEEK[]` antwortet der Server mit `BODY[]`** –
                    // das PEEK steht im Befehl, nicht in der Antwort. Die Flags
                    // kommen mit, weil Mimir sie aus derselben Antwort liest.
                    let mut v =
                        format!("* 1 FETCH (FLAGS (\\Seen) BODY[] {{{}}}\r\n", roh.len()).into_bytes();
                    v.extend_from_slice(roh);
                    v.extend_from_slice(format!(")\r\n{tag} OK FETCH done\r\n").as_bytes());
                    v
                } else {
                    format!("{tag} OK done\r\n").into_bytes()
                };

                let _ = schreiben.write_all(&antwort).await;
                let _ = schreiben.flush().await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    });

    let mut verbindung = Verbindung::new(hier);
    let ergebnis = hole_text(&mut verbindung, "INBOX", 1).await;

    if ergebnis.is_ok() {
        return Ok(gesendet.lock().unwrap().clone());
    }

    Err(ergebnis.unwrap_err())
}

// --- Gelesen- und Ungelesenmarkieren --------------------------------------

/// Spielt einen Server, der Befehle protokolliert und jede Auswahl annimmt.
async fn gegen_server_mit_protokoll() -> Vec<String> {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gesendet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    tokio::spawn({
        let notiert = gesendet.clone();
        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (lesen, mut schreiben) = tokio::io::split(dort);
            let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));

            let mut puffer = vec![0u8; 8192];

            for _ in 0..6 {
                let mut befehl = Vec::new();
                while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                    if anzahl == 0 {
                        break;
                    }
                    befehl.extend_from_slice(&puffer[..anzahl]);
                    if befehl.ends_with(b"\r\n") {
                        break;
                    }
                }

                if befehl.is_empty() {
                    break;
                }

                notiert.lock().unwrap().push(befehl.clone());
                let text = String::from_utf8_lossy(&befehl).to_string();

                // Die Nummer steht im Befehl – „B4 STORE 7 …" antwortet „B4 OK".
                let tag = text.split_whitespace().next().unwrap_or("B1").to_string();
                let antwort = format!("{tag} OK done\r\n");

                let _ = schreiben.write_all(antwort.as_bytes()).await;
                let _ = schreiben.flush().await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    });

    let mut verbindung = Verbindung::new(hier);

    // Beide Richtungen; ein Fehler hier würde die Prüfung darunter sinnlos
    // machen, also wird er auf Panik gesetzt statt ignoriert.
    setze_gelesen(&mut verbindung, "INBOX", 7, true)
        .await
        .expect("als gelesen markieren");
    setze_gelesen(&mut verbindung, "INBOX", 7, false)
        .await
        .expect("als ungelesen markieren");

    let texte: Vec<String> = gesendet
        .lock()
        .unwrap()
        .iter()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .collect();

    texte
}

#[tokio::test]
async fn markiert_eine_nachricht_als_gelesen_und_wieder_zurueck() {
    let gesendet = gegen_server_mit_protokoll().await;

    let stores: Vec<&String> = gesendet.iter().filter(|t| t.contains("STORE")).collect();

    assert_eq!(stores.len(), 2, "genau zweimal markiert: {stores:?}");

    // **`+FLAGS` setzt, `-FLAGS` nimmt weg.** Genau diese Unterscheidung ist der
    // ganze Unterschied zwischen „gelesen" und „ungelesen"; vertauscht wären
    // beide Klicks wirkungslos und niemand bemerkte es, weil die Anzeige
    // trotzdem umspringt.
    assert!(
        stores[0].contains("+FLAGS") && stores[0].contains("\\Seen"),
        "gelesen markieren mit +FLAGS: {}",
        stores[0]
    );
    assert!(
        stores[1].contains("-FLAGS") && stores[1].contains("\\Seen"),
        "ungelesen markieren mit -FLAGS: {}",
        stores[1]
    );

    // **Die Nummer der Nachricht wandert mit** – sonst würde immer dieselbe
    // Nachricht umspringen.
    for befehl in &stores {
        assert!(
            befehl.contains(" 7 "),
            "die Nummer steht im Befehl: {befehl}"
        );
    }

    // **Und auch hier gilt: erst der Ordner, dann der Befehl.** `STORE` wirkt
    // nur im ausgewählten Ordner.
    let select = gesendet.iter().position(|t| t.contains("SELECT"));
    let store = gesendet.iter().position(|t| t.contains("STORE"));
    assert!(
        select.is_some() && store.is_some(),
        "beide Schritte: {gesendet:?}"
    );
    assert!(
        select < store,
        "erst auswählen, dann markieren: {gesendet:?}"
    );
}

#[tokio::test]
async fn der_text_wird_erst_nach_der_ordnerauswahl_geholt() {
    let gesendet = gegen_server_mit_ordnerpflicht()
        .await
        .expect("der Abruf gelingt, weil der Ordner vorher ausgewählt wurde");

    let texte: Vec<String> = gesendet
        .iter()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .collect();

    let select = texte.iter().position(|t| t.contains("SELECT"));
    let fetch = texte.iter().position(|t| t.contains("FETCH"));

    assert!(
        select.is_some(),
        "der Ordner wurde nie ausgewählt: {texte:?}"
    );
    assert!(fetch.is_some(), "die Nachricht wurde nie geholt: {texte:?}");
    assert!(
        select < fetch,
        "erst auswählen, dann holen – in dieser Reihenfolge: {texte:?}"
    );
}

/// Spielt einen Server, der auf jeden Befehl eine Antwort schickt, und zeichnet
/// auf, **was Mimir gesendet hat**.
///
/// **Das ist die Gegenprobe zu den letzten beiden Raten.** Beide Male lag die
/// Antwort in der Leiste, aber nicht, welcher Befehl sie ausgelöst hatte. Hier
/// steht beides nebeneinander: der Befehl und die Antwort, die darauf kam.
async fn gegen_server(antworten: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, String> {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gesendet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let notiert = gesendet.clone();

    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Alles lesen, was der Client schreibt, und mitschreiben.
        let (lesen, mut schreiben) = tokio::io::split(dort);
        let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));

        let mut puffer = vec![0u8; 8192];

        for antwort in antworten {
            // Auf den nächsten Befehl warten.
            let mut befehl = Vec::new();
            while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                if anzahl == 0 {
                    break;
                }
                befehl.extend_from_slice(&puffer[..anzahl]);
                if befehl.ends_with(b"\r\n") {
                    break;
                }
            }

            if befehl.is_empty() {
                break;
            }

            notiert.lock().unwrap().push(befehl);
            let _ = schreiben.write_all(&antwort).await;
            let _ = schreiben.flush().await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut verbindung = Verbindung::new(hier);
    let ergebnis = hole_nachrichten(&mut verbindung, "INBOX", 3).await;

    // Die Aufzeichnung ist nur sinnvoll, wenn die Verbindung noch steht.
    if ergebnis.is_ok() {
        return Ok(gesendet.lock().unwrap().clone());
    }

    Err(ergebnis.unwrap_err())
}

#[tokio::test]
async fn die_befehlsfolge_wird_akzeptiert() {
    let antworten: Vec<Vec<u8>> = vec![
        // SELECT: der Ordner hat fünf Nachrichten.
        b"* 5 EXISTS\r\n* 0 RECENT\r\nB1 OK [READ-WRITE] SELECT completed\r\n".to_vec(),
        // FETCH der Köpfe: Nummer 5 (die neueste der drei).
        b"* 5 FETCH (UID 500 FLAGS (\\Seen) RFC822.SIZE 120 \
          ENVELOPE (\"Mon, 12 Oct 2026 09:14:00 +0200\" \"Termin\" \
          ((\"Sarah\" NIL \"sarah\" \"example.org\")) NIL NIL NIL NIL NIL NIL NIL))\r\n\
          B2 OK FETCH completed\r\n"
            .to_vec(),
        // FETCH der Vorschau.
        b"* 5 FETCH (BODY[] {11}\r\nHallo Kai!)\r\nB3 OK FETCH completed\r\n".to_vec(),
    ];

    let gesendet = match gegen_server(antworten).await {
        Ok(gesendet) => gesendet,
        Err(fehler) => panic!("der Server hätte antworten müssen, kam aber: {fehler}"),
    };

    let texte: Vec<String> = gesendet
        .iter()
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .collect();

    assert!(texte[0].starts_with("B1 SELECT"), "{}", texte[0]);
    assert!(texte[1].contains("B2 FETCH 3:5"), "{}", texte[1]);
    assert!(texte[2].contains("B3 FETCH 3:5"), "{}", texte[2]);

    // **Und jede geschweifte Klammer ist zu.** Genau daran scheitert es sonst.
    for zeile in &texte {
        let geschweift = zeile.matches('{').count();
        assert_eq!(
            geschweift,
            zeile.matches('}').count(),
            "ungeschlossene Literale in: {zeile}"
        );
    }
} // --- Die ganze Unterhaltung, vom Begrüßungstext bis zur Vorschau -------------

/// Spielt eine vollständige IMAP-Unterhaltung und zeichnet auf, was Mimir
/// gesendet hat.
///
/// **Das schließt die Lücke, an der das Raten scheiterte:** Bis eben lief nur
/// `hole_nachrichten` gegen die Attrappe – die Anmeldung, um die es zuerst geht,
/// war nicht darin.
async fn unterhaltung(
    begruessung: &'static [u8],
    antworten: Vec<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, String> {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gesendet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let notiert = gesendet.clone();

    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (lesen, mut schreiben) = tokio::io::split(dort);
        let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));
        let mut puffer = vec![0u8; 8192];

        // **Das Begrüßungszeichen kommt ohne Befehl** – ein Server wartet nicht
        // darauf, dass der Client zuerst etwas sagt.
        let _ = schreiben.write_all(begruessung).await;
        let _ = schreiben.flush().await;

        for antwort in antworten {
            let mut befehl = Vec::new();
            while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                if anzahl == 0 {
                    break;
                }
                befehl.extend_from_slice(&puffer[..anzahl]);
                if befehl.ends_with(b"\r\n") {
                    break;
                }
            }

            if befehl.is_empty() {
                break;
            }

            notiert.lock().unwrap().push(befehl);
            let _ = schreiben.write_all(&antwort).await;
            let _ = schreiben.flush().await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut verbindung = Verbindung::new(hier);

    verbindung.lies_zeile().await.map_err(|f| f.to_string())?;
    anmelden(&mut verbindung, "kai@example.org", "app-passwort").await?;
    hole_nachrichten(&mut verbindung, "INBOX", 3).await?;

    let aufzeichnung = gesendet.lock().unwrap().clone();
    Ok(aufzeichnung)
}

#[tokio::test]
async fn die_vollstaendige_unterhaltung_wird_verstanden() {
    let antworten: Vec<Vec<u8>> = vec![
        b"+\r\n".to_vec(),
        b"A1 OK [CAPABILITY IMAP4rev1] Logged in\r\n".to_vec(),
        b"* 5 EXISTS\r\n* 0 RECENT\r\nB1 OK [READ-WRITE] SELECT completed\r\n".to_vec(),
        b"* 5 FETCH (UID 500 FLAGS (\\Seen) RFC822.SIZE 120 \
          ENVELOPE (\"Mon, 12 Oct 2026 09:14:00 +0200\" \"Termin\" \
          ((\"Sarah\" NIL \"sarah\" \"example.org\")) NIL NIL NIL NIL NIL NIL NIL))\r\n\
          B2 OK FETCH completed\r\n"
            .to_vec(),
        b"* 5 FETCH (BODY[] {11}\r\nHallo Kai!)\r\nB3 OK FETCH completed\r\n".to_vec(),
    ];

    let gesendet = unterhaltung(b"* OK [CAPABILITY IMAP4rev1] Gimap ready\r\n", antworten)
        .await
        .expect("eine gewöhnliche Unterhaltung wird verstanden");

    let texte: Vec<String> = gesendet
        .iter()
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .collect();

    assert_eq!(texte[0], "A1 AUTHENTICATE PLAIN");
    // **Die Zugangsdaten kommen als eigene Zeile und nur als Base64.** Hier
    // steht keine im Klartext – und genau das ist der Grund, warum dieses
    // Verfahren statt `LOGIN` benutzt wird.
    assert!(
        !texte[1].contains("app-passwort"),
        "das App-Passwort geht im Klartext raus: {}",
        texte[1]
    );
    assert!(texte[1].starts_with("A"), "{}", texte[1]);
    assert!(texte[2].starts_with("B1 SELECT"), "{}", texte[2]);
    assert!(texte[3].contains("B2 FETCH 3:5"), "{}", texte[3]);
    assert!(texte[4].contains("B3 FETCH 3:5"), "{}", texte[4]);
}

/// **Was Gmail antwortet, wenn ihm ein Befehl unbekannt ist.** Die Zeile ist
/// keine gültige IMAP-Antwort – Mimir kann sie nicht als solche lesen und muss
/// sagen, **an welchem Befehl** es hing.
///
/// Ohne den Befehl im Text bleibt nur der Servertext, und genau daran ist dieser
/// Fehler zweimal gescheitert: Gmail nennt nicht, welchen Befehl es nicht
/// verstanden hat.
#[tokio::test]
async fn die_meldung_nennt_den_befehl() {
    let (hier, mut dort) = tokio::io::duplex(256 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort
            .write_all(b"* 502 5.5.1 Unrecognized command. https://support.google.com/a/x\r\n")
            .await;
        let _ = dort.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut verbindung = Verbindung::new(hier);
    let fehler = verbindung
        .befehl("B2 FETCH 1:5 (BODY.PEEK[]<0.2048>)")
        .await
        .expect_err("eine kaputte Zeile ist keine Antwort");

    assert!(
        fehler.contains("B2 FETCH 1:5"),
        "die Meldung nennt den Befehl: {fehler}"
    );
    assert!(fehler.contains("Unrecognized"), "{fehler}");
}

/// **Und ein abgelehnter Befehl wird auch als solcher gemeldet** – mit dem
/// Befehl dahinter, damit nicht der ganze Weg von Hand nachgestellt werden muss.
#[tokio::test]
async fn ein_abgelehnter_befehl_nennt_sich_selbst() {
    let (hier, mut dort) = tokio::io::duplex(256 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort
            .write_all(b"B2 NO [CANNOT] something is wrong\r\n")
            .await;
        let _ = dort.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });

    let mut verbindung = Verbindung::new(hier);
    let fehler = verbindung
        .befehl("B1 SELECT \"Postfach\"")
        .await
        .expect_err("NO ist kein Erfolg");

    assert!(fehler.contains("B1 SELECT"), "{fehler}");
}
// --- Wenn vor dem Ergebnis noch etwas kommt ---------------------------------

/// **Vor dem `A1 OK` dürfen ungetaggte Zeilen kommen** – und dann ist die
/// Meldung von Gmail eine Antwort, die man lesen kann.
///
/// Das ist der Fall, der zweimal die Fehlersuche gekostet hat: Vor dem Ergebnis
/// der Anmeldung schickte Gmail eine unlesbare Zeile, Mimir meldete **sie** als
/// Fehlschlag – und die eigentliche Antwort, `A1 NO [AUTHENTICATIONFAILED]`,
/// blieb ungegelesen im Strom. Wer nach den Zugangsdaten nur **eine** Zeile
/// liest, bekommt nie die, auf die es ankommt.
#[tokio::test]
async fn eine_untagged_zeile_vor_dem_ergebnis_stoert_nicht() {
    let (hier, mut dort) = tokio::io::duplex(64 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort
            .write_all(
                b"+\r\n\
                  * 502 5.5.1 Unrecognized command. https://support.google.com/a/x\r\n\
                  A1 OK [CAPABILITY IMAP4rev1] Logged in\r\n",
            )
            .await;
        let _ = dort.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    });

    let mut verbindung = Verbindung::new(hier);

    anmelden(&mut verbindung, "kai@example.org", "app-passwort")
        .await
        .expect("eine ungetaggte Zeile vor dem Ergebnis ist kein Fehlschlag");
}

/// **Und eine unlesbare Zeile als *einzige* Antwort** wird gemeldet – und dann
/// nennt die Meldung den Befehl.
#[tokio::test]
async fn eine_unlesbare_allein_antwort_nennt_den_befehl() {
    let (hier, mut dort) = tokio::io::duplex(64 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort
            .write_all(b"* 502 5.5.1 Unrecognized command. https://support.google.com/a/x\r\n")
            .await;
        let _ = dort.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    });

    let mut verbindung = Verbindung::new(hier);

    let fehler = anmelden(&mut verbindung, "kai@example.org", "app-passwort")
        .await
        .expect_err("ohne Ergebnis ist die Anmeldung nicht gelungen");

    assert!(
        fehler.contains("A1 AUTHENTICATE PLAIN"),
        "die Meldung nennt den Befehl: {fehler}"
    );
    assert!(fehler.contains("Unrecognized"), "{fehler}");
}

/// **Wird die Anmeldung wirklich abgelehnt, steht das in der Meldung** – nicht
/// irgendeine Zeile, die gerade günstig im Strom lag.
#[tokio::test]
async fn eine_abgelehnte_anmeldung_sagt_das_auch() {
    let (hier, mut dort) = tokio::io::duplex(64 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort
            .write_all(
                b"+\r\n\
                  * 502 5.5.1 Unrecognized command. https://support.google.com/a/x\r\n\
                  A1 NO [AUTHENTICATIONFAILED] Invalid credentials\r\n",
            )
            .await;
        let _ = dort.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    });

    let mut verbindung = Verbindung::new(hier);

    let fehler = anmelden(&mut verbindung, "kai@example.org", "falsch")
        .await
        .expect_err("eine abgelehnte Anmeldung ist kein Erfolg");

    assert!(
        fehler.contains("abgelehnt"),
        "die Meldung nennt den Grund: {fehler}"
    );
    assert!(
        !fehler.contains("unverständlich"),
        "die Meldung zeigt die Serverzeile statt des Ergebnisses: {fehler}"
    );
}

/// **Und der Server, der gar nicht erst nach Zugangsdaten fragt**, meldet das
/// über dieselbe Zeile.
#[tokio::test]
async fn ein_server_ohne_authenticate_plain_sagt_es() {
    let (hier, mut dort) = tokio::io::duplex(64 * 1024);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let _ = dort.write_all(b"A1 BAD Unknown command\r\n").await;
        let _ = dort.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    });

    let mut verbindung = Verbindung::new(hier);

    let fehler = anmelden(&mut verbindung, "kai@example.org", "app-passwort")
        .await
        .expect_err("BAD ist kein Erfolg");

    assert!(fehler.contains("A1 AUTHENTICATE PLAIN"), "{fehler}");
}

// --- Der Zustand kommt auch wieder zurück -------------------------------

/// Spielt einen Server, der Flags **wirklich festhält**.
///
/// **Der entscheidende Unterschied zu den Attrappen oben:** Die anderen Tests
/// prüfen, *ob* ein Befehl abging. Dieser prüft, was danach *im Ordner steht* –
/// denn ein `STORE`, das ankommt und nichts bewirkt, ist von einem, das nichts
/// ankommt, im Protokoll nicht zu unterscheiden.
async fn gegen_server_mit_gedaechtnis(
    dort: tokio::io::DuplexStream,
) -> std::sync::Arc<std::sync::Mutex<Vec<bool>>> {
    // Nummer -> Flag. Eine Nachricht, ungelesen.
    let gedaechtnis = std::sync::Arc::new(std::sync::Mutex::new(vec![false]));

    tokio::spawn({
        let stand = gedaechtnis.clone();

        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (lesen, mut schreiben) = tokio::io::split(dort);
            let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));

            let mut puffer = vec![0u8; 8192];
            let mut erwarten_zugangsdaten = false;

            let _ = schreiben
                .write_all(b"* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] bereit\r\n")
                .await;
            let _ = schreiben.flush().await;

            for _ in 0..12 {
                let mut befehl = Vec::new();
                while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                    if anzahl == 0 {
                        break;
                    }
                    befehl.extend_from_slice(&puffer[..anzahl]);
                    if befehl.ends_with(b"\r\n") {
                        break;
                    }
                }

                if befehl.is_empty() {
                    break;
                }

                let text = String::from_utf8_lossy(&befehl).to_string();
                let trim = text.trim();

                // Die Zugangsdaten kommen ohne Tag.
                if erwarten_zugangsdaten {
                    erwarten_zugangsdaten = false;
                    let _ = schreiben.write_all(b"A1 OK angemeldet\r\n").await;
                    let _ = schreiben.flush().await;
                    continue;
                }

                if trim.starts_with("A1 AUTHENTICATE") {
                    erwarten_zugangsdaten = true;
                    let _ = schreiben.write_all(b"+ \r\n").await;
                    let _ = schreiben.flush().await;
                    continue;
                }

                let tag = trim.split_whitespace().next().unwrap_or("B1").to_string();

                let antwort = if trim.contains("SELECT") {
                    format!("* FLAGS (\\Seen)\r\n* 1 EXISTS\r\n* OK [UIDVALIDITY 1] ok\r\n{tag} OK [READ-WRITE] SELECT completed\r\n")
                } else if trim.contains("STORE") {
                    // **Die Zahl aus dem Befehl nehmen** – das ist die Nummer,
                    // um die es geht, nicht eine geratene.
                    let wo = trim.find("STORE").unwrap() + "STORE".len();
                    let rest = &trim[wo..];
                    let nummer: usize = rest
                        .split_whitespace()
                        .next()
                        .unwrap_or("1")
                        .parse()
                        .unwrap_or(1);
                    {
                        let mut w = stand.lock().unwrap();
                        while w.len() < nummer {
                            w.push(false);
                        }
                        w[nummer - 1] = rest.contains("+FLAGS");
                    }
                    let flag = if rest.contains("+FLAGS") { "\\Seen" } else { "" };
                    format!("* {nummer} FETCH (FLAGS ({flag}))\r\n{tag} OK STORE completed\r\n")
                } else if trim.contains("RFC822.SIZE") {
                    let flag = if stand.lock().unwrap()[0] { "\\Seen" } else { "" };
                    format!(
                        "* 1 FETCH (UID 100 FLAGS ({flag}) RFC822.SIZE 64 ENVELOPE (\"Mon, 12 Oct 2026 09:14:00 +0200\" \"Termin\" ((\"Sarah\" NIL \"sarah\" \"example.org\")) ((\"kai\" NIL \"kai\" \"privat.invalid\")) ((\"kai\" NIL \"kai\" \"privat.invalid\")) ((\"kai\" NIL \"kai\" \"privat.invalid\")) NIL NIL NIL \"<a@example.org>\"))\r\n{tag} OK FETCH completed\r\n"
                    )
                } else if trim.contains("BODY.PEEK") {
                    let roh = "Hallo Kai!\r\n";
                    format!(
                        "* 1 FETCH (BODY[] {{{}}}\r\n{})\r\n{tag} OK FETCH completed\r\n",
                        roh.len(),
                        roh
                    )
                } else {
                    format!("{tag} OK done\r\n")
                };

                let _ = schreiben.write_all(antwort.as_bytes()).await;
                let _ = schreiben.flush().await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    });

    gedaechtnis
}

/// **Die ganze Kette, einmal durch:** Liste holen, markieren, Liste holen.
///
/// **Bis hierher prüfte jeder Test nur den Befehl, nicht den Zustand.** Ein
/// `+FLAGS`, das der Server annimmt und nicht ausführt, sieht in der
/// Befehlsprotokollierung genau wie ein `+FLAGS`, das wirkt – und die Leiste
/// zeigte weiterhin „ungelesen", ohne dass irgendwo ein Fehler aufgetaucht wäre.
#[tokio::test]
async fn ein_markiertes_flag_kommt_beim_naechsten_holen_wieder_zurueck() {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gedaechtnis = gegen_server_mit_gedaechtnis(dort).await;

    let mut verbindung = Verbindung::new(hier);

    // **Die erste Runde: ungelesen.**
    let vorher = hole_nachrichten(&mut verbindung, "INBOX", 10)
        .await
        .expect("die Liste holen");
    assert_eq!(vorher.len(), 1);
    assert!(
        !vorher[0].gelesen,
        "eine frische Nachricht ist ungelesen: {:?}",
        vorher[0].gelesen
    );

    // **Markieren – wie der Knopf es tut.**
    setze_gelesen(&mut verbindung, "INBOX", 1, true)
        .await
        .expect("als gelesen markieren");

    // **Und die zweite Runde: gelesen.**
    let nachher = hole_nachrichten(&mut verbindung, "INBOX", 10)
        .await
        .expect("die Liste noch einmal holen");

    assert!(
        nachher[0].gelesen,
        "nach dem Markieren muss die Nachricht als gelesen wiederkommen: {:?}",
        nachher[0].gelesen
    );

    assert!(
        gedaechtnis.lock().unwrap()[0],
        "und der Server muss sie sich gemerkt haben"
    );
}

// --- Das Öffnen markiert, und zwar ausdrücklich ---------------------------

/// Spielt einen Server, der auf `BODY.PEEK[]` eine Nachricht mit gegebenen Flags
/// antwortet – und ein `STORE` je nach `store_ok` annimmt oder ablehnt.
async fn server_mit_antwort(
    flags: &str,
    store_ok: bool,
) -> Result<(Vec<String>, NachrichtVoll), String> {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gesendet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let flags = flags.to_string();

    tokio::spawn({
        let notiert = gesendet.clone();

        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (lesen, mut schreiben) = tokio::io::split(dort);
            let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));

            let mut puffer = vec![0u8; 8192];
            let mut geoeffnet = false;

            for _ in 0..5 {
                let mut befehl = Vec::new();
                while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                    if anzahl == 0 {
                        break;
                    }
                    befehl.extend_from_slice(&puffer[..anzahl]);
                    if befehl.ends_with(b"\r\n") {
                        break;
                    }
                }

                if befehl.is_empty() {
                    break;
                }

                notiert.lock().unwrap().push(befehl.clone());
                let text = String::from_utf8_lossy(&befehl).to_string();
                let tag = text.split_whitespace().next().unwrap_or("B1").to_string();

                let antwort = if text.contains("SELECT") {
                    geoeffnet = true;
                    format!("* 1 EXISTS\r\n{tag} OK [READ-WRITE] SELECT done\r\n")
                } else if text.contains("STORE") {
                    if store_ok {
                        format!("* 1 FETCH (FLAGS (\\Seen))\r\n{tag} OK STORE completed\r\n")
                    } else {
                        format!("{tag} NO [CANNOT] der Ordner laesst sich nicht beschreiben\r\n")
                    }
                } else if text.contains("FETCH") && !geoeffnet {
                    format!("{tag} BAD Please select a mailbox first\r\n")
                } else {
                    let roh = "Subject: Hallo\r\nContent-Type: text/plain\r\n\r\nHallo!\r\n";
                    format!(
                        "* 1 FETCH (FLAGS ({flags}) BODY[] {{{}}}\r\n{})\r\n{tag} OK FETCH done\r\n",
                        roh.len(),
                        roh
                    )
                };

                let _ = schreiben.write_all(antwort.as_bytes()).await;
                let _ = schreiben.flush().await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    });

    let mut verbindung = Verbindung::new(hier);
    let voll = hole_text(&mut verbindung, "INBOX", 1).await?;

    let texte = gesendet
        .lock()
        .unwrap()
        .iter()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .collect();

    Ok((texte, voll))
}

/// **Der Rumpf wird ohne Nebenwirkung geholt.**
///
/// Nach RFC 3501 setzt ein Abruf von `BODY` oder `RFC822` das Gelesen-Flag als
/// Nebenwirkung, und der Server entscheidet, ob die Flags in seiner Antwort
/// das schon Getane tragen. Darauf sich zu verlassen hieße, den Zustand zu
/// raten – also wird ohne Nebenwirkung gelesen und das Flag ausdrücklich
/// gesetzt. Das ist der Grund, warum hier `PEEK` steht und nicht `BODY`.
#[tokio::test]
async fn der_rumpf_wird_ohne_nebenwirkung_geholt() {
    let (texte, _) = server_mit_antwort("", true)
        .await
        .expect("die Nachricht holen");

    let fetch = texte
        .iter()
        .find(|t| t.contains("FETCH"))
        .expect("überhaupt ein FETCH");

    assert!(
        fetch.contains("BODY.PEEK[]"),
        "der Abruf muss PEEK sein: {fetch}"
    );
    assert!(
        !fetch.contains("RFC822"),
        "ein Abruf von RFC822 setzt das Gelesen-Flag: {fetch}"
    );
}

/// **Und Öffnen markiert trotzdem – mit einem eigenen `STORE`.**
///
/// Das ist der Weg, den der Benutzer erwartet: Wer eine Mail aufklappt, hat sie
/// gelesen. Bis hierher markierte nur der Knopf, und die Leiste behielt beim
/// bloss Aufklappen ihren alten Stand.
#[tokio::test]
async fn das_oeffnen_markiert_als_gelesen() {
    let (texte, voll) = server_mit_antwort("", true)
        .await
        .expect("die Nachricht holen");

    let store = texte
        .iter()
        .find(|t| t.contains("STORE"))
        .unwrap_or_else(|| panic!("überhaupt kein STORE: {texte:?}"));

    assert!(
        store.contains("+FLAGS") && store.contains("\\Seen"),
        "gelesen markieren mit +FLAGS: {store}"
    );
    assert!(
        voll.kopf.gelesen,
        "die Nachricht kommt als gelesen zurück"
    );
    assert!(
        voll.markierungsfehler.is_none(),
        "es ging: {:?}",
        voll.markierungsfehler
    );
}

/// **Eine bereits gelesene Nachricht wird nicht noch einmal markiert.**
///
/// Sonst ginge ein `STORE` über die Leitung, der nichts bewirkt – und in einem
/// Ordner, der nicht schreiben darf, würde genau diese unnötige Runde als
/// Fehler gemeldet, obwohl gar nichts zu tun war.
#[tokio::test]
async fn eine_bereits_gelesene_nachricht_bleibt_ohne_store() {
    let (texte, voll) = server_mit_antwort("\\Seen", true)
        .await
        .expect("die Nachricht holen");

    assert!(
        !texte.iter().any(|t| t.contains("STORE")),
        "ein STORE ohne Anlass: {texte:?}"
    );
    assert!(voll.kopf.gelesen);
    assert!(voll.markierungsfehler.is_none());
}

/// **Scheitert das Markieren, kommt die Nachricht trotzdem – und sagt es.**
///
/// Eine Mail, die sich nicht als gelesen setzen lässt, ist immer noch lesbar.
/// Sie nur nicht lesbar zu machen wäre die schlimmere halbe Lösung – und ein
/// verschwiegener Fehler ist genau das, woran sich dieser Knopf nicht erklären
/// liess.
#[tokio::test]
async fn ein_fehlgeschlagenes_markieren_wird_gemeldet_und_nicht_verschluckt() {
    let (_, voll) = server_mit_antwort("", false)
        .await
        .expect("die Nachricht holen");

    assert!(
        !voll.kopf.gelesen,
        "das Flag liess sich nicht setzen, also gilt sie noch als ungelesen"
    );
    assert!(
        voll.markierungsfehler.is_some(),
        "der Fehler darf nicht verschluckt werden"
    );
    assert!(
        voll.text.contains("Hallo"),
        "die Nachricht ist trotzdem lesbar: {:?}",
        voll.text
    );
}

/// **Der Rumpf kommt an, PEEK hin oder her.**
#[tokio::test]
async fn der_text_kommt_auch_bei_peek_an() {
    let (_, voll) = server_mit_antwort("", true)
        .await
        .expect("die Nachricht holen");

    assert!(
        voll.text.contains("Hallo"),
        "der Rumpf fehlt: {:?}",
        voll.text
    );
    assert_eq!(voll.kopf.betreff, "Hallo");
}

// --- Löschen ---------------------------------------------------------------

/// Spielt einen Server, der jeden Befehl bestaetigt und die Reihenfolge merkt.
async fn gegen_server_mit_reihenfolge() -> Vec<String> {
    let (hier, dort) = tokio::io::duplex(256 * 1024);
    let gesendet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    tokio::spawn({
        let notiert = gesendet.clone();

        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let (lesen, mut schreiben) = tokio::io::split(dort);
            let lesen = std::sync::Arc::new(tokio::sync::Mutex::new(lesen));

            let mut puffer = vec![0u8; 8192];

            for _ in 0..6 {
                let mut befehl = Vec::new();
                while let Ok(anzahl) = lesen.lock().await.read(&mut puffer).await {
                    if anzahl == 0 {
                        break;
                    }
                    befehl.extend_from_slice(&puffer[..anzahl]);
                    if befehl.ends_with(b"\r\n") {
                        break;
                    }
                }

                if befehl.is_empty() {
                    break;
                }

                notiert.lock().unwrap().push(befehl.clone());
                let text = String::from_utf8_lossy(&befehl).to_string();
                let tag = text.split_whitespace().next().unwrap_or("B1").to_string();

                let _ = schreiben
                    .write_all(format!("{tag} OK done\r\n").as_bytes())
                    .await;
                let _ = schreiben.flush().await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    });

    let mut verbindung = Verbindung::new(hier);

    loesche(&mut verbindung, "INBOX", 7)
        .await
        .expect("loeschen gelingt");

    let texte: Vec<String> = gesendet
        .lock()
        .unwrap()
        .iter()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .collect();

    texte
}

/// **Löschen heisst `\Deleted` und dann `EXPUNGE` – nichts anderes.**
///
/// `+FLAGS (\Deleted)` allein ist nur ein Vermerk: Die Nachricht stünde danach
/// noch da, je nach Server. Erst `EXPUNGE` verschwindet sie. Wer nur den
/// Vermerk schickt, hat nichts gelöscht und der Leiste trotzdem etwas
/// vorgegaukelt.
#[tokio::test]
async fn loeschen_setzt_das_flag_und_expungiert() {
    let gesendet = gegen_server_mit_reihenfolge().await;

    let store = gesendet
        .iter()
        .position(|t| t.contains("STORE"))
        .unwrap_or_else(|| panic!("kein STORE: {gesendet:?}"));
    let expunge = gesendet
        .iter()
        .position(|t| t.contains("EXPUNGE"))
        .unwrap_or_else(|| panic!("kein EXPUNGE: {gesendet:?}"));

    assert!(
        gesendet[store].contains("+FLAGS") && gesendet[store].contains("\\Deleted"),
        "das Flag heisst Deleted: {}",
        gesendet[store]
    );
    assert!(
        store < expunge,
        "erst den Vermerk setzen, dann wegraeumen: {gesendet:?}"
    );

    // **Und die Nummer der Nachricht wandert mit** – sonst loeschte der Befehl
    // irgendeine andere Nachricht.
    assert!(
        gesendet[store].contains(" 7 "),
        "die Nummer steht im Befehl: {}",
        gesendet[store]
    );
}

/// **Und derselbe Dreisatz wie beim Markieren: erst der Ordner, dann der
/// Befehl.** `STORE` und `EXPUNGE` wirken beide nur im ausgewaehlten Ordner.
#[tokio::test]
async fn loeschen_waehlt_zuerst_den_ordner() {
    let gesendet = gegen_server_mit_reihenfolge().await;

    let select = gesendet
        .iter()
        .position(|t| t.contains("SELECT"))
        .unwrap_or_else(|| panic!("kein SELECT: {gesendet:?}"));
    let store = gesendet
        .iter()
        .position(|t| t.contains("STORE"))
        .unwrap_or_else(|| panic!("kein STORE: {gesendet:?}"));

    assert!(
        select < store,
        "erst auswaehlen, dann loeschen: {gesendet:?}"
    );
}

/// **Und das Gelesen-Flag hat mit dem Löschen nichts zu tun.** Ein Tippfehler
/// – `\\Seen` statt `\\Deleted` – wäre eine Nachricht, die als gelesen
/// markiert und nicht gelöscht ist: Kaum sichtbar, aber es geht eine Mail
/// verloren.
#[tokio::test]
async fn loeschen_benutzt_nicht_das_gelesen_flag() {
    let gesendet = gegen_server_mit_reihenfolge().await;

    let store = gesendet
        .iter()
        .find(|t| t.contains("STORE"))
        .unwrap_or_else(|| panic!("kein STORE: {gesendet:?}"));

    assert!(
        !store.contains("\\Seen"),
        "geloescht wird mit Deleted, nicht mit Seen: {store}"
    );
}
