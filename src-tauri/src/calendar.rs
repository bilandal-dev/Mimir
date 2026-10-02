//! CalDAV-Zugriff auf eine Nextcloud-Instanz im lokalen Netz.
//!
//! Aufbau: Einstellungen, Adressprüfung, Zertifikatsentscheidung, die Ablage des
//! App-Passworts und der Sitzungszustand stehen hier, der Netzteil in `client.rs`,
//! das Aufbereiten der Termine in `events.rs`, das Planen eines neuen Termins in
//! `write.rs`, das Planen von Änderung und Löschung in `edit.rs` und das
//! Zeileneditieren einer bestehenden ICS-Datei in `ics.rs`. Die Aufteilung ist
//! nötig, weil die Zeitrechnung und das Editieren für sich getestet werden sollen,
//! ohne einen Server zu brauchen.
//!
//! Der Schreibpfad ist bewusst schmal: Das Anlegen, Ändern und Löschen läuft
//! ausschließlich über die Werkzeuge des Agentenmodus und damit über die
//! Bestätigung im Fenster. Es gibt keine Oberfläche, die einen Termin ohne
//! Rückfrage verändert.

pub mod client;
pub mod edit;
pub mod events;
pub mod ics;
pub mod write;

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

/// Obergrenze für die DAV-Adresse. Länger ist keine Serveradresse, sondern ein
/// Tippfehler.
const MAX_DAV_URL_BYTES: usize = 2048;
const MAX_DAV_USER_BYTES: usize = 128;
/// So viele Termine kommen zurück. Alles darüber wird abgeschnitten, nicht
/// abgewiesen: Die Leiste soll nicht an einer Datenmenge hängen bleiben. Die Zahl
/// ist bewusst größer als die acht Einträge, die die Leiste zeigt, damit „Alle“ mehr
/// anzeigt als die Vorschau.
const MAX_CALENDAR_EVENTS: usize = 200;
/// Obergrenze für eine einzelne ICS-Antwort. Größere Körper werden abgewiesen,
/// statt beliebig viel Speicher zu belegen.
const MAX_ICS_BYTES: usize = 8 * 1024 * 1024;
/// Kein `SUMMARY` und kein `LOCATION` aus dem Kalender soll die Leiste sprengen.
/// Achtung: Der Titel wird in Zeichen gezählt (`events.rs`), der Ort in Oktett –
/// bei Umlauten ist der Ortswert also strenger.
const MAX_EVENT_SUMMARY_BYTES: usize = 512;
const MAX_EVENT_LOCATION_BYTES: usize = 256;
/// So viele Kategorien werden je Termin übernommen. Die Leiste kann eine Hand
/// voll zeigen; alles darüber ist Rauschen aus der Datei.
pub const MAX_EVENT_CATEGORIES: usize = 12;

/// Einstellungen der Kalenderleiste. Das App-Passwort gehört ausdrücklich nicht
/// dazu: Es steckt entweder im Sitzungszustand oder – auf Wunsch – in
/// `calendar-secret.json` (`StoredCredential`), niemals in `ollama.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CalendarConfig {
    /// Basisadresse der Instanz, etwa `https://cloud.example.org`. Leer heißt:
    /// noch keine Instanz eingetragen.
    ///
    /// Ohne Vorgabewert, und das ist Absicht: Eine eingebaute Adresse landet als
    /// Zeichenkette im Binary und ließe jeden, der die App bekommt, auf der
    /// Instanz des Entwicklers starten. Der Platzhalter im Eingabefeld ist ein
    /// Beispiel, kein Wert.
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub username: String,
    /// Nur diese Kalender abrufen. Leer heißt: alle lesbaren.
    #[serde(default)]
    pub calendars: Vec<String>,
    /// Das bestätigte Zertifikat der Instanz, kodiert. Nur bei `https`
    /// vorhanden; ohne diesen Wert verweigert Mimir den Zugang, weil es die
    /// Zertifikatskette der Instanz nicht prüfen kann.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_certificate: Option<String>,
}

impl CalendarConfig {
    /// Steht eine Anmeldung für den Kalender? Ohne sie gibt es kein Werkzeug:
    /// Das Modell würde es ankündigen und an jedem Aufruf scheitern.
    pub fn logged_in_hint(&self) -> bool {
        !self.server_url.is_empty() && !self.username.is_empty()
    }

    /// Läuft die Verbindung über HTTPS? Dann braucht es eine Zertifikatsentscheidung.
    pub fn uses_tls(&self) -> bool {
        self.server_url.starts_with("https://")
    }

    /// Nur der Fingerabdruck des hinterlegten Zertifikats, für die Anzeige.
    pub fn server_certificate_fingerprint(&self) -> Option<String> {
        self.server_certificate
            .as_deref()
            .map(crate::calendar::client::fingerprint_of)
    }

    /// Die Basis der Instanz, ohne DAV-Anhang.
    ///
    /// In den Nextcloud-Einstellungen steht als WebDAV-Adresse gewöhnlich
    /// `…/remote.php/dav/principals/users/<name>/`. Die ist zwar richtig, als
    /// Konfiguration aber unbrauchbar: Mimir hängt den Sammelpfad der Kalender
    /// selbst an und würde daraus `…/principals/users/<name>/calendars/…`
    /// bauen. Deshalb wird der DAV-Anhang abgeschnitten und die Adresse als das
    /// gespeichert, was sie eigentlich ist: die Instanz.
    pub fn instance_base(&self) -> String {
        let base = self.server_url.trim_end_matches('/');
        match base.find("/remote.php") {
            Some(position) => base[..position].trim_end_matches('/').to_string(),
            None => base.to_string(),
        }
    }

    /// Der Benutzername, der in einer vollständigen WebDAV-Adresse steckt.
    ///
    /// Wird die aus den Nextcloud-Einstellungen kopierte Adresse eingetragen,
    /// braucht es den Namen nicht zusätzlich: Er steht im Pfad.
    pub fn username_from_dav_path(&self) -> Option<String> {
        let base = self.server_url.trim_end_matches('/');
        let rest = base.split("/remote.php/dav/").nth(1)?;
        let mut teile = rest.split('/').filter(|teil| !teil.is_empty());

        match (teile.next(), teile.next(), teile.next()) {
            (Some("principals"), Some("users"), Some(name)) if !name.is_empty() => {
                Some(percent_decode(name))
            }
            _ => None,
        }
    }

    /// Der DAV-Wurzelbereich der Instanz, der Sammelpfad aller Kalender darunter.
    ///
    /// Der Standardpfad wird immer angehängt, auch wenn die eingegebene Adresse
    /// selbst schon eine DAV-Adresse war: `instance_base()` hat jeden
    /// `/remote.php`-Anhang abgeschnitten, es kann also kein doppelter entstehen.
    pub fn dav_root(&self) -> Result<String, String> {
        let base = self.instance_base();

        if base.is_empty() {
            return Err("Keine Instanz eingetragen".to_string());
        }

        Ok(format!("{base}/remote.php/dav"))
    }

    /// Sammelpfad der Kalender dieses Benutzers.
    pub fn calendars_root(&self) -> Result<String, String> {
        let username = escape_dav_segment(&self.username)?;

        Ok(format!("{}/calendars/{}/", self.dav_root()?, username))
    }
}

/// Prüft die Basisadresse und normalisiert sie.
///
/// Beide Schemata sind erlaubt: Viele Nextcloud-Instanzen im LAN liefern nur
/// HTTPS, und dann mit einem selbstsignierten Zertifikat, das separat bestätigt
/// werden muss. Eingebettete Zugangsdaten, Abfragen und Sprünge auf andere Hosts
/// bleiben ausgeschlossen.
pub fn normalize_server_url(raw: &str) -> Result<String, String> {
    let value = raw.trim();

    if value.is_empty() {
        return Err("Die Server-Adresse darf nicht leer sein".to_string());
    }

    if value.len() > MAX_DAV_URL_BYTES {
        return Err("Die Server-Adresse ist zu lang".to_string());
    }

    if value.chars().any(char::is_whitespace) {
        return Err("Die Server-Adresse darf keine Leerzeichen enthalten".to_string());
    }

    if value.starts_with('/') {
        return Err("Die Server-Adresse braucht Hostnamen oder IP-Adresse".to_string());
    }

    // Ohne Schema ergänzen: getippt wird meist nur die Adresse.
    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };

    let url = reqwest::Url::parse(&candidate)
        .map_err(|_| "Die Server-Adresse ist keine gültige URL".to_string())?;

    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("Die Adresse muss http oder https verwenden".to_string());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "Die Server-Adresse muss einen Hostnamen enthalten".to_string())?
        .to_string();

    if host.contains('@') || url.username() != "" || url.password().is_some() {
        return Err("Die Server-Adresse darf keine Zugangsdaten enthalten".to_string());
    }

    if url.query().is_some() || url.fragment().is_some() {
        return Err(
            "Die Server-Adresse darf keine Abfrage und keinen Sprung enthalten, \
             bei einer Installation im Unterverzeichnis genügt der Pfad"
                .to_string(),
        );
    }

    if url.port() == Some(0) {
        return Err("Der Server-Port muss zwischen 1 und 65535 liegen".to_string());
    }

    if host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_unspecified())
    {
        return Err("Die Server-Adresse ist nicht eindeutig".to_string());
    }

    let path = url.path().trim_end_matches('/').to_string();
    let scheme = url.scheme();

    Ok(format!(
        "{scheme}://{host}{}{path}",
        match url.port() {
            Some(port) => format!(":{port}"),
            None => String::new(),
        }
    ))
}

/// Der Benutzername landet im DAV-Pfad, deshalb muss er escapet werden.
pub fn validate_username(raw: &str) -> Result<String, String> {
    let value = raw.trim();

    if value.is_empty() {
        return Err("Ohne Benutzernamen lässt sich der Kalender nicht abrufen".to_string());
    }

    if value.len() > MAX_DAV_USER_BYTES {
        return Err("Der Benutzername ist zu lang".to_string());
    }

    if value
        .chars()
        .any(|character| character.is_control() || character == '/')
    {
        return Err("Der Benutzername darf keine Schrägstriche enthalten".to_string());
    }

    Ok(value.to_string())
}

/// Was die Oberfläche über das Zertifikat der Instanz wissen muss.
#[derive(Serialize, Clone, Debug)]
pub struct CertificateStatus {
    /// `ohne`, `ungeprüft`, `bestätigt` oder `geändert`.
    pub state: String,
    pub fingerprint: Option<String>,
    pub host: String,
    /// Zusätzlicher Hinweis, vor allem beim Wechsel des Zertifikats.
    pub detail: Option<String>,
}

/// Baut die Konfiguration für eine Adresse, die noch gar nicht eingetragen ist.
///
/// Wird eine andere Adresse genannt als die gespeicherte, gilt kein bestätigtes
/// Zertifikat: Eine Freigabe für `a` sagt nichts über `b`. Sonst würde die
/// Prüfung eine ungeprüfte Adresse mit der Freigabe einer anderen unterlaufen.
pub fn config_for_address(
    server_url: Option<String>,
    gespeichert: &CalendarConfig,
) -> Result<CalendarConfig, String> {
    let Some(raw) = server_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(gespeichert.clone());
    };

    let server_url = normalize_server_url(&raw)?;
    let same = server_url == gespeichert.server_url;

    Ok(CalendarConfig {
        server_url,
        username: gespeichert.username.clone(),
        calendars: if same {
            gespeichert.calendars.clone()
        } else {
            Vec::new()
        },
        server_certificate: if same {
            gespeichert.server_certificate.clone()
        } else {
            None
        },
    })
}

/// Prüft das Zertifikat, bevor irgendein Passwort gesendet wird.
///
/// Ohne bestätigtes Zertifikat gibt es bei `https` keinen Zugriff: Eine
/// selbstsignierte Instanz lässt sich mit der Prüfung der Kette nicht
/// erreichen, und sie ungeprüft zu nutzen hieße, jedem TLS-Server zu trauen.
/// Deshalb wird der Fingerabdruck einmalig im Fenster bestätigt und danach
/// verglichen.
pub async fn ensure_certificate(config: &CalendarConfig) -> Result<(), String> {
    let client = client::build_client(config.uses_tls())?;

    match client::probe_certificate(&client, config).await? {
        client::CertificateCheck::NotApplicable | client::CertificateCheck::Matches => Ok(()),
        client::CertificateCheck::NeedsApproval(certificate) => Err(format!(
            "CERTIFICATE:Das Zertifikat von {} ist nicht bestätigt. Fingerabdruck: {}",
            config.server_url, certificate.fingerprint
        )),
        client::CertificateCheck::Changed { expected, found } => Err(format!(
            "CERTIFICATE:Das Zertifikat von {} hat sich geändert. Bestätigt war {}, \
             angeboten wird {}.",
            config.server_url, expected, found.fingerprint
        )),
    }
}

/// Fragt das Zertifikat ab und beschreibt den Zustand, ohne etwas zu ändern.
pub async fn certificate_status(config: &CalendarConfig) -> Result<CertificateStatus, String> {
    let client = client::build_client(config.uses_tls())?;

    Ok(match client::probe_certificate(&client, config).await? {
        client::CertificateCheck::NotApplicable => CertificateStatus {
            state: "ohne".to_string(),
            fingerprint: None,
            host: config.server_url.clone(),
            detail: Some(
                "Die Adresse läuft über HTTP, es wird kein Zertifikat benötigt.".to_string(),
            ),
        },
        client::CertificateCheck::Matches => CertificateStatus {
            state: "bestätigt".to_string(),
            fingerprint: config.server_certificate_fingerprint(),
            host: config.server_url.clone(),
            detail: None,
        },
        client::CertificateCheck::NeedsApproval(certificate) => CertificateStatus {
            state: "ungeprüft".to_string(),
            fingerprint: Some(certificate.fingerprint),
            host: config.server_url.clone(),
            detail: Some(
                "Das Zertifikat ist selbstsigniert und muss einmal bestätigt werden.".to_string(),
            ),
        },
        client::CertificateCheck::Changed { expected, found } => CertificateStatus {
            state: "geändert".to_string(),
            fingerprint: Some(found.fingerprint),
            host: config.server_url.clone(),
            detail: Some(format!("Bisher bestätigt war {expected}.")),
        },
    })
}

/// Auf der Platte gehaltenes App-Passwort.
///
/// Anders als das Hauptpasswort des Kontos ist ein App-Passwort eigens dafür
/// gedacht, in fremden Programmen zu liegen: Es gilt nur für CalDAV und lässt
/// sich in Nextcloud jederzeit widerrufen, ohne dass das Hauptpasswort geändert
/// werden muss. Gespeichert wird es deshalb auf Wunsch, damit die Anmeldung nach
/// einem Neustart nicht neu gemacht werden muss.
///
/// Die Datei bekommt die Rechte 0600 und liegt in einer eigenen Datei, nicht in
/// `ollama.json`: Die Konfiguration wird gern kopiert oder übertragen, und
/// darin soll kein Zugangsdaten stehen.
#[derive(Serialize, Deserialize)]
pub struct StoredCredential {
    pub username: String,
    pub app_password: String,
}

impl Drop for StoredCredential {
    fn drop(&mut self) {
        // Beim Freigeben überschreiben, damit das Passwort nicht im
        // Standardspeicher des Prozesses liegen bleibt.
        self.app_password.zeroize();
    }
}

/// Liest das gespeicherte App-Passwort, falls eines da ist und es zum
/// eingetragenen Benutzer gehört.
pub fn read_stored_credential(path: &Path, username: &str) -> Option<StoredCredential> {
    let contents = std::fs::read_to_string(path).ok()?;
    let stored: StoredCredential = serde_json::from_str(&contents).ok()?;

    if stored.username != username || stored.app_password.is_empty() {
        return None;
    }

    Some(stored)
}

/// Schreibt das App-Passwort mit den Rechten 0600 und atomar.
pub fn write_stored_credential(path: &Path, username: &str, password: &str) -> Result<(), String> {
    let payload = StoredCredential {
        username: username.to_string(),
        app_password: password.to_string(),
    };
    let serialized = serde_json::to_string(&payload)
        .map_err(|error| format!("Zugangsdaten nicht serialisierbar: {}", error))?;

    let parent = crate::config_parent(path);
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Konfigurationsverzeichnis nicht erstellbar: {}", error))?;
    let (temporary_path, mut temporary_file) = crate::create_temporary_config_file(parent, path)?;
    let result = (|| -> Result<(), String> {
        temporary_file
            .write_all(serialized.as_bytes())
            .map_err(|error| format!("Zugangsdaten nicht schreibbar: {}", error))?;
        temporary_file
            .sync_all()
            .map_err(|error| format!("Zugangsdaten nicht synchronisierbar: {}", error))?;
        drop(temporary_file);
        std::fs::rename(&temporary_path, path)
            .map_err(|error| format!("Zugangsdaten nicht atomar ersetzbar: {}", error))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }

    result
}

/// Entfernt das gespeicherte App-Passwort. Ein fehlendes Verzeichnis ist kein
/// Fehler: Es gibt dann nichts zu löschen.
pub fn delete_stored_credential(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Zugangsdaten nicht löschbar: {}", error)),
    }
}

/// Ein aus dem Fenster kopiertes App-Passwort bringt oft ein Leerzeichen oder
/// einen Zeilenumbruch mit. Unbemerkt gesendet wäre das ein abgelehntes
/// Passwort ohne sichtbaren Grund, deshalb wird hier einmal getrimmt.
pub fn trimmed_app_password(raw: &str) -> String {
    raw.trim().to_string()
}

/// App-Passwort für die laufende Sitzung. Das Passwort kann auf Wunsch von der
/// Platte kommen; im Arbeitsspeicher steht es in jedem Fall nur in einer
/// `Zeroizing`-Hülle, die beim Freigeben überschrieben wird.
pub struct CalendarSession {
    secret: Mutex<Option<Zeroizing<String>>>,
    /// Wann wurde zuletzt erfolgreich geholt worden. Für die Anzeige in der
    /// Leiste.
    last_success: Mutex<Option<i64>>,
    /// Aus der letzten Anmeldung bekannte Kalender, damit die Leiste ihren
    /// Stand auch nach einem Neustart zeigen kann.
    calendars: Mutex<Vec<client::CalendarInfo>>,
    /// Version der Instanz, wenn sie sich nennen lässt.
    version: Mutex<Option<String>>,
}

impl Default for CalendarSession {
    fn default() -> Self {
        Self {
            secret: Mutex::new(None),
            last_success: Mutex::new(None),
            calendars: Mutex::new(Vec::new()),
            version: Mutex::new(None),
        }
    }
}

impl CalendarSession {
    pub fn set_password(&self, password: &str) -> Result<(), String> {
        let value = password.trim();

        if value.is_empty() {
            return Err("Das App-Passwort darf nicht leer sein".to_string());
        }

        // Die Grenze ist aus dem Benutzernamens-Limit abgeleitet, nicht selbst
        // gesetzt: Nextcloud-App-Passwörter sind deutlich kürzer, 512 Zeichen
        // sind nur eine grobe Obergrenze gegen eine Tipperei im Prompt.
        if value.len() > MAX_DAV_USER_BYTES * 4 {
            return Err("Das App-Passwort ist unplausibel lang".to_string());
        }

        if value
            .chars()
            .any(|character| character.is_control() && character != '\t')
        {
            return Err("Das App-Passwort enthält unerlaubte Zeichen".to_string());
        }

        *self
            .secret
            .lock()
            .map_err(|_| "Sitzungszustand defekt".to_string())? =
            Some(Zeroizing::new(value.to_string()));

        Ok(())
    }

    pub fn password(&self) -> Option<Zeroizing<String>> {
        self.secret.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .map(|value| Zeroizing::new(value.to_string()))
        })
    }

    pub fn is_logged_in(&self) -> bool {
        self.password().is_some()
    }

    /// Die aus der letzten Anmeldung bekannten Kalender, für die Auswahl.
    pub fn calendars(&self) -> Vec<client::CalendarInfo> {
        self.calendars
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    pub fn set_calendars(&self, calendars: Vec<client::CalendarInfo>, version: Option<String>) {
        if let Ok(mut guard) = self.calendars.lock() {
            *guard = calendars;
        }

        if let Some(version) = version {
            if let Ok(mut guard) = self.version.lock() {
                *guard = Some(version);
            }
        }
    }

    pub fn forget(&self) {
        if let Ok(mut guard) = self.secret.lock() {
            guard.take();
        }
    }

    /// Merkt, dass ein Termin geschrieben wurde. Die Leiste holt ihre Termine
    /// danach neu, sonst bliebe der neue Eintrag bis zum nächsten Abruf unsichtbar.
    pub fn mark_calendar_write(&self) {
        if let Ok(mut guard) = self.last_success.lock() {
            *guard = None;
        }
    }

    pub fn mark_success(&self, seconds_since_epoch: i64) {
        if let Ok(mut guard) = self.last_success.lock() {
            *guard = Some(seconds_since_epoch);
        }
    }

    pub fn last_success(&self) -> Option<i64> {
        self.last_success.lock().ok().and_then(|guard| *guard)
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut ergebnis: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");

            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                ergebnis.push(byte);
                index += 3;
                continue;
            }
        }

        ergebnis.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&ergebnis).to_string()
}

fn escape_dav_segment(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_matches('/');

    if trimmed.is_empty() {
        return Err("Ohne Benutzernamen lässt sich der Kalender nicht abrufen".to_string());
    }

    // Ein Benutzername ist genau ein Pfadabschnitt. Punkte dürfen darin stehen,
    // aber keine zwei Punkte am Stück, sonst wäre es ein Sprung nach oben.
    if trimmed == "." || trimmed == ".." || trimmed.contains("..") || trimmed.contains('/') {
        return Err("Der Benutzername ist ungültig".to_string());
    }

    Ok(trimmed
        .chars()
        .filter(|character| !character.is_control())
        .map(|character| match character {
            ' ' => "%20".to_string(),
            other => other.to_string(),
        })
        .collect())
}

/// Was die Leiste und der Statusbefehl über die Verbindung wissen.
#[derive(Serialize, Clone, Debug)]
pub struct CalendarStatus {
    pub configured: bool,
    pub logged_in: bool,
    pub server_url: String,
    pub username: String,
    /// Vom Benutzer ausgewählte Kalender, leer heißt alle lesbaren.
    pub calendars: Vec<String>,
    /// Alle Kalender der letzten Anmeldung, für die Auswahl in der Leiste.
    pub known_calendars: Vec<client::CalendarInfo>,
    /// Von der Instanz genannte Version, wenn sie sich nennen lässt.
    pub version: Option<String>,
    /// Zeitpunkt des letzten erfolgreichen Abrufs.
    pub last_success: Option<i64>,
    /// Ob das App-Passwort auf Wunsch auf der Platte liegt. Ausgefüllt wird das
    /// Feld nur von `calendar_login`; `build_status` kann es nicht wissen, weil
    /// der Pfad zur Ablage nicht hier liegt.
    pub remembered: bool,
}

/// Baut den Zustand für die Leiste. Ohne Netzabruf, damit der Text immer da
/// ist und ein hängender Server die Anzeige nicht verzögert.
pub fn build_status(
    config: &CalendarConfig,
    session: &CalendarSession,
    error: Option<String>,
) -> CalendarStatus {
    // `error` wird absichtlich nicht ausgewertet: Fehler stehen in der Leiste und
    // nicht im Zustand, und der Zustand darf nie an einem Netzabruf hängen. Der
    // Parameter bleibt, damit sich die Signatur bei jedem Aufrufer nicht ändert.
    let _ = error;
    let known_calendars = session
        .calendars
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();

    CalendarStatus {
        configured: !config.server_url.is_empty() && !config.username.is_empty(),
        logged_in: session.is_logged_in(),
        server_url: config.server_url.clone(),
        username: config.username.clone(),
        calendars: config.calendars.clone(),
        known_calendars,
        version: session.version.lock().ok().and_then(|guard| guard.clone()),
        last_success: session.last_success(),
        remembered: false,
    }
}

/// Ein Termin in der Form, in der die Oberfläche ihn anzeigt. Die Zeitpunkte
/// kommen als Sekunden seit 1970, damit die Oberfläche sie in der Zeitzone des
/// Rechners darstellen kann. Ohne Zeitzone im Termin bleibt `floating` gesetzt:
/// dann zeigt die Leiste die Uhrzeit so an, wie sie in der Datei steht.
#[derive(Serialize, Clone, Debug)]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    #[serde(default)]
    pub location: String,
    pub start: i64,
    pub end: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub floating: bool,
    pub calendar: String,
    /// Derselbe Kalender als Pfad. Die Leiste zeigt den Namen, für das Öffnen
    /// und Speichern braucht es den Pfad: Zwei Kalender können ähnlich heißen,
    /// und ein Treffer auf Verdacht wäre stiller Datenverlust.
    #[serde(default)]
    pub calendar_href: String,
    /// Wie viele Minuten vor dem Beginn die Erinnerung klingelt. `None`, wenn der
    /// Termin keine hat oder ihr Wert nicht lesbar ist – geraten wird hier
    /// nichts.
    #[serde(default)]
    pub reminder: Option<i64>,
    /// Derselbe Wert als Text für die Anzeige, aus derselben Funktion wie im
    /// Bestätigungsfenster. Bewusst beide: die Oberfläche soll sich die Formulierung
    /// nicht ein zweites Mal ausdenken, das Modell liest dieselbe Formulierung.
    #[serde(default)]
    pub reminder_text: String,
    /// Die Kategorien, wie sie in der Datei stehen.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Termine mit Status CANCELED werden nicht angezeigt, aber für die
    /// Wiederholungsauflösung gebraucht.
    #[serde(default)]
    pub canceled: bool,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod passwort {
    use super::*;

    #[test]
    fn ein_kopiertes_passwort_ist_nach_dem_trimmen_richtig() {
        // Der Dialog in Nextcloud endet mit einem Zeilenumbruch, der beim
        // Kopieren mit wandert.
        assert_eq!(trimmed_app_password("geheim123\n"), "geheim123");
        assert_eq!(trimmed_app_password("  geheim123  "), "geheim123");
        assert_eq!(trimmed_app_password("geheim123"), "geheim123");
        // Ein Tab im Inneren wäre ein anderes Passwort und bleibt stehen.
        assert_eq!(trimmed_app_password("gei\theim"), "gei\theim");
        assert!(trimmed_app_password("  ").is_empty());
    }

    #[test]
    fn die_regeln_fuer_das_passwort_greifen_nach_dem_trimmen() {
        let session = CalendarSession::default();
        assert!(session.set_password(&trimmed_app_password("  ")).is_err());
        assert!(session
            .set_password(&trimmed_app_password("geheim\n"))
            .is_ok());
        assert_eq!(session.password().unwrap().as_str(), "geheim");
    }
}
