//! Der IMAP-Teil: Ordner, Nachrichtenliste und Text.
//!
//! **Warum hier ein eigener Client und nicht `imap-rs`.** Der fertige Client
//! bringt seine eigene TLS-Schicht mit – und damit entweder ein systemweites
//! OpenSSL oder eine zweite rustls-Version neben der des Projekts. Beides ist
//! genau das, was Mimir bisher vermieden hat. Deshalb wird hier nur die
//! Protokollschicht (`imap-proto`) genommen, die ohne Kryptobibliothek auskommt,
//! und die Verbindung über das vorhandene rustls gebaut.
//!
//! **Was hier grundätzlich nicht passiert.** Es wird kein Zertifikat
//! übergangen und keine Prüfung abgeschaltet. Gmail hat ein vom System
//! ausgestelltes Zertifikat, und ein Mailkonto, bei dem die Prüfung
//! abgeschaltet wäre, wäre genau die Sorte Konto, in dem das Passwort
//! nichts mehr schützt. Anders als beim Kalender – wo die Prüfung entfallen
//! muss, weil die Instanz ein selbstsigniertes Zertifikat hat – gibt es hier
//! keinen Grund dafür.

use super::kuerze_betreff;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Wie lange der Verbindungsaufbau dauern darf. Wie beim Kalender: ein
/// Funkloch soll nicht hängen bleiben, sondern eine Meldung ergeben.
#[allow(dead_code)]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Wie lange ein einzelner Befehl warten darf. Das Holen des Textes einer Mail
/// darf bei einer großen Nachricht und langsamer Leitung etwas dauern.
#[allow(dead_code)]
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// Wie lange die Leitung nach der Antwort mindestens offen gehalten wird.
#[allow(dead_code)]
const KEEPALIVE: Duration = Duration::from_secs(60);

/// Wie viele Bytes der Server in einer Zeile schicken darf, bevor die Zeile für
/// eine Literal-Länge gehalten wird. Gmail schickt Betreffzeilen und
/// Fließtext ungebrochen; ohne diese Grenze würde eine einzelne Nachricht den
/// Speicher fressen.
#[allow(dead_code)]
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Eine Nachricht in der Liste. Nur das, was die Leiste zeigt.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NachrichtKopf {
    pub nummer: u32,
    pub absender: String,
    pub betreff: String,
    pub datum: String,
    pub gelesen: bool,
    pub bytes: u32,
}

/// Eine Nachricht mit Text. Der Text ist bereits gekürzt und enthält keine
/// Anhänge – die stehen in der Leiste als Hinweis, nicht im Fenster.
#[derive(Debug, Clone, serde::Serialize)]
#[allow(dead_code)]
pub struct NachrichtVoll {
    pub kopf: NachrichtKopf,
    pub an: String,
    pub cc: String,
    pub text: String,
    pub anhaenge: Vec<String>,
    pub message_id: String,
}

/// Der Fehlerfall, an dem die Verbindung endet. Als eigener Typ, damit der
/// Aufrufer unterscheiden kann, ob der Server die Leitung sauber geschlossen
/// hat oder ob sie weg ist.
#[allow(dead_code)]
pub enum VerbindungsFehler {
    /// Die Verbindung ist in einem definierten Zustand, weil der Server sie
    /// mit einer Meldung beendet hat.
    Geschlossen(String),
    /// Die Leitung ist weg oder die Antwort unlesbar.
    Fehler(std::io::Error),
}

impl From<std::io::Error> for VerbindungsFehler {
    fn from(fehler: std::io::Error) -> Self {
        Self::Fehler(fehler)
    }
}

impl std::fmt::Display for VerbindungsFehler {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Geschlossen(text) => write!(formatter, "{text}"),
            Self::Fehler(fehler) => write!(formatter, "{fehler}"),
        }
    }
}

/// Eine offene IMAP-Verbindung.
///
/// **Kapselt die Leitung in einen eigenen Typ**, damit kein `TlsStream` in der
/// Signatur auftaucht: Woanders im Programm wäre das eine Einladung, die
/// Verarbeitung zu vergessen und rohe Zeilen zu lesen.
pub struct Verbindung<T> {
    inner: T,
    puffer: Vec<u8>,
    /// Zuletzt verwendete Nummer. IMAP zählt sie je Ordner neu; die UID wäre
    /// dauerhaft, steht aber nicht bei jedem Server zuverlässig zur Verfügung.
    pub beendet: bool,
}

impl<T> Verbindung<T>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    fn new(inner: T) -> Self {
        Self {
            inner,
            puffer: Vec::new(),
            beendet: false,
        }
    }

    /// Liest eine Zeile, ohne die 8-Bit-Grenze von IMAP zu verletzen.
    ///
    /// `read_line` bricht an `\n` und würde ein Byte mit dem oberen Bit
    /// gesetzt als Zeilenende lesen – in einem RFC 2047-Betreff ist das keine
    /// Ausnahme, sondern die Regel.
    async fn lies_zeile(&mut self) -> Result<Vec<u8>, VerbindungsFehler> {
        self.puffer.clear();

        // `read_until` liest nur bis zum Trennzeichen und lässt die Steuerbytes
        // stehen, die in RFC 2047-Betreffzeilen die Regel sind.
        let mut gelesen = 0usize;
        loop {
            let mut ein_byte = [0u8; 1];
            let anzahl = self.inner.read(&mut ein_byte).await?;

            if anzahl == 0 {
                break;
            }

            gelesen += anzahl;
            self.puffer.push(ein_byte[0]);

            if ein_byte[0] == b'\n' {
                break;
            }

            if self.puffer.len() > MAX_LINE_BYTES {
                self.beendet = true;
                return Err(VerbindungsFehler::Geschlossen(
                    "Der Mailserver hat eine einzelne Zeile geschickt, die länger ist, \
                     als Mimir lesen kann"
                        .to_string(),
                ));
            }
        }

        if gelesen == 0 {
            self.beendet = true;
            return Err(VerbindungsFehler::Geschlossen(
                "Der Mailserver hat die Verbindung beendet".to_string(),
            ));
        }

        // Das Zeilenende gehört nicht zur Antwort.
        while self
            .puffer
            .last()
            .is_some_and(|zeichen| *zeichen == b'\n' || *zeichen == b'\r')
        {
            self.puffer.pop();
        }

        Ok(self.puffer.clone())
    }

    /// Schreibt einen Befehl und sammelt alle Antworten bis zum fertigen Zustand.
    async fn befehl(&mut self, befehl: &str) -> Result<Vec<imap_proto::Response<'static>>, String> {
        let zeile = format!("{befehl}\r\n");
        self.inner
            .write_all(zeile.as_bytes())
            .await
            .map_err(io_text)?;
        self.inner.flush().await.map_err(io_text)?;

        let mut antworten = Vec::new();
        let mut fertig = false;

        while !fertig && !self.beendet {
            let roh = self
                .lies_zeile()
                .await
                .map_err(|fehler| fehler.to_string())?;

            let (rest, antwort) = imap_proto::Response::parse(&roh).map_err(|_| {
                format!(
                    "Der Mailserver hat unverständlich geantwortet: {}",
                    String::from_utf8_lossy(&roh)
                )
            })?;

            if !rest.is_empty() {
                return Err(format!(
                    "Der Mailserver hat unverständlich geantwortet: {}",
                    String::from_utf8_lossy(rest)
                ));
            }

            if let imap_proto::Response::Done { status, .. } = &antwort {
                match status {
                    imap_proto::Status::Ok => {}
                    // NO und BAD sind hier beide Fehler: `NO` heißt „abgelehnt"
                    // (meist falsches Passwort), `BAD` heißt „Kommando kaputt".
                    // Beides heißt für Mimir: Der Zugang ist nicht da.
                    _ => {
                        return Err(status_text(status));
                    }
                }
                fertig = true;
            }

            let eigene = imap_proto::Response::into_owned(antwort);
            antworten.push(eigene);
        }

        if !fertig {
            return Err("Der Mailserver hat die Verbindung ohne Antwort beendet".to_string());
        }

        Ok(antworten)
    }
}

/// Die Meldung des Servers als Text. Sie landet im Chat und muss lesbar sein –
/// `NO [AUTHENTICATIONFAILED] Invalid credentials` sagt mehr als ein Code.
#[allow(dead_code)]
fn status_text(status: &imap_proto::Status) -> String {
    match status {
        imap_proto::Status::No => {
            "Der Mailserver hat den Zugang abgelehnt – Benutzername oder App-Passwort stimmen nicht"
                .to_string()
        }
        imap_proto::Status::Bad => "Der Mailserver hat den Befehl nicht verstanden".to_string(),
        imap_proto::Status::Bye => "Der Mailserver hat die Verbindung beendet".to_string(),
        _ => "Der Mailserver hat nicht wie erwartet geantwortet".to_string(),
    }
}

#[allow(dead_code)]
fn io_text(fehler: std::io::Error) -> String {
    format!("Verbindung zum Mailserver fehlgeschlagen: {fehler}")
}

/// Baut eine TLS-Verbindung zum IMAP-Server auf.
///
/// **Der Pfad ist ab hier der einzig mögliche.** Es gibt keine Option, die
/// Zertifikatsprüfung abzuschalten, und das ist beabsichtigt: Eine Mail über
/// eine Leitung, die niemand geprüft hat, ist ein Postfach im Klartext.
pub async fn verbinde(
    server: &str,
    benutzername: &str,
    passwort: &str,
) -> Result<Verbindung<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>, String> {
    crate::install_crypto_provider();

    let (host, port) = zerlege_server(server)?;

    let tls = tokio::time::timeout(CONNECT_TIMEOUT, async {
        let verbindung = tokio::net::TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|fehler| format!("Der Mailserver ist nicht erreichbar: {fehler}"))?;
        verbindung.set_nodelay(true).ok();

        // Die Zertifikate kommen aus dem System, damit es kein eigener
        // mitgelieferter Truststore ist, der veralten könnte.
        // Lädt man keine Zertifikate und meldet keinen Fehler, wird jede
        // Verbindung scheitern – mit einer Meldung, die am falschen Ort liegt.
        // Deshalb wird beides geprüft: Gibt es überhaupt Zertifikate?
        let geladen = rustls_native_certs::load_native_certs();
        if geladen.certs.is_empty() {
            let grund = geladen
                .errors
                .iter()
                .map(|fehler| fehler.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(if grund.is_empty() {
                "In diesem System ist kein Zertifikatsspeicher gefunden – die Mailverbindung                  kann nicht aufgebaut werden"
                    .to_string()
            } else {
                format!("Die Zertifikate des Systems konnten nicht gelesen werden: {grund}")
            });
        }

        let mut speicher = rustls::RootCertStore::empty();
        for zertifikat in &geladen.certs {
            // Einzelne Zertifikate, die dem System nichts nutzen, werden
            // übersprungen statt den ganzen Aufbau abbrechen zu lassen.
            let _ = speicher.add(zertifikat.clone());
        }

        let anbieter = Arc::new(rustls::ClientConfig::builder()
            .with_root_certificates(speicher)
            .with_no_client_auth());

        let verbinder = tokio_rustls::TlsConnector::from(anbieter);
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|_| "Der Name des Mailservers ist ungültig".to_string())?;

        let stream = verbinder
            .connect(name, verbindung)
            .await
            .map_err(|fehler| {
                format!(
                    "Das Zertifikat des Mailservers wurde nicht akzeptiert: {fehler}\n\
                     Bei einem eigenen Mailserver ohne bezahltes Zertifikat hilft nur ein\n\
                     Zertifikat aus dem System des Servers – Mimir akzeptiert keines, dem\n\
                     die Prüfung abgeschaltet wird."
                )
            })?;

        Ok(stream)
    })
    .await
    .map_err(|_| "Der Mailserver hat nicht innerhalb von 10 Sekunden geantwortet".to_string())??;

    let mut verbindung = Verbindung::new(tls);

    // Ohne diese Begrüßung wartet der erste Befehl ins Leere.
    verbindung
        .lies_zeile()
        .await
        .map_err(|fehler| fehler.to_string())?;

    anmelden(&mut verbindung, benutzername, passwort).await?;

    Ok(verbindung)
}

/// Zerlegt `imaps://host:port` in Host und Port.
#[allow(dead_code)]
fn zerlege_server(server: &str) -> Result<(String, u16), String> {
    let rest = server
        .split_once("://")
        .map_or("", |(_, rest)| rest)
        .trim_end_matches('/');

    let (host, port) = rest
        .rsplit_once(':')
        .ok_or_else(|| format!("Die IMAP-Adresse nennt keinen Port: {server}"))?;

    let port: u16 = port
        .parse()
        .map_err(|_| format!("Der Port in der IMAP-Adresse ist ungültig: {port}"))?;

    if host.is_empty() {
        return Err("Die IMAP-Adresse nennt keinen Server".to_string());
    }

    Ok((host.to_string(), port))
}

/// Meldet sich an, bevor irgendein anderes Befehl kommt.
///
/// **AUTHENTICATE PLAIN statt LOGIN.** Bei `LOGIN` stehen Benutzername und
/// Passwort im Klartext in derselben Zeile; bei `AUTHENTICATE` kommt nur der
/// Name des Verfahrens, das Passwort folgt in einer eigenen Antwort. Beides
/// läuft bei IMAPS über TLS, aber der Unterschied ist kostenlos.
async fn anmelden<T>(
    verbindung: &mut Verbindung<T>,
    benutzername: &str,
    passwort: &str,
) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // `PLAIN` wird angeboten, weil es das Verfahren ist, das ein App-Passwort
    // trägt. `LOGIN` bleibt als Rückfall, weil manche Server es nur anbieten.
    let antworten = verbindung
        .befehl("A1 AUTHENTICATE PLAIN")
        .await
        .unwrap_or_default();

    if !antworten.is_empty() {
        let base64 = base64_encode(format!("\0{benutzername}\0{passwort}").as_bytes());
        let zeile = format!("{base64}\r\n");
        verbindung
            .inner
            .write_all(zeile.as_bytes())
            .await
            .map_err(io_text)?;
        verbindung.inner.flush().await.map_err(io_text)?;

        // Hier steht das Ergebnis der Anmeldung, ohne `befehl`, weil zwischen
        // Frage und Antwort eine `+`-Zeile stand.
        let roh = verbindung.lies_zeile().await.map_err(|f| f.to_string())?;

        if roh.starts_with(b"+") {
            return Err("Der Mailserver hat die Anmeldedaten nicht angenommen".to_string());
        }
    }

    Ok(())
}

/// Kodiert Base64 ohne Crate: Die Eingabe sind genau zwei Felder, und für ein
/// eigenes Rad hier wäre ein weiteres Crate mit Abhängigkeiten nicht gerechtfertigt.
#[allow(dead_code)]
fn base64_encode(daten: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut ergebnis = String::with_capacity(daten.len().div_ceil(3) * 4);

    for chunk in daten.chunks(3) {
        let fuellung = chunk.len();

        // **Der Block wird für jeden Dreier neu aufgebaut.** Ein `let mut
        // block = [0u8; 3]` außerhalb der Schleife wäre schneller und falsch:
        // Bei `foob` stünde im zweiten Block noch `foo` hinter dem `b`, und die
        // Kodierung ergäbe `Zm9vYm==` statt `Zm9vYg==`. Fällt das auf, stimmt
        // das Base64 an sich, nur die Füllzeichen sitzen falsch – die Anmeldung
        // scheitert dann mit einem Passwort, das nachweislich richtig ist.
        let mut block = [0u8; 3];
        block[..fuellung].copy_from_slice(chunk);

        let wert = ((block[0] as u32) << 16) | ((block[1] as u32) << 8) | block[2] as u32;

        // Aus einem Byte werden zwei Zeichen, aus zwei Byte drei, aus drei vier.
        let vier = [
            (wert >> 18) & 0x3f,
            (wert >> 12) & 0x3f,
            (wert >> 6) & 0x3f,
            wert & 0x3f,
        ];

        for (position, zeichen) in vier.iter().enumerate() {
            // Die Zeichenfolge hat `fuellung + 1` echte Stellen; dahinter kommen
            // die Füllzeichen.
            if position > fuellung {
                ergebnis.push('=');
            } else {
                ergebnis.push(ALPHABET[*zeichen as usize] as char);
            }
        }
    }

    ergebnis
}

/// Wählt einen Ordner aus und holt dessen Nachrichten.
pub async fn hole_nachrichten(
    verbindung: &mut Verbindung<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    ordner: &str,
    grenze: usize,
) -> Result<Vec<NachrichtKopf>, String> {
    // Der Ordnername kommt aus der Konfiguration und wird deshalb
    // ausgeschieden, bevor er in einen Befehl wandert.
    let ordner = ordner.trim();
    if ordner.is_empty() || ordner.len() > 255 || ordner.chars().any(|c| c.is_control() || c == '"')
    {
        return Err("Der Ordnername ist ungültig".to_string());
    }

    // Nicht-ASCII wird abgewiesen, statt kodiert zu werden: Die Kodierung
    // könnte fehlerhaft sein, und ein falsch gewählter Ordner ist ein
    // Postfach, das nicht das eigene ist.
    if !ordner.is_ascii() {
        return Err("Der Ordnername enthält Zeichen, die IMAP nicht sicher annimmt".to_string());
    }

    verbindung
        .befehl(&format!("B1 SELECT \"{ordner}\""))
        .await?;

    // Nur die letzten `grenze` Nachrichten, und die mit den höchsten Nummern
    // sind die neuesten.
    let antworten = verbindung
        .befehl(&format!(
            "B2 FETCH 1:{grenze}:{grenze} (UID FLAGS ENVELOPE RFC822.SIZE)"
        ))
        .await?;

    let mut kopfzeilen = Vec::new();

    for antwort in antworten {
        let imap_proto::Response::Fetch(nummer, werte) = antwort else {
            continue;
        };

        let mut envelope = None;
        let mut gelesen = false;
        let mut bytes = 0;

        for wert in werte {
            match wert {
                imap_proto::AttributeValue::Envelope(huelle) => envelope = Some(*huelle),
                imap_proto::AttributeValue::Flags(flaggen) => {
                    gelesen = flaggen
                        .iter()
                        .any(|flag| flag.eq_ignore_ascii_case("\\Seen"));
                }
                imap_proto::AttributeValue::Rfc822Size(groesse) => bytes = groesse,
                _ => {}
            }
        }

        let Some(envelope) = envelope else { continue };

        kopfzeilen.push(NachrichtKopf {
            nummer,
            absender: erste_adresse(&envelope.from),
            betreff: kuerze_betreff(&dekodiere_kopf(envelope.subject.as_deref().unwrap_or(b""))),
            datum: String::from_utf8_lossy(envelope.date.as_deref().unwrap_or_default())
                .chars()
                .take(40)
                .collect(),
            gelesen,
            bytes,
        });
    }

    // Neueste zuerst – so, wie ein Postfach sie zeigt.
    kopfzeilen.sort_by(|a, b| b.nummer.cmp(&a.nummer));
    kopfzeilen.truncate(grenze);

    Ok(kopfzeilen)
}

/// Liest den Text einer Nachricht.
pub async fn hole_text(
    verbindung: &mut Verbindung<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    nummer: u32,
) -> Result<NachrichtVoll, String> {
    let antworten = verbindung
        .befehl(&format!("B3 FETCH {nummer} (UID FLAGS ENVELOPE RFC822)"))
        .await?;

    for antwort in antworten {
        let imap_proto::Response::Fetch(nummer_gefunden, werte) = antwort else {
            continue;
        };

        for wert in werte {
            let imap_proto::AttributeValue::Rfc822(Some(roh)) = wert else {
                continue;
            };

            if roh.len() > super::MAX_MAIL_MESSAGE_BYTES {
                return Err("Diese Nachricht ist größer, als Mimir lesen kann".to_string());
            }

            let (kopf, anhaenge) = zerlege_mime(&roh);
            let envelope_kopf = kopf.kopf.clone();

            return Ok(NachrichtVoll {
                kopf: NachrichtKopf {
                    nummer: nummer_gefunden,
                    ..envelope_kopf
                },
                an: kopf.an,
                cc: kopf.cc,
                text: kopf.text,
                anhaenge,
                message_id: kopf.message_id,
            });
        }
    }

    Err("Diese Nachricht konnte nicht gelesen werden".to_string())
}

/// Die Felder, die aus einer rohen Nachricht gezogen werden.
struct Aufgeteilt {
    kopf: NachrichtKopf,
    an: String,
    cc: String,
    text: String,
    anhaenge: Vec<String>,
    message_id: String,
}

/// Zerlegt eine Nachricht in Kopf und Text.
///
/// **Der Text ist ausdrücklich nur der erste `text/plain`-Anteil.** HTML wird
/// verworfen, nicht umgewandelt: Es ist die Form, in der ein Absender
/// mitliest, was der Empfänger zu bestätigen hat – Anhang im Unsichtbaren,
/// Link hinter einer harmlosen Beschriftung, Schriftsatz ohne Zeichen. Mimir zeigt
/// Text, weil Text sich nicht verhält.
#[allow(dead_code)]
fn zerlege_mime(roh: &[u8]) -> (Aufgeteilt, Vec<String>) {
    let (kopfzeilen, rumpf) = trenne_kopf(roh);
    let koerper = value(&kopfzeilen, "content-type").unwrap_or_default();

    let mut anhaenge = Vec::new();
    let mut text = None;
    let mut tiefen = 0;

    for teil in multipart(&rumpf, &koerper) {
        let (teil_kopf, teil_rumpf) = trenne_kopf(teil);
        let teil_typ = value(&teil_kopf, "content-type").unwrap_or_default();
        // **Nur der Feldname wird kleingeschrieben, nicht sein Wert.** Ein
        // `filename=Rechnung.pdf` stand sonst als `rechnung.pdf` in der Leiste –
        // und `Vertrag.PDF` hieße fortan `vertrag.pdf`. Der Name gehört dem
        // Absender, und der einzige Grund für das Kleinschreiben ist, ihn zu
        // finden; gefunden wird er über den Schlüssel, nicht über den Wert.
        let disposition = value(&teil_kopf, "content-disposition").unwrap_or_default();

        let datei = disposition
            .split(';')
            .skip(1)
            .filter_map(|teil| {
                let (schluessel, wert) = teil.trim().split_once('=')?;
                schluessel
                    .trim()
                    .eq_ignore_ascii_case("filename")
                    .then(|| wert.trim().trim_matches(['"', '\'']).to_string())
            })
            .next();

        if let Some(datei) = datei {
            anhaenge.push(datei);
            continue;
        }

        if teil_typ.starts_with("multipart/") {
            // Verschachtelte Mehrteile: eine Ebene tiefer, aber begrenzt. Eine
            // Mail mit zehn Ebenen Verschachtelung ist ein Angriff auf die
            // Verarbeitung, kein Aufruf von Mimir.
            tiefen += 1;
            if tiefen > 4 {
                continue;
            }
            for t in multipart(teil_rumpf, &teil_typ) {
                let (k, r) = trenne_kopf(t);
                let datei_k = value(&k, "content-disposition")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if datei_k.contains("filename=") {
                    continue;
                }
                if text.is_none()
                    && value(&k, "content-type")
                        .unwrap_or_default()
                        .starts_with("text/plain")
                {
                    text = Some(dekodiere_inhalt(
                        r,
                        &value(&k, "content-transfer-encoding")
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                    ));
                }
            }
            continue;
        }

        if teil_typ.starts_with("text/plain") && text.is_none() {
            text = Some(dekodiere_inhalt(
                teil_rumpf,
                &value(&teil_kopf, "content-transfer-encoding")
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
            ));
        }
    }

    let an = value(&kopfzeilen, "to").unwrap_or_default();
    let cc = value(&kopfzeilen, "cc").unwrap_or_default();
    let message_id = value(&kopfzeilen, "message-id").unwrap_or_default();

    let betreff = kuerze_betreff(&dekodiere_kopf(
        value(&kopfzeilen, "subject").unwrap_or_default().as_bytes(),
    ));

    (
        Aufgeteilt {
            kopf: NachrichtKopf {
                nummer: 0,
                absender: erste_adresse_aus(&value(&kopfzeilen, "from").unwrap_or_default()),
                betreff,
                datum: value(&kopfzeilen, "date").unwrap_or_default(),
                gelesen: true,
                bytes: roh.len() as u32,
            },
            an: aufloese_namen(&an),
            cc: aufloese_namen(&cc),
            text: text.unwrap_or_default(),
            anhaenge: Vec::new(),
            message_id,
        },
        anhaenge,
    )
}

/// Teilt einen Mehrteilrumpf an seinen Grenzen entlang.
#[allow(dead_code)]
fn multipart<'a>(rumpf: &'a [u8], koerper_typ: &str) -> Vec<&'a [u8]> {
    let grenze = koerper_typ
        .split(';')
        .filter_map(|teil| teil.trim().strip_prefix("boundary="))
        .map(|wert| wert.trim_matches(['"', '\'']))
        .next();

    let Some(grenze) = grenze else {
        return vec![rumpf];
    };

    let trenner = format!("--{grenze}");
    let mut teile = Vec::new();
    let mut position = 0;
    let mut von: Option<usize> = None;

    while position < rumpf.len() {
        let Some(relative) = rumpf[position..]
            .windows(trenner.len())
            .position(|fenster| fenster == trenner.as_bytes())
        else {
            break;
        };

        let treffer = position + relative;

        // Der vorherige Anteil endet **vor** diesem Trenner. Sein eigener
        // Zeilenumbruch gehört nicht zum Inhalt (RFC 2046): Die CRLF vor `--`
        // trennen den Anteil vom Trenner und nicht zwei Absätze.
        if let Some(start) = von.take() {
            let mut bis = treffer;

            if rumpf[..bis].ends_with(b"\r\n") {
                bis -= 2;
            } else if rumpf[..bis].ends_with(b"\n") {
                bis -= 1;
            }

            teile.push(&rumpf[start..bis.max(start)]);
        }

        let danach = treffer + trenner.len();

        // `--WAND--` schließt die Liste; danach folgt nichts mehr.
        if rumpf[danach..].starts_with(b"--") {
            break;
        }

        // **Der Zeilenumbruch hinter dem Trenner gehört zum Trenner, nicht zum
        // Anteil** (RFC 2046). Bliebe er stehen, begönne der Anteil mit einer
        // Leerzeile, und `trenne_kopf` hielte ihn für kopf-los: Die erste Zeile
        // wäre genau diese Leerzeile, und der eigentliche Inhalt rutschte in
        // den Rumpf – der Text einer Mail bliebe dann leer.
        von = Some(if rumpf[danach..].starts_with(b"\r\n") {
            danach + 2
        } else if rumpf[danach..].starts_with(b"\n") {
            danach + 1
        } else {
            danach
        });

        position = von.unwrap_or(danach);
    }

    teile
}

/// Trennt die Kopfzeilen vom Rumpf. Die erste Leerzeile beendet den Kopf.
#[allow(dead_code)]
fn trenne_kopf(roh: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let mut kopf: Vec<(String, String)> = Vec::new();
    let mut position = 0;
    let mut ende = roh.len();

    while position < roh.len() {
        let zeilen_ende = roh[position..]
            .iter()
            .position(|zeichen| *zeichen == b'\n')
            .map_or(roh.len(), |stelle| position + stelle + 1);

        let zeile = &roh[position..zeilen_ende];

        // Die Leerzeile beendet den Kopf. **Ihr Zeilenende gehört zum Trenner**,
        // nicht zum Rumpf: Ließe man es stehen, begann jeder Text mit zwei
        // Bytes, die kein Inhalt sind, und der erste Mehrteil-Trenner – etwa
        // `--WAND` – wäre um zwei Bytes verschoben und würde nicht gefunden.
        if zeile.starts_with(b"\r\n") || zeile == b"\n" {
            ende = zeilen_ende.min(roh.len());
            break;
        }

        // **Vor** der Suche nach dem Doppelpunkt. Eine Fortsetzungszeile trägt
        // gewöhnlich keinen Doppelpunkt, würde an einer Prüfung danach aber
        // vorbeikommen und als eigenes Feld mit leerem Namen landen – oder
        // schlimmer: gar nicht erst verarbeitet und der Betreff bliebe abgeschnitten.
        let ist_fortsetzung = zeile.first() == Some(&b' ');

        if ist_fortsetzung {
            let inhalt = String::from_utf8_lossy(zeile).trim().to_string();
            if !inhalt.is_empty() {
                match kopf.last_mut() {
                    Some((_, letzter)) => {
                        letzter.push(' ');
                        letzter.push_str(&inhalt);
                    }
                    None => kopf.push(("".to_string(), inhalt)),
                }
            }

            position = zeilen_ende;
            continue;
        }

        if let Some(umbruch) = zeile.iter().position(|zeichen| *zeichen == b':') {
            kopf.push((
                String::from_utf8_lossy(&zeile[..umbruch])
                    .trim()
                    .to_lowercase(),
                String::from_utf8_lossy(&zeile[umbruch + 1..])
                    .trim()
                    .to_string(),
            ));
        }

        position = zeilen_ende;
    }

    (kopf, &roh[ende..])
}

#[allow(dead_code)]
fn value(kopf: &[(String, String)], name: &str) -> Option<String> {
    kopf.iter()
        .find(|(feld, _)| feld == name)
        .map(|(_, inhalt)| inhalt.clone())
}

/// Dekodiert Base64 und quoted-printable für den Nachrichtentext.
#[allow(dead_code)]
fn dekodiere_inhalt(rumpf: &[u8], verfahren: &str) -> String {
    let daten = match verfahren.trim() {
        "base64" => base64_decode(rumpf),
        "quoted-printable" => dekodiere_quoted_printable(rumpf),
        _ => rumpf.to_vec(),
    };

    // Die Zeilenenden einer Mail sind CRLF; im Fenster wären sie ein Umbruch
    // zu viel.
    String::from_utf8_lossy(&daten).replace("\r\n", "\n")
}

#[allow(dead_code)]
fn dekodiere_quoted_printable(rumpf: &[u8]) -> Vec<u8> {
    let mut ergebnis = Vec::with_capacity(rumpf.len());
    let mut position = 0;

    while position < rumpf.len() {
        match rumpf[position] {
            b'=' if position + 2 < rumpf.len() => {
                if rumpf[position + 1] == b'\n' {
                    // Weicher Umbruch
                    position += 2;
                } else if rumpf[position + 2] == b'\n' {
                    position += 3;
                } else {
                    let hex = &rumpf[position + 1..position + 3];
                    match u8::from_str_radix(&String::from_utf8_lossy(hex), 16) {
                        Ok(wert) => {
                            ergebnis.push(wert);
                            position += 3;
                        }
                        Err(_) => {
                            ergebnis.push(b'=');
                            position += 1;
                        }
                    }
                }
            }
            zeichen => {
                ergebnis.push(zeichen);
                position += 1;
            }
        }
    }

    ergebnis
}

#[allow(dead_code)]
fn base64_decode(daten: &[u8]) -> Vec<u8> {
    let mut ergebnis = Vec::with_capacity(daten.len() / 4 * 3);
    let mut puffer = 0u32;
    let mut bits = 0u32;

    for zeichen in daten {
        let wert = match zeichen {
            b'A'..=b'Z' => zeichen - b'A',
            b'a'..=b'z' => zeichen - b'a' + 26,
            b'0'..=b'9' => zeichen - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            // Umbruch, Füllzeichen, Leerraum: kein Wert
            _ => continue,
        } as u32;

        puffer = (puffer << 6) | wert;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            ergebnis.push(((puffer >> bits) & 0xff) as u8);
        }
    }

    ergebnis
}

/// Löst die Kodierung eines Kopfzeilenwerts auf (RFC 2047).
///
/// **`=?UTF-8?Q?...?=` ist verbreitet genug, um unverzichtbar zu sein.** Ohne
/// diese Stufe stünde in der Leiste für jede Mail mit Umlauten im Betreff eine
/// Zeichenkette wie `=?UTF-8?Q?Gr=C3=BC=C3=9Fe?=` – der Nutzer sähe etwas, das
/// nichts bedeutet.
#[allow(dead_code)]
fn dekodiere_kopf(wert: &[u8]) -> String {
    let roh = String::from_utf8_lossy(wert);
    let bytes = roh.as_bytes();
    let mut ergebnis = String::new();
    let mut position = 0;
    // Merkt sich, ob das zuletzt Übernommene ein kodiertes Feld war. Nur
    // zwischen zwei solchen Feldern fällt Leerraum weg.
    let mut vorletztes_war_feld = false;

    while position < bytes.len() {
        // **Die Markierung beginnt mit `=?`** – RFC 2047 schreibt
        // `=?charset?verfahren?text?=`, und gesucht wird deshalb nach `=?`, nicht
        // nach `?=`. Die erste Fassung suchte verkehrt herum und lieferte
        // kodierte Felder unverändert zurück.
        let Some(start) = finde(bytes, position, b"=?") else {
            ergebnis.push_str(&roh[position..]);
            break;
        };

        let mut wörtlich = &roh[position..start];

        // **Zwischen zwei benachbarten kodierten Feldern wird der Leerraum auf
        // ein Leerzeichen gebracht.** RFC 2047 §6.2 verlangt hier, ihn ganz zu
        // löschen; befolgt ergäbe `=?..?Q?Rechnung?= =?..?Q?f=C3=BCr?=` aber
        // „Rechnungfür" – zwei Wörter, die der Absender mit einem Leerzeichen
        // geschrieben hat. Der Leerraum ist hier der Umbruch einer zu langen
        // Zeile, und der Betreff soll lesbar bleiben.
        //
        // **Nur zwischen zwei kodierten Feldern.** Zwischen gewöhnlichem Text
        // und einem Feld gehört jedes Leerzeichen zum Betreff – dort wird
        // nichts entfernt.
        if vorletztes_war_feld && !wörtlich.is_empty() && wörtlich.chars().all(char::is_whitespace)
        {
            wörtlich = " ";
        }

        ergebnis.push_str(wörtlich);

        let inhalt_start = start + 2;
        let Some(ende) = finde(bytes, inhalt_start, b"?=") else {
            // Kein Ende: Der Rest war gar kein kodiertes Feld. Unverändert
            // übernehmen, statt den Rest zu verschlucken.
            ergebnis.push_str("=?");
            position = inhalt_start;
            continue;
        };

        let teile: Vec<&str> = roh[inhalt_start..ende].splitn(3, '?').collect();

        if teile.len() == 3 {
            let daten = match teile[1].to_ascii_uppercase().as_str() {
                "B" => base64_decode(teile[2].as_bytes()),
                // `_` steht für ein Leerzeichen, `=XX` für ein Byte.
                "Q" => dekodiere_quoted_printable(teile[2].replace('_', " ").as_bytes()),
                _ => teile[2].as_bytes().to_vec(),
            };

            ergebnis.push_str(&String::from_utf8_lossy(&daten));
            vorletztes_war_feld = true;
        } else {
            ergebnis.push_str(&roh[inhalt_start..ende]);
            vorletztes_war_feld = false;
        }

        position = ende + 2;
    }

    ergebnis
}

/// Sucht `nadel` in `heu` ab `ab` und gibt den **Anfang** des Treffers zurück.
///
/// Eigenes Suchen statt `str::find`, weil hier über Bytes gearbeitet wird: Der
/// Betreff einer Mail ist Fast-Text mit einzelnen Sonderzeichen, und ein
/// Umweg über `String` würde jedes Mal eine Kopie erzeugen.
#[allow(dead_code)]
fn finde(heu: &[u8], ab: usize, nadel: &[u8]) -> Option<usize> {
    if ab >= heu.len() || nadel.is_empty() {
        return None;
    }

    heu[ab..]
        .windows(nadel.len())
        .position(|fenster| fenster == nadel)
        .map(|stelle| ab + stelle)
}

/// Nimmt die erste Adresse aus einem Adressfeld.
#[allow(dead_code)]
fn erste_adresse(adressen: &Option<Vec<imap_proto::Address<'_>>>) -> String {
    adressen
        .as_ref()
        .and_then(|liste| liste.first())
        .map(adresse_text)
        .unwrap_or_default()
}

#[allow(dead_code)]
fn erste_adresse_aus(feld: &str) -> String {
    feld.split(',')
        .map(str::trim)
        .find(|teil| !teil.is_empty())
        .unwrap_or_default()
        .to_string()
}

#[allow(dead_code)]
fn adresse_text(adresse: &imap_proto::Address<'_>) -> String {
    let mailbox = adresse
        .mailbox
        .as_ref()
        .map(|wert| String::from_utf8_lossy(wert).to_string())
        .unwrap_or_default();
    let host = adresse
        .host
        .as_ref()
        .map(|wert| String::from_utf8_lossy(wert).to_string())
        .unwrap_or_default();

    if mailbox.is_empty() || host.is_empty() {
        String::new()
    } else {
        format!("{mailbox}@{host}")
    }
}

/// Wandelt `Kai <kai@example.org>` in `Kai (kai@example.org)` – die Form, in der
/// der Absender auch in der Leiste lesbar bleibt, wenn er einen Namen trägt.
#[allow(dead_code)]
fn aufloese_namen(feld: &str) -> String {
    feld.split(',')
        .map(str::trim)
        .filter(|teil| !teil.is_empty())
        .map(|teil| match (teil.find('<'), teil.rfind('>')) {
            (Some(von), Some(bis)) if bis > von => {
                let name = teil[..von].trim().trim_matches(['"', '\'']).trim();
                let adresse = &teil[von + 1..bis];
                if name.is_empty() {
                    adresse.to_string()
                } else {
                    format!("{name} ({adresse})")
                }
            }
            _ => teil.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "imap_tests.rs"]
mod tests;
