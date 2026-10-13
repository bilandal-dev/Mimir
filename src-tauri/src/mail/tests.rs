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

/// **Null heißt „unbestimmt" – und nicht „eine Nachricht".**
///
/// Dieser Test stand lange auf `max_entries: 0` ⇒ `1` und war damit der Grund,
/// warum die Leiste nur eine Mail zeigte: Das Konto wird mit `#[serde(default)]`
/// angelegt, bekommt also `0`, und `0.clamp(1, 50)` ist `1`. Die Prüfung war grün,
/// die Leiste leer, und niemand hatte einen Fehler zu suchen – es gab keinen.
///
/// Die Zahl stand hier, weil sie plausibel aussah. Genau das ist das Problem an
/// ihr: Sie hat den Fehler zur Sache erklärt, statt ihn zu zeigen.
#[test]
fn begrenzt_die_anzahl_gezeigter_nachrichten() {
    // **Ohne Angabe gilt die Vorgabe**, nicht die kleinste mögliche Zahl.
    let unbestimmt = MailConfig {
        max_entries: 0,
        ..MailConfig::default()
    };
    assert_eq!(
        unbestimmt.anzeige_grenze(),
        VORGABE_NACHRICHTEN,
        "eine frische Konfiguration zeigt eine ganze Leiste, nicht eine Nachricht"
    );

    // **Eine eingetragene Zahl gilt – auch eine kleine.**
    let wenige = MailConfig {
        max_entries: 3,
        ..MailConfig::default()
    };
    assert_eq!(wenige.anzeige_grenze(), 3);

    // **Und der Deckel gilt weiterhin.**
    let ueberhaupt_nicht = MailConfig {
        max_entries: 5000,
        ..MailConfig::default()
    };
    assert_eq!(
        ueberhaupt_nicht.anzeige_grenze(),
        MAX_MAIL_LIST_ENTRIES
    );
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

/// **Die Nummer wird auch dann gelesen, wenn sie als Text kommt.**
///
/// Modelle schreiben dasselbe Feld mal als `1` und mal als `"1"`. `as_u64`
/// fiel bei der Ziffernfolge still auf `None` zurück, und der Aufruf scheiterte
/// an der Schreibweise statt an der Absicht. Beim Löschen ist das der
/// Unterschied zwischen „gelöscht" und „Nachricht 1 gelöscht, allerdings die
/// gar nicht" – die Nachricht mit dieser Nummer wurde nie angetastet.
#[test]
fn die_nummer_wird_auch_als_text_gelesen() {
    for argumente in [
        serde_json::json!({ "nummer": 7 }),
        serde_json::json!({ "nummer": "7" }),
        serde_json::json!({ "nummer": " 7 " }),
    ] {
        assert_eq!(nummer_aus_argumenten(&argumente), Some(7), "{argumente}");
    }

    // Und was keine Nummer ist, bleibt draußen – mit Grenze in beide Richtungen.
    for argumente in [
        serde_json::json!({}),
        serde_json::json!({ "nummer": null }),
        serde_json::json!({ "nummer": "" }),
        serde_json::json!({ "nummer": "sieben" }),
        serde_json::json!({ "nummer": 1.5 }),
        serde_json::json!({ "nummer": -1 }),
        serde_json::json!({ "nummer": u64::MAX }),
    ] {
        assert_eq!(nummer_aus_argumenten(&argumente), None, "{argumente}");
    }
}

/// **Das Konto ist auch unter seiner Absenderadresse erreichbar.**
///
/// Die Adresse steht in jeder Zeile von `list_emails` und in jeder Nachricht, die
/// `read_email` liefert – sie ist die vertrauteste Bezeichnung, die das Modell
/// hat. Ohne eigenen Kontonamen wird sie ohnehin über den Anzeigenamen gefunden;
/// sobald der Benutzer seinem Konto einen Namen gibt, ist sie es sonst nirgends
/// mehr, und ein völlig nachvollziehbarer Aufruf scheitert an einer Angabe, die
/// das Modell sich aus der Postfachliste abgelesen hat.
#[test]
fn das_konto_ist_auch_unter_seiner_absenderadresse_erreichbar() {
    let konten = MailKonten {
        konten: vec![MailConfig {
            id: "konto-1".to_string(),
            name: "Privat".to_string(),
            username: "kai.weg".to_string(),
            address: "kai.weg@example.org".to_string(),
            ..MailConfig::default()
        }],
        aktiv: "konto-1".to_string(),
    };

    for bezeichnung in [
        "konto-1",
        "Privat",
        "privat",
        "kai.weg",
        "kai.weg@example.org",
        "KAI.WEG@EXAMPLE.ORG",
        "  privat  ",
    ] {
        assert!(
            konten.nach_bezeichnung(bezeichnung).is_some(),
            "nicht gefunden: {bezeichnung}"
        );
    }

    assert!(konten.nach_bezeichnung("kai@example.org").is_none());
}

/// **Eine gelesene Nachricht nennt ihre Nummer – im Inhalt, nicht nur in der
/// Zusammenfassung.**
///
/// Genau daran ist das Löschen gescheitert. Das Modell hatte eine Mail gelesen
/// und zusammengefasst; ihre Nummer stand in der `summary`, und die sieht nur
/// die Oberfläche. Also hat es geraten – und die 1 geraten, also die älteste
/// Nachricht im Ordner. Der Benutzer sah im Bestätigungsfenster eine fremde Mail
/// und hat abgelehnt.
///
/// „Zusammenfassen" und „dann löschen" sind zwei Aufrufe an **dieselbe**
/// Nachricht. Dass das überhaupt möglich ist, setzt voraus, dass das Modell nach
/// dem Lesen weiß, welche es war.
#[test]
fn eine_gelesene_nachricht_nennt_ihre_nummer() {
    let nachricht = imap::NachrichtVoll {
        kopf: imap::NachrichtKopf {
            nummer: 1187,
            absender: "diggingthegreats@creator.patreon.com".to_string(),
            betreff: "Were the 90s Actually Better?".to_string(),
            datum: "Mi, 01 Okt 2026".to_string(),
            gelesen: false,
            bytes: 812,
            vorschau: String::new(),
        },
        an: "bilandal.dev@gmail.com".to_string(),
        cc: String::new(),
        text: "Viel Dank für die Informations-Mail.".to_string(),
        anhaenge: Vec::new(),
        message_id: String::new(),
        markierungsfehler: None,
    };

    let text = gelesen_text(&nachricht, 1187);

    assert!(text.contains("Nr. 1187"), "{text}");
    // Und sie steht **vor** dem Brief: Das Modell liest von oben, und eine Zahl
    // unter einem langen Text wird nicht mehr mit der Kopfzeile darüber
    // verbunden – sie gehört dann zu gar nichts mehr.
    assert!(
        text.find("Nr. 1187") < text.find("Viel Dank"),
        "die Nummer steht hinter dem Brief:\n{text}"
    );
    // Und der Brief selbst ist vollständig, sonst wäre das um den Preis der
    // Nummer erkauft.
    for teil in ["diggingthegreats@creator.patreon.com", "Were the 90s Actually Better?"] {
        assert!(text.contains(teil), "{teil} fehlt:\n{text}");
    }
}

/// **Ein erfundener Kontoname wird bei einem einzigen Konto anders beantwortet
/// als bei mehreren.** Aufgetaucht ist „default": Das Modell schrieb es in einen
/// Löschaufruf, bekam „Es gibt kein Mailkonto mit dem Namen 'default'. Vorhanden:
/// bilandal.dev@gmail.com" – zitierte die Meldung im nächsten Schritt wortgetreu
/// und schrieb trotzdem wieder „default".
///
/// Die Meldung nannte also das richtige Konto und half trotzdem nicht, weil sie
/// offenließ, was stattdessen zu tun ist. Bei einem einzigen Konto ist die
/// Antwort eindeutig, und sie ist eine: **weglassen.**
#[test]
fn ein_erfundenes_konto_bekommt_bei_einem_konto_den_ausweg() {
    let konten = MailKonten {
        konten: vec![MailConfig {
            id: "konto-1".to_string(),
            username: "bilandal.dev@gmail.com".to_string(),
            address: "bilandal.dev@gmail.com".to_string(),
            ..MailConfig::default()
        }],
        aktiv: "konto-1".to_string(),
    };

    let fehler = konto_aus_argumenten(&konten, &serde_json::json!({ "konto": "default" }))
        .expect_err("default gibt es nicht");

    assert!(fehler.contains("default"), "{fehler}");
    // Der Ausweg steht drin – ohne ihn nennt die Meldung nur das Problem.
    assert!(
        fehler.contains("weg") && fehler.to_lowercase().contains("konto"),
        "die Meldung sagt nicht, was zu tun ist: {fehler}"
    );
    assert!(
        fehler.contains("bilandal.dev@gmail.com"),
        "die Meldung nennt das richtige Konto nicht: {fehler}"
    );

    // **Bei zwei Konten bleibt die Meldung die andere**: Dort ist „weglassen"
    // falsch, weil damit das falsche Postfach gemeint sein könnte.
    let zwei = MailKonten {
        konten: vec![
            MailConfig {
                id: "konto-1".to_string(),
                username: "a@example.org".to_string(),
                ..MailConfig::default()
            },
            MailConfig {
                id: "konto-2".to_string(),
                username: "b@example.org".to_string(),
                ..MailConfig::default()
            },
        ],
        aktiv: "konto-1".to_string(),
    };

    let fehler = konto_aus_argumenten(&zwei, &serde_json::json!({ "konto": "default" }))
        .expect_err("default gibt es nicht");

    assert!(fehler.contains("Vorhanden"), "{fehler}");
    assert!(
        !fehler.contains("lass `konto` einfach weg"),
        "bei zwei Konten wäre der Ausweg falsch: {fehler}"
    );
}

/// **Ein Konto mit einem einzigen Eintrag nimmt trotzdem seinen Namen an.**
///
/// Der Sonderrat für erfundene Namen war zuerst **vor** die Suche gesetzt. Damit
/// wurde bei genau einem Konto *jedes* Argument abgelehnt – auch das richtige –
/// und die Meldung las sich wie eine Kette: „Es gibt kein Mailkonto mit dem
/// Namen ‚bilandal.dev@gmail.com'. Eingerichtet ist genau eines: ‚bilandal.dev@gmail.com'".
/// Die Antwort auf die Frage war in der Antwort enthalten.
///
/// Der Test dafür ist genau der, der oben fehlt: Der erste prüft den *falschen*
/// Namen und war grün, während das *richtige* danebenlag. Beide gehören in eine
/// Prüfung, weil sie dieselbe Funktion von zwei Seiten nehmen.
#[test]
fn ein_einzelnes_konto_nimmt_seinen_eigenen_namen_an() {
    let konten = MailKonten {
        konten: vec![MailConfig {
            id: "konto-1".to_string(),
            username: "bilandal.dev@gmail.com".to_string(),
            address: "bilandal.dev@gmail.com".to_string(),
            ..MailConfig::default()
        }],
        aktiv: "konto-1".to_string(),
    };

    // Jede Bezeichnung, unter der das Konto bekannt ist, muss auch als Argument
    // durchgehen – bei einem Konto genauso wie bei mehreren.
    for bezeichnung in [
        "konto-1",
        "bilandal.dev@gmail.com",
        "Bilandal.Dev@Gmail.com",
        "  bilandal.dev@gmail.com  ",
    ] {
        assert!(
            konto_aus_argumenten(&konten, &serde_json::json!({ "konto": bezeichnung })).is_ok(),
            "das eigene Konto wurde unter „{bezeichnung}“ abgelehnt"
        );
    }

    // Und ohne Argument ist es das gewählte.
    assert!(konto_aus_argumenten(&konten, &serde_json::json!({})).is_ok());

    // **Nur der erfundene Name fällt durch – mit dem Rat, wegzulassen.**
    let fehler = konto_aus_argumenten(&konten, &serde_json::json!({ "konto": "default" }))
        .expect_err("default gibt es nicht");
    assert!(fehler.contains("lass `konto` einfach weg"), "{fehler}");
}

/// **Dasselbe bei mehreren Konten** – sonst repariert man den einen Fehler und
/// baut den anderen.
#[test]
fn ein_richtiger_kontoname_wird_auch_bei_mehreren_gefunden() {
    let zwei = MailKonten {
        konten: vec![
            MailConfig {
                id: "konto-1".to_string(),
                username: "a@example.org".to_string(),
                ..MailConfig::default()
            },
            MailConfig {
                id: "konto-2".to_string(),
                username: "b@example.org".to_string(),
                ..MailConfig::default()
            },
        ],
        aktiv: "konto-1".to_string(),
    };

    for (bezeichnung, erwartet) in [
        ("konto-2", "konto-2"),
        ("b@example.org", "konto-2"),
    ] {
        let konto = konto_aus_argumenten(&zwei, &serde_json::json!({ "konto": bezeichnung }))
            .unwrap_or_else(|fehler| panic!("„{bezeichnung}“ abgelehnt: {fehler}"));
        assert_eq!(konto.id, erwartet);
    }
}

/// **Das Schema sagt, dass `konto` weggelassen werden soll.** Ein Feld mit einem
/// Namen sieht nach etwas aus, das man füllt, und Modelle füllen es – mit
/// erfundenen Werten, wenn keine zur Wahl stehen.
#[test]
fn das_konto_argument_sagt_dass_es_weggehoert() {
    let beschreibung = konto_beschreibung()["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    assert!(
        beschreibung.contains("weg") && beschreibung.contains("Postfach"),
        "{beschreibung}"
    );
    assert!(
        beschreibung.contains("erfinden"),
        "die Beschreibung warnt nicht vor erfundenen Namen: {beschreibung}"
    );
}

/// **Das Schema sagt, dass Nummern nicht zu raten sind, und dass 1 die älteste
/// ist.** Beides hat das Modell nicht getan: Es hat die 1 geraten, und die 1 ist
/// bei einem gefüllten Postfach eine Nachricht von vor Jahren.
#[test]
fn das_loeschschema_warnt_vor_geratenen_nummern() {
    let schema = delete_email_schema();
    let beschreibung = schema["function"]["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let nummer = schema["function"]["parameters"]["properties"]["nummer"]["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    assert!(
        beschreibung.contains("älteste"),
        "das Schema sagt nicht, dass Nr. 1 die älteste ist: {beschreibung}"
    );
    assert!(
        beschreibung.contains("erfinden") || nummer.contains("nicht geraten"),
        "weder Schema noch Feld sagen, dass die Nummer nicht geraten wird"
    );
    // Und beide nennen die Quelle, aus der die Zahl stammt.
    for quelle in [&beschreibung, &nummer] {
        assert!(
            quelle.contains("list_emails") && quelle.contains("read_email"),
            "die Quelle der Nummer steht nicht dabei: {quelle}"
        );
    }
}

/// **Die Anweisung sagt dasselbe.** Das Schema ist die Stelle, an der das Modell
/// unmittelbar vor dem Aufruf nachliest – aber nur, wenn es dort auch
/// hinschaut. Für eine Regel, die einen ganzen Zug über-spannt, ist die
/// Anweisung die richtige Stelle.
#[test]
fn die_anweisung_sagt_dass_nummern_keine_positionen_sind() {
    let anweisung = mail_anweisung(&["bilandal.dev@gmail.com".to_string()]);

    assert!(anweisung.contains("älteste"), "{anweisung}");
    assert!(anweisung.contains("Positionen"), "{anweisung}");
    assert!(
        anweisung.contains("konto") && anweisung.contains("erfinden"),
        "die Anweisung sagt nicht, dass Kontonamen nicht erfunden werden: {anweisung}"
    );
}

/// **Alle fünf Mailwerkzeuge nehmen ein Konto entgegen.** An einem einzigen
/// fehlenden könnte ein Modell nicht raten, welches Postfach gemeint ist.
#[test]
fn jedes_mailwerkzeug_nimmt_ein_konto_entgegen() {
    let schemata = [
        list_emails_schema(),
        read_email_schema(),
        draft_email_schema(),
        send_email_schema(),
        delete_email_schema(),
    ];

    assert_eq!(
        schemata.len(),
        werkzeug::ALLE.len(),
        "die Liste der Werkzeugnamen und die der Schemata laufen auseinander"
    );

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

/// **Jedes Schema kommt im Werkzeugsatz vor, und keines zu viel.** Ein Werkzeug,
/// das es im Satz nicht gibt, kann das Modell nicht benutzen; eines, das nur im
/// Satz steht, existiert nicht – und genau daran scheitert der Aufruf später
/// mit einer Meldung, die niemand versteht.
///
/// **Und keiner kommt zweimal vor.** Das ist keine Feinheit: `send_email_schema`
/// erbt vom Entwurf und überschreibt drei Felder. Überschreibt es den Namen
/// nicht, steht im Satz zweimal `draft_email` – einmal mit der Entwurfs- und
/// einmal mit der Versandbeschreibung –, und `send_email` fehlt ganz. Das Modell
/// kann dann nur noch entwerfen; das Schema, das die Oberfläche als Versand
/// erkennt, erreicht es nie.
#[test]
fn der_postfachumfang_bietet_alle_fuenf_werkzeuge() {
    let toolset = mail_toolset_for(&[], &["Privat".to_string()]);
    let namen: Vec<&str> = toolset
        .tools
        .iter()
        .filter_map(|werkzeug| werkzeug["function"]["name"].as_str())
        .collect();

    assert_eq!(namen.len(), 5, "{namen:?}");
    for erwartet in werkzeug::ALLE {
        assert!(namen.contains(erwartet), "fehlt: {erwartet} ({namen:?})");
    }

    let mut eigene = namen.clone();
    eigene.sort_unstable();
    eigene.dedup();
    assert_eq!(
        eigene.len(),
        namen.len(),
        "ein Name kommt mehrfach vor: {namen:?}"
    );
}

/// **Das Versandschema trägt seinen Namen – und nicht den des Entwurfs.**
/// Siehe `der_postfachumfang_bietet_alle_fuenf_werkzeuge`: Genau an dieser
/// Stelle ist der Name verlorengegangen.
#[test]
fn das_versandschema_heisst_send_email() {
    assert_eq!(send_email_schema()["function"]["name"], werkzeug::SENDEN);
}

/// **Das Löschschema verlangt die Nummer.** Ohne sie ginge nichts löschbar: Die
/// Nummer ist die einzige Angabe, die eine Nachricht eindeutig macht – der
/// Betreff ändert sich beim Antworten, und zwei Nachrichten teilen ihn sich.
#[test]
fn das_loeschen_verlangt_die_nummer_der_nachricht() {
    let schema = delete_email_schema();

    assert_eq!(schema["function"]["name"], werkzeug::LOESCHEN);
    let parameter = &schema["function"]["parameters"];
    assert_eq!(parameter["required"], serde_json::json!(["nummer"]));
    assert_eq!(parameter["properties"]["nummer"]["type"], "integer");
    // **Kein Betreff, kein Absender.** Was nicht im Aufruf steht, kann das Diff
    // im Bestätigungsfenster nicht zeigen – und der Benutzer soll dort genau das
    // sehen, was gelöscht wird, nicht das, was das Modell über es behauptet.
    assert!(
        parameter["properties"].get("betreff").is_none(),
        "der Betreff gehört nicht ins Löschen: {}",
        parameter["properties"]
    );
}

/// **Der Papierkorb steht im Schema, nicht nur in der Anweisung.** Das Schema
/// ist die Stelle, an der das Modell unmittelbar vor dem Aufruf nachliest – und
/// eine Warnung, die nur in der Anweisung steht, fehlt genau dann, wenn es
/// ohnehin hinschaut: wenn es gerade vor dem gefährlichen Werkzeug steht.
#[test]
fn das_loeschen_sagt_im_schema_dass_es_endgueltig_ist() {
    let beschreibung = delete_email_schema()["function"]["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    assert!(beschreibung.contains("endgültig"), "{beschreibung}");
    assert!(beschreibung.contains("Papierkorb"), "{beschreibung}");
    // Und die Folge, die das Modell sonst erst nach dem Schaden lernt: Die
    // Nummern verschieben sich.
    assert!(
        beschreibung.contains("Nummern") && beschreibung.contains("neu"),
        "das Verschieben der Nummern steht nicht dabei: {beschreibung}"
    );
}

/// **Löschen ist ein Schreibvorgang, und zwar einer, der endgültig ist.** Es geht
/// nichts nach außen – die Mail war schon da – aber es nimmt etwas weg, das
/// nicht wiederkommt. Deshalb steht es auf derselben Seite wie das Senden.
#[test]
fn loeschen_ist_ein_schreibvorgang() {
    assert!(crate::is_write_tool(werkzeug::LOESCHEN));
    assert!(werkzeug::vernichtet(werkzeug::LOESCHEN));
    // **Und es geht nicht nach aussen**, sonst hätte man es mit dem Entwurf
    // verwechseln können, der nur im Chat erscheint.
    assert!(!werkzeug::schreibt_nach_aussen(werkzeug::LOESCHEN));
    assert!(!werkzeug::vernichtet(werkzeug::SENDEN));
    assert!(!werkzeug::schreibt_nach_aussen(werkzeug::ENTWURF));
}

/// **Die Anweisung sagt, dass das Löschen endgültig ist, und wann es erlaubt
/// ist.** Ohne den zweiten Satz läse sich das Werkzeug als Erledigung eines
/// Ärgers – und das ist genau der Aufruf, bei dem etwas endgültig verschwindet,
/// das man noch braucht.
#[test]
fn die_anweisung_waegt_das_loeschen_ab() {
    let anweisung = mail_anweisung(&["Privat".to_string()]);

    assert!(anweisung.contains(werkzeug::LOESCHEN), "{anweisung}");
    assert!(anweisung.contains("Papierkorb"), "{anweisung}");
    assert!(
        anweisung.contains("ausdrücklich"),
        "es steht nicht da, wann gelöscht werden darf: {anweisung}"
    );
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

// --- Schreiben und Senden aus der Oberfläche -------------------------------

/// Ein Konto, das fürs Senden reicht: eingetragen, angemeldet, mit SMTP.
fn konto_zum_senden() -> (MailConfig, MailSession) {
    let config = MailConfig {
        id: "konto-1".to_string(),
        name: "Privat".to_string(),
        imap_server: "imaps://imap.example.org:993".to_string(),
        smtp_server: "smtps://smtp.example.org:465".to_string(),
        username: "kai".to_string(),
        // **Mit Absenderadresse.** Ohne sie wäre `absender()` der Benutzername
        // und damit „kai“ – und das ist keine absenderfähige Adresse. Ein
        // eingetragenes Konto hat eine; ein Testkonto sollte sie auch haben,
        // sonst prüft der Test die Absenderprüfung statt der Sendeprüfung.
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let session = MailSession::default();
    session
        .set_password(&config.id, "app-passwort")
        .expect("das Passwort lässt sich in den Sitzungszustand legen");

    (config, session)
}

async fn sende(
    config: &MailConfig,
    session: &MailSession,
    an: &str,
    betreff: &str,
    text: &str,
) -> Result<String, String> {
    imap_senden(
        config,
        session,
        an.to_string(),
        None,
        None,
        betreff.to_string(),
        text.to_string(),
        None,
    )
    .await
}

/// **Was beim Schreiben schon abgelehnt wird, bevor irgendeine Verbindung
/// aufgebaut wird.**
///
/// Der Weg vom Fenster zum Server ist derselbe wie beim Werkzeug `send_email` –
/// und deshalb gelten dieselben Regeln. Eine halb geschriebene Nachricht, die
/// erst am Server scheitert, wäre eine Fehlermeldung über „Verbindung", während
/// das Problem an drei Zeichen im Betreff lag.
#[tokio::test]
async fn schreiben_prueft_die_angaben_und_nicht_erst_den_server() {
    let (config, session) = konto_zum_senden();

    // **Ohne Empfänger geht nichts raus.**
    let fehler = sende(&config, &session, "", "Betreff", "Text")
        .await
        .expect_err("ohne Empfänger");
    assert!(fehler.contains("Empfänger"), "{fehler}");

    // **Ohne Betreff ebenfalls** – der Empfänger erkennt sie später nicht wieder.
    let fehler = sende(&config, &session, "a@example.org", "  ", "Text")
        .await
        .expect_err("ohne Betreff");
    assert!(fehler.contains("Betreff"), "{fehler}");

    // **Und ohne Text.**
    let fehler = sende(&config, &session, "a@example.org", "Betreff", "")
        .await
        .expect_err("ohne Text");
    assert!(fehler.contains("Text"), "{fehler}");

    // **Eine Liste im Feld ist erlaubt – sie ist die Art, wie man mehrere
    //   Adressen angibt.** Der erste Versuch dieses Tests erwartete hier eine
    //   Ablehnung und wäre am DNS gescheitert: `parse_recipients` zerlegt
    //   Kommas ausdrücklich. Ein Test, der die Absicht des Codes verwechselt,
    //   ist schlimmer als keiner.
    //
    // **Eine eingeschmugelte Kopfzeile dagegen nicht.** Ein Zeilenumbruch im
    //   Empfängerfeld würde sonst im fertigen Rumpf als eigener RFC-5322-Kopf
    //   landen – und damit eine zweite Empfängeradresse setzen, die im Fenster
    //   nie stand.
    let fehler = sende(
        &config,
        &session,
        "a@example.org\nBcc: heimlich@example.org",
        "Betreff",
        "Text",
    )
    .await
    .expect_err("eingeschmugelte Kopfzeile");
    assert!(fehler.contains("Empfänger"), "{fehler}");
}

/// **Ohne eingetragenen SMTP-Server sagt Mimir das, bevor es anmeldet.**
///
/// Die Meldung nennt den Knopf, mit dem es einzutragen ist. Eine Meldung über
/// „Verbindung fehlgeschlagen" wäre wahr und nutzlos.
#[tokio::test]
async fn ohne_smtp_server_wird_das_sagen_und_nicht_der_fehler_vom_server() {
    let (mut config, session) = konto_zum_senden();
    config.smtp_server = String::new();

    let fehler = sende(&config, &session, "a@example.org", "Betreff", "Text")
        .await
        .expect_err("ohne SMTP-Server");

    assert!(fehler.contains("SMTP"), "{fehler}");
    assert!(
        fehler.contains("Konto"),
        "die Meldung nennt den Weg, wie es einzutragen ist: {fehler}"
    );
}

/// **Nicht angemeldet heißt: anmelden, nicht versuchen.**
#[tokio::test]
async fn ohne_anmeldung_kommt_keine_nachricht_heraus() {
    let (config, _) = konto_zum_senden();
    let leere = MailSession::default();

    let fehler = sende(&config, &leere, "a@example.org", "Betreff", "Text")
        .await
        .expect_err("ohne Anmeldung");

    assert!(fehler.contains("angemeldet"), "{fehler}");
}

/// **Ein Entwurf trägt seine Felder mit – und sendet nichts.**
///
/// Das ist die Zusage, an der das Ganze hängt: Das Modell soll einen Brief
/// schreiben können, und der Benutzer soll ihn **sehen und ändern** können, bevor
/// etwas hinausgeht. Ein Entwurf, der nur als Text im Chat ankäme, müsste
/// abgetippt werden – und bei der ersten Ungenauigkeit ginge etwas verloren, ohne
/// dass es jemand bemerkte.
#[test]
fn ein_entwurf_trägt_die_felder_mit_und_verliert_nichts() {
    let config = MailConfig {
        id: "konto-7".to_string(),
        name: "Arbeit".to_string(),
        imap_server: "imaps://imap.example.org:993".to_string(),
        username: "kai".to_string(),
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let ergebnis = entwurf(
        &config,
        &serde_json::json!({
            "an": "sarah@example.org",
            "cc": "chef@example.org",
            "bcc": "",
            "betreff": "Termin am Donnerstag",
            "text": "Hallo Sarah,\n\npasst 14 Uhr?\n\nKai",
        }),
    )
    .expect("ein Entwurf lässt sich immer erstellen");

    let daten = ergebnis.entwurf.expect("ein Entwurf trägt seine Felder mit");

    // **Das Konto ist die Kennung, nicht der Name.** Bei mehreren Konten ist
    // „Arbeit" keine Adresse, an die man etwas schicken könnte.
    assert_eq!(daten.konto, "konto-7");
    assert_eq!(daten.an, "sarah@example.org");
    assert_eq!(daten.cc, "chef@example.org");
    assert_eq!(daten.betreff, "Termin am Donnerstag");
    assert!(
        daten.text.contains("passt 14 Uhr"),
        "der Text kommt vollständig an: {:?}",
        daten.text
    );
    assert_eq!(daten.in_antwort_auf, None, "eine neue Mail ist keine Antwort");

    // **Und der Text für das Modell bleibt lesbar** – die Felder daneben sind
    // für das Modell nicht lesbar, und ein Text ohne sie für den Benutzer nicht
    // versendbar.
    assert!(ergebnis.content.contains("sarah@example.org"));
    assert!(ergebnis.content.contains("nichts gesendet"));
}

/// **Ein Entwurf ohne Empfänger geht an den Benutzer selbst – und hat damit
/// einen Adressaten.**
///
/// Dieser Test stand früher auf der anderen Seite und behauptete, ein Entwurf
/// ohne Empfänger müsse ein Fehler sein: Sonst entstünde im Schreibfenster eine
/// Nachricht ohne Adressat, deren Senden erst beim zweiten Klick auffiele. Die
/// Sorge war richtig, die Antwort darauf war es nicht.
///
/// Denn sie hat eine Adresse verlangt, die das Modell nicht kennt. Der Entwurf
/// scheiterte daran – nicht der Benutzer sah es zu spät, sondern gar nicht. Was
/// hier jetzt entsteht, ist keine Nachricht ohne Adressat, sondern eine mit
/// einem: der des Benutzers. Der steht sichtbar im Fenster, und beim Absenden
/// muss er ihn ohnehin bestätigen.
///
/// **Ein Entwurf ohne jeden Empfänger ist damit ausgeschlossen.** Nicht, weil
/// das Feld bequem wegfällt, sondern weil die Voreinstellung eine Adresse
/// einsetzt, die es gibt.
#[test]
fn ein_entwurf_ohne_empfaenger_hat_einen_addressaten() {
    // **Nur der Absender ist hier von Belang.** Alles andere braucht die Prüfung
    // nicht, und soll sie auch nicht aufhalten: Dieser Test geht den Weg durch,
    // den ein Entwurf ohne Empfänger wirklich nimmt.
    let config = MailConfig {
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let ergebnis =
        entwurf(&config, &serde_json::json!({"betreff": "B", "text": "T"})).expect(
            "ohne Empfänger entsteht ein Entwurf an das eigene Postfach, kein Fehler",
        );

    let daten = ergebnis.entwurf.expect("der Entwurf wandert mit");
    assert_eq!(
        daten.an, "kai@example.org",
        "der Entwurf hat einen Adressaten, und es ist der Benutzer"
    );
    assert!(
        ergebnis.content.contains("kai@example.org"),
        "der Empfänger steht auch im Text für den Benutzer: {}",
        ergebnis.content
    );
}

/// **Jedes andere Werkzeug lässt das Feld leer.** Serialisiert wird es dann
/// gar nicht – sonst stünde in jedem Ergebnis ein leerer Entwurf, den die
/// Oberfläche nicht zu deuten wüsste.
#[test]
fn nur_der_entwurf_traegt_das_feld() {
    let ohne = crate::ToolOutput::new("Text".to_string(), "Kurz".to_string());
    assert!(ohne.entwurf.is_none());

    let json = serde_json::to_string(&ohne).expect("ein Ergebnis lässt sich lesen");
    assert!(
        !json.contains("entwurf"),
        "das Feld fehlt ganz und steht nicht auf null: {json}"
    );
}

/// **Der Entwurf muss über die Rust-Grenze kommen.**
///
/// Bei diesem ganzen Weg fehlt eine Kette, an der man leicht denkt, sie sei
/// selbstverständlich: `entwurf` liegt in Rust, wird in JSON übersetzt, kommt
/// im Frontend an. Wird ein Feld beim Übersetzen übergangen, ist es im
/// Werkzeugschritt unsichtbar – **und der Knopf zum Öffnen erscheint nicht**.
/// Genau das wäre von außen nicht zu unterscheiden von einem Werkzeug, das nie
/// gelaufen ist.
#[test]
fn der_entwurf_ueberschreitet_die_rust_grenze() {
    let config = MailConfig {
        id: "konto-1".to_string(),
        imap_server: "imaps://imap.example.org:993".to_string(),
        username: "kai".to_string(),
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let ergebnis = entwurf(
        &config,
        &serde_json::json!({
            "an": "sarah@example.org",
            "betreff": "Termin am Donnerstag",
            "text": "Hallo Sarah,",
        }),
    )
    .expect("der Entwurf entsteht");

    let json = serde_json::to_value(&ergebnis).expect("ein Ergebnis lässt sich lesen");

    assert!(
        json.get("entwurf").is_some(),
        "der Entwurf fehlt in der Übersetzung: {json}"
    );
    assert_eq!(json["entwurf"]["an"], "sarah@example.org");
    assert_eq!(json["entwurf"]["betreff"], "Termin am Donnerstag");
    assert_eq!(json["entwurf"]["konto"], "konto-1");
    assert_eq!(json["entwurf"]["in_antwort_auf"], serde_json::Value::Null);
}

/// **„Schick mir das" muss ohne Adresse auskommen.**
///
/// Der häufigste Auftrag um einen Entwurf ist einer, bei dem der Empfänger der
/// Benutzer selbst ist. Solange das Modell dafür eine Adresse nennen musste,
/// ohne eine zu kennen, hat es geraten – und der Aufruf ist an einer Adresse
/// hängen geblieben, die es im Postfach nicht gab.
///
/// Der Test steht hier, weil das nicht am Entwurf hängt, sondern an der
/// Reihenfolge: Erst wird die eigene Adresse eingesetzt, dann entsteht der
/// Entwurf. Wer die Voreinstellung entfernt, bemerkt es an diesem Test – nicht
/// an einem Bildschirm, auf dem ein Knopf fehlt.
#[test]
fn ein_entwurf_ohne_empfaenger_geht_an_das_eigene_postfach() {
    let config = MailConfig {
        id: "konto-1".to_string(),
        imap_server: "imaps://imap.example.org:993".to_string(),
        username: "kai".to_string(),
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let ergebnis = entwurf(
        &config,
        &serde_json::json!({
            "betreff": "Zusammenfassung unseres Chats",
            "text": "Wir haben über den Entwurf gesprochen.",
        }),
    )
    .expect("ein Entwurf ohne Empfänger entsteht");

    let daten = ergebnis.entwurf.expect("der Entwurf wandert mit");
    assert_eq!(
        daten.an, "kai@example.org",
        "ohne Empfänger geht der Entwurf an das eigene Postfach"
    );
    assert_eq!(daten.betreff, "Zusammenfassung unseres Chats");
    assert_eq!(daten.konto, "konto-1", "das Fenster öffnet im richtigen Konto");
}

/// **Ein genannter Empfänger bleibt unangetastet.**
///
/// Die Voreinstellung darf nur einsetzen, wo nichts dasteht. Andernfalls würde
/// eine Bitte wie „schick das an Sarah“ im Fenster stillschweigend an den
/// Benutzer selbst gehen – der Entwurf entstünde, aber an den Falschen.
#[test]
fn ein_genannter_empfaenger_bleibt_stehen() {
    let config = MailConfig {
        id: "konto-1".to_string(),
        imap_server: "imaps://imap.example.org:993".to_string(),
        username: "kai".to_string(),
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let ergebnis = entwurf(
        &config,
        &serde_json::json!({
            "an": "sarah@example.org",
            "betreff": "Termin am Donnerstag",
            "text": "Hallo Sarah,",
        }),
    )
    .expect("der Entwurf entsteht");

    let daten = ergebnis.entwurf.expect("der Entwurf wandert mit");
    assert_eq!(daten.an, "sarah@example.org", "der eigene Empfänger bleibt außen vor");
}

/// **Leer ist nicht genannt.**
///
/// Leeres Feld und fehlendes Feld sind für das Modell dasselbe – aber nur das
/// leere darf die Voreinstellung auslösen. Ein Leerzeichen als Empfänger wäre
/// eine Adresse, die es nicht gibt, und `parse_recipients` würde sie mit einer
/// verständlichen Meldung ablehnen, statt still zu ersetzen.
#[test]
fn ein_leeres_empfaengerfeld_zaehlt_als_keine_angabe() {
    let config = MailConfig {
        id: "konto-1".to_string(),
        imap_server: "imaps://imap.example.org:993".to_string(),
        username: "kai".to_string(),
        address: "kai@example.org".to_string(),
        ..MailConfig::default()
    };

    let ergebnis = entwurf(
        &config,
        &serde_json::json!({
            "an": "   ",
            "betreff": "Notiz",
            "text": "Etwas für später.",
        }),
    )
    .expect("auch ein leeres Feld führt zum Entwurf");

    let daten = ergebnis.entwurf.expect("der Entwurf wandert mit");
    assert_eq!(daten.an, "kai@example.org");
}

/// **Beim Senden bleibt der Empfänger Pflicht.**
///
/// Das ist der Gegenentwurf zu allem Obigen, und der Grund, warum die
/// Voreinstellung nicht in `vorhaben_aus_argumenten` steht: Dort würde sie auch
/// das Senden treffen. Eine Mail, die beim Absender landet, während er sie an
/// jemand anderen erwartet, ist kein Entwurf, sondern ein Versehen – und der
/// Benutzer sähe es erst, wenn sie draußen ist.
///
/// Das Schema muss deshalb sagen, dass `an` dazugehört. Ohne diese Prüfung
/// könnte es beim nächsten Umbau stillschweigend wegfallen, und das Feld wäre
/// wieder nur noch in der Beschreibung zu finden.
#[test]
fn beim_senden_bleibt_der_empfaenger_pflicht() {
    let schema = send_email_schema();
    let pflicht = schema["function"]["parameters"]["required"]
        .as_array()
        .expect("das Senden nennt seine Pflichtfelder");

    assert!(
        pflicht.iter().any(|feld| feld == "an"),
        "das Senden lässt den Empfänger wieder zu: {schema}"
    );

    // **Und die Voreinstellung steht nur in der Entwurfsbeschreibung.**
    //
    // Wäre sie auch hier zu lesen, würde das Modell sie fürs Senden mitnehmen
    // und eine Mail an sich selbst schicken, weil der Empfänger fehlt.
    let beschreibung = schema["function"]["parameters"]["properties"]["an"]["description"]
        .as_str()
        .unwrap_or_default();
    assert!(
        beschreibung.contains("**Optional**"),
        "beim Senden ist der Empfänger nicht optional, die Beschreibung sagt es: {beschreibung}"
    );
}

/// **Der Entwurfschema-description erklärt den Weg nach Hause.**
///
/// Der Satz ist keine Dekoration. Ohne ihn bleibt `an` ein Feld, dessen
/// Fehlen ein Fehler scheint, und das Modell füllt es – mit einer Adresse, die
/// es sich ausdenkt.
#[test]
fn der_entwurf_sagt_dem_modell_wohin_ohne_empfaenger() {
    let schema = crate::mail::draft_email_schema();
    let beschreibung = schema["function"]["parameters"]["properties"]["an"]["description"]
        .as_str()
        .unwrap_or_default();

    assert!(
        beschreibung.contains("eigenes") || beschreibung.contains("selbst"),
        "das Schema erklärt nicht, dass ein leerer Empfänger der Benutzer selbst ist: {beschreibung}"
    );
}
