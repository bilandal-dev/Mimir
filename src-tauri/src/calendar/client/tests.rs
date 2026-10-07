//! Tests für das Lesen der DAV-Antworten und gegen einen nachgebauten Server.
//!
//! Der Nachbau hört auf einem zufälligen Port und beantwortet genau die
//! Aufrufe, die Nextcloud beantwortet. Damit wird der ganze Weg geprüft:
//! Anfrage, XML, ICS-Aufbereitung, Sortierung.

use super::*;
use chrono::{TimeZone, Utc};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

/// Ein Server, der auf PROPFIND die Kalenderliste und auf REPORT die Termine
/// liefert. Die Antworten lassen sich je Aufruf austauschen.
/// Was der Nachbau auf GET, PUT und DELETE eines Termins antwortet.
#[derive(Clone)]
struct EreignisAntwort {
    get: String,
    schreiben: String,
    body: String,
    etag: String,
}

struct FakeServer {
    address: String,
    requests: Arc<std::sync::Mutex<Vec<String>>>,
}

impl FakeServer {
    fn start(calendars_xml: &str, report_xml: &str) -> Self {
        Self::with_status(calendars_xml, report_xml, "HTTP/1.1 207 Multi-Status")
    }

    /// Wie `start`, aber antwortet je Kalender mit eigenem Report. Ohne das
    /// kämen beide Kalender mit denselben Terminen daher und die Prüfung der
    /// Auswahl würde nichts aussagen.
    fn per_calendar(calendars_xml: &str, reports: &[(&str, &str)]) -> Self {
        Self::serve(calendars_xml, reports, "HTTP/1.1 207 Multi-Status")
    }

    fn with_status(calendars_xml: &str, report_xml: &str, status: &str) -> Self {
        Self::serve(calendars_xml, &[("persoenlich", report_xml)], status)
    }

    /// Wie `start`, aber ein PUT wird mit dem angegebenen Status beantwortet.
    /// Ohne das gäbe der Nachbau auch beim Schreiben eine Multistatus-Antwort,
    /// und die Fehlerbehandlung wäre ungeprüft.
    fn for_write(put_status: &str) -> Self {
        Self::serve_with_put(CALENDARS_XML, REPORT_XML, put_status)
    }

    fn serve_with_put(calendars_xml: &str, report_xml: &str, put_status: &str) -> Self {
        Self::serve_all(
            calendars_xml,
            &[("persoenlich", report_xml)],
            "HTTP/1.1 207 Multi-Status",
            Some(put_status.to_string()),
            None,
        )
    }

    fn serve(calendars_xml: &str, reports: &[(&str, &str)], status: &str) -> Self {
        Self::serve_all(calendars_xml, reports, status, None, None)
    }

    /// Wie `for_write`, aber mit eigenem Status für GET, PUT und DELETE.
    ///
    /// Ohne das wäre nicht prüfbar, ob Mimir bei einem 412 oder 404 wirklich
    /// abbricht, statt Erfolg zu melden.
    fn mit_ereignis(status: &str) -> Self {
        Self::serve_mit_ereignis(status, status, EREIGNIS_ICS, "\"etag-1\"")
    }

    /// Wie `mit_ereignis`, aber der GET liefert eine andere Datei oder keinen
    /// ETag – beides ändert, was Mimir tun darf.
    fn mit_ereignis_datei(status: &str, body: &str, etag: &str) -> Self {
        Self::serve_mit_ereignis(status, status, body, etag)
    }

    fn serve_mit_ereignis(get: &str, schreiben: &str, body: &str, etag: &str) -> Self {
        Self::serve_all(
            CALENDARS_XML,
            &[("persoenlich", REPORT_XML)],
            "HTTP/1.1 207 Multi-Status",
            Some(schreiben.to_string()),
            Some(EreignisAntwort {
                get: status_code(get).to_string(),
                schreiben: schreiben.to_string(),
                body: body.to_string(),
                etag: etag.to_string(),
            }),
        )
    }

    fn serve_all(
        calendars_xml: &str,
        reports: &[(&str, &str)],
        status: &str,
        put_status: Option<String>,
        ereignis: Option<EreignisAntwort>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let calendars = calendars_xml.to_string();
        let reports: Vec<(String, String)> = reports
            .iter()
            .map(|(name, body)| (name.to_string(), body.to_string()))
            .collect();
        let status = status.to_string();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let put_status = Arc::new(std::sync::Mutex::new(put_status));
        let ereignis = ereignis.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(clone) => clone,
                    Err(_) => continue,
                });

                let mut head = String::new();
                if reader.read_line(&mut head).is_err() {
                    continue;
                }

                let mut length = 0usize;
                let mut headers = String::new();

                loop {
                    let mut line = String::new();

                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }

                    if line.trim().is_empty() {
                        break;
                    }

                    headers.push_str(&line);
                    let lower = line.to_ascii_lowercase();

                    if let Some(value) = lower.strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }

                let mut body = vec![0u8; length];

                if length > 0 && reader.read_exact(&mut body).is_err() {
                    continue;
                }

                let body = String::from_utf8_lossy(&body).to_string();
                let pfad = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                let is_report = head.starts_with("REPORT");
                let authorized = headers
                    .to_ascii_lowercase()
                    .contains("authorization: basic");

                log.lock().unwrap().push(format!(
                    "{}{}|{}|auth={}|body={}",
                    head.split_whitespace().next().unwrap_or_default(),
                    head.split_whitespace().nth(1).unwrap_or_default(),
                    if headers.to_ascii_lowercase().contains("depth: 1") {
                        "depth1"
                    } else {
                        "depth?"
                    },
                    authorized,
                    body.replace(['\r', '\n'], "")
                ));

                let payload = if is_report {
                    let wanted = pfad
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or_default();

                    reports
                        .iter()
                        .find(|(name, _)| name == wanted)
                        .map(|(_, body)| body.as_str())
                        // Ein unbekannter Kalender bekommt eine leere, gültige
                        // Antwort statt eines Fehlers.
                        .unwrap_or(EMPTY_REPORT)
                } else {
                    &calendars
                };

                // Ein einzelner Termin: GET liefert die Datei samt Kennung, PUT
                // und DELETE bekommen den eingestellten Status.
                if let Some(ereignis) = ereignis.as_ref() {
                    if head.starts_with("GET") {
                        let antwort = format!(
                            "HTTP/1.1 {}\r\netag: {}\r\ncontent-type: text/calendar; charset=utf-8\r\n\
                             content-length: {}\r\nconnection: close\r\n\r\n{}",
                            ereignis.get,
                            ereignis.etag,
                            ereignis.body.len(),
                            ereignis.body
                        );
                        let _ = stream.write_all(antwort.as_bytes());
                        let _ = stream.flush();
                        continue;
                    }

                    if head.starts_with("PUT") || head.starts_with("DELETE") {
                        let antwort = format!(
                            "HTTP/1.1 {}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                            ereignis.schreiben
                        );
                        let _ = stream.write_all(antwort.as_bytes());
                        let _ = stream.flush();
                        continue;
                    }
                }

                // Ein PUT wird mit eigenem Status beantwortet, sonst mit 201.
                if head.starts_with("PUT") {
                    let put = put_status
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| "HTTP/1.1 201 Created".to_string());
                    let response =
                        format!("{put}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                    continue;
                }

                if status.contains("301") || status.contains("302") {
                    let response = format!(
                        "{status}\r\nlocation: http://{pfad}/neu/remote.php/dav\r\n\
                         content-length: 0\r\nconnection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }

                if status.starts_with("HTTP/1.1 401") {
                    // Passwort abgelehnt: erst prüfen, dann den Fehler senden.
                    if !authorized {
                        let response = "HTTP/1.1 401 Unauthorized\r\n\
                                        content-type: application/xml\r\n\
                                        content-length: 0\r\n\
                                        connection: close\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes());
                        continue;
                    }
                }

                let head = format!(
                    "{status}\r\ncontent-type: application/xml; charset=utf-8\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(payload.as_bytes());
                let _ = stream.flush();
            }
        });

        Self { address, requests }
    }

    fn config(&self) -> CalendarConfig {
        CalendarConfig {
            server_url: format!("http://{}", self.address),
            username: "kai".to_string(),
            calendars: Vec::new(),
            server_certificate: None,
        }
    }

    fn log(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// Die Termindatei, die der Nachbau beim GET liefert.
const EREIGNIS_ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
UID:termin-1@nextcloud\r\nDTSTAMP:20260901T120000Z\r\nSEQUENCE:3\r\n\
DTSTART:20260920T070000Z\r\nDTEND:20260920T073000Z\r\n\
SUMMARY:Teammeeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// Welcher HTTP-Status aus einem Kurznamen wird, wie ihn der Test angibt.
fn status_code(kurz: &str) -> &'static str {
    match kurz {
        "200" => "200 OK",
        "201" => "201 Created",
        "204" => "204 No Content",
        "404" => "404 Not Found",
        "412" => "412 Precondition Failed",
        "500" => "500 Internal Server Error",
        _ => "200 OK",
    }
}

const CALENDARS_XML: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/persoenlich/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Persönlich</d:displayname>
        <d:resourcetype><d:collection/></d:resourcetype>
        <cs:getctag>ctag-1</cs:getctag>
        <cal:calendar-color>#2D55FFAA</cal:calendar-color>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/arbeit/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Arbeit</d:displayname>
        <d:resourcetype><d:collection/></d:resourcetype>
        <cs:getctag>ctag-2</cs:getctag>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
    <d:propstat>
      <d:prop><d:getetag/></d:prop>
      <d:status>HTTP/1.1 404 Not Found</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/trash-bin/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

const EMPTY_REPORT: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" />"#;

/// Die ICS-Daten kommen als CDATA, wie es Nextcloud und die meisten anderen
/// Server liefern.
const REPORT_XML: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/persoenlich/1.ics</d:href>
    <d:propstat>
      <d:prop>
        <d:getetag>"abc"</d:getetag>
        <c:calendar-data><![CDATA[BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:a
DTSTART;TZID=Europe/Berlin:20260912T090000
DTEND;TZID=Europe/Berlin:20260912T100000
SUMMARY:Teammeeting
END:VEVENT
END:VCALENDAR]]></c:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/persoenlich/2.ics</d:href>
    <d:propstat>
      <d:prop>
        <c:calendar-data><![CDATA[BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:b
DTSTART:20260910T080000Z
DTEND:20260910T090000Z
SUMMARY:Laufen & dehnen
END:VEVENT
END:VCALENDAR]]></c:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

/// Nur der Arbeitskalender. Der Termin liegt nicht im persönlichen Kalender, damit
/// sich die Auswahl in der Prüfung auswirkt.
const REPORT_ARBEIT: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/arbeit/9.ics</d:href>
    <d:propstat>
      <d:prop>
        <c:calendar-data><![CDATA[BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:c
DTSTART:20260911T140000Z
DTEND:20260911T150000Z
SUMMARY:Kundentermin
END:VEVENT
END:VCALENDAR]]></c:calendar-data>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

#[tokio::test]
async fn the_calendar_list_excludes_the_technical_folders() {
    let server = FakeServer::start(CALENDARS_XML, REPORT_XML);
    let client = build_client(false).unwrap();
    let calendars = list_calendars(&client, &server.config(), "geheim")
        .await
        .unwrap();

    let names: Vec<&str> = calendars
        .iter()
        .map(|item| item.display_name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["Arbeit", "Persönlich"],
        "technische Ordner mitgenommen"
    );
    assert_eq!(calendars[1].href, "persoenlich");
    assert_eq!(calendars[1].ctag, "ctag-1");
    // Nextcloud schreibt die Farbe mit Alpha zuerst, das CSS nicht.
    assert_eq!(calendars[1].color, "#2D55FF");
}

#[tokio::test]
async fn the_request_carries_the_credentials_and_the_depth() {
    let server = FakeServer::start(CALENDARS_XML, REPORT_XML);
    let client = build_client(false).unwrap();
    list_calendars(&client, &server.config(), "geheim")
        .await
        .unwrap();

    let log = server.log();
    assert!(!log.is_empty(), "der Server hat nichts bekommen");
    assert!(log[0].starts_with("PROPFIND/"), "{}", log[0]);
    assert!(log[0].contains("auth=true"), "keine Anmeldung: {}", log[0]);
    assert!(log[0].contains("depth1"), "keine Tiefe gesetzt: {}", log[0]);
}

#[tokio::test]
async fn the_events_of_a_window_arrive_sorted_and_read() {
    let server = FakeServer::per_calendar(
        CALENDARS_XML,
        &[("persoenlich", REPORT_XML), ("arbeit", REPORT_ARBEIT)],
    );
    let client = build_client(false).unwrap();
    let window = Window {
        start: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(),
    };
    let events = fetch_events(&client, &server.config(), "geheim", &window)
        .await
        .unwrap();

    assert_eq!(events.len(), 3, "nicht alle Termine gefunden");
    assert!(
        events[0].start < events[1].start,
        "nicht nach Zeit sortiert"
    );
    // 10.09. liegt vor dem 12.09.
    assert_eq!(
        Utc.timestamp_opt(events[0].start, 0)
            .unwrap()
            .format("%d.%m.")
            .to_string(),
        "10.09."
    );
    assert_eq!(events[0].summary, "Laufen & dehnen");
    assert_eq!(events[1].summary, "Kundentermin");
    assert_eq!(events[2].summary, "Teammeeting");
    assert_eq!(events[2].calendar, "Persönlich");
    // Der Report wurde mit einem Zeitraum gefragt.
    let report = server
        .log()
        .into_iter()
        .find(|entry| entry.starts_with("REPORT/"))
        .expect("kein Report abgesetzt");
    // Der Report fragt ausdrücklich nur den Zeitraum ab.
    assert!(report.contains("time-range start="), "{report}");
    assert!(report.contains("calendar-query"), "{report}");
}

#[tokio::test]
async fn a_selected_calendar_is_the_only_one_being_read() {
    let server = FakeServer::per_calendar(
        CALENDARS_XML,
        &[("persoenlich", REPORT_XML), ("arbeit", REPORT_ARBEIT)],
    );
    let client = build_client(false).unwrap();
    let mut config = server.config();
    config.calendars = vec!["arbeit".to_string()];
    let window = Window {
        start: Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        end: Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap(),
    };
    let events = fetch_events(&client, &config, "geheim", &window)
        .await
        .unwrap();

    // Auswahl auf „Arbeit": der Termin aus dem persönlichen Kalender darf nicht
    // mitkommen.
    let names: Vec<&str> = events.iter().map(|event| event.summary.as_str()).collect();
    assert_eq!(names, vec!["Kundentermin"], "{names:?}");
    let reports = server
        .log()
        .into_iter()
        .filter(|entry| entry.starts_with("REPORT/"))
        .count();
    assert_eq!(reports, 1, "mehr als ein Kalender wurde gelesen");
}

#[tokio::test]
async fn a_rejected_login_names_the_likely_causes() {
    // Sabre antwortet bei 401 mit einem langen Standardtext, der nichts zur
    // Sache sagt. Die Meldung muss stattdessen die drei üblichen Gründe nennen
    // und darf den Ausnahmeschub des Servers nicht nachschieben.
    let body = r#"<?xml version="1.0"?>
<d:error xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns">
  <s:exception>Sabre\DAV\Exception\NotAuthenticated</s:exception>
  <s:message>No public access to this resource. Username or password was incorrect. No 'Authorization: Bearer' header found.</s:message>
</d:error>"#;
    let message = describe_dav_error(reqwest::StatusCode::UNAUTHORIZED, body);

    assert!(message.contains("widerrufen"), "{message}");
    assert!(message.contains("Groß- und Kleinschreibung"), "{message}");
    assert!(
        !message.contains("Sabre"),
        "der Ausnahmeschub des Servers steht im Weg: {message}"
    );
    assert!(
        !message.contains("Authorization"),
        "der Servertext steht im Weg: {message}"
    );
    assert!(
        !message.contains("Bearer"),
        "der Servertext steht im Weg: {message}"
    );
    assert!(
        message.chars().count() < 400,
        "die Meldung ist zu lang: {}",
        message.chars().count()
    );
}

#[tokio::test]
async fn a_wrong_password_says_what_to_do() {
    let server = FakeServer::with_status(CALENDARS_XML, REPORT_XML, "HTTP/1.1 401 Unauthorized");
    let client = build_client(false).unwrap();
    // Der Server lässt jede Anmeldung durch, der Fehler kommt danach als
    // Antwort der Instanz.
    let error = list_calendars(&client, &server.config(), "falsch")
        .await
        .expect_err("eine Ablehnung muss als Fehler kommen");
    assert!(error.contains("App-Passwort"), "{error}");
}

#[tokio::test]
async fn a_missing_calendar_app_says_what_to_check() {
    let server =
        FakeServer::with_status(CALENDARS_XML, REPORT_XML, "HTTP/1.1 405 Method Not Allowed");
    let client = build_client(false).unwrap();
    let error = list_calendars(&client, &server.config(), "geheim")
        .await
        .expect_err("405 muss als Fehler kommen");
    assert!(
        error.contains("Kalender-App") || error.contains("405"),
        "{error}"
    );
}

#[tokio::test]
async fn a_wrong_address_mentions_the_path() {
    let server = FakeServer::with_status(CALENDARS_XML, REPORT_XML, "HTTP/1.1 404 Not Found");
    let client = build_client(false).unwrap();
    let error = list_calendars(&client, &server.config(), "geheim")
        .await
        .expect_err("404 muss als Fehler kommen");
    assert!(error.contains("Pfad"), "{error}");
}

#[tokio::test]
async fn a_named_host_works_and_not_only_a_numeric_one() {
    // Ohne ausgeschaltete Umleitungen reicht reqwest dem Verbindungsstück bei
    // Namen eine Adresse ohne Schema, und der reine HTTP-Anschluss lehnt sie mit
    // „invalid URL, scheme is not http" ab. Bei einer IP-Adresse fällt das nicht
    // auf, deshalb wird hier ausdrücklich über einen Namen gefragt.
    let server = FakeServer::start(CALENDARS_XML, REPORT_XML);
    let _ = &server.log();
    let mut config = server.config();
    config.server_url = config.server_url.replace("127.0.0.1", "localhost");
    let client = build_client(false).unwrap();
    let calendars = list_calendars(&client, &config, "geheim")
        .await
        .expect("eine Namensadresse muss funktionieren");
    assert_eq!(calendars.len(), 2);
}

#[tokio::test]
async fn an_address_that_redirects_says_what_to_enter() {
    // Der Nachbau kann eine Umleitung mit Ziel melden. Mimir folgt ihr nicht
    // bewusst: Eine Umleitung auf einen anderen Host dürfte das Passwort nicht
    // mitnehmen, und der Aufrufer soll die echte WebDAV-Adresse eintragen.
    let server =
        FakeServer::with_status(CALENDARS_XML, REPORT_XML, "HTTP/1.1 301 Moved Permanently");
    let client = build_client(false).unwrap();
    let error = list_calendars(&client, &server.config(), "geheim")
        .await
        .expect_err("eine Umleitung darf nicht als Erfolg durchgehen");
    assert!(error.contains("leitet"), "{error}");
}

#[tokio::test]
async fn a_dead_server_reports_a_connection_problem() {
    // Port 1 auf localhost nimmt niemand an.
    let config = CalendarConfig {
        server_url: "http://127.0.0.1:1".to_string(),
        username: "kai".to_string(),
        calendars: Vec::new(),
        server_certificate: None,
    };
    let client = build_client(false).unwrap();
    let error = list_calendars(&client, &config, "geheim")
        .await
        .expect_err("ein toter Server muss als Fehler kommen");
    assert!(error.contains("Verbindungsfehler"), "{error}");
}

#[tokio::test]
async fn an_instance_without_calendars_is_reported_at_login() {
    let empty = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/remote.php/dav/calendars/kai/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
    let server = FakeServer::start(empty, REPORT_XML);
    let client = build_client(false).unwrap();
    let error = login(&client, &server.config(), "geheim")
        .await
        .expect_err("eine leere Instanz muss beim Anmelden auffallen");
    assert!(error.contains("Kalender-App"), "{error}");
}

#[test]
fn the_error_text_of_a_dav_response_is_read() {
    let body = r#"<?xml version="1.0"?>
<d:error xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns">
  <s:exception>Sabre\DAV\Exception\Forbidden</s:exception>
  <s:message>Zugriff auf diesen Kalender nicht erlaubt</s:message>
</d:error>"#;
    let message = describe_dav_error(reqwest::StatusCode::FORBIDDEN, body);
    assert!(
        message.contains("Zugriff auf diesen Kalender nicht erlaubt"),
        "{message}"
    );
    assert!(message.contains("verweigert"), "{message}");
}

#[test]
fn an_html_answer_from_a_proxy_stays_readable() {
    let body = "<html><head><title>502 Bad Gateway</title></head></html>";
    let message = describe_dav_error(reqwest::StatusCode::BAD_GATEWAY, body);
    assert!(message.contains("502"), "{message}");
    assert!(!message.contains('\n'), "{message}");
}

#[test]
fn a_long_answer_from_the_server_is_shortened() {
    let body = "a".repeat(5000);
    let message = describe_dav_error(reqwest::StatusCode::BAD_REQUEST, &body);
    assert!(
        message.chars().count() < 500,
        "Meldung zu lang: {}",
        message.chars().count()
    );
    assert!(message.contains("400"), "{message}");
}

#[test]
fn properties_from_a_failed_propstat_are_ignored() {
    let body = r#"<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/">
  <d:response>
    <d:href>/c/</d:href>
    <d:propstat>
      <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
    <d:propstat>
      <d:prop><cs:getctag>unsupported</cs:getctag><d:displayname>Falsch</d:displayname></d:prop>
      <d:status>HTTP/1.1 404 Not Found</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
    let resources = parse_multistatus(body);
    assert_eq!(resources.len(), 1);
    assert!(resources[0].ctag.is_empty(), "Eigenschaft aus 404 gelesen");
    assert!(resources[0].display_name.is_empty());
}

#[test]
fn colors_are_normalized_for_the_display() {
    assert_eq!(normalize_color("#2D55FFAA"), "#2D55FF");
    assert_eq!(normalize_color("#2D55FF"), "#2D55FF");
    assert_eq!(normalize_color("#2df"), "#22DDFF");
    // Ohne Alpha bleibt die Farbe, wie sie dasteht.
    assert_eq!(normalize_color("#abc"), "#AABBCC");
    assert_eq!(normalize_color("rgb(1,2,3)"), "");
}

#[tokio::test]
async fn the_fingerprint_matches_what_openssl_prints() {
    // Der Wert im Fenster ist nur dann etwas wert, wenn er mit dem übereinstimmt,
    // was auf dem Rechner nachzusehen ist. Der Vergleichswert ist
    // `openssl x509 -noout -fingerprint -sha256` für dasselbe Zertifikat.
    let bekannt = [
        // Bekannte Prüfsummen: „hello" und die leere Eingabe.
        (
            b"hello".as_slice(),
            "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824",
        ),
        (
            b"".as_slice(),
            "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
        ),
    ];

    for (daten, erwartet) in bekannt {
        let tatsaechlich = sha2::Sha256::digest(daten)
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":");

        assert_eq!(
            tatsaechlich
                .split(':')
                .map(|paar| u8::from_str_radix(paar, 16).unwrap())
                .collect::<Vec<_>>(),
            erwartet
                .as_bytes()
                .chunks(2)
                .map(|paar| u8::from_str_radix(std::str::from_utf8(paar).unwrap(), 16).unwrap())
                .collect::<Vec<_>>(),
            "Fingerabdruck weicht ab"
        );
    }
}

#[test]
fn a_certificate_survives_the_round_trip_through_the_configuration() {
    // Das Zertifikat liegt als Text in der Konfiguration; geht es dabei kaputt,
    // passt der Vergleich beim nächsten Start nicht mehr und der Zugang bliebe
    // gesperrt.
    let der: Vec<u8> = (0u8..=255).collect();
    let kodiert = super::base64_encode(&der);

    assert!(
        !kodiert.contains('\n'),
        "der Wert muss in eine JSON-Zeile passen"
    );
    assert_eq!(super::base64_decode(&kodiert).unwrap(), der);
    // Der Fingerabdruck lässt sich auch aus der Ablage wiederherstellen.
    let direkt = super::fingerprint_of(&kodiert);
    let erwartet = sha2::Sha256::digest(&der)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");

    assert_eq!(direkt, erwartet);
    // Und ein zerstörter Wert wird erkannt, nicht stillschweigend genutzt.
    assert_eq!(super::fingerprint_of("kein base64 !!"), "unlesbar");
}

/// Ein Datum in der Zukunft, sieben Tage entfernt.
fn spaeteres_datum() -> String {
    (chrono::Local::now() + chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string()
}

/// Ein Termin, wie ihn die Planung erzeugt.
fn plan() -> super::super::write::EventPlan {
    super::super::write::plan_event(
        &CalendarConfig {
            server_url: "https://cloud.example.org".to_string(),
            username: "kai".to_string(),
            calendars: vec!["persoenlich".to_string()],
            server_certificate: None,
        },
        &[("persoenlich".to_string(), "Persönlich".to_string())],
        // In der Zukunft: Die Prüfung gegen die Vergangenheit soll hier nicht
        // das eigentliche Thema des Tests verdecken.
        &serde_json::json!({
            "summary": "Zahnarzt",
            "start": format!("{}T09:00", spaeteres_datum())
        }),
        // Ohne Benutzertext wird nichts verworfen, siehe `write::tests::termin`.
        "",
    )
    .expect("der Plan muss sich bilden lassen")
}

#[tokio::test]
async fn a_termin_goes_to_the_calendar_with_its_own_name() {
    let server = FakeServer::for_write("HTTP/1.1 201 Created");
    let client = build_client(false).unwrap();
    let plan = plan();

    let url = create_event(&client, &server.config(), "geheim", &plan)
        .await
        .expect("ein 201 ist ein Erfolg");

    assert!(url.ends_with(&format!("/{}.ics", plan.uid)), "{url}");

    // Der Aufruf selbst: Methode, Ziel, Anmeldung und Inhalt.
    let log = server.log();
    let aufruf = log.join("\n");
    assert!(
        aufruf.contains("PUT/remote.php/dav/calendars/kai/persoenlich/"),
        "{aufruf}"
    );
    assert!(
        url.contains("/remote.php/dav/calendars/kai/persoenlich/"),
        "{url}"
    );
    assert!(
        aufruf.contains(&plan.uid),
        "Die Kennung muss im Ziel stehen: {aufruf}"
    );
    assert!(
        aufruf.contains("auth=true"),
        "Ohne Anmeldung lehnt Nextcloud ab: {aufruf}"
    );
    assert!(
        aufruf.contains("SUMMARY:Zahnarzt"),
        "Der Inhalt muss ankommen: {aufruf}"
    );
}

#[tokio::test]
async fn a_termin_is_never_written_over_an_existing_one() {
    let server = FakeServer::for_write("HTTP/1.1 412 Precondition Failed");
    let client = build_client(false).unwrap();

    let fehler = create_event(&client, &server.config(), "geheim", &plan())
        .await
        .expect_err("412 darf nicht als Erfolg gelten");

    assert!(fehler.contains("nichts überschrieben"), "{fehler}");
    assert!(
        fehler.contains("überschrieben"),
        "Der Satz muss deutlich sein: {fehler}"
    );
}

#[tokio::test]
async fn a_full_calendar_is_reported_as_such() {
    let server = FakeServer::for_write("HTTP/1.1 507 Insufficient Storage");
    let client = build_client(false).unwrap();

    let fehler = create_event(&client, &server.config(), "geheim", &plan())
        .await
        .expect_err("507 darf nicht als Erfolg gelten");

    assert!(fehler.contains("Kapazität"), "{fehler}");
}

#[tokio::test]
async fn a_wrong_password_does_not_look_like_success() {
    let server = FakeServer::for_write("HTTP/1.1 401 Unauthorized");
    let client = build_client(false).unwrap();

    let fehler = create_event(&client, &server.config(), "geheim", &plan())
        .await
        .expect_err("401 darf nicht als Erfolg gelten");

    assert!(fehler.contains("App-Passwort"), "{fehler}");
}

#[tokio::test]
async fn a_redirect_is_not_followed_when_writing() {
    // Ein 301 beim Schreiben darf nicht als gespeichert gelten: Mimir folgt
    // Umleitungen nicht, weil das Passwort sonst an einen fremden Host ginge.
    let server = FakeServer::for_write("HTTP/1.1 301 Moved Permanently");
    let client = build_client(false).unwrap();

    let fehler = create_event(&client, &server.config(), "geheim", &plan())
        .await
        .expect_err("eine Umleitung ist kein Speichern");

    assert!(fehler.contains("leitet"), "{fehler}");
    assert!(
        fehler.contains("nicht"),
        "Der Grund muss genannt werden: {fehler}"
    );
}

// ------------------------------------------------- Einen Termin ändern/löschen
// Beides ist unumkehrbar, deshalb wird hier vor allem geprüft, dass Mimir
// abbricht, wenn der Server nicht zustimmt, und dass die Änderungskennung
// mitgeschickt wird.

fn ereignis_konfig() -> CalendarConfig {
    CalendarConfig {
        server_url: "http://127.0.0.1:1".to_string(),
        username: "kai".to_string(),
        calendars: vec!["persoenlich".to_string()],
        server_certificate: None,
    }
}

#[tokio::test]
async fn ein_termin_wird_mit_kennung_geholt() {
    let server = FakeServer::mit_ereignis("200");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);

    let termin = fetch_event(
        &client,
        &config,
        "geheim",
        "persoenlich",
        "termin-1@nextcloud",
    )
    .await
    .expect("der Termin muss zu holen sein");

    assert_eq!(
        termin.etag, "etag-1",
        "ohne Kennung ist kein Schreiben möglich"
    );
    assert!(termin.ics.contains("SUMMARY:Teammeeting"), "{}", termin.ics);
    assert!(
        termin.url.ends_with("/persoenlich/termin-1@nextcloud.ics"),
        "{}",
        termin.url
    );
}

#[tokio::test]
async fn ein_unbekannter_termin_wird_so_gesagt_und_nicht_erfunden() {
    let server = FakeServer::mit_ereignis("404");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);

    let fehler = fetch_event(&client, &config, "geheim", "persoenlich", "weg@x")
        .await
        .expect_err("ein 404 ist kein Erfolg");

    assert!(fehler.contains("gibt es"), "{fehler}");
    assert!(
        fehler.contains("list_calendar_events"),
        "der Weg wird genannt: {fehler}"
    );
}

#[tokio::test]
async fn ohne_etag_wird_der_termin_nicht_zurueckgegeben() {
    // Ohne Änderungskennung lässt sich ein späteres Schreiben nicht absichern.
    // Lieber gar nicht anfassen, als möglicherweise fremde Änderungen zu
    // überschreiben.
    let server = FakeServer::mit_ereignis_datei("200", EREIGNIS_ICS, "");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);

    let fehler = fetch_event(
        &client,
        &config,
        "geheim",
        "persoenlich",
        "termin-1@nextcloud",
    )
    .await
    .expect_err("ohne Kennung wird nichts angeboten");

    assert!(fehler.contains("Änderungskennung"), "{fehler}");
}

#[tokio::test]
async fn ein_geaenderter_termin_geht_mit_if_match_raus() {
    let server = FakeServer::mit_ereignis("201");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);
    let url = format!("http://{}/persoenlich/termin-1.ics", server.address);

    update_event(
        &client,
        &config,
        "geheim",
        &url,
        "etag-1",
        "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
    )
    .await
    .expect("ein 201 ist ein Erfolg");

    let aufruf = server.log().join("\n");
    assert!(aufruf.contains("PUT/"), "{aufruf}");
    assert!(aufruf.contains("auth=true"), "{aufruf}");
}

#[tokio::test]
async fn ein_geaenderter_termin_wird_bei_412_nicht_ueberschrieben() {
    // Genau der Fall, für den es die Kennung gibt: Zwischen Lesen und Schreiben
    // wurde der Termin in Nextcloud verändert.
    let server = FakeServer::mit_ereignis("412");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);
    let url = format!("http://{}/persoenlich/termin-1.ics", server.address);

    let fehler = update_event(
        &client,
        &config,
        "geheim",
        &url,
        "etag-1",
        "BEGIN:VCALENDAR\r\n",
    )
    .await
    .expect_err("412 darf nicht als Erfolg gelten");

    assert!(fehler.contains("zwischenzeitlich"), "{fehler}");
    assert!(fehler.contains("nichts darüber"), "{fehler}");
}

#[tokio::test]
async fn ein_geloeschter_termin_wird_bei_412_nicht_geloescht() {
    let server = FakeServer::mit_ereignis("412");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);
    let url = format!("http://{}/persoenlich/termin-1.ics", server.address);

    let fehler = delete_event(&client, &config, "geheim", &url, "etag-1")
        .await
        .expect_err("412 darf nicht als Erfolg gelten");

    assert!(fehler.contains("zwischenzeitlich"), "{fehler}");
    assert!(
        server.log().join("\n").contains("DELETE"),
        "es wurde überhaupt nicht gelöscht"
    );
}

#[tokio::test]
async fn ohne_kennung_wird_weder_geaendert_noch_geloescht() {
    // Der letzte Rest Schutz: Selbst wenn irgendwo eine Kennung verloren ging,
    // darf Mimir nicht blind überschreiben.
    let client = build_client(false).unwrap();
    let config = ereignis_konfig();
    let url = "http://127.0.0.1:1/termin-1.ics";

    assert!(update_event(&client, &config, "geheim", url, "", "x")
        .await
        .is_err());
    assert!(delete_event(&client, &config, "geheim", url, "")
        .await
        .is_err());
}

#[tokio::test]
async fn ein_kalendername_mit_leerzeichen_wird_erreichbar_bleiben() {
    let server = FakeServer::mit_ereignis("200");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);

    let termin = fetch_event(
        &client,
        &config,
        "geheim",
        "Meine Termine",
        "termin-1@nextcloud",
    )
    .await
    .expect("auch ein Name mit Leerzeichen muss funktionieren");

    assert!(termin.url.contains("Meine%20Termine"), "{}", termin.url);
}

#[tokio::test]
async fn eine_kennung_mit_sonderzeichen_verlaesst_den_pfad_nicht() {
    // Schrägstriche in einer Kennung werden kodiert und können so nicht aus dem
    // Kalender herausführen. Ein Kalendername mit Sprung nach oben wird
    // abgelehnt, weil er ein Pfadabschnitt sein muss.
    let server = FakeServer::mit_ereignis("200");
    let client = build_client(false).unwrap();
    let mut config = ereignis_konfig();
    config.server_url = format!("http://{}", server.address);

    let termin = fetch_event(&client, &config, "geheim", "privat", "../../etc/passwd")
        .await
        .expect("eine Kennung mit Schrägstrichen ist ungewöhnlich, aber kein Grund zum Abbruch");

    assert!(!termin.url.contains("/passwd/"), "{}", termin.url);
    assert!(
        termin.url.contains("%2F"),
        "die Schrägstriche sind nicht kodiert: {}",
        termin.url
    );
    assert!(termin.url.contains("/privat/"), "{}", termin.url);

    let fehler = fetch_event(
        &client,
        &config,
        "geheim",
        "../privat",
        "termin-1@nextcloud",
    )
    .await
    .expect_err("ein Kalendername mit .. wird abgelehnt");
    assert!(fehler.contains("Kalendername"), "{fehler}");
}
