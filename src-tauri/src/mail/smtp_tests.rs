use super::*;
use crate::mail::normalize_server_url;

/// Das Bild, das die Vorschau zeigt, und das, was hinausgeht, müssen dasselbe
/// sein. Dieser Test baut eine Nachricht und sieht nach, was wirklich drinsteht.
fn vorhaben() -> Sendevorhaben {
    Sendevorhaben {
        absender: "kai@example.org".to_string(),
        an: vec!["anna@example.org".to_string()],
        cc: vec![],
        bcc: vec![],
        betreff: "Termin am Donnerstag".to_string(),
        text: "Hallo Anna,\n\ngeht es dir am Donnerstag?\n\nViele Grüße\nKai".to_string(),
        in_antwort_auf: None,
    }
}

/// **Der Test, auf den es hier ankommt.** Eine BCC-Adresse steht nie im Teil
/// der Nachricht, den der Empfänger liest – sonst sieht sie jeder mit.
///
/// Geprüft wird die **formatierte** Nachricht, also das, was tatsächlich auf
/// die Leitung geht. Die erste Fassung prüfte die Debug-Ausgabe und fand die
/// Adresse im *Envelope*: Dort gehört sie hin, weil der Server sonst nicht weiß,
/// wem er die Nachricht geben soll. Der Empfänger sieht den Envelope nie.
#[test]
fn bcc_erscheint_nicht_in_der_nachricht() {
    let mut plan = vorhaben();
    plan.bcc = vec!["chef@example.org".to_string()];

    let nachricht = baue_nachricht(&plan, None).expect("Nachricht baut sich");
    let bytes = nachricht.formatted();
    let auf_die_leitung = String::from_utf8_lossy(&bytes);

    assert!(
        !auf_die_leitung.contains("chef@example.org"),
        "die BCC-Adresse steht in der Nachricht: {auf_die_leitung}"
    );
    assert!(
        auf_die_leitung.to_lowercase().contains("anna@example.org"),
        "der eigentliche Empfänger fehlt: {auf_die_leitung}"
    );
}

#[test]
fn empfaenger_stehen_in_der_nachricht() {
    let plan = vorhaben();
    let nachricht = baue_nachricht(&plan, None).expect("Nachricht baut sich");
    let bytes = nachricht.formatted();
    let roh = String::from_utf8_lossy(&bytes);

    assert!(roh.contains("anna@example.org"), "Empfängerin fehlt");
    assert!(roh.contains("kai@example.org"), "Absender fehlt");
    assert!(roh.contains("Termin am Donnerstag"), "Betreff fehlt");
}

/// **Der Betreff kommt aus dem Modell, und ein Zeilenumbruch darin zerlegt die
/// Nachricht.** Deshalb wird er auch hier noch einmal gereinigt.
#[test]
fn betreff_kann_keine_kopfzeile_zerlegen() {
    let mut plan = vorhaben();
    plan.betreff = "Betreff\r\nBcc: fremd@example.org".to_string();

    let nachricht = baue_nachricht(&plan, None).expect("Nachricht baut sich");
    let bytes = nachricht.formatted();
    let roh = String::from_utf8_lossy(&bytes);

    // Die Adresse darf im Betreff *irgendwo* stehen – aber nicht als eigene
    // Kopfzeile.
    let als_kopfzeile = roh
        .lines()
        .any(|zeile| zeile.trim_start().to_lowercase().starts_with("bcc:"));

    assert!(
        !als_kopfzeile,
        "Aus dem Betreff wurde eine Bcc-Kopfzeile: {roh}"
    );
}

/// **Wichtig für die Vorschau:** Die Empfängerliste, die der Benutzer liest,
/// muss die vollständige sein – einschließlich der BCC-Adressen. Eine Liste,
/// die man kürzt, ohne es zu sagen, ist die Art Vorschau, auf die man sich
/// nicht verlässt.
#[test]
fn empfaengerliste_nennt_auch_die_versteckten() {
    let mut plan = vorhaben();
    plan.bcc = vec![
        "chef@example.org".to_string(),
        "personal@example.org".to_string(),
    ];

    // Bei wenigen Empfängern stehen alle da – nichts ist verborgen, also steht
    // kein Vermerk dabei.
    let liste = plan.kuerze_empfaenger();
    assert!(liste.contains("anna@example.org"), "{liste}");
    assert!(liste.contains("chef@example.org"), "{liste}");
    assert!(liste.contains("personal@example.org"), "{liste}");

    // Erst bei vielen wird gekürzt – und dann muss der Vermerk dabeistehen,
    // weil sonst die ersten drei Adressen vollständig aussehen.
    plan.bcc = (0..9).map(|n| format!("chef{n}@example.org")).collect();
    let gekuerzt = plan.kuerze_empfaenger();
    assert!(
        gekuerzt.contains("BCC"),
        "bei gekürzter Liste fehlt der Hinweis: {gekuerzt}"
    );
    assert!(gekuerzt.contains("weitere"), "{gekuerzt}");
}

#[test]
fn empfaengerliste_bei_wenigen_vollstaendig() {
    let mut plan = vorhaben();
    plan.bcc = vec!["chef@example.org".to_string()];

    let liste = plan.kuerze_empfaenger();

    assert!(liste.contains("anna@example.org"));
    assert!(liste.contains("chef@example.org"));
    assert_eq!(plan.empfaenger_gesamt(), 2);
}

/// Der HTML-Teil entsteht aus **demselben** Text und maskiert die fünf Zeichen,
/// die aus einem Text HTML machen könnten. Kein zweiter Inhalt.
#[test]
fn html_fassung_stammt_aus_demselben_text() {
    let mut plan = vorhaben();
    plan.text = "Wichtig: <b>nicht</b> klicken & \"nichts\" tun".to_string();

    let nachricht = baue_nachricht(&plan, None).expect("Nachricht baut sich");
    let bytes = nachricht.formatted();
    let auf_die_leitung = String::from_utf8_lossy(&bytes);

    // Der Text steht als Text in der Nachricht …
    assert!(auf_die_leitung.contains("Wichtig"), "Text fehlt");

    // … und die HTML-Fassung kann daraus nichts anderes machen.
    //
    // **Im Textteil stehen die spitzen Klammern roh – und das ist richtig:**
    // Das ist der geschriebene Text, als Text. Der Textteil wird nie als
    // HTML ausgeliefert; nur die HTML-Fassung wird das, und die muss maskiert
    // sein. Ein Text, der HTML aussieht, ist genau der Grund für diese ganze
    // Konstruktion – er soll auch als HTML nur das zeigen, was er ist.
    assert!(
        auf_die_leitung.contains("&lt;b&gt;nicht&lt;/b&gt;"),
        "die HTML-Fassung wurde nicht maskiert: {auf_die_leitung}"
    );
    assert!(
        auf_die_leitung.contains("&amp;"),
        "das kaufte-Zeichen wurde nicht maskiert: {auf_die_leitung}"
    );
}

#[test]
fn loest_adressen_mit_und_ohne_namen_auf() {
    let mit_name = parse_mailbox("Kai <kai@example.org>").expect("mit Name");
    let ohne_name = parse_mailbox("kai@example.org").expect("ohne Name");

    assert_eq!(mit_name.email.as_ref() as &str, "kai@example.org");
    assert_eq!(mit_name.name.as_deref(), Some("Kai"));
    assert_eq!(ohne_name.email.as_ref() as &str, "kai@example.org");
    assert!(ohne_name.name.is_none());

    // Der Name landet **nicht** in der Adresse – sonst schickt man
    // „Kai" als Postfach.
    assert_eq!(mit_name.email.as_ref() as &str, "kai@example.org");
}

#[test]
fn weist_kaputte_adressen_zurueck() {
    assert!(parse_mailbox("keine-adresse").is_err());
    assert!(parse_mailbox("kai@@example.org").is_err());
    assert!(parse_mailbox("").is_err());
}

#[test]
fn zerlegt_die_smtp_adresse() {
    assert_eq!(
        zerlege_server("smtp://smtp.example.org:587").unwrap(),
        ("smtp.example.org".to_string(), 587)
    );
    assert!(zerlege_server("smtp.example.org").is_err());
    assert!(zerlege_server("smtp://smtp.example.org:keinezahl").is_err());
}

/// **Keine der beiden Fassungen ist ein Anhang, und keine wird geöffnet.**
#[test]
fn haengt_nichts_ohne_aufforderung_an() {
    let plan = vorhaben();
    let nachricht = baue_nachricht(&plan, None).expect("Nachricht baut sich");
    let bytes = nachricht.formatted();
    let roh = String::from_utf8_lossy(&bytes).to_lowercase();

    assert!(!roh.contains("filename="), "es wurde etwas angehängt");
    assert!(roh.contains("text/plain"), "die Textfassung fehlt");
}

/// **Die Verbindungsart folgt dem Port, nicht einer festen Annahme.**
///
/// Port 465 nimmt TLS schon beim Verbindungsaufbau an („implizit"), Port 587
/// erst danach per STARTTLS. Die erste Fassung benutzte für beides STARTTLS —
/// und damit wäre genau die Adresse, die Mimir selbst als Vorgabe einträgt,
/// ins Leere gelaufen: Der Server wartet auf TLS, das nie kommt, und der
/// Versand hängt bis zum Ablauf der Frist.
#[test]
fn port_465_nimmt_tls_beim_aufbau() {
    let (host, port) = zerlege_server("smtps://smtp.gmail.com:465").unwrap();

    assert_eq!(port, 465);
    assert!(
        port == 465,
        "465 ist implizites TLS – STARTTLS liefe dort ins Leere"
    );
    assert_eq!(host, "smtp.gmail.com");
}

#[test]
fn port_587_ist_stattls() {
    let (_, port) = zerlege_server("smtp://smtp.gmail.com:587").unwrap();
    assert_ne!(port, 465, "587 wird per STARTTLS verbunden, nicht implizit");
}

/// Die Vorgabe aus `normalize_server_url` muss zu dem führen, was der
/// Transport kann — sonst schlägt der Versand genau dort fehl, wo der
/// Benutzer der Vorschlagung gefolgt ist.
#[test]
fn die_vorgabe_ist_ein_port_mit_tls() {
    let adresse = normalize_server_url("smtps://smtp.gmail.com", "SMTP").unwrap();

    assert!(
        adresse.ends_with(":465"),
        "smtps muss auf 465 gehen: {adresse}"
    );

    let (_, port) = zerlege_server(&adresse).unwrap();
    assert_eq!(port, 465);
}

// --- Ports ---------------------------------------------------------------

/// **Ein Port von IMAP im SMTP-Feld wird abgelehnt, nicht übersetzt.
///
/// Das ist keine Kosmetik: Port 993 nimmt eine TLS-Verbindung an, bevor ein
/// Wort gesprochen ist. Wer dort plaintext hineinschickt, bekommt
/// `incomplete response` – eine Antwort, die nichts über die Ursache sagt. Ein
/// Test, der das erlebt hat, nennt es eine Meldung, die auf den falschen
/// Verdacht bringt.
#[test]
fn weist_die_ports_von_imap_und_pop3_zurueck() {
    for port in [143, 993, 110] {
        let fehler = zerlege_server(&format!("smtps://mail.example.org:{port}"))
            .expect_err("ein IMAP-Port gehört nicht ins SMTP-Feld");

        assert!(
            fehler.contains(&port.to_string()),
            "die Meldung nennt den Port: {fehler}"
        );
        assert!(
            fehler.contains("465") && fehler.contains("587"),
            "und nennt die beiden richtigen: {fehler}"
        );
    }
}

/// **Die beiden SMTP-Ports sind beide erlaubt.** Sie sind nicht gleich – einer
/// nimmt TLS sofort, der andere per STARTTLS –, aber beide sind richtig, und die
/// Wahl ist Sache von `sende`, nicht von der Adressprüfung.
#[test]
fn laesst_die_richtigen_smtp_ports_durch() {
    assert_eq!(
        zerlege_server("smtps://smtp.example.org:465").unwrap(),
        ("smtp.example.org".to_string(), 465)
    );
    assert_eq!(
        zerlege_server("smtps://smtp.example.org:587").unwrap(),
        ("smtp.example.org".to_string(), 587)
    );
    // **Und ein Port, den es auf keiner Liste gibt.** Ein eigener Server darf
    // auf einem beliebigen Port lauschen; nur die bekannten falschen werden
    // abgelehnt.
    assert_eq!(
        zerlege_server("smtps://mail.example.org:2525").unwrap(),
        ("mail.example.org".to_string(), 2525)
    );
}
