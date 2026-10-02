//! Tests für URL-Prüfung, Sitzungszustand und die DAV-Antworten. Für die
//! Netzpfade gibt es einen Nachbau in `client.rs::tests`.

use super::*;

#[test]
fn the_address_is_completed_and_cleaned() {
    assert_eq!(
        normalize_server_url(" 192.168.2.176:8080/ ").unwrap(),
        "http://192.168.2.176:8080"
    );
    assert_eq!(
        normalize_server_url("http://cloud.lan/nextcloud/").unwrap(),
        "http://cloud.lan/nextcloud"
    );
    assert_eq!(
        normalize_server_url("http://192.168.2.1").unwrap(),
        "http://192.168.2.1"
    );
    // HTTPS bleibt erhalten, das Schema wird nicht umgeschrieben.
    assert_eq!(
        normalize_server_url("https://cloud.example.org/nextcloud").unwrap(),
        "https://cloud.example.org/nextcloud"
    );
    // Port 80 ist die Vorgabe und wird nicht geschrieben.
    assert_eq!(
        normalize_server_url("http://192.168.2.1:80").unwrap(),
        "http://192.168.2.1"
    );
}

#[test]
fn a_broken_address_is_refused_with_a_reason() {
    for (input, needle) in [
        ("", "nicht leer"),
        ("   ", "nicht leer"),
        // https ist erlaubt, weil viele Instanzen im LAN nur HTTPS liefern.
        ("ftp://192.168.2.1", "http oder https"),
        ("nextcloud://192.168.2.1", "http oder https"),
        ("192.168.2.1?a=b", "Abfrage"),
        ("192.168.2.1#teil", "Sprung"),
        ("192.168.2.1:0", "65535"),
        ("user@192.168.2.1", "Zugangsdaten"),
        ("http://user:pass@192.168.2.1", "Zugangsdaten"),
        ("http://192.168.2.1 mit leerzeichen", "Leerzeichen"),
        ("/nur/pfad", "Hostnamen"),
        ("0.0.0.0", "eindeutig"),
    ] {
        let error = normalize_server_url(input)
            .expect_err(&format!("„{input}“ hätte abgelehnt werden müssen"));
        assert!(
            error.contains(needle),
            "bei „{input}“ steht „{error}“ statt eines Hinweises auf „{needle}“"
        );
    }
}

#[test]
fn a_too_long_address_is_refused() {
    let input = format!("192.168.2.1/{}", "a".repeat(MAX_DAV_URL_BYTES));
    assert!(normalize_server_url(&input).is_err());
}

#[test]
fn a_full_webdav_address_from_nextcloud_is_understood() {
    // Genau die Adresse, die in den Nextcloud-Einstellungen steht: Sie zeigt auf
    // den eigenen Principal, nicht auf den Sammelpfad der Kalender. Ungeprüft
    // übernommen hätte Mimir daraus
    // `…/principals/users/<name>/calendars/…` gebaut und nichts gefunden.
    let kopiert = CalendarConfig {
        server_url: "https://cloud.example.org/remote.php/dav/principals/users/benutzer".to_string(),
        username: String::new(),
        calendars: Vec::new(),
        server_certificate: None,
    };
    assert_eq!(kopiert.instance_base(), "https://cloud.example.org");
    assert_eq!(kopiert.username_from_dav_path().as_deref(), Some("benutzer"));
    assert_eq!(
        kopiert.dav_root().unwrap(),
        "https://cloud.example.org/remote.php/dav"
    );

    // Der Name wird beim Anmelden in den Sammelpfad übernommen.
    let mit_name = CalendarConfig {
        username: "Kai".to_string(),
        ..kopiert.clone()
    };
    assert_eq!(
        mit_name.calendars_root().unwrap(),
        "https://cloud.example.org/remote.php/dav/calendars/Kai/"
    );

    // Prozentkodierte Namen und ein Unterverzeichnis funktionieren ebenso.
    let kodiert = CalendarConfig {
        server_url: "https://cloud.lan/nextcloud/remote.php/dav/principals/users/j%C3%BCrgen"
            .to_string(),
        ..kopiert.clone()
    };
    assert_eq!(kodiert.instance_base(), "https://cloud.lan/nextcloud");
    assert_eq!(kodiert.username_from_dav_path().as_deref(), Some("jürgen"));

    // Eine Adresse, die selbst schon DAV-Wurzel ist, trägt keinen Namen.
    let wurzel = CalendarConfig {
        server_url: "https://cloud.lan/nextcloud/remote.php/dav".to_string(),
        ..kopiert.clone()
    };
    assert_eq!(wurzel.instance_base(), "https://cloud.lan/nextcloud");
    assert_eq!(wurzel.username_from_dav_path(), None);

    // Und eine reine Instanzadresse ebenso wenig.
    let instanz = CalendarConfig {
        server_url: "https://cloud.example.org".to_string(),
        ..kopiert
    };
    assert_eq!(instanz.username_from_dav_path(), None);
    assert_eq!(
        instanz.dav_root().unwrap(),
        "https://cloud.example.org/remote.php/dav"
    );
}

#[test]
fn the_dav_paths_are_built_from_the_address() {
    let config = CalendarConfig {
        server_url: "http://192.168.2.176:8080".to_string(),
        username: "kai".to_string(),
        calendars: Vec::new(),
        server_certificate: None,
    };
    assert_eq!(
        config.dav_root().unwrap(),
        "http://192.168.2.176:8080/remote.php/dav"
    );
    assert_eq!(
        config.calendars_root().unwrap(),
        "http://192.168.2.176:8080/remote.php/dav/calendars/kai/"
    );
}

#[test]
fn an_address_that_already_points_at_dav_is_left_alone() {
    let config = CalendarConfig {
        server_url: "http://cloud.lan/nextcloud/remote.php/dav".to_string(),
        username: "kai".to_string(),
        calendars: Vec::new(),
        server_certificate: None,
    };
    assert_eq!(
        config.dav_root().unwrap(),
        "http://cloud.lan/nextcloud/remote.php/dav"
    );
}

#[test]
fn a_benutzername_with_a_slash_cannot_escape_the_path() {
    let config = CalendarConfig {
        server_url: "http://192.168.2.1".to_string(),
        username: "../../admin".to_string(),
        calendars: Vec::new(),
        server_certificate: None,
    };
    assert!(
        config.calendars_root().is_err(),
        "Pfad-Ausbruch nicht verhindert"
    );
}

#[test]
fn the_username_is_checked_before_it_is_used() {
    assert!(validate_username("kai").is_ok());
    assert!(validate_username(" kai ").is_ok());
    assert!(validate_username("").is_err());
    assert!(validate_username(" ").is_err());
    assert!(validate_username("a/b").is_err());
    assert!(validate_username("kai\nx").is_err());
    assert!(validate_username(&"a".repeat(200)).is_err());
}

#[test]
fn the_password_stays_in_memory_and_is_forgettable() {
    let session = CalendarSession::default();
    assert!(!session.is_logged_in());
    assert!(session.password().is_none());

    session.set_password("geheim123").unwrap();
    assert!(session.is_logged_in());
    assert_eq!(session.password().unwrap().as_str(), "geheim123");

    session.forget();
    assert!(!session.is_logged_in());
    assert!(session.password().is_none());
}

#[test]
fn an_empty_or_broken_password_is_refused() {
    let session = CalendarSession::default();
    assert!(session.set_password("").is_err());
    assert!(session.set_password("   ").is_err());
    assert!(session.set_password("mit\nZeilenumbruch").is_err());
    assert!(session.set_password(&"a".repeat(1000)).is_err());
    assert!(!session.is_logged_in());
}

#[test]
fn the_status_tells_the_panel_what_to_show() {
    let session = CalendarSession::default();
    let empty = CalendarConfig::default();
    let status = build_status(&empty, &session, None);
    assert!(!status.configured);
    assert!(!status.logged_in);
    assert!(status.known_calendars.is_empty());
    assert!(status.last_success.is_none());

    let config = CalendarConfig {
        server_url: "http://192.168.2.176".to_string(),
        username: "kai".to_string(),
        calendars: vec!["persoenlich".to_string()],
        server_certificate: None,
    };
    session.set_password("geheim").unwrap();
    session.mark_success(1_700_000_000);
    let status = build_status(&config, &session, None);
    assert!(status.configured);
    assert!(status.logged_in);
    assert_eq!(status.calendars, vec!["persoenlich".to_string()]);
    assert_eq!(status.last_success, Some(1_700_000_000));
}

#[test]
fn the_app_password_can_be_stored_with_tight_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let path =
        std::env::temp_dir().join(format!("mimir-credential-test-{}.json", std::process::id()));
    std::fs::remove_file(&path).ok();

    crate::calendar::write_stored_credential(&path, "kai", "app-passwort").unwrap();
    assert!(path.exists(), "das Passwort wurde nicht abgelegt");

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "die Datei ist zu weit lesbar: {mode:o}");
    }

    // Wieder einlesen, wie es beim Start geschieht.
    let stored = crate::calendar::read_stored_credential(&path, "kai").unwrap();
    assert_eq!(stored.app_password, "app-passwort");
    assert_eq!(stored.username, "kai");

    // Zu einem anderen Benutzer gehört es nicht.
    assert!(crate::calendar::read_stored_credential(&path, "jemand-anderes").is_none());

    // Und es lässt sich wieder entfernen.
    crate::calendar::delete_stored_credential(&path).unwrap();
    assert!(!path.exists());
    // Ein zweites Löschen ist kein Fehler.
    crate::calendar::delete_stored_credential(&path).unwrap();
}

#[test]
fn a_broken_credential_file_is_ignored_instead_of_failing() {
    use std::io::Write;

    let path = std::env::temp_dir().join(format!(
        "mimir-credential-defekt-{}.json",
        std::process::id()
    ));

    let mut datei = std::fs::File::create(&path).unwrap();
    datei.write_all(b"kein json").unwrap();
    drop(datei);

    assert!(crate::calendar::read_stored_credential(&path, "kai").is_none());
    std::fs::remove_file(&path).ok();
}
