use super::*;

/// Adressen werden **nicht** auf Plausibilität imRFC-Sinne geprüft, sondern auf
/// das, was tatsächlich schadet: Leerzeichen, Listen, fehlende Domain.
#[test]
fn laesst_gewoehnliche_adressen_zu() {
    assert_eq!(
        normalize_address("kai@example.org", "Empfänger").unwrap(),
        "kai@example.org"
    );
    assert_eq!(
        normalize_address("  kai.weg.123+tag@sub.example.co.uk ", "Empfänger").unwrap(),
        "kai.weg.123+tag@sub.example.co.uk"
    );
}

#[test]
fn weist_adressen_zurueck_die_zu_viel_bedeuten() {
    // Genau hier scheitert ein unbedachter Empfängeraufbau: Die Mail ginge an
    // jemanden, den niemand vorgesehen hat.
    assert!(normalize_address("a@example.org,b@example.org", "Empfänger").is_err());
    assert!(normalize_address("a@example.org;c@example.org", "Empfänger").is_err());
    assert!(normalize_address("kai example@org", "Empfänger").is_err());
    assert!(normalize_address("kai@example.org\nBcc: x@example.org", "Empfänger").is_err());
    assert!(normalize_address("kein-at.example.org", "Empfänger").is_err());
    assert!(normalize_address("@example.org", "Empfänger").is_err());
    assert!(normalize_address("kai@", "Empfänger").is_err());
    assert!(normalize_address("kai@localhost", "Empfänger").is_err());
}

#[test]
fn trennt_mehrere_empfaenger_und_begrenzt_sie() {
    let drei = "a@example.org, b@example.org;c@example.org";
    assert_eq!(parse_recipients(drei).unwrap().len(), 3);

    let zu_viele = "a@example.org,".repeat(MAX_MAIL_RECIPIENTS + 1);
    assert!(parse_recipients(&zu_viele).is_err());

    assert!(parse_recipients("  ").is_err());
}

#[test]
fn normalisiert_benutzernamen() {
    assert_eq!(normalize_username(" kai ").unwrap(), "kai");
    assert_eq!(normalize_username("\"kai\"").unwrap(), "kai");
    assert!(normalize_username(" ").is_err());
    assert!(normalize_username("kai\nweg").is_err());
}

/// Der Standardport hängt am Schema. Ein Benutzer, der `imaps://` einträgt,
/// meint Port 993 – nicht 143, und nicht „irgendeinen“.
#[test]
fn setzt_standardports_je_schema() {
    assert_eq!(
        normalize_server_url("imaps://imap.example.org", "IMAP").unwrap(),
        "imaps://imap.example.org:465"
    );
    assert_eq!(
        normalize_server_url("smtps://smtp.example.org", "SMTP").unwrap(),
        "smtps://smtp.example.org:465"
    );
    assert_eq!(
        normalize_server_url("imap://imap.example.org", "IMAP").unwrap(),
        "imap://imap.example.org:143"
    );
    assert_eq!(
        normalize_server_url("smtp://smtp.example.org", "SMTP").unwrap(),
        "smtp://smtp.example.org:587"
    );
}

#[test]
fn laesst_gesetzten_port_stehen() {
    assert_eq!(
        normalize_server_url("imap://imap.example.org:1143", "IMAP").unwrap(),
        "imap://imap.example.org:1143"
    );
}

#[test]
fn weist_falsches_schema_zurueck() {
    // `https://` wäre technisch möglich und praktisch falsch: Gmail spricht
    // IMAP, nicht HTTPS. Der Fehler nennt das, statt es später zu zeigen.
    assert!(normalize_server_url("https://imap.example.org", "IMAP").is_err());
    assert!(normalize_server_url("smtps://imap.example.org", "IMAP").is_err());
    assert!(normalize_server_url("imap.example.org", "IMAP").is_err());
    assert!(normalize_server_url("", "IMAP").is_err());
}

/// Zugangsdaten im Adressfeld würden beim Verbindungsfehler mit im Text stehen.
#[test]
fn weist_zugangsdaten_in_der_adresse_zurueck() {
    assert!(normalize_server_url("imaps://kai:geheim@imap.example.org", "IMAP").is_err());
}

/// **Das ist die Eigenschaft, auf der der ganze Schutz beruht:** In der
/// Konfiguration steht von sich aus keine fremde Adresse. Die Prüfung in
/// `binary-pruefen.mjs` kann also überhaupt etwas finden.
#[test]
fn standard_konfiguration_nennt_keinen_fremden_server() {
    let config = MailConfig::default();

    assert!(
        config.imap_server.is_empty(),
        "IMAP darf keinen Vorgabewert haben"
    );
    assert!(
        config.smtp_server.is_empty(),
        "SMTP darf keinen Vorgabewert haben"
    );
    assert!(!config.ist_eingerichtet());
}

#[test]
fn waehlt_ordner_und_absender_vernuenftig() {
    let mut config = MailConfig {
        username: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    // Ohne eigene Adresse wird die Adresse des Benutzers verwendet: Wer als
    // `kai@example.org` angemeldet ist, will nicht als `kai` schreiben.
    assert_eq!(config.absender(), "kai@example.org");
    assert_eq!(config.ordner(), "INBOX");

    config.address = "mimir@example.org".to_string();
    assert_eq!(config.absender(), "mimir@example.org");

    config.folder = "Posteingang/Neu".to_string();
    assert_eq!(config.ordner(), "Posteingang/Neu");
}

#[test]
fn begrenzt_die_anzahl_gezeigter_nachrichten() {
    let config = MailConfig {
        max_entries: 0,
        ..MailConfig::default()
    };
    assert_eq!(config.anzeige_grenze(), 1);

    let config = MailConfig {
        max_entries: 5000,
        ..MailConfig::default()
    };
    assert_eq!(config.anzeige_grenze(), MAX_MAIL_LIST_ENTRIES);
}

/// Ein Text, der ohne Hinweis endet, liest sich wie ein vollständiger.
#[test]
fn weist_auf_gekuerzten_text_hin() {
    let lang = "Absatz eins.\n\n".repeat(500);
    let kurz = kuerze(&lang, 200);

    assert!(
        kurz.len() <= 200,
        "Ergebnis über der Grenze: {} Bytes",
        kurz.len()
    );
    assert!(
        kurz.ends_with("[…gekürzt…]"),
        "gekürzt ohne Hinweis: {kurz}"
    );
}

/// Der Text wird an einer Absatzgrenze abgeschnitten, damit kein halber Satz
/// stehenbleibt – aber nur, wenn dabei auch etwas übrig bleibt.
#[test]
fn kuerzt_an_absatzgrenze() {
    let text = "Erster Absatz.\n\nZweiter Absatz, der länger ist als der erste.";
    let gekuerzt = kuerze(text, 40);

    // Position 14 liegt hinter der Hälfte der Grenze, deshalb greift die
    // Absatzgrenze; der Rest bleibt ein vollständiger Satz.
    assert!(gekuerzt.starts_with("Erster Absatz."), "{gekuerzt}");
    assert!(!gekuerzt.contains("Zweiter"), "{gekuerzt}");
    assert!(gekuerzt.ends_with("[…gekürzt…]"), "{gekuerzt}");
}

/// Zu kurze Vorspalte: Lieber am Zeichen abschneiden als fast nichts behalten.
#[test]
fn kuerzt_auch_ohne_brauchbare_absatzgrenze() {
    let text = "a".repeat(300) + "\n\nEnde.";
    let gekuerzt = kuerze(&text, 100);

    assert!(gekuerzt.len() > 50, "zu viel abgeschnitten: {gekuerzt}");
    assert!(gekuerzt.ends_with("[…gekürzt…]"));
}

#[test]
fn laesst_kurzen_text_unangetastet() {
    assert_eq!(kuerze("Hallo", 100), "Hallo");
    assert_eq!(kuerze("", 100), "");
}

/// An der Grenze darf kein UTF-8-Zeichen zerrissen werden – sonst entsteht ein
/// Text, den das Modell nicht anzeigen kann.
#[test]
fn kuerzt_nicht_mitten_im_zeichen() {
    let text = "a".repeat(40) + "äöü";
    let gekuerzt = kuerze(&text, 60);

    assert!(
        gekuerzt.len() <= 60,
        "Ergebnis über der Grenze: {}",
        gekuerzt.len()
    );
    // Ein zerrissenes UTF-8-Zeichen wird zu `U+FFFD`. Erscheint eines, wurde
    // mitten in einem Zeichen geschnitten.
    assert!(
        !gekuerzt.contains('\u{fffd}'),
        "Zeichen zerrissen: {gekuerzt}"
    );
    assert!(gekuerzt.starts_with(&text[..20]));
    assert!(gekuerzt.ends_with("[…gekürzt…]"));
}

/// Ein Zeilenumbruch im Betreff zerlegt die Nachricht auf dem Weg.
#[test]
fn reinigt_den_betreff() {
    assert_eq!(
        kuerze_betreff("  Betreff\r\nBcc: fremd@example.org  "),
        "Betreff Bcc: fremd@example.org"
    );
    assert_eq!(kuerze_betreff("Normaler Betreff"), "Normaler Betreff");
}

#[test]
fn der_schluesselbund_eintraeg_ist_je_konto_eigen() {
    // **Zwei Konten auf derselben Maschine dürfen sich nicht ins Gehege
    // kommen.** Ohne den Server im Eintrag würde das Passwort von Kai an Kai
    // gehen, sobald ein zweiter Mailserver eingetragen ist – und niemand
    // bekäme es zu sehen.
    let a = MailConfig {
        username: "kai@example.org".to_string(),
        imap_server: "imaps://imap.gmail.com:993".to_string(),
        ..MailConfig::default()
    };

    let mut b = a.clone();
    b.imap_server = "imaps://imap.example.org:993".to_string();

    let c = MailConfig {
        username: "anna@example.org".to_string(),
        ..a.clone()
    };

    assert_ne!(schluesselbund_eintrag(&a), schluesselbund_eintrag(&b));
    assert_ne!(schluesselbund_eintrag(&a), schluesselbund_eintrag(&c));
    assert_eq!(
        schluesselbund_eintrag(&a),
        schluesselbund_eintrag(&a.clone())
    );
}
