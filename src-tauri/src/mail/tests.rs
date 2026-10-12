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

// --- Mehrere Konten ---------------------------------------------------------

/// Zwei Konten, wie sie nach dem Anlegen dastehen.
fn zwei_konten() -> MailKonten {
    MailKonten {
        konten: vec![
            MailConfig {
                id: "konto-1".to_string(),
                name: "Privat".to_string(),
                imap_server: "imaps://imap.example.org:465".to_string(),
                username: "kai@example.org".to_string(),
                ..MailConfig::default()
            },
            MailConfig {
                id: "konto-2".to_string(),
                name: "Arbeit".to_string(),
                imap_server: "imaps://imap.firma.invalid:465".to_string(),
                username: "kai@firma.invalid".to_string(),
                ..MailConfig::default()
            },
        ],
        aktiv: "konto-1".to_string(),
    }
}

/// **Ein Konto von 0.2 darf nicht verloren gehen.** Bis dahin stand unter
/// `"mail"` das Konto selbst; jetzt steht dort eine Liste. Wer die
/// Konfigurationsdatei nicht anfasst, muss trotzdem sein Postfach wiederfinden.
#[test]
fn uebernimmt_ein_konto_aus_der_alten_konfiguration() {
    let alt = serde_json::json!({
        "imap_server": "imaps://imap.example.org:465",
        "smtp_server": "smtps://smtp.example.org:465",
        "username": "kai@example.org",
        "folder": "Posteingang"
    });

    let konten: MailKonten = serde_json::from_value(alt).expect("die alte Form wird gelesen");

    assert_eq!(konten.konten.len(), 1);

    let konto = &konten.konten[0];

    // **Ohne Kennung fände das Konto weder seinen Sitzungsplatz noch seine
    // Auswahl** – und beide Fehler fallen erst auf, wenn jemand ein zweites
    // Konto anlegt.
    assert!(!konto.id.is_empty(), "die Kennung wurde nicht vergeben");
    assert_eq!(konto.username, "kai@example.org");
    assert_eq!(konto.folder, "Posteingang");
    assert_eq!(konten.aktiv, konto.id, "das alte Konto wird nicht gewählt");
}

/// **Ein altes Konto, das nie eingetragen war, wird nicht gerettet.** Es kann
/// sich nicht anmelden, und eine Zeile, an der nichts steht und nichts stehen
/// kann, ist in einer Liste aus Konten nur ein weiterer Weg, den man sich merken
/// muss.
#[test]
fn verwirft_ein_altes_leeres_konto() {
    let alt = serde_json::json!({ "imap_server": "", "username": "" });
    let konten: MailKonten = serde_json::from_value(alt).expect("die alte Form wird gelesen");

    assert!(konten.konten.is_empty());
    assert!(konten.aktiv().is_none());
}

/// Kennungen, die fehlen oder doppelt sind, stehen in einer von Hand
/// bearbeiteten Datei. Ohne sie hätten zwei Konton denselben Sitzungsplatz und
/// denselben Schlüsselbund-Eintrag.
#[test]
fn vergibt_und_ent_schachtelt_kennungen() {
    let roh = serde_json::json!({
        "konten": [
            { "username": "a@example.org", "imap_server": "imaps://a.invalid:465" },
            { "id": "konto-1", "username": "b@example.org", "imap_server": "imaps://b.invalid:465" },
            { "id": "konto-1", "username": "c@example.org", "imap_server": "imaps://c.invalid:465" }
        ],
        "aktiv": "gibt-es-nicht"
    });

    let konten: MailKonten = serde_json::from_value(roh).expect("auch die neue Form wird gelesen");

    let ids: Vec<&str> = konten.konten.iter().map(|k| k.id.as_str()).collect();

    assert_eq!(ids, ["konto-1", "konto-1-1", "konto-1-2"]);
    assert_eq!(
        konten.aktiv, "konto-1",
        "eine Auswahl ins Leere fällt auf das erste Konto zurück"
    );
}

/// **Die Leiste zeigt nie ein Konto, das es nicht gibt.** Das passiert, wenn das
/// gezeigte Konto entfernt wird; das nächste nimmt seinen Platz.
#[test]
fn die_auswahl_zeigt_auf_ein_vorhandenes_konto() {
    let konten = MailKonten {
        konten: vec![MailConfig {
            id: "konto-7".to_string(),
            ..MailConfig::default()
        }],
        // Zeigt ins Leere – etwa weil die Datei von Hand bearbeitet wurde.
        aktiv: "weg".to_string(),
    };

    assert_eq!(konten.aktiv().map(|k| k.id.as_str()), Some("konto-7"));
}

/// **Eine Auswahl, die beim Entfernen auf ein Konto zeigt, das fort ist**, ließe
/// die Leiste leer. Sie rückt stattdessen auf das Nachbarkonto.
#[test]
fn entfernen_rueckt_die_auswahl_nach() {
    let mut konten = drei_konten();
    konten.aktiv = "konto-2".to_string();

    let entfernt = konten.entfernen("konto-2").expect("das Konto gibt es");

    assert_eq!(entfernt.id, "konto-2");
    assert_eq!(konten.konten.len(), 2);
    assert_eq!(
        konten.aktiv, "konto-3",
        "die Auswahl muss auf ein Konto zeigen, das noch da ist"
    );
    assert!(konten.aktiv().is_some());
}

/// Auch das letzte Konto darf verschwinden – dann ist die Liste leer, und das
/// ist ein gültiger Zustand, kein Fehler.
#[test]
fn entfernen_lasst_die_liste_leer_werden() {
    let mut konten = MailKonten {
        konten: vec![MailConfig {
            id: "konto-1".to_string(),
            ..MailConfig::default()
        }],
        aktiv: "konto-1".to_string(),
    };

    assert!(konten.entfernen("konto-1").is_some());
    assert!(konten.konten.is_empty());
    assert_eq!(konten.aktiv, "");
    assert!(konten.entfernen("konto-1").is_none(), "zweimal geht nicht");
}

fn drei_konten() -> MailKonten {
    MailKonten {
        konten: vec![
            MailConfig {
                id: "konto-1".to_string(),
                name: "Privat".to_string(),
                ..MailConfig::default()
            },
            MailConfig {
                id: "konto-2".to_string(),
                name: "Arbeit".to_string(),
                ..MailConfig::default()
            },
            MailConfig {
                id: "konto-3".to_string(),
                name: "Verein".to_string(),
                ..MailConfig::default()
            },
        ],
        aktiv: "konto-1".to_string(),
    }
}

/// Die nächste Kennung ist die kleinste freie. Wird ein Konto in der Mitte
/// entfernt, ist sie wieder frei – und wird neu vergeben, ohne dass die
/// übrigen ihre Kennung verlieren.
#[test]
fn vergibt_die_kleinste_freie_kennung() {
    let konten = drei_konten();
    assert_eq!(konten.naechste_id(), "konto-4");

    let mut ohne_erstes = drei_konten();
    ohne_erstes.entfernen("konto-1");
    assert_eq!(ohne_erstes.naechste_id(), "konto-1");

    assert_eq!(MailKonten::default().naechste_id(), "konto-1");
}

/// Ohne eigenen Namen zeigt die Leiste die Adresse, und ohne Adresse den
/// Benutzernamen. **Nie leer**: Ein leeres Etikett in einer Liste aus Konten
/// sieht nach einem Fehler aus, und ist einer.
#[test]
fn der_anzeigename_faellt_vom_namen_auf_die_adresse() {
    let mut konto = MailConfig {
        id: "konto-1".to_string(),
        username: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    assert_eq!(konto.anzeige_name(), "kai@example.org");

    konto.address = "mimir@example.org".to_string();
    assert_eq!(konto.anzeige_name(), "mimir@example.org");

    konto.name = "  Privat  ".to_string();
    assert_eq!(konto.anzeige_name(), "Privat");
}

/// Das Modell nennt „Gmail", der Benutzer schreibt „gmail", und die Oberfläche
/// kennt die Kennung. Alle drei müssen dasselbe Konto treffen.
#[test]
fn findet_ein_konto_ueber_kennung_name_und_benutzer() {
    let konten = zwei_konten();

    assert_eq!(
        konten.nach_bezeichnung("konto-2").map(|k| k.name.as_str()),
        Some("Arbeit")
    );
    assert_eq!(
        konten.nach_bezeichnung("Arbeit").map(|k| k.name.as_str()),
        Some("Arbeit")
    );
    assert_eq!(
        konten
            .nach_bezeichnung("  arbeIT ")
            .map(|k| k.name.as_str()),
        Some("Arbeit")
    );
    assert_eq!(
        konten
            .nach_bezeichnung("kai@firma.invalid")
            .map(|k| k.name.as_str()),
        Some("Arbeit")
    );

    assert!(konten.nach_bezeichnung("").is_none());
    assert!(konten.nach_bezeichnung("Gmx").is_none());
}

/// **Ohne Kontoangabe das gewählte.** Das ist der Weg, den ein Aufruf aus einer
/// Unterhaltung von gestern geht, und der Grund, warum das Argument optional
/// ist: Wer ein Postfach hat, argumentiert nicht mit Konten.
/// Der Name des Kontos, das ein Aufruf meint – oder `None`, wenn keiner gemeint
/// ist und keiner ausgewählt werden konnte.
fn aufgerufenes_konto(konten: &MailKonten, argumenten: serde_json::Value) -> Option<String> {
    konto_aus_argumenten(konten, &argumenten)
        .ok()
        .map(|konto| konto.name.clone())
}

#[test]
fn ohne_kontoangabe_wird_das_gewaehlte_genommen() {
    let mut konten = zwei_konten();

    assert_eq!(
        aufgerufenes_konto(&konten, serde_json::json!({})).as_deref(),
        Some("Privat")
    );

    // **Die Kontoangabe schlägt die Auswahl.** Sonst könnte ein Aufruf, der
    // ausdrücklich „Arbeit" sagt, im Postfach „Privat" landen.
    assert_eq!(
        aufgerufenes_konto(&konten, serde_json::json!({ "konto": "Arbeit" })).as_deref(),
        Some("Arbeit")
    );

    konten.aktiv = "gibt-es-nicht".to_string();
    assert_eq!(
        aufgerufenes_konto(&konten, serde_json::json!({})).as_deref(),
        Some("Privat"),
        "eine Auswahl ins Leere fällt auf das erste Konto zurück"
    );
}

/// **Ein unbekannter Name nennt die vorhandenen.** Sonst rät das Modell einen
/// zweiten Namen, und der Fehler wiederholt sich, bis der Zug aufgebraucht ist.
#[test]
fn ein_unbekanntes_konto_nennt_die_vorhandenen() {
    let konten = zwei_konten();

    let fehler = konto_aus_argumenten(&konten, &serde_json::json!({ "konto": "Gmx" }))
        .expect_err("Gmx gibt es nicht")
        .to_string();

    assert!(fehler.contains("Gmx"), "{fehler}");
    assert!(fehler.contains("Privat"), "{fehler}");
    assert!(fehler.contains("Arbeit"), "{fehler}");
}

#[test]
fn ohne_konten_gibt_es_die_schuetzende_antwort() {
    let leer = MailKonten::default();

    assert!(konto_aus_argumenten(&leer, &serde_json::json!({})).is_err());
    assert!(leer.aktiv_gefordert().is_err());
}

/// **Alle vier Mailwerkzeuge nehmen ein Konto entgegen.** An einem einzigen
/// fehlenden könnte ein Modell nicht raten, welches Postfach gemeint ist.
#[test]
fn jedes_mailwerkzeug_nimmt_ein_konto_entgegen() {
    let schemata = [
        list_emails_schema(),
        read_email_schema(),
        draft_email_schema(),
        send_email_schema(),
    ];

    for schema in schemata {
        let name = schema["function"]["name"].as_str().unwrap_or_default();
        let konto = &schema["function"]["parameters"]["properties"][werkzeug::KONTO];

        assert!(
            konto.is_object(),
            "{name} nimmt kein Konto entgegen – ohne das Argument ginge bei \
             mehreren Postfächern kein Weg zum richtigen"
        );

        let beschreibung = konto["description"].as_str().unwrap_or_default();
        assert!(
            beschreibung.contains("Mailleiste"),
            "{name} sagt nicht, was ohne die Angabe gilt: {beschreibung}"
        );
    }
}

/// Die Anweisung nennt die Konten. **Ohne Namen kann das Modell eines raten.**
#[test]
fn die_anweisung_nennt_die_konten() {
    let konten = vec!["Privat".to_string(), "Arbeit".to_string()];
    let anweisung = mail_anweisung(&konten);

    assert!(anweisung.contains("Privat"), "{anweisung}");
    assert!(anweisung.contains("Arbeit"), "{anweisung}");

    // Ohne eingetragenes Konto steht das auch da – sonst hielte das Modell ein
    // Postfach für vorhanden.
    let ohne = mail_anweisung(&[]);
    assert!(ohne.contains("kein Postfach angemeldet"), "{ohne}");
}

/// **Ein Konto ohne Server kann kein Passwort im Schlüsselbund haben.** Die
/// Abfrage kehrt vor jedem Zugriff auf den Schlüsselbund zurück – sonst würde
/// ein leeres Konto einen fremden Eintrag melden können, nur weil sein
/// Benutzername mit dem eines anderen zusammenfällt.
#[test]
fn ein_unvollstaendiges_konto_gilt_als_nicht_gespeichert() {
    let store = CredentialStore;

    let leer = MailConfig::default();
    assert!(!store.exists(&leer));
    assert!(!store.exists(&MailConfig {
        username: "kai@example.org".to_string(),
        ..MailConfig::default()
    }));

    // **Der Eintrag, den `exists` und `merken` benutzen, ist derselbe.** Genau
    // das war der Fehler: `exists` baute einen eigenen Namen ohne den Server
    // und antwortete deshalb immer `false` – das Ankreuzfeld „Passwort merken"
    // war nie vorbelegt, obwohl das Passwort sicher im Schlüsselbund lag.
    let konto = MailConfig {
        id: "konto-1".to_string(),
        username: "kai@example.org".to_string(),
        imap_server: "imaps://imap.example.org:465".to_string(),
        ..MailConfig::default()
    };

    assert_eq!(
        schluesselbund_eintrag(&konto),
        "mail:kai@example.org@imaps://imap.example.org:465"
    );
}

/// **Der Sitzungszustand hält je Konto ein eigenes Passwort.** Genau darum geht
/// es: Ein einziger Platz für alle hieße, dass sich die Konten beim Senden
/// gegenseitig das Passwort reichten.
#[test]
fn der_sitzungszustand_trennt_die_konten() {
    let sitzung = MailSession::default();

    assert!(!sitzung.angemeldet("konto-1"));

    sitzung.set_password("konto-1", "erstes").unwrap();
    sitzung.set_password("konto-2", "zweites").unwrap();

    assert_eq!(
        sitzung.password("konto-1").map(|w| w.to_string()),
        Some("erstes".to_string())
    );
    assert_eq!(
        sitzung.password("konto-2").map(|w| w.to_string()),
        Some("zweites".to_string())
    );

    // **Abmelden trifft nur das eine Konto.**
    sitzung.abmelden("konto-1");
    assert!(!sitzung.angemeldet("konto-1"));
    assert!(sitzung.angemeldet("konto-2"));

    // **Entfernen nimmt auch den Zeitstempel mit**, sonst behauptete die Leiste
    // für ein Konto, das es nicht mehr gibt, es sei gerade erfolgreich gewesen.
    sitzung.mark_success("konto-2");
    assert!(sitzung.last_success("konto-2").is_some());
    sitzung.entfernen("konto-2");
    assert!(sitzung.last_success("konto-2").is_none());
    assert!(!sitzung.angemeldet("konto-2"));
}

/// Die Kennung folgt dem Konto, nicht dem Benutzernamen: Ein Konto, dessen
/// Server oder Adresse geändert wird, verliert damit weder Anzeige noch
/// Auswahl.
#[test]
fn die_kennung_bleibt_stehen_wenn_sich_die_anschrift_aendert() {
    let mut konto = MailConfig {
        id: "konto-3".to_string(),
        name: "Arbeit".to_string(),
        username: "kai@alt.invalid".to_string(),
        imap_server: "imaps://imap.alt.invalid:465".to_string(),
        ..MailConfig::default()
    };

    let sitzung = MailSession::default();
    sitzung.set_password(&konto.id, "passwort").unwrap();

    konto.imap_server = "imaps://imap.neu.invalid:465".to_string();
    konto.username = "kai@neu.invalid".to_string();

    assert_eq!(konto.id, "konto-3", "die Kennung darf nicht mitwandern");
    assert_eq!(konto.anzeige_name(), "Arbeit");
}

/// **Die Konfiguration, die auf der Maschine des Benutzers lag**, als Testfall.
#[test]
fn die_konfiguration_des_benutzers_wird_gelesen() {
    let roh = r#"{
        "server_url": "http://localhost:11434",
        "provider": "remote",
        "theme": "dunkel",
        "mail": {
            "aktiv": "konto-1",
            "konten": [
                {
                    "id": "konto-1",
                    "name": "",
                    "imap_server": "imaps://imap.gmail.com:465",
                    "smtp_server": "smtps://smtp.gmail.com:465",
                    "username": "kai@example.org",
                    "address": "",
                    "folder": "",
                    "max_entries": 0
                }
            ]
        }
    }"#;

    let config: crate::OllamaConfig =
        serde_json::from_str(roh).expect("die Konfiguration wird gelesen");

    assert_eq!(config.mail.konten.len(), 1, "die Kontenliste ist leer");
    assert_eq!(config.mail.aktiv, "konto-1");

    let konto = config.mail.aktiv().expect("das aktive Konto");
    assert!(konto.ist_eingerichtet(), "{}", konto.imap_server);
    assert_eq!(konto.anzeige_name(), "kai@example.org");
}

/// **Ein Schlüsselbund, der nicht antwortet, darf die Leiste nicht aufhalten.**
///
/// `keyring` spricht über D-Bus mit dem Secret Service, und das ist ein
/// blockierender Aufruf. Ein gesperrter Schlüsselbund lässt ihn nicht fehlschlagen,
/// sondern warten – und wartet in einem asynchronen Befehl einen Arbeiter der
/// Laufzeit fest. Das `await` im Frontend kommt nie zurück, und die Leiste behält
/// den Text, den sie beim Start hatte: *„Kein Mailkonto eingetragen"*, während das
/// Konto längst in der Konfiguration steht.
///
/// Der Aufrufer ist deshalb `spawn_blocking` mit Frist. Hier wird nur geprüft,
/// dass der vollständige Kontoeintrag eine solche Frage überhaupt zulässt – der
/// Schlüsselbund selbst steht in der Testumgebung nicht zur Verfügung.
#[test]
fn die_schluesselbundfrage_braucht_einen_vollstaendigen_eintrag() {
    let unvollstaendig = MailConfig::default();

    assert!(
        !unvollstaendig.ist_eingerichtet(),
        "ohne Server und Benutzername gibt es nichts zu fragen"
    );

    assert_eq!(
        SCHLUESSELBUND_FRAGE,
        std::time::Duration::from_secs(3),
        "die Frist gehört in den Test: Wer sie ändert, muss das hier auch ändern"
    );
}
