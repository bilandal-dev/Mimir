//! Der Netzeiteil: CalDAV ist WebDAV, also PROPFIND zum Finden der Kalender und
//! REPORT zum Lesen der Termine. Bewusst ohne CalDAV-Bibliothek, weil die zwei
//! Aufrufe überschaubar sind und die Antwort ohnehin als XML gelesen wird.

use chrono::{DateTime, Utc};
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use sha2::Digest;
use std::time::Duration;

use super::events::{self, Window};
use super::{CalendarConfig, CalendarEvent, MAX_CALENDAR_EVENTS, MAX_ICS_BYTES};

/// Verbindungsaufbau. Das lokale Netz antwortet schnell, aber nicht immer; ein
/// zweiter Versuch macht die Leiste unempfindlicher gegen ein einzelnes
/// Aussetzen, ohne im Fehlerfall länger zu warten.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PROP_FIND_TIMEOUT: Duration = Duration::from_secs(15);
const REPORT_TIMEOUT: Duration = Duration::from_secs(30);
/// Gesamtzeit der Zertifikats- und Versionsabfrage. Beides sind reine
/// Lesevorgänge ohne Zugangsdaten, deshalb deutlich kürzer als die eigentlichen
/// Aufrufe.
const STATUS_TIMEOUT: Duration = Duration::from_secs(4);
/// Wie oft ein Verbindungsfehler wiederholt wird. HTTP-Fehler werden nie
/// wiederholt, die sind eindeutig.
const CONNECTION_ATTEMPTS: usize = 2;
/// TCP-Keepalive für die Kalenderverbindung. Kürzer als beim Ollama-Client
/// (60 s statt 120 s), weil ein Kalenderabruf nie länger als 30 s dauert. Das
/// User-Timeout wird hier – anders als im Ollama-Client – nicht abgeschaltet; die
/// Anfragezeitgrenzen liegen darunter.
const KEEPALIVE: Duration = Duration::from_secs(60);

/// Ein Kalender, wie er in der Leiste angezeigt wird.
#[derive(Clone, Debug, Serialize)]
pub struct CalendarInfo {
    /// Letzter Pfadbestandteil, der zugleich der Schlüssel für weitere Abrufe ist.
    pub href: String,
    pub display_name: String,
    /// Änderungskennung. Bleibt sie gleich, kann ein Abruf ausfallen.
    pub ctag: String,
    pub color: String,
}

/// Ergebnis einer Anmeldung.
#[derive(Debug)]
pub struct Session {
    pub version: Option<String>,
    pub calendars: Vec<CalendarInfo>,
}

/// Alles, was ein Antwortdokument über einen Kalender sagt.
#[derive(Debug, Default)]
struct DavResource {
    href: String,
    display_name: String,
    ctag: String,
    etag: String,
    color: String,
    calendar_data: String,
    is_collection: bool,
    /// Manche Server antworten zweimal, einmal mit 200 und einmal mit 404 für
    /// nicht unterstützte Eigenschaften. Nur der erfolgreiche Teil zählt.
    ok: bool,
}

/// Das vom Server vorgelegte Zertifikat. reqwest reicht es als Erweiterung der
/// Antwort weiter, wenn der Client mit `tls_info` gebaut wurde.
fn peer_certificate(response: &reqwest::Response) -> Option<Vec<u8>> {
    response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(reqwest::tls::TlsInfo::peer_certificate)
        .map(<[u8]>::to_vec)
}

/// Nur der Fingerabdruck aus einem kodierten Zertifikat, ohne Netzverkehr.
pub fn fingerprint_of(encoded: &str) -> String {
    match base64_decode(encoded) {
        Some(der) => sha2::Sha256::digest(&der)
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
        None => "unlesbar".to_string(),
    }
}

fn base64_decode(value: &str) -> Option<Vec<u8>> {
    let mut tabelle = [255u8; 256];

    for (index, zeichen) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        tabelle[*zeichen as usize] = index as u8;
    }

    let mut ergebnis = Vec::with_capacity(value.len() / 4 * 3);
    let mut puffer = 0u32;
    let mut bits = 0u32;

    for zeichen in value.bytes() {
        if zeichen == b'=' || zeichen.is_ascii_whitespace() {
            continue;
        }

        let wert = tabelle[zeichen as usize];

        if wert == 255 {
            return None;
        }

        puffer = (puffer << 6) | wert as u32;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            ergebnis.push((puffer >> bits) as u8);
        }
    }

    Some(ergebnis)
}

/// Baut den HTTP-Client der Kalenderleiste.
///
/// Bei einer Adresse mit `https` muss die Prüfung der Zertifikatskette
/// entfallen, sonst käme man an einer selbstsignierten Instanz gar nicht erst bis
/// zum Zertifikat. Die Prüfung übernimmt stattdessen `probe_certificate` samt
/// `ensure_certificate`: Sie vergleicht das vorgelegte Zertifikat mit dem
/// bestätigten – und zwar bevor irgendein Passwort gesendet wird. Bei `http`
/// bleibt alles unverändert, dort gibt es kein Zertifikat.
///
/// `accept_any_certificate` steht nur deshalb offen, weil das vorgelegte
/// Zertifikat sonst nie sichtbar würde. Es wird nie ungeprüft verwendet: Ohne
/// bestätigtes Zertifikat verweigert `ensure_certificate` den Zugang.
pub fn build_client(accept_any_certificate: bool) -> Result<reqwest::Client, String> {
    // reqwest braucht den Anbieter schon beim Bauen des Clients, auch ohne
    // Verschlüsselung. Beim Start ist er schon gesetzt; das hier deckt jeden
    // Aufruf ab, der vorher kommt.
    crate::install_crypto_provider();
    let mut builder = reqwest::Client::builder().http1_only();

    if accept_any_certificate {
        builder = builder.danger_accept_invalid_certs(true);
    }

    // Nur hiermit lässt sich das vorgelegte Zertifikat überhaupt auslesen.
    builder = builder.tls_info(true);

    builder
        // Umleitungen werden nicht verfolgt. Einmal mitgeführte Umleitungen
        // reichen dem Verbindungsstück eine Adresse ohne Schema, was der reine
        // HTTP-Anschluss mit „scheme is not http" ablehnt. Für CalDAV ist das
        // ohnehin richtig: Eine Umleitung auf einen anderen Host darf nicht
        // stillschweigend das Passwort mitnehmen. Auf eine Umleitung wird
        // deshalb mit einer eigenen Meldung geantwortet.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .tcp_keepalive(Some(KEEPALIVE))
        .user_agent(concat!("Mimir/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("Client-Fehler: {}", crate::format_reqwest_error(&error)))
}

/// Liest das Zertifikat, das der Server vorlegt, und berechnet daraus den
/// Fingerabdruck, der im Fenster bestätigt wird.
///
/// Der Vergleichswert ist die Ausgabe von `openssl x509 -noout -fingerprint
/// -sha256` ohne das Präfix `SHA256 Fingerprint=`: gleiche Schreibweise, zwei
/// Hexzeichen pro Oktett, Großbuchstaben, Doppelpunkte als Trenner. So lässt sich
/// auf dem Terminal nachsehen, ob es dasselbe Zertifikat ist.
pub fn certificate(response: &reqwest::Response) -> Result<Certificate, String> {
    let der = peer_certificate(response).ok_or_else(|| {
        "Der Server hat beim Aufbau von HTTPS kein Zertifikat vorgelegt".to_string()
    })?;
    let digest = sha2::Sha256::digest(&der);

    Ok(Certificate {
        // Base64 ohne neue Zeilen: Der Wert gehört in die Konfigurationsdatei.
        der: base64_encode(&der),
        fingerprint: digest
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
    })
}

/// Das Zertifikat einer Instanz, wie es in der Konfiguration hinterlegt wird.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Certificate {
    /// Das Zertifikat selbst, kodiert. Ohne Systemvertrauen ist es das einzige,
    /// was die Prüfung ersetzen kann.
    pub der: String,
    /// Nur für die Anzeige im Fenster.
    pub fingerprint: String,
}

/// Ergebnis der Prüfung vor dem ersten Zugang.
#[derive(Debug)]
pub enum CertificateCheck {
    /// Die Adresse nutzt kein HTTPS, es gibt nichts zu prüfen.
    NotApplicable,
    /// Das Zertifikat passt zum bestätigten.
    Matches,
    /// Noch nichts bestätigt: Der Nutzer muss entscheiden.
    NeedsApproval(Certificate),
    /// Das Zertifikat weicht vom bestätigten ab.
    Changed {
        expected: String,
        found: Certificate,
    },
}

/// Fragt das Zertifikat der Instanz ab, ohne Zugangsdaten zu senden.
///
/// Der Aufruf geht an `status.php`, das Nextcloud auch ohne Anmeldung beantwortet.
/// Erst wenn das Zertifikat passt, wird mit Zugangsdaten gearbeitet; so kann ein
/// fremder Server das Passwort nicht abfangen, indem er ein neues Zertifikat
/// vorlegt.
pub async fn probe_certificate(
    client: &reqwest::Client,
    config: &CalendarConfig,
) -> Result<CertificateCheck, String> {
    if !config.uses_tls() {
        return Ok(CertificateCheck::NotApplicable);
    }

    let base = config.server_url.trim_end_matches('/');
    let response = client
        .get(format!("{base}/status.php"))
        .timeout(STATUS_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            format!(
                "Die Instanz antwortet nicht: {}",
                crate::format_reqwest_error(&error)
            )
        })?;
    let found = certificate(&response)?;

    match &config.server_certificate {
        None => Ok(CertificateCheck::NeedsApproval(found)),
        Some(der) if *der == found.der => Ok(CertificateCheck::Matches),
        Some(_) => Ok(CertificateCheck::Changed {
            expected: config
                .server_certificate_fingerprint()
                .unwrap_or_else(|| "unbekannt".to_string()),
            found,
        }),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ZEICHEN: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut ergebnis = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for stueck in bytes.chunks(3) {
        let b0 = stueck[0] as u32;
        let b1 = *stueck.get(1).unwrap_or(&0) as u32;
        let b2 = *stueck.get(2).unwrap_or(&0) as u32;
        let wert = (b0 << 16) | (b1 << 8) | b2;

        ergebnis.push(ZEICHEN[((wert >> 18) & 63) as usize] as char);
        ergebnis.push(ZEICHEN[((wert >> 12) & 63) as usize] as char);
        ergebnis.push(if stueck.len() > 1 {
            ZEICHEN[((wert >> 6) & 63) as usize] as char
        } else {
            '='
        });
        ergebnis.push(if stueck.len() > 2 {
            ZEICHEN[(wert & 63) as usize] as char
        } else {
            '='
        });
    }

    ergebnis
}

/// Fehlerantworten von Nextcloud sehen unterschiedlich aus: DAV liefert XML mit
/// einem `error`-Knoten, ein Proxy oft eine HTML-Seite. Beides wird lesbar
/// gemacht, damit in der Leiste eine Handlung statt eines Codes steht.
pub fn describe_dav_error(status: reqwest::StatusCode, body: &str) -> String {
    let detail = xml_error_text(body)
        .or_else(|| plain_text(body))
        .unwrap_or_default();
    let detail: String = detail.chars().take(300).collect();
    let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let base = match status.as_u16() {
        401 => "Die Instanz lehnt Benutzername oder App-Passwort ab. Am \
                häufigsten stimmt eines von dreien nicht: Das Passwort wurde \
                widerrufen, es wurde ein neues erzeugt und das alte steht noch \
                im Fenster, oder der Benutzername ist anders geschrieben als in \
                Nextcloud – im DAV-Pfad zählt die Groß- und Kleinschreibung."
            .to_string(),
        403 => "Der Zugriff wurde verweigert. In Nextcloud prüfen, ob die Kalender-App \
                aktiv ist und die Adresse stimmt."
            .to_string(),
        404 => "Der Pfad wurde nicht gefunden. Adresse und Unterverzeichnis der Instanz \
                prüfen, etwa /nextcloud."
            .to_string(),
        405 => "Die Instanz beantwortet die Kalenderabfrage nicht. Vermutlich fehlt die \
                Kalender-App oder ein vorgeschalteter Proxy liefert die Anfrage nicht \
                an PHP weiter."
            .to_string(),
        415 => "Die Instanz lehnt den Inhalt der Anfrage ab. Bei einem Proxy ist oft \
                die Weiterleitung von /remote.php nicht eingerichtet."
            .to_string(),
        _ if status.is_server_error() => {
            format!(
                "Der Server meldet einen Fehler (Status {}).",
                status.as_u16()
            )
        }
        _ => format!(
            "Die Instanz lehnt die Anfrage ab (Status {}).",
            status.as_u16()
        ),
    };

    // Bei 401 nennt der Server nur seinen eigenen Standardtext; der wäre
    // länger als die eigentliche Erklärung und würde sie verdecken.
    if detail.is_empty() || status.as_u16() == 401 {
        base
    } else {
        format!("{base} Antwort: {detail}")
    }
}

/// Zieht den Text aus einem DAV-Fehlerdokument, etwa
/// `<d:error><s:message>Kein Zugriff</s:message></d:error>`.
fn xml_error_text(body: &str) -> Option<String> {
    if !body.trim_start().starts_with('<') {
        return None;
    }

    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    let mut depth = 0i32;
    let mut inside_error = false;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.local_name().into_inner();

                if name == "error" {
                    inside_error = true;
                    depth = 0;
                } else if inside_error {
                    depth += 1;
                }
            }
            Ok(Event::End(_)) => {
                if inside_error {
                    depth -= 1;

                    if depth < 0 {
                        break;
                    }
                }
            }
            Ok(Event::Text(value)) => {
                if inside_error {
                    text.push_str(&unescape(&value).unwrap_or_default());
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }

    let trimmed = text.trim();

    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Nimmt den Text einer Antwort, die kein XML ist.
fn plain_text(body: &str) -> Option<String> {
    let trimmed = body.trim();

    if trimmed.is_empty() || trimmed.starts_with('<') {
        return None;
    }

    Some(trimmed.to_string())
}

/// Liest eine Multistatus-Antwort in ihre Einträge.
///
/// Die Eigenschaften eines `response` stehen in eigenen `propstat`-Blöcken, und
/// ein Block kann für nicht unterstützte Eigenschaften `404` melden, während ein
/// zweiter `200` liefert. Deshalb werden die Werte eines Blocks erst nach dem
/// Lesen seines Status übernommen.
fn parse_multistatus(body: &str) -> Vec<DavResource> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    let mut resources: Vec<DavResource> = Vec::new();
    let mut current: Option<DavResource> = None;
    let mut property = String::new();
    let mut inside_resourcetype = false;
    // Werte des gerade gelesenen propstat, bis sein Status feststeht. `href`
    // steht im `response` und nicht im `propstat` und wird direkt übernommen.
    let mut staged: Vec<(&'static str, String)> = Vec::new();
    let mut is_collection = false;
    let mut propstat_ok = true;
    let mut saw_ok = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.local_name().into_inner();

                if name == "response" {
                    current = Some(DavResource::default());
                    saw_ok = false;
                    continue;
                }

                if current.is_none() {
                    continue;
                }

                match name {
                    "propstat" => {
                        staged.clear();
                        is_collection = false;
                        propstat_ok = true;
                    }
                    "resourcetype" => inside_resourcetype = true,
                    _ => {
                        if inside_resourcetype {
                            is_collection = true;
                        } else {
                            property = name.to_string();
                        }
                    }
                }
            }
            Ok(Event::Empty(element)) => {
                let name = element.local_name().into_inner();

                // <d:collection/> steht ohne Inhalt im resourcetype.
                if name == "collection" && inside_resourcetype {
                    is_collection = true;
                }
            }
            Ok(Event::End(element)) => {
                let name = element.local_name().into_inner();

                match name {
                    "propstat" => {
                        if propstat_ok {
                            saw_ok = true;

                            if let Some(entry) = current.as_mut() {
                                entry.is_collection = entry.is_collection || is_collection;

                                for (key, value) in staged.drain(..) {
                                    match key {
                                        // `href` kommt gewöhnlich als Kind von
                                        // `response` und wird schon beim Text
                                        // gelesen. Dieser Zweig greift nur, wenn
                                        // ein Server ihn innerhalb eines
                                        // `propstat` meldet – dann ist die
                                        // Position innerhalb des Blocks die
                                        // richtige.
                                        "href" => entry.href = value,
                                        "displayname" => entry.display_name = value,
                                        "getctag" => entry.ctag = value,
                                        "getetag" => entry.etag = value,
                                        "calendar-data" => entry.calendar_data = value,
                                        "calendar-color" => entry.color = value,
                                        _ => {}
                                    }
                                }
                            }
                        }

                        staged.clear();
                        is_collection = false;
                        propstat_ok = true;
                    }
                    "resourcetype" => inside_resourcetype = false,
                    "status" => property.clear(),
                    "response" => {
                        if let Some(mut entry) = current.take() {
                            entry.ok = saw_ok;
                            resources.push(entry);
                        }

                        saw_ok = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(value)) => {
                if property == "status" {
                    propstat_ok = unescape(&value).unwrap_or_default().contains(" 200 ");
                    property.clear();
                    continue;
                }

                if property.is_empty() {
                    continue;
                }

                let text = unescape(&value).unwrap_or_default();

                // `href` steht als direktes Kind von `response` und ist nicht
                // vom Status eines propstat abhaengig.
                if property == "href" {
                    if let Some(entry) = current.as_mut() {
                        entry.href = text.to_string();
                    }

                    property.clear();
                    continue;
                }

                match property.as_str() {
                    "displayname" => staged.push(("displayname", text.to_string())),
                    "getctag" => staged.push(("getctag", text.to_string())),
                    "getetag" => staged.push(("getetag", text.to_string())),
                    "calendar-data" => staged.push(("calendar-data", text.to_string())),
                    "calendar-color" => staged.push(("calendar-color", text.to_string())),
                    _ => {}
                }
            }
            Ok(Event::CData(value)) => {
                // Manche Server liefern die ICS-Daten als CDATA.
                if property == "calendar-data" {
                    staged.push(("calendar-data", value.to_string()));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    if let Some(mut entry) = current.take() {
        entry.ok = saw_ok;
        resources.push(entry);
    }

    resources
}

const PROP_FIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<d:propfind xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:displayname />
    <d:resourcetype />
    <d:getetag />
    <cs:getctag />
    <cal:calendar-color />
  </d:prop>
</d:propfind>"#;

/// Zeitraum-Report: nur Termine, die im Fenster liegen, samt ICS-Daten.
fn report_body(window: &Window) -> String {
    let start = window.start.format("%Y%m%dT%H%M%SZ");
    let end = window.end.format("%Y%m%dT%H%M%SZ");

    format!(
        r#"<?xml version="1.0" encoding="utf-8" ?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag />
    <c:calendar-data />
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start}" end="{end}" />
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
    )
}

async fn read_body(response: reqwest::Response) -> Result<String, String> {
    use futures_util::StreamExt;

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let Ok(chunk) = item else { break };

        if body.len().saturating_add(chunk.len()) > MAX_ICS_BYTES {
            return Err("Die Antwort der Instanz ist zu groß".to_string());
        }

        body.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&body).to_string())
}

fn is_transport_error(message: &str) -> bool {
    message.starts_with("Verbindungsfehler")
}

/// PROPFIND und REPORT sind WebDAV-Methoden, die reqwest nicht als Konstante
/// mitbringt.
fn dav_method(name: &[u8]) -> Result<reqwest::Method, String> {
    reqwest::Method::from_bytes(name).map_err(|_| "Unbekannte DAV-Methode".to_string())
}

/// Führt einen DAV-Aufruf aus und wiederholt nur echte Verbindungsprobleme.
async fn send(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    config: &CalendarConfig,
    password: &str,
    body: String,
    timeout: Duration,
) -> Result<String, String> {
    let mut last = String::new();

    for attempt in 1..=CONNECTION_ATTEMPTS {
        let request = client
            .request(method.clone(), url)
            .basic_auth(&config.username, Some(password))
            .header("Depth", "1")
            .timeout(timeout)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body.clone());

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let text = read_body(response).await?;

                if status.is_success() {
                    return Ok(text);
                }

                if status.is_redirection() {
                    // Fast immer wird von http auf https umgeleitet. Die
                    // Umleitung wird nicht verfolgt, weil sie das Passwort an
                    // einen anderen Host tragen könnte; deshalb wird hier der
                    // Grund genannt statt stillschweigend hingegangen zu werden.
                    return Err(match location {
                        Some(ziel) if ziel.starts_with("https://") => format!(
                            "Die Adresse leitet auf {ziel} um. Die Instanz läuft über \
                             HTTPS: https:// in die Adresse eintragen und das \
                             Zertifikat bestätigen."
                        ),
                        Some(ziel) => format!(
                            "Die Adresse leitet auf {ziel} um. In Nextcloud unter \
                             Einstellungen, WebDAV die dort angezeigte Adresse übernehmen."
                        ),
                        None => "Die Adresse leitet um. Die WebDAV-Adresse aus den \
                                 Nextcloud-Einstellungen übernehmen."
                            .to_string(),
                    });
                }

                return Err(describe_dav_error(status, &text));
            }
            Err(error) => {
                last = format!("Verbindungsfehler: {}", crate::format_reqwest_error(&error));

                if !is_transport_error(&last) || attempt == CONNECTION_ATTEMPTS {
                    return Err(last);
                }
            }
        }
    }

    Err(last)
}

/// Liest die Serverversion. Eine fehlgeschlagene Abfrage ist kein Fehler: die
/// Leiste zeigt dann eben keine Version an.
pub async fn fetch_version(client: &reqwest::Client, config: &CalendarConfig) -> Option<String> {
    let base = config.server_url.trim_end_matches('/');
    let response = client
        .get(format!("{base}/status.php"))
        .timeout(STATUS_TIMEOUT)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let text = read_body(response).await.ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    value
        .get("version")
        .and_then(|version| version.as_str())
        .map(|version| version.to_string())
}

/// Holt die Kalenderliste. Wird auch von jedem Terminabruf gebraucht, weil dort
/// die Namen für die Anzeige herkommen.
pub async fn list_calendars(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
) -> Result<Vec<CalendarInfo>, String> {
    let root = config.calendars_root()?;
    let body = send(
        client,
        dav_method(b"PROPFIND")?,
        &root,
        config,
        password,
        PROP_FIND_BODY.to_string(),
        PROP_FIND_TIMEOUT,
    )
    .await?;

    Ok(parse_calendars(&parse_multistatus(&body), &root))
}

fn parse_calendars(resources: &[DavResource], root: &str) -> Vec<CalendarInfo> {
    let mut calendars: Vec<CalendarInfo> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for resource in resources {
        if !resource.ok {
            continue;
        }

        // Der eigene Sammelpfad und technische Ordner wie Papierkorb oder
        // Posteingang tragen weder ctag noch Namen.
        let is_calendar = resource.is_collection
            && resource.href.trim_end_matches('/') != root.trim_end_matches('/')
            && (!resource.ctag.is_empty() || !resource.display_name.is_empty());

        if !is_calendar {
            continue;
        }

        // Der Pfad endet auf dem Namen des Kalenders, etwa
        // /remote.php/dav/calendars/kai/persoenlich/ . Nach dem Splitten von
        // hinten steht der Name nicht immer an erster Stelle, weil der Pfad mit
        // einem Schrägstrich endet.
        let href = percent_decode(
            resource
                .href
                .trim_matches('/')
                .rsplit('/')
                .find(|part| !part.is_empty())
                .unwrap_or_default(),
        );

        if href.is_empty() || seen.contains(&href) {
            continue;
        }

        seen.push(href.clone());
        calendars.push(CalendarInfo {
            display_name: if resource.display_name.is_empty() {
                href.clone()
            } else {
                percent_decode(&resource.display_name)
            },
            href,
            ctag: resource.ctag.clone(),
            color: normalize_color(&resource.color),
        });
    }

    calendars.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    calendars
}

/// Bringt die Kalenderfarbe in die Form, die CSS versteht. CalDAV schreibt
/// `#RRGGBBAA`, das Alpha steht also hinten und wird für die Anzeige weggelassen.
/// Ältere Server liefern sechs Stellen, die bleiben unverändert.
fn normalize_color(value: &str) -> String {
    let hex = value.trim().strip_prefix('#').unwrap_or_default();

    if hex.is_empty() || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return String::new();
    }

    match hex.len() {
        // #RRGGBBAA: das Alpha steht im CalDAV-Format hinten und wird für die
        // Anzeige weggelassen.
        8 => format!("#{}", &hex[..6]).to_uppercase(),
        6 => format!("#{hex}").to_uppercase(),
        3 => format!("#{0}{0}{1}{1}{2}{2}", &hex[0..1], &hex[1..2], &hex[2..3]).to_uppercase(),
        _ => String::new(),
    }
}

/// Holt die Termine der gewählten Kalender im Zeitraum.
pub async fn fetch_events(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
    window: &Window,
) -> Result<Vec<CalendarEvent>, String> {
    let root = config.calendars_root()?;
    let available = list_calendars(client, config, password).await?;
    let selected: Vec<&CalendarInfo> = if config.calendars.is_empty() {
        available.iter().collect()
    } else {
        available
            .iter()
            .filter(|calendar| config.calendars.contains(&calendar.href))
            .collect()
    };

    if selected.is_empty() {
        return Err(if available.is_empty() {
            "Die Instanz meldet keine lesbaren Kalender. Läuft die Kalender-App \
             auf dem Server?"
                .to_string()
        } else {
            "Der ausgewählte Kalender ist nicht mehr vorhanden".to_string()
        });
    }

    let mut events: Vec<CalendarEvent> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for calendar in &selected {
        let url = format!("{root}{}/", escape_path_segment(&calendar.href));
        let body = report_body(window);

        match send(
            client,
            dav_method(b"REPORT")?,
            &url,
            config,
            password,
            body,
            REPORT_TIMEOUT,
        )
        .await
        {
            Ok(answer) => {
                for resource in parse_multistatus(&answer) {
                    if resource.calendar_data.is_empty() {
                        continue;
                    }

                    events.extend(events::parse_events(
                        &resource.calendar_data,
                        &calendar.display_name,
                        &calendar.href,
                        window,
                    ));
                }
            }
            // Ein einzelner nicht erreichbarer Kalender darf die Leiste nicht
            // leeren, solange ein anderer antwortet.
            Err(error) => failures.push(format!("{}: {}", calendar.display_name, error)),
        }
    }

    if events.is_empty() && !failures.is_empty() {
        return Err(failures.join(" "));
    }

    events.sort_by_key(|event| event.start);
    // Was über `MAX_CALENDAR_EVENTS` hinaus liegt, wird abgeschnitten statt
    // abgewiesen: Die Leiste soll nicht an einer Datenmenge hängen bleiben. Für
    // eine vollständige Liste ist `list_calendar_events` mit einem benannten
    // Zeitraum der Weg.
    events.truncate(MAX_CALENDAR_EVENTS);

    Ok(events)
}

/// Kodiert ein Pfadsegment für die URL.
///
/// Kodiert werden nur Leerzeichen, denn der Pfad kommt hier bereits dekodiert aus
/// `parse_calendars`. Achtung: `%`, `?` und `#` sind an dieser Stelle **nicht**
/// kodiert, im Schreibpfad (`write::escape_calendar_segment`) dagegen schon. Beide
/// Wege sollten denselben Satz Zeichen behandeln; ein Kalender, dessen Name
/// wirklich ein `%` enthält, ist über den Lesepfad derzeit nicht erreichbar.
fn escape_path_segment(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .map(|character| match character {
            ' ' => "%20".to_string(),
            other => other.to_string(),
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");

            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                result.push(byte);
                index += 3;
                continue;
            }
        }

        result.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&result).to_string()
}

/// Der Termin, wie er jetzt im Kalender steht, samt seiner Änderungskennung.
///
/// Die Kennung (`ETag`) ist das, was ein Ändern oder Löschen absichert: Stimmt
/// sie beim Schreiben nicht mehr, wurde der Termin zwischenzeitlich in Nextcloud
/// oder von einem zweiten Gerät verändert – dann wird nichts überschrieben.
#[derive(Clone, Debug)]
pub struct EventOnServer {
    pub ics: String,
    pub etag: String,
    /// Die Adresse, unter der er liegt. Nötig fürs Löschen.
    pub url: String,
}

/// Holt einen einzelnen Termin.
///
/// Nextcloud legt CalDAV-Termine unter dem Dateinamen ihrer Kennung ab, wie es
/// die CalDAV-Kalender von Google und Apple auch tun. Deshalb wird die Kennung
/// als Dateiname angesprochen; ein 404 bedeutet schlicht „diesen Termin gibt es
/// nicht (mehr)“.
pub async fn fetch_event(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
    calendar: &str,
    uid: &str,
) -> Result<EventOnServer, String> {
    let kalender = super::write::escape_calendar_segment(calendar)?;
    // Die Endung `.ics` gehört dazu: Nextcloud legt CalDAV-Termine so ab, und
    // genau unter diesem Namen legt Mimir sie beim Anlegen ab.
    let url = format!(
        "{}{}/{}.ics",
        config.calendars_root()?,
        kalender,
        super::edit::datei_name(uid)
    );

    let antwort = client
        .get(&url)
        .basic_auth(&config.username, Some(password))
        .timeout(REPORT_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("Verbindungsfehler: {}", crate::format_reqwest_error(&error)))?;

    let status = antwort.status();
    let etag = antwort
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|wert| wert.to_str().ok())
        .map(|wert| wert.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let text = read_body(antwort).await?;

    if status.is_success() {
        if etag.is_empty() {
            // Ohne Kennung gibt es nichts, womit ein späteres Schreiben sich
            // absichern ließe. Dann wird nicht geschrieben.
            return Err(
                "Der Server nennt für diesen Termin keine Änderungskennung. Ohne sie lässt sich \
                 nicht erkennen, ob der Termin inzwischen verändert wurde; Mimir schreibt ihn \
                 deshalb nicht."
                    .to_string(),
            );
        }

        return Ok(EventOnServer {
            ics: text,
            etag,
            url,
        });
    }

    if status.as_u16() == 404 {
        return Err(
            "Diesen Termin gibt es im Kalender nicht mehr. Vielleicht wurde er in Nextcloud \
             gelöscht oder verschoben; mit list_calendar_events nachsehen."
                .to_string(),
        );
    }

    Err(describe_dav_error(status, &text))
}

/// Schreibt einen geänderten Termin zurück.
///
/// `If-Match` ist nicht höflich, sondern notwendig: Ohne die Kennung würde der
/// Schreibvorgang eine zwischenzeitliche Änderung in Nextcloud überschreiben, und
/// bei einem Löschen sogar einen fremden Termin entfernen.
pub async fn update_event(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
    url: &str,
    etag: &str,
    ics: &str,
) -> Result<(), String> {
    if etag.is_empty() {
        return Err("Ohne Änderungskennung wird nicht geschrieben.".to_string());
    }

    let antwort = client
        .put(url)
        .basic_auth(&config.username, Some(password))
        .header("Content-Type", "text/calendar; charset=utf-8")
        .header("If-Match", format!("\"{etag}\""))
        .timeout(REPORT_TIMEOUT)
        .body(ics.to_string())
        .send()
        .await
        .map_err(|error| format!("Verbindungsfehler: {}", crate::format_reqwest_error(&error)))?;

    let status = antwort.status();
    let text = read_body(antwort).await?;

    if status.is_success() || status == reqwest::StatusCode::NO_CONTENT {
        return Ok(());
    }

    if status.as_u16() == 412 {
        return Err(
            "Der Termin wurde zwischenzeitlich in Nextcloud verändert. Mimir schreibt nichts \
             darüber. In Nextcloud nachsehen und es erneut versuchen."
                .to_string(),
        );
    }

    if status.as_u16() == 403 {
        return Err(
            "Die Instanz verweigert das Ändern. In Nextcloud prüfen, ob der Kalender \
             beschreibbar ist und ob die CalDAV-Rechte stimmen."
                .to_string(),
        );
    }

    Err(describe_dav_error(status, &text))
}

/// Löscht einen Termin, mit derselben Absicherung wie beim Ändern.
pub async fn delete_event(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
    url: &str,
    etag: &str,
) -> Result<(), String> {
    if etag.is_empty() {
        return Err("Ohne Änderungskennung wird nicht gelöscht.".to_string());
    }

    let antwort = client
        .delete(url)
        .basic_auth(&config.username, Some(password))
        .header("If-Match", format!("\"{etag}\""))
        .timeout(REPORT_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("Verbindungsfehler: {}", crate::format_reqwest_error(&error)))?;

    let status = antwort.status();
    let text = read_body(antwort).await?;

    if status.is_success() || status == reqwest::StatusCode::NO_CONTENT {
        return Ok(());
    }

    if status.as_u16() == 412 {
        return Err(
            "Der Termin wurde zwischenzeitlich in Nextcloud verändert. Mimir löscht ihn \
             deshalb nicht; in Nextcloud nachsehen."
                .to_string(),
        );
    }

    if status.as_u16() == 404 {
        return Err("Der Termin ist im Kalender nicht mehr vorhanden.".to_string());
    }

    Err(describe_dav_error(status, &text))
}

/// Die Adresse, unter der der Termin abgelegt wird.
///
/// Der Kalendername ist der letzte Pfadabschnitt, wie beim Lesen; die Adresse
/// entsteht deshalb aus dem Sammelpfad der Kalender, den Mimir für den Abruf
/// schon benutzt. So wandert kein fremder Pfad aus einer Serverantwort in einen
/// Schreibaufruf, und Lesen und Schreiben zeigen garantiert auf denselben Ort.
fn event_url(config: &CalendarConfig, plan: &super::write::EventPlan) -> Result<String, String> {
    let kalender = super::write::escape_calendar_segment(&plan.calendar_href)?;

    if plan.file_name.contains('/') || plan.file_name.contains("..") {
        return Err("Der Dateiname des Termins ist ungültig".to_string());
    }

    Ok(format!(
        "{}{}/{}",
        config.calendars_root()?,
        kalender,
        plan.file_name
    ))
}

/// Nur für die Tests: die Adresse ohne Netz zu prüfen.
#[cfg(test)]
pub fn event_url_for_test(config: &CalendarConfig, plan: &super::write::EventPlan) -> String {
    event_url(config, plan).expect("Adresse muss bildbar sein")
}

/// Legt einen Termin im Kalender an.
///
/// `If-None-Match: *` sorgt dafür, dass nichts Bestehendes überschrieben wird:
/// Sollte die Kennung wider Erwarten schon vergeben sein, antwortet der Server
/// mit 412 und der Termin bleibt unangetastet.
pub async fn create_event(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
    plan: &super::write::EventPlan,
) -> Result<String, String> {
    let url = event_url(config, plan)?;

    // Umleitungen werden nicht verfolgt: Der Client ist so gebaut, damit das
    // Passwort keinen Weg auf einen anderen Host findet.
    let request = client
        .put(&url)
        .basic_auth(&config.username, Some(password))
        .header("Content-Type", "text/calendar; charset=utf-8")
        .header("If-None-Match", "*")
        .timeout(REPORT_TIMEOUT)
        .body(plan.ics.clone());

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let text = read_body(response).await?;

            if status.is_success() || status == reqwest::StatusCode::CREATED {
                return Ok(url);
            }

            if status.as_u16() == 412 {
                return Err(
                    "Ein Termin mit dieser Kennung besteht auf dem Server schon; es wurde \
                     nichts überschrieben."
                        .to_string(),
                );
            }

            if status.is_redirection() {
                // Wie beim Lesen: Die Umleitung wird nicht verfolgt, weil sie das
                // Passwort an einen anderen Host tragen könnte. Der Benutzer muss
                // die echte WebDAV-Adresse eintragen.
                return Err(format!(
                    "Die Adresse leitet um. Beim Schreiben wird eine Umleitung nicht \
                     verfolgt. In Nextcloud unter Einstellungen, WebDAV die dort \
                     angezeigte Adresse übernehmen. Status {}.",
                    status.as_u16()
                ));
            }

            if status.as_u16() == 507 {
                return Err(
                    "Der Kalender hat keine freie Kapazität mehr. In Nextcloud prüfen, ob die \
                     Speicherquote des Kontos erschöpft ist."
                        .to_string(),
                );
            }

            Err(describe_dav_error(status, &text))
        }
        Err(error) => Err(format!(
            "Verbindungsfehler: {}",
            crate::format_reqwest_error(&error)
        )),
    }
}

/// Meldet sich an und liefert Kalender und Serverversion.
pub async fn login(
    client: &reqwest::Client,
    config: &CalendarConfig,
    password: &str,
) -> Result<Session, String> {
    let calendars = list_calendars(client, config, password).await?;

    if calendars.is_empty() {
        return Err(
            "Die Anmeldung hat geklappt, es wurden aber keine Kalender gefunden. \
                    Läuft die Kalender-App auf der Instanz?"
                .to_string(),
        );
    }

    Ok(Session {
        version: fetch_version(client, config).await,
        calendars,
    })
}

/// Nur für die Anzeige, damit der Aufrufer die Zeit nicht selbst ermitteln muss.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests;
