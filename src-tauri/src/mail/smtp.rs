//! Der SMTP-Teil: Nachricht bauen und senden.
//!
//! **Warum hier nichts gesendet wird, ohne dass ein Mensch es bestätigt hat.**
//! Der Kalender hat dieselbe Regel, und dort ist sie lösbar: Ein gelöschter
//! Termin kann wieder angelegt werden. Eine gesendete Mail nicht — sie liegt
//! beim Empfänger, sie ist in fremden Archiven gespiegelt, und sie lässt sich
//! nicht zurücknehmen. Deshalb ist die Freigabe hier nicht dieselbe wie beim
//! Schreiben in eine Datei, wo die Datei wenigstens im eigenen Verzeichnis bleibt.
//!
//! **Was vor dem Senden geprüft wird.** Wer, an wen, welcher Betreff, welcher
//! Text, und ob eine BCC-Liste mitgeht. Die BCC-Liste steht deshalb in der
//! Vorschau und nicht nur im Aufruf: Eine unsichtbare Empfängeradresse ist das
//! einzige, was hier wirklich Schaden anrichten kann, ohne dass es jemand
//! bemerkt.

use lettre::message::header::ContentType;
use lettre::message::{header, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

/// So lang darf der Rumpf einer Nachricht sein. Der Server mag 25 MiB, das Modell
/// nicht: Der Text geht in den Kontext und in die Vorschau.
pub const MAX_RUMPF_BYTES: usize = 256 * 1024;
/// Wie lange das Senden selbst dauern darf. Eine Nachricht, die so lange hängt,
/// ist nicht angekommen.
const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Was gesendet werden soll. Geprüft und bereinigt, bevor sie gebaut wird.
#[derive(Debug, Clone)]
pub struct Sendevorhaben {
    pub absender: String,
    pub an: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub betreff: String,
    pub text: String,
    /// Antwort auf eine bestehende Nachricht. Nur als Hinweis – der Server
    /// entscheidet, ob er sie als Antwort sortiert.
    pub in_antwort_auf: Option<String>,
}

impl Sendevorhaben {
    /// Wie viele Empfänger insgesamt. Für die Anzeige: „an drei Personen".
    pub fn empfaenger_gesamt(&self) -> usize {
        self.an.len() + self.cc.len() + self.bcc.len()
    }

    /// Die kürzeste Form für eine Anzeige.
    pub fn kuerze_empfaenger(&self) -> String {
        let mut alle: Vec<&String> = self.an.iter().chain(self.cc.iter()).collect();

        if self.bcc.is_empty() {
            // Ohne BCC ist die Liste vollständig, und ihre Länge ist eine
            // Information: Wer fünf Empfänger sieht, kann sie zählen.
            return alle
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(", ");
        }

        alle.extend(self.bcc.iter());

        if alle.len() <= 6 {
            alle.iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            format!(
                "{} und {} weitere – darunter eine BCC-Liste",
                alle[..3]
                    .iter()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                alle.len() - 3
            )
        }
    }
}

/// Baut die Nachricht, ohne sie zu senden.
///
/// **Getrennt vom Senden**, damit die Vorschau und der Versand aus *derselben*
/// Funktion kommen. Das ist der ganze Schutz: Was der Benutzer bestätigt, ist
/// exakt das, was hinausgeht. Ein zweiter Weg zum Versand – oder eine andere
/// Aufbereitung – hieße, dass die Vorschau eine Zusage über etwas anderes ist.
pub fn baue_nachricht(
    vorhaben: &Sendevorhaben,
    antwort_kopfzeilen: Option<&[(String, String)]>,
) -> Result<Message, String> {
    let absender = parse_mailbox(&vorhaben.absender)?;

    let mut aufbau = Message::builder()
        .from(absender)
        .subject(vorhaben.betreff.clone());

    // `lettre` nimmt eine Adresse je Aufruf und baut daraus je einen Kopf.
    // Deshalb wird hier nicht gesammelt: Jeder Empfänger bekommt seine eigene
    // To-Zeile, und die Reihenfolge im Gespräch bleibt erhalten.
    for adresse in &vorhaben.an {
        aufbau = aufbau.to(parse_mailbox(adresse)?);
    }

    for adresse in &vorhaben.cc {
        aufbau = aufbau.cc(parse_mailbox(adresse)?);
    }

    // BCC steht **nicht** in den Kopfzeilen. Sie geht nur an den Server und
    // taucht in der Nachricht selbst nicht auf – sonst empfängt jeder sie mit.
    for adresse in &vorhaben.bcc {
        aufbau = aufbau.bcc(parse_mailbox(adresse)?);
    }

    // Der Betreff wird hier noch einmal gereinigt, weil er aus dem Modell
    // kommt: Ein Zeilenumbruch darin zerlegt die Nachricht.
    let betreff = super::kuerze_betreff(&vorhaben.betreff);
    aufbau = aufbau.subject(betreff.clone());

    if let Some(message_id) = &vorhaben.in_antwort_auf {
        if !message_id.trim().is_empty() && !message_id.contains(['\r', '\n']) {
            aufbau = aufbau.header(header::InReplyTo::from(message_id.trim().to_string()));
        }
    }

    // **Aus der Vorlage kommt nur `References`.** Das ist die Kette, an der ein
    // Client erkennt, dass dies eine Antwort ist. Alles andere wird bewusst
    // nicht übernommen: `To` würde Empfänger hinzufügen, die in der Vorlage
    // standen und nicht mehr sollen; `Bcc` gehört nie in eine Vorlage, weil es
    // sonst jeder mitläse; `Subject` überschreibt den, den der Benutzer
    // gerade in der Vorschau gelesen hat.
    //
    // **Das ist auch die sichere Variante.** Eine Kopfzeile aus fremdem Text
    // ungeprüft übernehmen hieße, einem Absender zu erlauben, beliebige Felder
    // in eine von dir gesendete Nachricht zu schreiben.
    if let Some(antwort) = antwort_kopfzeilen {
        if let Some((_, kette)) = antwort
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("references"))
        {
            let bereinigt = kette
                .chars()
                .filter(|c| c.is_ascii() && *c != '\r' && *c != '\n')
                .collect::<String>();

            if !bereinigt.trim().is_empty() {
                aufbau = aufbau.header(header::References::from(bereinigt));
            }
        }
    }

    let rumpf = vorhaben
        .text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");

    // **Zwei Fassungen, eine Nachricht.** Die Textfassung ist die, die gelesen
    // wird; die HTML-Fassung existiert für Clients, die kein Text verstehen.
    // Sie entsteht aus demselben Text – es gibt keinen zweiten Inhalt, der
    // etwas anderes sagen könnte als das Gezeigte.
    aufbau
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(rumpf),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(format!("<pre>{}</pre>", html_escape(&vorhaben.text))),
                ),
        )
        .map_err(|fehler| format!("Die Nachricht ließ sich nicht zusammenbauen: {fehler}"))
}

/// Wandelt eine geprüfte Adresse in das, was der Server erwartet.
///
/// `Kai <kai@example.org>` wird aufgelöst; ohne Anzeigenamen bleibt die Adresse
/// allein. Ohne Namensfeld stünden die Anführungszeichen mit im Empfängerfeld,
/// und manche Server lehnen das ab.
fn parse_mailbox(adresse: &str) -> Result<Mailbox, String> {
    let wert = adresse.trim();

    if let (Some(von), Some(bis)) = (wert.find('<'), wert.rfind('>')) {
        if bis > von {
            let name = wert[..von].trim().trim_matches(['"', '\'']).trim();
            let adresse = wert[von + 1..bis].trim();

            // Der reine Adressenteil wird geprüft; der Anzeigename kommt
            // darüber. `parse::<Mailbox>()` akzeptiert beides, aber dann stünde
            // der Name im Adressenteil.
            let postfach = adresse
                .parse::<Mailbox>()
                .map_err(|_| format!("Die Adresse {adresse} ist nicht gültig"))?;

            if name.is_empty() {
                return Ok(postfach);
            }

            return Ok(Mailbox::new(Some(name.to_string()), postfach.email.clone()));
        }
    }

    wert.parse::<Mailbox>()
        .map_err(|_| format!("Die Adresse {wert} ist nicht gültig"))
}

/// Maskiert fünf Zeichen für die HTML-Fassung.
///
/// **Nur fünf.** Es geht nicht darum, HTML zu erzeugen, sondern darum, dass ein
/// Text, der aussieht wie HTML, auch als HTML genau das zeigt und nichts sonst.
/// Die HTML-Fassung ist eine zweite Darstellung derselben Nachricht für Clients,
/// die kein Text verstehen – sie ist nicht die, die man liest.
fn html_escape(text: &str) -> String {
    let mut ergebnis = String::with_capacity(text.len());
    for zeichen in text.chars() {
        match zeichen {
            '&' => ergebnis.push_str("&amp;"),
            '<' => ergebnis.push_str("&lt;"),
            '>' => ergebnis.push_str("&gt;"),
            '"' => ergebnis.push_str("&quot;"),
            '\'' => ergebnis.push_str("&#39;"),
            andere => ergebnis.push(andere),
        }
    }
    ergebnis
}

/// **Der einzige Weg, auf dem Mimir etwas sendet.** Kein Aufrufer baut eine
/// Nachricht selbst zusammen; jeder kommt hierher, und damit gibt es genau eine
/// Stelle, an der das Senden passiert.
///
/// Die Nachricht wird **hier** gebaut und nicht oben beim Aufrufer – sonst
/// gäbe es zwei Wege zum Versand, und die Vorschau wäre eine Zusage über eine
/// Nachricht, die vielleicht gar nicht die ist, die hinausgeht.
pub async fn sende(
    vorhaben: &Sendevorhaben,
    server: &str,
    benutzername: &str,
    passwort: &str,
) -> Result<(), String> {
    let nachricht = baue_nachricht(vorhaben, None)?;
    let (host, port) = zerlege_server(server)?;

    // **Zwei verschiedene Verbindungen, und der Unterschied ist nicht
    // Kosmetik.** Port 465 nimmt die TLS-Verbindung schon beim Verbindungsaufbau
    // an („implizit"); Port 587 nimmt sie erst danach an, per STARTTLS.
    //
    // Die erste Fassung benutzte für beides STARTTLS. Bei 465 – dem Port, den
    // Mimir selbst als Vorgabe einträgt – wartet der Server auf TLS, das nie
    // kommt, und der Versand bleibt hängen, bis die Zeit abläuft. Ein Gmail-
    // Konto hätte also über genau die Adresse, die das Programm vorschlägt,
    // nicht gesendet.
    let transport = if port == 465 {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
    }
    .map_err(|fehler| format!("Der Mailserver für den Versand ist nicht erreichbar: {fehler}"))?
    .port(port)
    .credentials(Credentials::new(
        benutzername.to_string(),
        passwort.to_string(),
    ))
    .timeout(Some(SEND_TIMEOUT))
    .build();

    // **Der Versand wird begrenzt.** Ein Server, der die Annahme bestätigt und
    // danach hängt, würde den Zug sonst endlos halten; der Benutzer sähe eine
    // laufende Anzeige und wüsste nicht, ob die Nachricht angekommen ist.
    match tokio::time::timeout(SEND_TIMEOUT, transport.send(nachricht)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(fehler)) => Err(format!(
            "Die Nachricht konnte nicht gesendet werden: {fehler}"
        )),
        Err(_) => Err(format!(
            "Der Mailserver hat die Nachricht nach {} Sekunden nicht bestätigt",
            SEND_TIMEOUT.as_secs()
        )),
    }
}

/// Zerlegt `smtp://host:port` in Host und Port.
///
/// **Eigene Funktion statt der aus `imap.rs`.** Beide Protokolle benutzen
/// dieselbe Syntax, haben aber nicht dieselbe Vorgabe: SMTP läuft über 587 mit
/// STARTTLS oder 465 direkt. Eine gemeinsame Funktion würde einem Aufrufer die
/// falsche Zahl anbieten, sobald sich eine der beiden ändert.
fn zerlege_server(server: &str) -> Result<(String, u16), String> {
    let rest = server
        .split_once("://")
        .map_or("", |(_, rest)| rest)
        .trim_end_matches('/');

    let (host, port) = rest
        .rsplit_once(':')
        .ok_or_else(|| format!("Die SMTP-Adresse nennt keinen Port: {server}"))?;

    let port: u16 = port
        .parse()
        .map_err(|_| format!("Der Port in der SMTP-Adresse ist ungültig: {port}"))?;

    if host.is_empty() {
        return Err("Die SMTP-Adresse nennt keinen Server".to_string());
    }

    Ok((host.to_string(), port))
}

#[cfg(test)]
#[path = "smtp_tests.rs"]
mod tests;
