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

use super::{kuerze, kuerze_betreff, MAX_MAIL_PREVIEW_BYTES, VORSCHAU_BYTES};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Wie lange der Verbindungsaufbau dauern darf. Wie beim Kalender: ein
/// Funkloch soll nicht hängen bleiben, sondern eine Meldung ergeben.
#[allow(dead_code)]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Wie lange ein einzelner Befehl warten darf. Das Holen des Textes einer Mail
/// darf bei einer großen Nachricht und langsamer Leitung etwas dauern.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// Wie lange auf die nächste Antwort gewartet wird, bevor die Leitung gilt.
///
/// **Ohne diese Frist ist „es passiert nichts" der einzige Zustand, den ein
/// Mailserver herstellen kann**, der die Verbindung annimmt und dann schweigt –
/// ein stiller Filter, ein ausgehängter Server, ein Port, den jemand blockiert.
/// Jeder Lesevorgang und jeder Schreibvorgang läuft durch sie; `CONNECT_TIMEOUT`
/// allein genügt nicht, weil sie nur den Verbindungsaufbau deckt.
const ANTWORT_FRIST: Duration = Duration::from_secs(45);
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
    /// Die ersten Zeilen des Textes, damit die Leiste mehr zeigt als einen
    /// Betreff. Leer heißt: der Server hat nichts geliefert – etwa weil er
    /// einen begrenzten Abruf nicht kannte. Die Liste steht dann genau so da,
    /// wie sie vor dieser Erweiterung dastand.
    pub vorschau: String,
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
    /// Was beim Markieren schiefging, falls etwas schiefging.
    ///
    /// **Die Nachricht kommt auch dann zurück, wenn das Markieren nicht
    /// klappt.** Eine Mail, die sich nicht als gelesen setzen lässt, ist immer
    /// noch lesbar – sie nur nicht lesbar zu machen wäre die schlimmere
    /// halbe Lösung. `None` heisst: sie steht jetzt als gelesen im Ordner.
    pub markierungsfehler: Option<String>,
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
    /// Bytes, die beim Lesen über die Zeilengrenze hinaus mitgekommen sind und
    /// zur nächsten Zeile gehören.
    ///
    /// **Das ist die Kehrseite des Pufferns.** Ein Aufruf holt 8 KiB auf einmal,
    /// und darin liegen am Ende die ersten Bytes der *nächsten* Antwort. Läge
    /// der Rest nur in einer örtlichen Variable, ginge er beim nächsten
    /// `lies_roh_zeile` verloren – die zweite Zeile einer Antwort fehlte, und
    /// der Aufrufer suchte den Fehler im Protokoll statt im Puffer.
    unbenutzt: Vec<u8>,
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
            unbenutzt: Vec::new(),
            beendet: false,
        }
    }

    /// Liest eine Zeile **mit** ihrem Zeilenende.
    ///
    /// `read_line` bricht an `\n` und würde ein Byte mit dem oberen Bit
    /// gesetzt als Zeilenende lesen – in einem RFC 2047-Betreff ist das keine
    /// Ausnahme, sondern die Regel. Deshalb byteweise.
    ///
    /// **Das Zeilenende bleibt stehen**, und das ist der ganze Unterschied zu
    /// [`Verbindung::lies_zeile`]: `imap-proto` parst eine Antwort nur, wenn sie
    /// mit CRLF endet. Eine Zeile ohne das CRLF zu übergeben scheitert an *jeder*
    /// Antwort – auch an `* OK`.
    async fn lies_roh_zeile(&mut self) -> Result<Vec<u8>, VerbindungsFehler> {
        self.lies_roh_zeile_bis(ANTWORT_FRIST).await
    }

    /// Wie [`Verbindung::lies_roh_zeile`], nur mit eigener Frist.
    ///
    /// **Die Frist ist ein Parameter, damit sie prüfbar ist.** Als Konstante
    /// wäre sie nur im Quelltext nachlesbar; ein Test müsste die echte, lange
    /// Wartezeit abwarten oder die Konstante verändern. Mit dem Parameter prüft
    /// die Attrappe in Millisekunden, was in der Wirklichkeit Minuten dauert.
    async fn lies_roh_zeile_bis(&mut self, frist: Duration) -> Result<Vec<u8>, VerbindungsFehler> {
        // **Was der vorige Aufruf zu viel gelesen hat, kommt vornan.**
        self.puffer.clear();
        self.puffer.append(&mut self.unbenutzt);

        // **Gepuffert, nicht Byte für Byte.** Vorher wurde ein einzelnes Byte
        // gelesen und auf `\n` geprüft: ein Systemaufruf pro Byte, bei einer
        // Betreffzeile von achtzig Zeichen achtzig Aufrufe, bei einer
        // Fließtextzeile Tausende. Auf einer Leitung mit Taunus-Wartezeit
        // summiert sich das zu Minuten, in denen die Leiste nichts anzeigt und
        // nicht zu sagen weiß, warum. Ein Puffer holt dieselben Bytes mit
        // einem Aufruf.
        let mut brot = [0u8; 8192];

        loop {
            if let Some(stelle) = self.puffer.iter().position(|z| *z == b'\n') {
                // **Der Rest gehört der nächsten Zeile**, nicht der Antwort:
                // Weglassen wäre stiller Datenverlust, und der fällt erst auf,
                // wenn eine Antwort fehlt, die der Server längst geschickt hat.
                self.unbenutzt.extend_from_slice(&self.puffer[stelle + 1..]);
                self.puffer.truncate(stelle + 1);
                return Ok(self.puffer.clone());
            }

            if self.puffer.len() > MAX_LINE_BYTES {
                self.beendet = true;
                return Err(VerbindungsFehler::Geschlossen(
                    "Der Mailserver hat eine einzelne Zeile geschickt, die länger ist, \
                     als Mimir lesen kann"
                        .to_string(),
                ));
            }

            // **Und mit einer Frist.** Ohne sie wartet ein Server, der die
            // Verbindung annimmt und dann schweigt, für immer – der Auftrag
            // kehrt nie zurück, die Leiste bleibt auf „wird geladen" stehen und
            // kein Klick im Programm hilft. Eine Leitung ohne Antwort ist ein
            // Fehler mit einem Satz, kein Warten.
            let anzahl = match tokio::time::timeout(frist, self.inner.read(&mut brot)).await {
                Ok(Ok(anzahl)) => anzahl,
                Ok(Err(fehler)) => return Err(fehler.into()),
                Err(_) => {
                    self.beendet = true;
                    return Err(VerbindungsFehler::Geschlossen(format!(
                        "Der Mailserver hat nicht innerhalb von {} Sekunden geantwortet",
                        frist.as_secs_f64().ceil() as u64
                    )));
                }
            };

            if anzahl == 0 {
                // **Die Leitung ist zu.** Eine halbe Zeile ist keine Zeile:
                // Gmail beendet die Verbindung nach `LOGOUT` so, und das ist
                // ein Zustand, kein Fehler beim Lesen.
                if self.puffer.is_empty() {
                    self.beendet = true;
                    return Err(VerbindungsFehler::Geschlossen(
                        "Der Mailserver hat die Verbindung beendet".to_string(),
                    ));
                }

                self.unbenutzt.clear();
                return Ok(self.puffer.clone());
            }

            self.puffer.extend_from_slice(&brot[..anzahl]);
        }
    }

    /// Liest **genau** so viele Bytes, wie verlangt sind.
    ///
    /// **Zuerst der Rest aus dem Puffer, dann die Leitung.** Wird ein Literal
    /// geholt, liegen die ersten seiner Bytes womöglich noch im Rest des letzten
    /// Lesevorgangs – sie gehören zum Inhalt, nicht zur Antwortzeile. Ohne diese
    /// Reihenfolge würde `read_exact` auf der Leitung warten, obwohl die Bytes
    /// längst da sind, und der Abruf stünde wieder still.
    async fn lies_genau(&mut self, ziel: &mut [u8]) -> Result<(), VerbindungsFehler> {
        let mut geschrieben = 0usize;

        if !self.unbenutzt.is_empty() {
            let n = self.unbenutzt.len().min(ziel.len());
            ziel[..n].copy_from_slice(&self.unbenutzt[..n]);
            self.unbenutzt.drain(..n);
            geschrieben = n;
        }

        if geschrieben == ziel.len() {
            return Ok(());
        }

        let rest = &mut ziel[geschrieben..];

        match tokio::time::timeout(ANTWORT_FRIST, self.inner.read_exact(rest)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_)) => Err(VerbindungsFehler::Geschlossen(
                "Der Mailserver hat die Verbindung mitten im Inhalt beendet".to_string(),
            )),
            Err(_) => {
                self.beendet = true;
                Err(VerbindungsFehler::Geschlossen(format!(
                    "Der Mailserver hat den angekündigten Inhalt nicht innerhalb von {} Sekunden geliefert",
                    ANTWORT_FRIST.as_secs()
                )))
            }
        }
    }

    /// Liest eine Zeile ohne ihr Zeilenende – für Begrüßung und `+`-Zeilen, die
    /// nur auf ein Stichwort angeschaut werden.
    async fn lies_zeile(&mut self) -> Result<Vec<u8>, VerbindungsFehler> {
        let mut zeile = self.lies_roh_zeile().await?;

        // Das Zeilenende gehört hier nicht zur Antwort.
        while zeile
            .last()
            .is_some_and(|zeichen| *zeichen == b'\n' || *zeichen == b'\r')
        {
            zeile.pop();
        }

        Ok(zeile)
    }

    /// **Liest eine Antwortzeile und holt die darin angekündigten Literale nach.**
    ///
    /// **IMAP schickt den Inhalt eines Abrufs nicht in der Zeile, sondern
    /// danach.** So sieht ein Abruf aus:
    ///
    /// ```text
    /// * 1 FETCH (BODY[] {11}\r\n
    /// Hallo Kai!\r\n
    /// )\r\n
    /// ```
    ///
    /// Die Längenangabe steht am Ende der ersten Zeile, die elf Byte folgen erst
    /// in den folgenden – und dürfen selbst Zeilenumbrüche enthalten. Wer nur bis
    /// zum ersten `\n` liest, reicht dem Parser eine Zeile, die mitten im Literal
    /// abbricht, und versteht die Antwort nicht.
    ///
    /// Betrifft **jeden** Abruf von Inhalt: die Vorschau in der Leiste
    /// (`BODY[]`) ebenso wie das Öffnen einer Nachricht (`RFC822`).
    ///
    /// **Nicht synchronisierende Literale (`{n+}`) zählen nicht.** Sie kündigen
    /// an, dass der Server sie *irgendwann* schickt, nicht jetzt; danach ginge der
    /// Inhalt in die Auswertung der nächsten Antwort ein.
    async fn lies_antwort(&mut self) -> Result<Vec<u8>, VerbindungsFehler> {
        let mut roh = self.lies_roh_zeile().await?;

        // **Suche ab hier**, nicht von vorn: Das Literal, das gerade abgeholt
        // wurde, steht mit seiner Längenangabe weiter im Puffer – wer wieder von
        // vorn sucht, findet es ein zweites Mal, liest dieselben Bytes noch mal
        // und läuft in eine Schleife, an deren Ende die Leitung schief ist.
        let mut ab = 0;

        // **Mehrfach, nicht einmal.** Eine Antwort kann mehrere Literale tragen –
        // etwa ein Abruf, der Text und Kopfzeile zugleich holt.
        while let Some((versatz, laenge)) = literal_anteil(&roh[ab..]) {
            if laenge > MAX_LINE_BYTES {
                self.beendet = true;
                return Err(VerbindungsFehler::Geschlossen(
                    "Der Mailserver hat einen Inhalt angekündigt, der größer ist, \
                     als Mimir lesen kann"
                        .to_string(),
                ));
            }

            let ende = ab + versatz;

            let mut daten = vec![0u8; laenge];

            if self.lies_genau(&mut daten).await.is_err() {
                self.beendet = true;
                return Err(VerbindungsFehler::Geschlossen(
                    "Der Mailserver hat die Verbindung mitten im Inhalt beendet".to_string(),
                ));
            }

            // Der Rest der Zeile – meist nur die schließende Klammer – folgt als
            // eigene Zeile.
            let rest = self.lies_roh_zeile().await?;

            // **Das CRLF *vor* dem Inhalt bleibt, das danach kommt nicht dazu.**
            // `imap-proto` liest ein Literal als `"{" Zahl "}" CRLF *CHAR8`: Die
            // Leerzeile gehört vor die Daten, und auf die Daten folgt der Rest
            // der Antwort unmittelbar. Ein eingeschobenes CRLF dazwischen
            // liefe in den nächsten Anteil, und die Antwort wäre unlesbar.
            let behalten = if roh.get(ende..ende + 2) == Some(b"\r\n") {
                ende + 2
            } else {
                ende
            };

            roh.truncate(behalten);
            roh.extend_from_slice(&daten);
            roh.extend_from_slice(&rest);

            // Alles bis hier ist erledigt: Marke, Inhalt und der Rest der Zeile.
            ab = behalten + laenge;
        }

        Ok(roh)
    }

    /// Schreibt einen Befehl und sammelt alle Antworten bis zum fertigen Zustand.
    async fn befehl(&mut self, befehl: &str) -> Result<Vec<imap_proto::Response<'static>>, String> {
        // **Jede Meldung nennt den Befehl, an dem sie hängen blieb.**
        //
        // Ohne das bekommt man nur den Text, den der Server schickte – und
        // Gmail schickt bei einem unbekannten Befehl *502 5.5.1 Unrecognized
        // command*, ohne zu sagen, welcher es war. Genau daran ist dieser Fehler
        // zweimal gescheitert: Man wusste nicht, ob es die Anmeldung, das Öffnen
        // des Ordners oder der Abruf war, und musste raten.
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
                .lies_antwort()
                .await
                .map_err(|fehler| fehler.to_string())?;

            let (rest, antwort) = imap_proto::Response::parse(&roh).map_err(|_| {
                format!(
                    "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich \
                     geantwortet: {}",
                    ohne_zeilenende(&roh)
                )
            })?;

            if !rest.is_empty() {
                return Err(format!(
                    "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich \
                     geantwortet: {}",
                    ohne_zeilenende(rest)
                ));
            }

            if let imap_proto::Response::Done { status, .. } = &antwort {
                match status {
                    imap_proto::Status::Ok => {}
                    // NO und BAD sind hier beide Fehler: `NO` heißt „abgelehnt"
                    // (meist falsches Passwort), `BAD` heißt „Kommando kaputt".
                    // Beides heißt für Mimir: Der Zugang ist nicht da.
                    _ => {
                        return Err(format!("{} – bei `{befehl}`", status_text(status)));
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

/// Wo in einer Zeile ein Literal angekündigt wird, und wie viele Byte folgen.
///
/// Liefert `(Stelle hinter der schließenden Klammer, Länge)`.
///
/// **`{n+}` zählt nicht.** Ein nicht synchronisierendes Literal sagt dem Client,
/// dass der Server es *irgendwann* schickt, auch wenn kein Befeld danach fragt –
/// die Daten stehen also nicht im nächsten Befehl, sondern irgendwann später.
/// Würde man es hier als Literal lesen, käme der eigentliche Inhalt in die
/// Auswertung der folgenden Antwort hinein.
fn literal_anteil(zeile: &[u8]) -> Option<(usize, usize)> {
    let mut position = 0;

    while position < zeile.len() {
        if zeile[position] != b'{' {
            position += 1;
            continue;
        }

        let ziffern_start = position + 1;
        let mut ziffern_ende = ziffern_start;

        while ziffern_ende < zeile.len() && zeile[ziffern_ende].is_ascii_digit() {
            ziffern_ende += 1;
        }

        if ziffern_ende == ziffern_start
            || ziffern_ende >= zeile.len()
            || zeile[ziffern_ende] != b'}'
        {
            position += 1;
            continue;
        }

        // `{n+}` – nicht synchronisierend, kein Inhalt für diese Antwort.
        if zeile.get(ziffern_ende + 1) == Some(&b'+') {
            position = ziffern_ende + 1;
            continue;
        }

        let laenge = std::str::from_utf8(&zeile[ziffern_start..ziffern_ende])
            .ok()
            .and_then(|zahl| zahl.parse::<usize>().ok());

        return laenge.map(|laenge| (ziffern_ende + 1, laenge));
    }

    None
}

/// Die Zeile ohne ihr Zeilenende – für Meldungen an den Benutzer.
///
/// Nur für Fehlertexte: Der Parser braucht das CRLF, ein Mensch nicht, und eine
/// Meldung, die mit einem unsichtbaren Wagenrücklauf endet, liest sich wie ein
/// Absturz.
fn ohne_zeilenende(zeile: &[u8]) -> String {
    String::from_utf8_lossy(zeile)
        .trim_end_matches(['\r', '\n'])
        .to_string()
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
                "In diesem System ist kein Zertifikatsspeicher gefunden – die Mailverbindung kann nicht aufgebaut werden"
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

    // **465 ist der Port für Postausgang, nicht für Posteingang.** Es ist der
    // häufigste Fehler bei Gmail, weil dort beide Adressen nebeneinander
    // eingetragen werden und sich nur in der Zahl unterscheiden. Mimir erkennt
    // das an der Zahl und sagt, welche hingehört: Auf 465 antwortet kein
    // IMAP-Server, und statt einer Meldung käme nur eine Verbindung, die
    // wartet – der schlimmste Fehler, den ein Mailprogramm machen kann, weil er
    // nicht als Fehler auftritt, sondern als Warten.
    if port == 465 {
        return Err(format!(
            "Port 465 gehört zu SMTP, nicht zu IMAP. Für „{host}“ gehört 993 in die \
             Adresse – etwa imaps://{host}:993."
        ));
    }

    Ok((host.to_string(), port))
}

/// Meldet sich an, bevor irgendein anderes Befehl kommt.
///
/// **AUTHENTICATE PLAIN statt LOGIN.** Bei `LOGIN` stehen Benutzername und
/// Passwort im Klartext in derselben Zeile; bei `AUTHENTICATE` kommt nur der
/// **In drei Schritten, nicht in einem Befehl.** `AUTHENTICATE` ist eine
/// Unterhaltung: Der Server antwortet mit `+`, *wartet* auf die Zugangsdaten und
/// meldet das Ergebnis erst danach. `befehl` liest dagegen bis zum fertigen
/// Zustand – es hätte also sowohl das `+` als auch das Ergebnis gelesen, bevor
/// überhaupt etwas geschickt worden wäre. Deshalb hier Zeile für Zeile.
///
/// **Und es wird bis zur *getaggten* Antwort gelesen, nicht nur bis zur ersten.**
/// Vor dem Ergebnis dürfen beliebig viele ungetaggte Zeilen kommen – `* OK
/// [CAPABILITY …]`, Fehlermeldungen, Zustandsänderungen. Wer nach dem
/// Zugangsdatensatz genau **eine** Zeile liest und sie auswertet, bekommt die
/// erste davon und meldet sie als Fehlschlag – während die eigentliche Antwort,
/// `A1 NO [AUTHENTICATIONFAILED]`, ungegelesen im Strom bleibt. Genau das ist
/// geschehen: Die Leiste zeigte eine unlesbare Serverzeile statt der Antwort,
/// die den Benutzer wirklich weitergebracht hätte.
async fn anmelden<T>(
    verbindung: &mut Verbindung<T>,
    benutzername: &str,
    passwort: &str,
) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // `PLAIN` wird angeboten, weil es das Verfahren ist, das ein App-Passwort
    // trägt. **`LOGIN` ist kein Rückfall** – es steht zwar in manchen
    // Mailprogrammen als Ausweichverfahren, hier gibt es es nicht: Bei `LOGIN`
    // stehen Benutzername und Passwort in derselben Zeile im Klartext, und
    // beides gehört in eine Antwort für sich. Meldet sich ein Server damit, dass
    // er `AUTHENTICATE` nicht kennt, sagt Mimir das – statt sich mit einem
    // zweiten, unsichereren Verfahren durchzuschlagen.
    //
    // **Ein Fehler hier wird nicht verschluckt.** Einst stand hier ein
    // `unwrap_or_default()`: Kam vom Server keine parsebare Antwort, galt die
    // Anmeldung als geglückt, der ganze weitere Zustand wurde mit leeren
    // Antworten gefüttert, und der Benutzer sah erst an der *Nachrichtenliste*
    // eine Meldung, die nichts mit dem eigentlichen Problem zu tun hatte.
    let befehl = format!("{ANMELDE_TAG} AUTHENTICATE PLAIN");
    schreibe(verbindung, &befehl).await?;

    // Bis der Server entweder um die Zugangsdaten bittet oder schon ein Ergebnis
    // schickt – etwa weil er das Verfahren nicht kennt.
    let mut zugangsdaten_gesendet = false;

    while !zugangsdaten_gesendet {
        let zeile = lies_bis_getaggt(verbindung, ANMELDE_TAG, &befehl, true).await?;

        if zeile.starts_with(b"+") {
            let base64 = base64_encode(format!("\0{benutzername}\0{passwort}").as_bytes());
            schreibe(verbindung, &base64).await?;
            zugangsdaten_gesendet = true;
            continue;
        }

        return bewerte_ergebnis(&zeile, &befehl);
    }

    bewerte_ergebnis(
        &lies_bis_getaggt(verbindung, ANMELDE_TAG, &befehl, false).await?,
        &befehl,
    )
}

/// Der Tag, unter dem Mimir sich anmeldet.
const ANMELDE_TAG: &str = "A1";

/// So viele ungetaggte Zeilen werden übersprungen, bevor es als Fehler gilt.
///
/// Eine Zahl und keine Zeit: Es gibt keine Wartezeit, nur Zeilen. Und die Zahl
/// ist klein, weil ein Server, der zehn Zeilen ohne Antwort auf einen Befehl
/// schickt, entweder kaputt ist oder Mimir die Antwort schon gelesen und
/// verworfen hat – in beiden Fällen hilft weiteres Warten nicht.
const MAX_UNTAGGTE_ZEILEN: usize = 32;

/// **Liest Zeilen, bis eine Antwort mit dem eigenen Tag kommt** – oder, wenn
/// `plus` gesetzt ist, bis eine `+`-Zeile kommt.
///
/// **Ungetaggte Zeilen davor werden übersprungen.** Sie sind Statusmeldungen,
/// keine Antworten: `* CAPABILITY …`, `* 231 EXISTS`, Fehlerzeilen eines Servers.
/// Vor dem Ergebnis der Anmeldung steht dort erfahrungsgemäß eine, die der
/// Protokoll-Parser nicht lesen kann – Gmail schickt auf einen unbekannten
/// Befehl `* 502 5.5.1 Unrecognized command` und **danach** erst die getaggte
/// Antwort, die sagt, was wirklich los war.
///
/// **Und `+` ist keine Antwort, sondern eine Frage.** Der Server bittet damit um
/// die Zugangsdaten und wartet darauf – wer sie überlesen will, wartet seinerseits
/// auf eine Antwort, die erst nach den Zugangsdaten kommen kann. Beide warten
/// ewig. Deshalb fragt diese Funktion die `+`-Zeile ab, wenn man sie braucht.
async fn lies_bis_getaggt<T>(
    verbindung: &mut Verbindung<T>,
    tag: &str,
    befehl: &str,
    plus: bool,
) -> Result<Vec<u8>, String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut unlesbar: Option<Vec<u8>> = None;

    for _ in 0..MAX_UNTAGGTE_ZEILEN {
        let roh = match verbindung.lies_antwort().await {
            Ok(roh) => roh,
            // **Die Verbindung ist weg, während noch keine Antwort da war.** Die
            // zuletzt gesehene unlesbare Zeile erklärt mehr als der Abbruch – und
            // der Befehl wird ohnehin genannt.
            Err(abbruch) => {
                return Err(match unlesbar {
                    Some(roh) => format!(
                        "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich \
                         geantwortet: {}\n(Die Verbindung wurde danach beendet: {abbruch})",
                        ohne_zeilenende(&roh)
                    ),
                    None => abbruch.to_string(),
                });
            }
        };

        if plus && roh.starts_with(b"+") {
            return Ok(roh);
        }

        match imap_proto::Response::parse(&roh) {
            Ok((
                rest,
                imap_proto::Response::Done {
                    tag: empfaenger, ..
                },
            )) if !rest.is_empty() || empfaenger.0 == tag => {
                if !rest.is_empty() {
                    return Err(format!(
                        "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich \
                         geantwortet: {}",
                        ohne_zeilenende(rest)
                    ));
                }

                return Ok(roh);
            }
            // Ungetaggt, aber lesbar: Status, keine Antwort. Weiterlesen.
            Ok(_) => continue,
            // Unlesbar: merken und weiterlesen. Es kann eine ungetaggte Zeile
            // sein, vor der noch die eigentliche Antwort kommt.
            Err(_) => unlesbar = Some(roh),
        }
    }

    Err(match unlesbar {
        Some(roh) => format!(
            "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich geantwortet: {}",
            ohne_zeilenende(&roh)
        ),
        None => format!("Der Mailserver hat auf den Befehl \"{befehl}\" nicht geantwortet."),
    })
}

/// Prüft das `A1 OK` / `A1 NO` nach der Anmeldung – und nennt den Befehl, wenn
/// es keines von beidem ist.
fn bewerte_ergebnis(roh: &[u8], befehl: &str) -> Result<(), String> {
    let (rest, antwort) = imap_proto::Response::parse(roh).map_err(|_| {
        format!(
            "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich geantwortet: {}",
            ohne_zeilenende(roh)
        )
    })?;

    if !rest.is_empty() {
        return Err(format!(
            "Der Mailserver hat auf den Befehl \"{befehl}\" unverständlich geantwortet: {}",
            ohne_zeilenende(rest)
        ));
    }

    match antwort {
        imap_proto::Response::Done {
            status: imap_proto::Status::Ok,
            ..
        } => Ok(()),
        imap_proto::Response::Done { status, .. } => {
            Err(format!("{} – bei `{befehl}`", status_text(&status)))
        }
        _ => Err(format!(
            "Der Mailserver hat auf den Befehl \"{befehl}\" nicht wie erwartet geantwortet."
        )),
    }
}

/// Schreibt einen Befehl oder eine Antwortzeile.
async fn schreibe<T>(verbindung: &mut Verbindung<T>, zeile: &str) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // **Auch das Schreiben hat eine Frist.** Eine Verbindung, die im Lesen
    // hängen kann, kann es im Schreiben auch: Der Puffer ist voll, der Gegner
    // liest nicht, und ohne Frist wartet der Auftrag still vor sich hin.
    tokio::time::timeout(COMMAND_TIMEOUT, async {
        verbindung
            .inner
            .write_all(format!("{zeile}\r\n").as_bytes())
            .await
            .map_err(io_text)?;
        verbindung.inner.flush().await.map_err(io_text)
    })
    .await
    .map_err(|_| {
        "Der Mailserver nimmt keine Befehle mehr an – die Verbindung ist zu lange offen."
    })?
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

/// Wählt einen Ordner aus.
///
/// **Eigener Schritt, weil beides ihn braucht.** Die Leiste holt Nachrichten
/// und das Fenster den Text einer einzelnen Nachricht – und beide brauchen
/// vorher denselben Zustand: einen *ausgewählten* Ordner. Ohne `SELECT` steht
/// die Verbindung im Zustand „angemeldet, aber nichts geöffnet", und ein `FETCH`
/// wird vom Server mit `BAD` beantwortet: „Please select a mailbox first".
/// Genau das kam zurück, und die Meldung sprach von einem kaputten Befehl, obwohl
/// der Befehl richtig war – was fehlte, war der Zustand davor.
async fn waehle_ordner<T>(
    verbindung: &mut Verbindung<T>,
    ordner: &str,
) -> Result<Vec<imap_proto::Response<'static>>, String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
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

    verbindung.befehl(&format!("B1 SELECT \"{ordner}\"")).await
}

/// Wählt einen Ordner aus und holt dessen Nachrichten.
pub async fn hole_nachrichten<T>(
    verbindung: &mut Verbindung<T>,
    ordner: &str,
    grenze: usize,
) -> Result<Vec<NachrichtKopf>, String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let gewaehlt = waehle_ordner(verbindung, ordner).await?;

    // **Wie viele Nachrichten der Ordner hat.** Die Zahl steht in der Antwort auf
    // `SELECT`, und ohne sie lässt sich die gewünschte Menge nicht bilden.
    let vorhanden = gewaehlt.iter().find_map(|antwort| match antwort {
        imap_proto::Response::MailboxData(imap_proto::MailboxDatum::Exists(anzahl)) => {
            Some(*anzahl as usize)
        }
        _ => None,
    });

    // **Ein Ordner ohne Nachrichten ist ein gültiger Zustand**, kein Fehler: Die
    // Leiste sagt „Keine Nachrichten", und ein `FETCH` ins Leere wäre nur eine
    // weitere Gelegenheit für eine Fehlermeldung, die nichts erklärt.
    if vorhanden == Some(0) {
        return Ok(Vec::new());
    }

    let menge = nachrichten_menge(vorhanden, grenze);

    if menge.is_empty() {
        return Ok(Vec::new());
    }

    let antworten = verbindung
        .befehl(&format!(
            "B2 FETCH {menge} (UID FLAGS ENVELOPE RFC822.SIZE)"
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
            vorschau: String::new(),
        });
    }

    // Neueste zuerst – so, wie ein Postfach sie zeigt.
    kopfzeilen.sort_by_key(|nachricht| std::cmp::Reverse(nachricht.nummer));
    kopfzeilen.truncate(grenze);

    hole_vorschauen(verbindung, &mut kopfzeilen, &menge).await;

    Ok(kopfzeilen)
}

/// **Die Nachrichtenmenge, die ein `FETCH` bekommen soll.**
///
/// **Die Formulierung entscheidet, ob der Server den Befehl überhaupt
/// versteht.** Nach RFC 3501 ist ein Element einer Menge entweder eine einzelne
/// Nummer (`5`) oder ein Bereich (`5:9`) – nicht `1:5:5`. Die Form
/// `1:{grenze}:{grenze}` sah aus wie „die ersten `grenze`", ergibt aber eine
/// ungültige Menge, und Gmail antwortet darauf mit *„Unrecognized command"*.
///
/// **Und die Formulierung sagt auch, *welche* Nachrichten kommen sollen.** Ein
/// Ordner nummeriert von 1, und die höchsten Nummern sind die neuesten – die
/// gewünschten `grenze` stehen also ganz hinten. `1:{grenze}` hätte die
/// *ältesten* geholt und wäre, selbst wenn der Server es verstünde, genau
/// umgekehrt herum.
#[allow(dead_code)]
fn nachrichten_menge(vorhanden: Option<usize>, grenze: usize) -> String {
    let Some(vorhanden) = vorhanden else {
        // **Ohne `EXISTS`-Antwort** lässt sich „die letzten N" nicht bilden. Dann
        // alles holen und hinten abschneiden – mehr Arbeit für den Server, aber
        // nie ein ungültiger Befehl.
        return "1:*".to_string();
    };

    if vorhanden == 0 || grenze == 0 {
        return String::new();
    }

    let von = vorhanden.saturating_sub(grenze.saturating_sub(1)).max(1);

    format!("{von}:{vorhanden}")
}

/// **Holt zu jedem Kopf die ersten Zeilen des Textes — in einer Runde.**
///
/// **Ein Abruf für die ganze Liste**, nicht einer je Nachricht: Bei fünfzig
/// Nachrichten wären das fünfzig Anfragen und fünfzig Wartezeiten, und die
/// Leiste wäre bei jedem Aufruf spürbar langsam. IMAP darf dieselbe Sektion für
/// eine Menge von Nummern anfordern, und der Server antwortet mit einer Zeile
/// je Nachricht – die Nummern stehen in der Antwort, sie müssen also nicht
/// erraten werden.
///
/// **Warum begrenzt, und warum `PEEK`.** `<0.2048` holt die ersten 2048 Bytes
/// der Nachricht, nicht ihre Rumpfes: Die Kopfzeilen stehen mit drin und werden
/// unten abgetrennt, der Rest ist Anzeige. Ohne die Grenze käme bei fünfzig
/// Nachrichten der ganze Postfachinhalt über die Leitung – und die Leiste
/// braucht zwei Zeilen, nicht fünfzig Nachrichten.
///
/// `PEEK` statt `BODY` ist der Unterschied zwischen einer Liste und einem
/// Postfach, in dem beim Scrollen alles als gelesen gilt: `PEEK` setzt das
/// Gelesen-Flag nicht. Genau dieses Verhalten ist der Grund, warum Mailprogramme
/// es verwenden, und Mimir gäbe es sonst auf.
///
/// **Ein Server, der das nicht kann, verliert nur die Vorschau.** Der Abruf
/// schlägt fehl, die Liste steht trotzdem da – eine kürzere Liste wäre ein
/// schlechterer Tausch als ein fehlender Anriss.
async fn hole_vorschauen<T>(
    verbindung: &mut Verbindung<T>,
    kopfzeilen: &mut [NachrichtKopf],
    menge: &str,
) where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    if kopfzeilen.is_empty() || menge.is_empty() {
        return;
    }

    let antworten = match verbindung
        .befehl(&format!(
            "B3 FETCH {menge} (BODY.PEEK[]<0.{VORSCHAU_BYTES}>)"
        ))
        .await
    {
        Ok(antworten) => antworten,
        Err(fehler) => {
            println!(
                "Der Mailserver konnte die Vorschauen nicht liefern ({fehler}). Die Liste \
                 zeigt Absender und Betreff wie bisher."
            );
            return;
        }
    };

    // **Nach Nummer, nicht nach Reihenfolge.** Die Antwort kommt zwar in der
    // Reihenfolge der Anfrage, aber der Server darf sie auch in einer anderen
    // liefern, und eine Vorschau am falschen Kopf sähe aus wie eine aus dem
    // falschen Postfach.
    //
    // Gespeichert wird **schon der fertige Text**, nicht der Rohabchnitt: Die
    // Antwort hängt an einem Puffer, der mit der Schleife endet, und 50 Nachrichten
    // mit je 2 KiB Rohbytes im Speicher zu halten, um sie hinterher abzuschneiden,
    // wäre genau die Menge, die diese Funktion vermeiden soll.
    let mut vorschauen: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    for antwort in antworten {
        let imap_proto::Response::Fetch(nummer, werte) = antwort else {
            continue;
        };

        for wert in werte {
            let imap_proto::AttributeValue::BodySection { data, .. } = wert else {
                continue;
            };

            if let Some(roh) = data {
                vorschauen.insert(nummer, vorschau_aus_chunk(roh.as_ref()));
            }
        }
    }

    for kopf in kopfzeilen.iter_mut() {
        if let Some(vorschau) = vorschauen.get(&kopf.nummer) {
            kopf.vorschau = vorschau.clone();
        }
    }
}

/// Macht aus einem begrenzten Nachrichten-Anfang zwei lesbare Zeilen.
///
/// **Bricht in Absätze, nicht mitten im Satz.** Eine Vorschau, die mitten im Wort
/// endet, sieht aus wie ein vollständiger Gedanke – und liest sich als einer,
/// auch wenn sie keiner ist. Wer mehr sehen will, öffnet die Nachricht.
///
/// **Leer, statt falsch.** Ohne Kopf-Rumpf-Trenner ist der ganze Anhang ein
/// Kopf; ihn als Vorschau zu zeigen hieße, dem Benutzer `Content-Type:
/// multipart/mixed` als ersten Satz einer Mail vorzumachen. Dann lieber nichts.
/// Dasselbe gilt für eine Nachricht ohne Textteil: Eine reine HTML-Mail hat
/// keinen Text, den man zeigen dürfte.
fn vorschau_aus_chunk(chunk: &[u8]) -> String {
    let (aufgeteilt, _anhaenge) = zerlege_mime(chunk);

    let text = aufgeteilt.text;
    let mut absaetze = Vec::new();

    for absatz in text.split("\n\n") {
        // Zeilenumbrüche innerhalb eines Absatzes sind Umbruch, nicht Absatz:
        // Eine Signatur mit fünf Leerzeilen ist kein fünfteiliger Text.
        let zeile = absatz
            .lines()
            .map(|zeile| zeile.trim())
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if !zeile.is_empty() {
            absaetze.push(zeile);
        }
    }

    kuerze(&absaetze.join("\n\n"), MAX_MAIL_PREVIEW_BYTES)
}

/// Liest den Text einer Nachricht – **und markiert sie dabei als gelesen.**
///
/// **Erst der Ordner, dann die Nachricht.** `FETCH` gilt nur in einem
/// ausgewählten Ordner; wer ihn vorher schickt, bekommt `BAD` zurück und sucht
/// den Fehler im Befehl statt im fehlenden Zustand davor.
///
/// **Lesen macht gelesen – so wie in jedem anderen Postfach auch.** Eine Mail,
/// die man aufklappt, hat man gelesen; Mimir soll dabei nicht weniger
/// vernünftig sein als ein Mailprogramm.
///
/// **`PEEK` beim Rumpf, `STORE` ausdrücklich danach.** Nach RFC 3501 setzt ein
/// Abruf von `BODY` oder `RFC822` das Gelesen-Flag als Nebenwirkung – der
/// Server bringt sie an, nicht Mimir. Darauf sich zu verlassen hieße, den
/// Zustand zu raten: Ob die Flags in der Antwort schon das Gelesen tragen,
/// entscheidet der Server, nicht Mimir. Also wird ohne Nebenwirkung gelesen und
/// das Flag mit einem eigenen `STORE` gesetzt – derselbe Befehl, den auch der
/// Knopf schickt. **Damit weiß Mimir danach, ob es geklappt hat**, und kann es
/// sagen, statt eine Mail anzubieten, die beim nächsten Nachladen wieder
/// ungelesen ist.
///
/// **Nur wenn sie noch ungelesen ist.** Sonst ginge ein `STORE` über die Leitung,
/// der nichts bewirkt – und bei einem Ordner, der nicht schreiben darf, würde
/// genau diese unnötige Runde als Fehler gemeldet.
///
/// **Der Gelesen-Stand kommt aus derselben Antwort.** `FLAGS` steht in der
/// FETCH-Antwort; ihn zu lesen ist billiger, ihn zu raten nicht.
pub async fn hole_text<T>(
    verbindung: &mut Verbindung<T>,
    ordner: &str,
    nummer: u32,
) -> Result<NachrichtVoll, String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    waehle_ordner(verbindung, ordner).await?;

    let antworten = verbindung
        .befehl(&format!(
            "B3 FETCH {nummer} (UID FLAGS ENVELOPE BODY.PEEK[])"
        ))
        .await?;

    for antwort in antworten {
        let imap_proto::Response::Fetch(nummer_gefunden, werte) = antwort else {
            continue;
        };

        // **Beides in einer Runde über die Felder.** Der Rumpf ist ein Literal und
        // deshalb groß; die Flags stehen daneben. Wer sie in einer zweiten Schleife
        // sucht, hat sie längst verworfen, sobald der Rumpf gefunden war.
        let mut roh: Option<Vec<u8>> = None;
        let mut gelesen = false;

        for wert in werte {
            match wert {
                imap_proto::AttributeValue::Flags(flaggen) => {
                    gelesen = flaggen
                        .iter()
                        .any(|flag| flag.eq_ignore_ascii_case("\\Seen"));
                }
                imap_proto::AttributeValue::BodySection {
                    data: Some(teil), ..
                } => roh = Some(teil.as_ref().to_vec()),
                _ => {}
            }
        }

        let Some(roh) = roh else { continue };

        if roh.len() > super::MAX_MAIL_MESSAGE_BYTES {
            return Err("Diese Nachricht ist größer, als Mimir lesen kann".to_string());
        }

        let (kopf, anhaenge) = zerlege_mime(&roh);
        let mut kopfzeile = kopf.kopf.clone();
        kopfzeile.gelesen = gelesen;

        // **Der Ordner ist schon gewählt, deshalb ohne erneutes `SELECT`.**
        let mut markierungsfehler = None;

        if !gelesen {
            match store_gelesen(verbindung, nummer_gefunden, true).await {
                Ok(()) => kopfzeile.gelesen = true,
                Err(fehler) => markierungsfehler = Some(fehler),
            }
        }

        return Ok(NachrichtVoll {
            kopf: NachrichtKopf {
                nummer: nummer_gefunden,
                ..kopfzeile
            },
            an: kopf.an,
            cc: kopf.cc,
            text: kopf.text,
            anhaenge,
            message_id: kopf.message_id,
            markierungsfehler,
        });
    }

    Err("Diese Nachricht konnte nicht gelesen werden".to_string())
}

/// Markiert eine Nachricht als gelesen – oder als ungelesen.
///
/// **Auch hier gilt: erst der Ordner, dann der Befehl.** `STORE` wirkt nur im
/// ausgewählten Ordner; ohne `SELECT` lehnt der Server ihn ab.
///
/// **`+FLAGS` statt `.FLAGS`.** RFC 3501 hat die alte Form abgelöst; Gmail
/// nimmt beide, aber ein Server, der nur eine kennt, wäre mit der alten Form
/// nicht bedienbar. `\\Seen` ist das einzige Flag, das hier vergeben wird: Mimir
/// zeigt einen Ordner, keinen Posteingang mit drei Spalten, und ein zweites
/// Flag wäre eine Anzeige ohne Wirkung.
pub async fn setze_gelesen<T>(
    verbindung: &mut Verbindung<T>,
    ordner: &str,
    nummer: u32,
    gelesen: bool,
) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    waehle_ordner(verbindung, ordner).await?;

    store_gelesen(verbindung, nummer, gelesen).await
}

/// **Der `STORE` allein, für einen bereits ausgewählten Ordner.**
///
/// `hole_text` markiert im Anschluss an seinen Abruf, auf derselben Verbindung
/// – dort ist der Ordner längst gewählt. Noch einmal auszuwählen wäre ein
/// weiterer Hin und her über die Leitung für etwas, das schon stimmt.
async fn store_gelesen<T>(
    verbindung: &mut Verbindung<T>,
    nummer: u32,
    gelesen: bool,
) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let art = if gelesen { "+FLAGS" } else { "-FLAGS" };

    verbindung
        .befehl(&format!("B4 STORE {nummer} {art} (\\Seen)"))
        .await?;

    Ok(())
}

/// Löscht eine Nachricht aus dem Ordner.
///
/// **Zwei Schritte, und beide gehören dazu.** Erst `+FLAGS (\Deleted)` – das ist
/// nur ein Vermerk, die Nachricht steht noch da. Dann `EXPUNGE`, und der Server
/// entfernt, was als gelöscht vermerkt ist. Nur der Vermerk allein würde die
/// Nachricht je nach Server gar nicht verschwinden lassen.
///
/// **Und `EXPUNGE` nimmt mehr mit, als Mimer markiert hat.** Der Befehl kennt
/// keine Nachricht, sondern nur das Flag: Alles, was im Ordner als gelöscht
/// vermerkt steht, geht mit – auch das, was ein anderes Programm vorgemerkt
/// hat. Das ist keine Eigenheit dieser Implementierung, sondern die des
/// Protokolls; ein Mailprogramm, das eine Nachricht wirklich löschen will,
/// kommt daran nicht vorbei.
///
/// **Danach sind die Nummern anders.** `EXPUNGE` rückt alles nach der
/// gelöschten Nachricht um eine Nummer nach vorn. Wer nach dem Löschen mit
/// alten Nummern weiterarbeitet, trifft die falsche Nachricht – der Aufrufer
/// holt deshalb die Liste neu, statt sie zu ändern.
///
/// **Gelöscht wird wirklich gelöscht.** Es gibt keinen Papierkorb: Was
/// `EXPUNGE` entfernt, hat der Server entschieden, und Mimir führt es nicht
/// irgendwo auf.
pub async fn loesche<T>(
    verbindung: &mut Verbindung<T>,
    ordner: &str,
    nummer: u32,
) -> Result<(), String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    waehle_ordner(verbindung, ordner).await?;

    verbindung
        .befehl(&format!("B4 STORE {nummer} +FLAGS (\\Deleted)"))
        .await?;

    verbindung.befehl("B5 EXPUNGE").await?;

    Ok(())
}

/// Die Felder, die aus einer rohen Nachricht gezogen werden.
struct Aufgeteilt {
    kopf: NachrichtKopf,
    an: String,
    cc: String,
    text: String,
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
    let koerper_klein = koerper.trim_start().to_ascii_lowercase();

    let mut anhaenge = Vec::new();
    let mut text = None;

    // **Kein Mehrteil heißt: der Rumpf *ist* die Nachricht.**
    //
    // Ohne `boundary` gibt es keinen Teil, dessen eigener Kopf einen
    // `content-type` tragen könnte – der einzige Kopf mit diesem Feld ist der
    // Kopf der Nachricht. Ginge man trotzdem in die Teilschleife, zerlegte sie den
    // *Text* als wäre er ein Kopf, fände keinen Typ und gäbe eine leere Nachricht
    // zurück: So verlor Mimir den Text **jeder einfachen Textmail** – also der
    // allermeisten. Es war nur nicht aufgefallen, weil die Vorschau in der Leiste
    // erst mit derselben Funktion gebaut wurde und man an einer gleich leeren
    // Liste nichts sieht, was fehlt.
    if !koerper_klein.starts_with("multipart/") {
        if koerper_klein.starts_with("text/plain") {
            text = Some(dekodiere_inhalt(
                rumpf,
                &value(&kopfzeilen, "content-transfer-encoding")
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
            ));
        }
    } else {
        let mut tiefen = 0;

        for teil in multipart(rumpf, &koerper) {
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
                // **Hier steht kein Gelesen-Stand.** Aus einer rohen Nachricht
                // lässt sich keiner ablesen – das Flag steht in den Flags des
                // Servers, nicht in den Kopfzeilen der Nachricht. Bis hierher
                // stand hier fest `true`; das war geraten und stimmte nur, wenn
                // der Server die Nachricht ohnehin schon führte. `hole_text`
                // setzt danach das, was die Antwort wirklich sagt.
                gelesen: false,
                bytes: roh.len() as u32,
                // **Hier steht keine Vorschau.** Diese Zerlegung läuft beim Öffnen
                // einer einzelnen Nachricht; die Vorschau gehört in die Liste und
                // wird dort aus einem begrenzten Abruf gebildet. Sie hier zu
                // füllen hieße, den ganzen Text an einer Stelle zu lesen, die ihn
                // nicht braucht.
                vorschau: String::new(),
            },
            an: aufloese_namen(&an),
            cc: aufloese_namen(&cc),
            text: text.unwrap_or_default(),
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

    // **Was nach dem letzten Trenner noch da ist, gehört zum letzten Anteil.**
    //
    // Eine vollständige Nachricht endet auf `--Grenze--`, und dort bricht die
    // Schleiche vorher ab – dieser Rest ist also immer nur bei einer
    // *abgeschnittenen* Nachricht vorhanden. Genau die holt der begrenzte Abruf
    // für die Vorschau: Die ersten 2048 Bytes einer Mail mit großem Anhang gehen
    // mitten in einem Mehrteil aus, und ohne diesen Rest zeigte die Leiste für
    // solche Nachrichten gar nichts.
    if let Some(start) = von {
        let mut bis = rumpf.len();

        if rumpf[..bis].ends_with(b"\r\n") {
            bis -= 2;
        } else if rumpf[..bis].ends_with(b"\n") {
            bis -= 1;
        }

        teile.push(&rumpf[start..bis.max(start)]);
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
