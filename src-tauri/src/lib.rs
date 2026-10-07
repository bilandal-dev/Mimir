mod calendar;

use ammonia::{Builder, UrlRelative};
use futures_util::StreamExt;
use pulldown_cmark::{html, Options, Parser};
use reqwest::{redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{ipc::Channel, Emitter, Manager, State};
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

/// Vorgabe, solange der Benutzer keinen Server eingetragen hat.
///
/// Bewusst `localhost` und **nicht** die Adresse des Rechners, an dem Mimir
/// entwickelt wurde: Der Wert landet als Zeichenkette im ausgelieferten Binary
/// und wird beim ersten Start in die Konfiguration geschrieben. Mit einer fremden
/// Adresse startet jeder, der die App bekommt, auf dem Server des Entwicklers – und
/// `strings` auf dem Binary zeigt sie jedem. `localhost` ist zugleich der
/// wahrscheinlichste Wert, wenn Ollama auf demselben Rechner läuft.
///
/// `localhost` deckt den Fall ab, dass Ollama auf demselben Rechner läuft. Wer
/// Mimir auf einem anderen Rechner startet, sieht einen ausgefallenen Server und
/// trägt die Adresse über den Knopf **Adresse** im Kopf ein. Das war vorher nicht
/// möglich: Es gab nur `/server-url` im Chat, und wer den Server nicht erreicht,
/// erreicht auch den Chat nicht sinnvoll.
const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
/// Zeitgrenze der Statusanzeige im Kopf.
///
/// Drei Sekunden waren zu knapp: Ist der Server gerade mit dem Laden oder
/// Erzeugen beschäftigt, gilt er sonst als ausgefallen, obwohl er läuft. Fünf
/// Sekunden sind noch schnell genug, um eine kurze Funkstelle von einem echten
/// Ausfall zu trennen – und für letzteren ist ohnehin der Neustart der Weg.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const OLLAMA_MODELS_TIMEOUT: Duration = Duration::from_secs(10);
/// Wartezeit auf die Antwortköpfe. Bei einer lokalen Inferenz kann der erste
/// Token auch bei kleinem Prompt lange brauchen, wenn der Rechner gerade
/// ausgelastet ist: gemessen wurden 2,6 s bei freier Maschine, ein laufender
/// Übersetzungsvorgang verlängert das aber um ein Vielfaches. Die Grenze ist
/// bewusst großzügig, weil die Regeln für den laufenden Strom (Health-Probe nach
/// fünf Minuten, Abbruch nach 15 Minuten ohne Token) den Lauf ohnehin begrenzen.
const OLLAMA_HEADER_TIMEOUT: Duration = Duration::from_secs(120);
const OLLAMA_TCP_KEEPALIVE: Duration = Duration::from_secs(120);
const OLLAMA_TCP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const OLLAMA_TCP_KEEPALIVE_RETRIES: u32 = 4;
/// Die vier Werte steuern gemeinsam die Funkstellen-Erkennung während einer
/// laufenden Antwort.
///
/// `INTERVAL` ist der Takt der Nebenprüfung, `MIN_SILENCE` die Ruhe, ab der
/// überhaupt erst geprüft wird: In den ersten sechzig Sekunden nach dem letzten
/// Zeichen soll die Leiste nicht schon einen Servertest neben dem Strom
/// aufmachen. `TIMEOUT` ist die Zeitgrenze dieses Tests, `FAILURE_LIMIT` die Zahl
/// der Fehlversuche, ab der die Verbindung als unterbrochen gilt. Zwei statt
/// dreier Versuche wären schneller, könnten aber eine einzelne Funklücke als
/// Abbruch melden.
const OLLAMA_STALL_PROBE_INTERVAL: Duration = Duration::from_secs(60);
const OLLAMA_STALL_PROBE_MIN_SILENCE: Duration = Duration::from_secs(60);
const OLLAMA_STALL_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const OLLAMA_STALL_FAILURE_LIMIT: usize = 3;
/// Vor dem Neuversand nach einem Verbindungsabbruch wird der Server einmal
/// geprüft. Ein Verbindungsfehler bedeutet nicht, dass der Server weg ist – der
/// Abbruch kann ebenso gut die Strecke betroffen haben.
const OLLAMA_RETRY_HEALTH_ATTEMPTS: usize = 3;
const OLLAMA_RETRY_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OLLAMA_RETRY_HEALTH_DELAY: Duration = Duration::from_secs(5);
const OLLAMA_TOKEN_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const OLLAMA_NO_TOKEN_LIMIT: Duration = Duration::from_secs(900);
const OLLAMA_HEALTH_FAILURE_LIMIT: usize = 2;
/// Für den Chat gilt dieselbe Regel, aber großzügiger.
///
/// Gemessen wurde im WLAN zwischen Mimir und dem Server eine Runtrip-Zeit
/// zwischen 60 und 265 ms, zeitweise mit Verlust. Linux wiederholt einen
/// Verbindungsaufbau nach 1, 2 und 4 Sekunden; bei Funkstille braucht der
/// Handshake damit leicht mehr als fünf Sekunden, und die Meldung „operation
/// timed out" beschreibt einen verlorenen Handshake, keinen ausgefallenen
/// Server. Für die kleinen Prüfungen bleibt die kurze Grenze: Die sollen
/// schnell und schmerzfrei antworten.
const OLLAMA_CHAT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Wie lange eine unbenutzte Verbindung im Pool liegen bleibt. Zwischen zwei
/// Nachrichten ist das genug, um die bestehende Verbindung weiterzuverwenden.
const OLLAMA_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Mehrere offene Verbindungen: Eine für die laufende Antwort, eine für die
/// parallele Statusprüfung während einer Funkstelle.
const OLLAMA_POOL_MAX_IDLE: usize = 4;
const SSH_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_SERVER_URL_BYTES: usize = 2048;
const MAX_MODEL_NAME_BYTES: usize = 256;
const MAX_MODEL_COUNT: usize = 1000;
const MAX_HISTORY_MESSAGES: usize = 100;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Obergrenze für eine angehängte Datei. Großzügiger als `MAX_MESSAGE_BYTES`,
/// weil `prepare_attachment` den Inhalt auf die Grenze kürzt, die das Backend
/// tatsächlich durchlässt – so kann eine große Datei noch nützlich sein, statt
/// mit einer Fehlermeldung zu scheitern.
const MAX_ATTACHMENT_BYTES: usize = 1024 * 1024;
/// Obergrenze für den Denktext eines einzelnen Chunks. Der Gesamtstrom ist über
/// `MAX_STREAMED_RESPONSE_BYTES` begrenzt, hier wird nur ein einzelner Chunk
/// zusätzlich in der Länge gedeckelt.
const MAX_THINKING_BYTES: usize = 256 * 1024;
const MAX_PROMPT_BYTES: usize = 512 * 1024;
const MAX_MODELS_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_STREAMED_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_STREAM_LINE_BYTES: usize = 256 * 1024;
const MAX_STREAM_LINES: usize = 100_000;
const MAX_CHAT_ATTEMPTS: usize = 3;
const MAX_MARKDOWN_BYTES: usize = 512 * 1024;
const MAX_RENDERED_MARKDOWN_BYTES: usize = 2 * 1024 * 1024;
const MAX_SSH_ERROR_BYTES: usize = 4096;
const MAX_SSH_PASSWORD_BYTES: usize = 4096;
const MAX_SSH_TARGET_BYTES: usize = 255;
const MAX_SSH_IDENTITY_BYTES: usize = 4096;
const MAX_CONFIG_TEMP_ATTEMPTS: usize = 100;

// --- Agentenmodus -------------------------------------------------------------
// Harte Grenzen des Werkzeugzugriffs. Sie gelten unabhängig von allem, was das
// Modell oder die Oberfläche senden, weil sie im Backend durchgesetzt werden.
const MAX_TOOL_NAME_BYTES: usize = 64;
const MAX_TOOL_PATH_BYTES: usize = 1024;
const MAX_TOOL_ARGUMENT_BYTES: usize = 8 * 1024;
const MAX_TOOL_PATTERN_BYTES: usize = 256;
const MAX_TOOL_FILE_BYTES: u64 = 128 * 1024;
const MAX_TOOL_DIRECTORY_ENTRIES: usize = 500;
const MAX_TOOL_SEARCH_DEPTH: usize = 8;
const MAX_TOOL_SEARCH_FILES: usize = 2000;
const MAX_TOOL_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TOOL_SEARCH_MATCHES: usize = 200;
const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_TOOL_CALLS_PER_MESSAGE: usize = 8;
const MAX_TOOL_SCHEMAS_BYTES: usize = 64 * 1024;
const MAX_AGENT_ROOT_BYTES: usize = 1024;
const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;
const MAX_SYSTEM_PROMPT_BYTES: usize = 8 * 1024;
const MIN_CONTEXT_TOKENS: usize = 2048;
const MAX_CONTEXT_TOKENS: usize = 262144;
const MAX_HISTORY_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_AGENT_STEPS_LIMIT: usize = 20;
const DEFAULT_AGENT_MAX_STEPS: usize = 8;

// --- Schreibende Werkzeuge --------------------------------------------------
// Der Schreibmodus gilt nur für die laufende Sitzung und wird bewusst nicht
// gespeichert: Ein dauerhaft gesetzter Schreibzugriff in der Konfigurationsdatei
// würde einen Zugang hinterlassen, den niemand mehr erwartet.
const MAX_TOOL_WRITE_BYTES: usize = 256 * 1024;
const MAX_TOOL_WRITE_OLD_BYTES: usize = 64 * 1024;
const MAX_TOOL_WRITE_ARGUMENT_BYTES: usize = 512 * 1024;
const MAX_TOOL_PREVIEW_BYTES: usize = 64 * 1024;
const MAX_TOOL_WRITES_PER_TURN: usize = 12;
const MAX_TOOL_WRITE_TOTAL_BYTES: usize = 512 * 1024;
const MAX_TOOL_UNDO_ENTRIES: usize = 12;

/// Exit-Code, den OpenSSH für Verbindungs-, Host-Key- und Authentifizierungs-
/// fehler verwendet. Andere Codes stammen aus dem Remote-Skript.
const SSH_TRANSPORT_FAILURE_CODE: i32 = 255;

#[cfg(windows)]
const SSH_CONFIG_FILE: &str = "NUL";
#[cfg(not(windows))]
const SSH_CONFIG_FILE: &str = "/dev/null";

/// Env-Teil des Startbefehls, mit dem der Server tatsächlich gestartet wird.
/// Zusammen mit dem Programmnamen ergibt das exakt
/// `OLLAMA_HOST="0.0.0.0" OLLAMA_ORIGINS="*" ollama serve`.
///
/// `OLLAMA_HOST="0.0.0.0"` bindet den Server auf alle Interfaces, damit er über
/// die eingetragene Adresse aus dem LAN erreichbar ist. `OLLAMA_ORIGINS="*"`
/// hebt die CORS-Beschränkung auf – Mimir selbst braucht das nicht, weil alle
/// Anfragen aus Rust kommen und keinen `Origin`-Header mitsenden. Nötig ist es für
/// jeden Browser im LAN, der den Server benutzen will. Achtung: Ollama hat keine
/// eigene Authentifizierung, der Port ist damit im LAN offen.
const OLLAMA_SERVE_ENV: &str = r#"OLLAMA_HOST="0.0.0.0" OLLAMA_ORIGINS="*""#;

/// Port, auf dem Ollama seinen eigenen Default-Listen-Port hat. Wird nur für
/// die Erreichbarkeitsprüfung im Remote-Skript gebraucht und muss dort in
/// Hexadezimal stehen, weil `/proc/net/tcp` Ports so führt.
const OLLAMA_PORT: u16 = 11434;

// Startet den Ollama-Server auf dem Remote-System fest auf 0.0.0.0, damit er
// über die eingegebene Server-Adresse aus dem LAN erreichbar ist. Läuft als
// `sh -c`, deshalb POSIX-kompatibel und ohne Bashismen.
// `@OLLAMA_SERVE_ENV@` und `@OLLAMA_PORT_HEX@` werden in `start_ollama_script`
// ersetzt, damit Startbefehl und Portprüfung an genau einer Stelle definiert
// sind und nicht auseinanderlaufen können.
const START_OLLAMA_TEMPLATE: &str = r#"
# Bekannte Installationsorte zuerst, das geerbte PATH danach. Ein Ersetzen würde
# ein Ollama übersehen, das nur im eigenen PATH des Benutzers liegt.
PATH="$HOME/.ollama/bin:$HOME/.local/share/ollama/bin:$HOME/ollama/bin:$HOME/.local/bin:$HOME/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin:/opt/ollama/bin:/opt/homebrew/bin${PATH:+:$PATH}"
export PATH
umask 077

log_dir="$HOME/.local/state/mimir"
mkdir -p "$log_dir" || exit 1
log_file="$log_dir/ollama.log"
if [ -L "$log_file" ]; then rm -f "$log_file"; fi
if [ -f "$log_file" ] && [ "$(wc -c < "$log_file")" -ge 10485760 ]; then
  mv -f "$log_file" "$log_file.1"
fi

abort() {
  printf '%s\n' "$1" >&2
  printf '%s\n' "$1" >> "$log_file"
  exit "$2"
}

# Prueft, ob auf 0.0.0.0:@OLLAMA_PORT_DEC@ ueberhaupt ein Listener liegt. Der Port
# steht in /proc/net/tcp hexadezimal, 0A ist LISTEN. Eine Adresse aus lauter
# Nullen ist die Wildcard: 00000000 fuer IPv4, 32 Nullen fuer IPv6. Alles
# andere, auch 127.0.0.1, ist eine einzelne Adresse und damit aus dem LAN nicht
# erreichbar. Rueckgabe: 0 = wildcard, 1 = definitiv keine, 2 = nicht
# feststellbar.
listening_on_all_interfaces() {
  [ -r /proc/net/tcp ] || return 2
  awk '
    $4 == "0A" {
      split($2, address, ":")
      if (address[2] == "@OLLAMA_PORT_HEX@" && address[1] ~ /^0+$/) { found = 1 }
    }
    END { exit(found ? 0 : 1) }
  ' /proc/net/tcp /proc/net/tcp6 2>/dev/null
}

# Ein laufender Prozess allein genuegt nicht: Die Ollama-Desktop-App lauscht
# standardmaessig nur auf 127.0.0.1 und waere aus dem LAN nicht erreichbar.
# Statt stillschweigend Erfolg zu melden wird dann mit einer Handlungsanweisung
# abgebrochen, weil ein zweiter Start am belegten Port scheitern wuerde.
if command -v pgrep >/dev/null 2>&1 && pgrep -x ollama >/dev/null 2>&1; then
  listening_on_all_interfaces
  reachable=$?
  if [ "$reachable" -eq 0 ]; then
    exit 0
  fi
  if [ "$reachable" -eq 1 ]; then
    abort "Ollama laeuft bereits, lauscht aber nicht auf 0.0.0.0:11434 und ist aus dem LAN nicht erreichbar. Laeuenden Prozess beenden oder mit OLLAMA_HOST=\"0.0.0.0\" neu starten." 2
  fi
  exit 0
fi

ollama_bin="$(command -v ollama 2>/dev/null)"
if [ -z "$ollama_bin" ]; then
  for candidate in "$HOME/ollama/ollama" /opt/ollama/ollama /opt/homebrew/sbin/ollama /opt/homebrew/bin/ollama /usr/local/bin/ollama /usr/bin/ollama /Applications/Ollama.app/Contents/Resources/ollama; do
    if [ -f "$candidate" ] && [ -x "$candidate" ]; then
      ollama_bin="$candidate"
      break
    fi
  done
fi
if [ -z "$ollama_bin" ]; then
  abort "ollama binary not found (PATH=$PATH)" 127
fi

# Entspricht OLLAMA_HOST="0.0.0.0" OLLAMA_ORIGINS="*" ollama serve. nohup und die
# Umleitung trennen den Prozess von der SSH-Sitzung, damit er sie ueberlebt.
nohup env @OLLAMA_SERVE_ENV@ "$ollama_bin" serve </dev/null >"$log_file" 2>&1 &
exit 0
"#;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Ein Werkzeugaufruf, wie ihn Ollama im `message`-Objekt liefert.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ToolCall {
    function: ToolCallFunction,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ToolCallFunction {
    name: String,
    /// Modelle senden das Argument-Objekt mal als Objekt, mal als
    /// JSON-Zeichenkette. Beides wird akzeptiert, gesendet wird immer ein
    /// Objekt.
    arguments: ToolArguments,
}

/// Nimmt die beiden Formen an, die Modelle für Werkzeugargumente benutzen, und
/// gibt sie immer als Objekt weiter. Unparsbarer Text bleibt als String
/// erhalten, damit die Werkzeugprüfung eine verständliche Meldung liefert.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolArguments {
    value: serde_json::Value,
}

impl ToolArguments {
    fn to_value(&self) -> serde_json::Value {
        self.value.clone()
    }

    #[cfg(test)]
    fn from_value(value: serde_json::Value) -> Self {
        Self { value }
    }
}

impl Serialize for ToolArguments {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ToolArguments {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Bewusst über `Value` und nicht per `untagged`: Ein JSON-Text ist auch
        // ein gültiger `Value`, der zweite Zweig der Alternative wäre sonst nie
        // erreichbar.
        let value = serde_json::Value::deserialize(deserializer)?;

        let value = match value {
            serde_json::Value::String(text) => {
                serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text))
            }
            other => other,
        };

        Ok(Self { value })
    }
}

impl ToolCallFunction {
    fn validated(&self) -> Result<(String, serde_json::Value), String> {
        if self.name.is_empty() || self.name.len() > MAX_TOOL_NAME_BYTES {
            return Err("Das Modell hat einen ungültigen Werkzeugnamen angefordert".to_string());
        }
        if !self
            .name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err("Das Modell hat einen ungültigen Werkzeugnamen angefordert".to_string());
        }
        Ok((self.name.clone(), self.arguments.to_value()))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ChatMessage {
    role: String,
    content: String,
    /// Denktext eines Reasoning-Modells. Ollama liefert ihn je Stream-Chunk
    /// zusätzlich zu `content`. Er wird nur gelesen und angezeigt, nie
    /// zurückgeschickt, damit der Folgeauftrag nicht unnötig um diesen Text
    /// wächst.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    thinking: String,
    /// Werkzeugaufrufe der Antwort. Sie gehören zur Nachricht im Verlauf und
    /// werden deshalb mitgesendet, im Gegensatz zum Denktext.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ToolCall>,
    /// Name des Werkzeugs bei einer `role: "tool"`-Nachricht.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    tool_name: String,
}

/// Ein Textstück des Antwortstroms. Denk- und Antworttext werden getrennt
/// übertragen, damit die Oberfläche sie unterschiedlich darstellen kann.
#[derive(Clone, Serialize, Debug, PartialEq)]
struct StreamChunk {
    /// Antworttext des Modells.
    content: String,
    /// Denktext; nur gesetzt, wenn das Modell reasoning liefert.
    #[serde(skip_serializing_if = "String::is_empty")]
    thinking: String,
    /// Vom Modell angeforderte Werkzeugaufrufe; treffen sie mit dem letzten
    /// Chunk einer Runde auf und sind dann kein Text mehr.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ToolCall>,
}

impl StreamChunk {
    fn is_empty(&self) -> bool {
        self.content.is_empty() && self.thinking.is_empty() && self.tool_calls.is_empty()
    }
}

impl ChatMessage {
    /// Liefert die Nachricht nur dann als Fortschritt, wenn sie echten Text
    /// enthält. Reine Metadaten-Chunks ohne Text sind kein Lebenszeichen.
    fn has_text(&self) -> bool {
        !self.content.is_empty() || !self.thinking.is_empty()
    }
}

/// Der Denktext dient nur der Anzeige. Vor jedem Request wird er entfernt, damit
/// er nicht als Kontext an Ollama zurückgeht und der Folgeauftrag nicht unnötig
/// um ihn wächst.
fn without_thinking(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .map(|message| ChatMessage {
            thinking: String::new(),
            ..message
        })
        .collect()
}

/// Einstellungen des Chats: dauerhafte Anweisungen, Kontextgröße und ob der
/// Verlauf gesichert werden darf.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct ChatConfig {
    /// Dauerhafte Anweisung, die vor jeder Unterhaltung mitgeschickt wird.
    /// Leer heißt: keine.
    #[serde(default)]
    system_prompt: String,
    /// Kontextfenster in Token. `0` heißt: die Vorgabe des Modells gilt.
    #[serde(default)]
    context_tokens: usize,
    /// Verlauf auf der Platte halten. Standardmäßig aus, weil der Verlauf die
    /// eigenen Daten enthält.
    #[serde(default)]
    save_history: bool,
}

impl ChatConfig {
    fn context_option(&self) -> Option<OllamaOptions> {
        match self.context_tokens {
            0 => None,
            tokens => Some(OllamaOptions { num_ctx: tokens }),
        }
    }
}

pub fn normalize_system_prompt(value: &str) -> Result<String, String> {
    let prompt = value.trim();

    if prompt.is_empty() {
        return Ok(String::new());
    }

    if prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
        return Err(format!(
            "Die Systemanweisung ist zu lang. Erlaubt sind höchstens {} KiB.",
            MAX_SYSTEM_PROMPT_BYTES / 1024
        ));
    }

    // Steuerzeichen würden die Antwort verfälschen; Zeilenumbrüche und der
    // Wagenrücklauf sind erlaubt, weil ein aus dem Browser übernommener Text
    // beide enthalten kann.
    if prompt
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t' | '\r'))
    {
        return Err("Die Systemanweisung enthält ungültige Zeichen".to_string());
    }

    Ok(prompt.to_string())
}

/// Prüft die Kontextgröße und rundet sie auf ein Vielfaches von 256. Eine
/// Obergrenze ist hier keine Formsache: Ollama reserviert den Speicher für das
/// angeforderte Fenster, eine manipulierte Anfrage dürfte den Server nicht
/// beliebig belasten können.
fn validate_context_tokens(value: usize) -> Result<usize, String> {
    if value == 0 {
        return Ok(0);
    }

    if !(MIN_CONTEXT_TOKENS..=MAX_CONTEXT_TOKENS).contains(&value) {
        return Err(format!(
            "Das Kontextfenster muss zwischen {} und {} Token liegen, oder 0 für die Modellvorgabe.",
            MIN_CONTEXT_TOKENS, MAX_CONTEXT_TOKENS
        ));
    }

    Ok(value / 256 * 256)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct OllamaConfig {
    server_url: String,
    /// Woher die Modelle kommen. Fehlt das Feld in einer alten Konfiguration,
    /// gilt das bisherige Verhalten: der eingetragene Server.
    #[serde(default)]
    provider: Provider,
    #[serde(default)]
    ssh: SshConfig,
    #[serde(default)]
    agent: AgentConfig,
    #[serde(default)]
    chat: ChatConfig,
    /// Kalenderleiste. Leer, solange keine Instanz eingetragen ist.
    #[serde(default)]
    calendar: crate::calendar::CalendarConfig,
}

/// Woher die Modelle kommen.
///
/// Vorher gab es nur eine Adresse, und die zeigte auf einen Rechner im Netz. Wer
/// ein Modell auf dem eigenen Rechner laufen lassen will, musste die Adresse von
/// Hand umstellen – mitten im Betrieb, und der Verlauf des anderen Servers blieb
/// stehen. Der Provider ist deshalb eine eigene, gespeicherte Wahl: `remote`
/// benutzt die eingetragene Adresse, `local` die feste Vorgabe auf diesem Rechner.
///
/// Das ist bewusst kein eingebettetes Modell: `local` redet weiterhin mit Ollama,
/// nur eben mit dem, das auf diesem Rechner läuft.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Provider {
    /// Ollama an der eingetragenen Adresse, heute meist ein Rechner im Netz.
    #[default]
    Remote,
    /// Ollama auf diesem Rechner, immer unter der Vorgabeadresse.
    Local,
}

impl Provider {
    /// Was die Oberfläche und die Fehlermeldungen dem Benutzer nennen.
    fn bezeichnung(self) -> &'static str {
        match self {
            Provider::Remote => "entferntes Ollama",
            Provider::Local => "lokales Ollama",
        }
    }

    /// Ob über diesen Provider das Dateisystem erreichbar sein darf.
    ///
    /// Das lokale Modell ist ein kleiner Assistent auf demselben Rechner, auf dem
    /// auch Mimir läuft: Es bekommt dort nur die Kalenderwerkzeuge. Nicht als
    /// Sicherheitsgrenze – die Adresse ist frei wählbar –, sondern weil ein
    /// kleines Modell, das neben dem Assistenten auf demselben Rechner läuft,
    /// keinen Grund hat, in dessen Arbeitsverzeichnis zu stöbern.
    fn erlaubt_dateizugriff(self) -> bool {
        matches!(self, Provider::Remote)
    }

    /// Die Adresse, unter der dieser Provider antwortet.
    fn adresse(self, remote: &str) -> &str {
        match self {
            Provider::Remote => remote,
            // Fest, weil es auf diesem Rechner keine andere gibt: Hier liefe ein
            // Ollama, das nicht auf `localhost` lauscht, nicht.
            Provider::Local => DEFAULT_OLLAMA_BASE_URL,
        }
    }
}

/// Der Umfang, der tatsächlich gilt.
///
/// Der gespeicherte Umfang bleibt beim Wechsel des Providers unberührt: Wer auf
/// das lokale Modell wechselt und wieder zurück, findet seinen Umfang so vor,
/// wie er war. Nur solange das lokale Modell läuft, gilt der Terminumfang – und
/// zwar im Backend, an jeder Stelle, an der Werkzeuge angeboten oder geprüft
/// werden. Eine nur in der Oberfläche gesetzte Anzeige wäre an einer Stelle, die
/// jemand später neu baut, wieder weg.
fn wirksamer_umfang(config: &OllamaConfig) -> Scope {
    if config.provider.erlaubt_dateizugriff() {
        config.agent.scope
    } else {
        Scope::Termine
    }
}

/// Wie weit das Modell reichen darf.
///
/// Der Umfang steht nicht im Chat, sondern in der Konfiguration: Er ist eine
/// Absicht, die über einen Neustart hinweg gilt, und im Chat würde er sich nur
/// schwer vom Schreibmodus unterscheiden lassen. Gespeichert wird er trotzdem nur
/// deshalb, weil er den Zugriff nur verkleinern kann – `Termine` nimmt dem Modell
/// die Dateiwerkzeuge und das Arbeitsverzeichnis, es gibt keine Form davon, die
/// mehr erlaubt als `Agent`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Scope {
    /// Bisheriger Stand: lesende und freigegebene schreibende Dateiwerkzeuge im
    /// festen Arbeitsverzeichnis, dazu der Kalender.
    #[default]
    Agent,
    /// Nur die Kalenderwerkzeuge. Ohne Arbeitsverzeichnis und damit ohne jeden
    /// Dateizugriff.
    Termine,
}

impl Scope {
    /// Was `/scope` und die Fehlermeldungen dem Benutzer nennen.
    fn bezeichnung(self) -> &'static str {
        match self {
            Scope::Agent => "Agentenmodus",
            Scope::Termine => "Terminumfang",
        }
    }
}

/// Einstellungen des Agentenmodus. Das Arbeitsverzeichnis ist die einzige
/// Stelle, aus der gelesen werden darf, und es wird bei jedem Zugriff erneut
/// geprüft.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct AgentConfig {
    /// Absolutes Arbeitsverzeichnis. Leer heißt: der Agentenmodus hat noch kein
    /// Arbeitsverzeichnis und verweigert jeden Zugriff. Im Terminumfang bleibt
    /// das Feld unberührt, weil dort nichts gelesen wird.
    #[serde(default)]
    root: String,
    /// Obergrenze der Werkzeugschritte je Zug.
    #[serde(default = "default_agent_max_steps")]
    max_steps: usize,
    /// Wie weit das Modell reichen darf.
    #[serde(default)]
    scope: Scope,
}

fn default_agent_max_steps() -> usize {
    DEFAULT_AGENT_MAX_STEPS
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            // Startwert ist das eigene Home-Verzeichnis des Benutzers. Es wird
            // nur gesetzt, wenn es existiert; sonst bleibt der Modus gesperrt,
            // bis /agent-dir einen gültigen Pfad setzt.
            root: default_agent_root(),
            max_steps: default_agent_max_steps(),
            scope: Scope::default(),
        }
    }
}

/// Liest das Home-Verzeichnis aus der Umgebung. Ohne nutzbares HOME bleibt der
/// Wert leer, damit nichts erfunden wird.
fn default_agent_root() -> String {
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty() && !home.contains('\\') && !home.contains('\0'))
        .unwrap_or_default()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SshConfig {
    #[serde(default)]
    target: String,
    #[serde(default = "default_ssh_port")]
    port: u16,
    /// Optionaler absoluter Pfad zu einem privaten SSH-Schlüssel. Leer heißt:
    /// nur die OpenSSH-Standardpfade und der `ssh-agent` werden verwendet.
    #[serde(default)]
    identity_file: String,
}

fn default_ssh_port() -> u16 {
    22
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            target: String::new(),
            port: default_ssh_port(),
            identity_file: String::new(),
        }
    }
}

struct OllamaSettings {
    config: tokio::sync::RwLock<OllamaConfig>,
    config_path: PathBuf,
    history_path: PathBuf,
    /// Das App-Passwort steht in einer eigenen Datei mit den Rechten 0600, damit
    /// es nicht mit der frei kopierbaren Konfiguration wandert.
    credential_path: PathBuf,
    /// Wann zuletzt eine Antwort von Ollama ankam, als Sekunden seit 1970.
    ///
    /// Damit unterscheidet die Statusanzeige einen ausgefallenen Server von einer
    /// Funkstelle. Sonst springt der Zustand bei jedem kurzen Aussetzer auf
    /// „Offline“ und bietet einen Neustart an, obwohl der Server die ganze Zeit
    /// gelaufen ist.
    last_contact: std::sync::Mutex<Option<i64>>,
}

struct ChatControl {
    cancel: tokio::sync::watch::Sender<bool>,
}

impl ChatControl {
    fn new() -> Self {
        Self {
            cancel: tokio::sync::watch::channel(false).0,
        }
    }
}

/// Baut die Einstellungen für einen Test, ohne eine Datei anzufassen.
///
/// Mehrere Prüfungen brauchen die gleiche Grundlage: eine Konfiguration, einen
/// Ort für die Dateien und das Feld für den letzten Kontakt.
#[cfg(test)]
fn test_settings() -> OllamaSettings {
    OllamaSettings {
        config: tokio::sync::RwLock::new(OllamaConfig {
            server_url: "http://localhost:11434".to_string(),
            provider: Provider::Remote,
            ssh: SshConfig::default(),
            agent: AgentConfig::default(),
            chat: ChatConfig::default(),
            calendar: crate::calendar::CalendarConfig::default(),
        }),
        config_path: std::env::temp_dir().join("mimir-test-ollama.json"),
        history_path: std::env::temp_dir().join("mimir-test-chat-history.json"),
        credential_path: std::env::temp_dir().join("mimir-test-calendar-secret.json"),
        last_contact: std::sync::Mutex::new(None),
    }
}

/// Die Uhrzeit in Sekunden seit 1970. Läuft die Rechnung aus, ist das keine
/// Zeitangabe, sondern ein Fehler – dann eben ohne Vermerk.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dauer| dauer.as_secs() as i64)
        .unwrap_or(0)
}

impl OllamaSettings {
    fn load(config_path: PathBuf) -> Result<Self, String> {
        let mut config = match std::fs::read_to_string(&config_path) {
            Ok(contents) => serde_json::from_str::<OllamaConfig>(&contents)
                .map_err(|error| format!("Ungültige Ollama-Konfiguration: {}", error))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => OllamaConfig {
                server_url: DEFAULT_OLLAMA_BASE_URL.to_string(),
                provider: Provider::default(),
                ssh: SshConfig::default(),
                agent: AgentConfig::default(),
                chat: ChatConfig::default(),
                calendar: crate::calendar::CalendarConfig::default(),
            },
            Err(error) => return Err(format!("Ollama-Konfiguration nicht lesbar: {}", error)),
        };

        config.server_url = normalize_server_url(&config.server_url)?;
        config.ssh = validate_ssh_config(config.ssh)?;
        config.agent = validate_agent_config(config.agent)?;
        config.chat = validate_chat_config(config.chat)?;
        config.calendar = validate_calendar_config(config.calendar)?;

        // Verlauf und Zugangsdaten liegen je in eigener Datei: Der Verlauf ist
        // Datenbestand und wächst, die Konfiguration soll klein und frei kopierbar
        // bleiben, und in ihr darf kein Passwort stehen.
        let credential_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("calendar-secret.json");
        let history_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("chat-history.json");

        Ok(Self {
            config: tokio::sync::RwLock::new(config),
            config_path,
            history_path,
            credential_path,
            last_contact: std::sync::Mutex::new(None),
        })
    }

    /// Vermerkt, dass Ollama gerade geantwortet hat.
    pub fn mark_contact(&self) {
        if let Ok(mut slot) = self.last_contact.lock() {
            *slot = Some(now_seconds());
        }
    }

    /// Wann zuletzt eine Antwort kam, in Sekunden seit 1970.
    pub fn last_contact(&self) -> Option<i64> {
        self.last_contact.lock().ok().and_then(|slot| *slot)
    }

    /// Nimmt ein gespeichertes App-Passwort in den Sitzungszustand, wenn es zum
    /// eingetragenen Benutzer gehört. Wird beim Start aufgerufen, damit die
    /// Anmeldung nach einem Neustart nicht neu gemacht werden muss.
    fn restore_calendar_session(&self, session: &crate::calendar::CalendarSession) -> bool {
        let config = match self.config.try_read() {
            Ok(config) => config,
            Err(_) => return false,
        };

        if config.calendar.username.is_empty() {
            return false;
        }

        let Some(stored) = crate::calendar::read_stored_credential(
            &self.credential_path,
            &config.calendar.username,
        ) else {
            return false;
        };

        session.set_password(&stored.app_password).is_ok()
    }

    /// Merkt das App-Passwort auf Wunsch auf der Platte. Ein nicht
    /// gewünschtes Passwort wird dort entfernt, damit kein alter Stand
    /// zurückbleibt.
    fn persist_calendar_credential(
        &self,
        username: &str,
        password: &str,
        remember: bool,
    ) -> Result<(), String> {
        if remember {
            return crate::calendar::write_stored_credential(
                &self.credential_path,
                username,
                password,
            );
        }

        crate::calendar::delete_stored_credential(&self.credential_path)
    }

    fn forget_calendar_credential(&self) -> Result<(), String> {
        crate::calendar::delete_stored_credential(&self.credential_path)
    }

    /// Liegt das App-Passwort zu diesem Benutzer auf der Platte? Nur die
    /// Existenz wird geprüft, das Passwort selbst wird nicht gelesen – das
    /// braucht nur `calendar_status`, um den Zustand der Leiste zu füllen.
    fn has_stored_calendar_credential(&self, username: &str) -> bool {
        if username.is_empty() {
            return false;
        }

        crate::calendar::read_stored_credential(&self.credential_path, username).is_some()
    }

    /// Schreibt den Verlauf atomar und mit restriktiven Rechten. Ohne Freischaltung
    /// wird nichts geschrieben: Der Verlauf enthält, was der Benutzer geschrieben
    /// hat, und soll nicht ungefragt auf der Platte liegen.
    async fn persist_history(&self, messages: &[ChatMessage]) -> Result<(), String> {
        if !self.get_config().await.chat.save_history {
            return Ok(());
        }

        // Werkzeugergebnisse und leere Assistenten-Toolaufrufe sind für eine
        // spätere Fortsetzung wertlos; ohne sie wäre die Datei nur Ballast.
        let payload: Vec<&ChatMessage> = messages
            .iter()
            .filter(|message| message.role != "tool" && !message.content.is_empty())
            .collect();

        validate_history(&payload)?;
        let serialized = serde_json::to_string_pretty(&payload)
            .map_err(|error| format!("Verlauf nicht serialisierbar: {}", error))?;

        let parent = config_parent(&self.history_path);
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Konfigurationsverzeichnis nicht erstellbar: {}", error))?;
        let (temporary_path, mut temporary_file) =
            create_temporary_config_file(parent, &self.history_path)?;
        let result = (|| -> Result<(), String> {
            temporary_file
                .write_all(serialized.as_bytes())
                .map_err(|error| format!("Verlauf nicht schreibbar: {}", error))?;
            temporary_file
                .sync_all()
                .map_err(|error| format!("Verlauf nicht synchronisierbar: {}", error))?;
            drop(temporary_file);
            std::fs::rename(&temporary_path, &self.history_path)
                .map_err(|error| format!("Verlauf nicht atomar ersetzbar: {}", error))?;
            Ok(())
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&temporary_path);
        }

        result
    }

    /// Liest den gespeicherten Verlauf. Ohne Freischaltung wird nichts gelesen,
    /// damit sich die Datei nicht über einen Aufruf der Oberfläche auslesen
    /// lässt.
    async fn load_history(&self) -> Option<Vec<ChatMessage>> {
        if !self.get_config().await.chat.save_history {
            return None;
        }

        let contents = std::fs::read_to_string(&self.history_path).ok()?;
        let messages: Vec<ChatMessage> = serde_json::from_str(&contents).ok()?;
        let references: Vec<&ChatMessage> = messages.iter().collect();
        validate_history(&references).ok()?;
        Some(messages)
    }

    fn delete_history(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.history_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Verlauf nicht löschbar: {}", error)),
        }
    }

    async fn get_config(&self) -> OllamaConfig {
        self.config.read().await.clone()
    }

    /// Die Adresse, unter der der eingestellte Provider antwortet.
    ///
    /// Jede Anfrage im Backend läuft hierüber: `/api/tags` für die Modellliste,
    /// `/api/chat` für Antworten und alle Prüfungen dazwischen. Der eingetragene
    /// `server_url` bleibt dabei unangetastet, damit das Zurückwechseln auf das
    /// entfernte Ollama ohne erneutes Abtippen funktioniert.
    async fn get_base_url(&self) -> String {
        let config = self.get_config().await;
        config.provider.adresse(&config.server_url).to_string()
    }

    async fn get_provider(&self) -> Provider {
        self.get_config().await.provider
    }

    /// Stellt zwischen entfernem und lokalem Ollama um.
    ///
    /// Geprüft wird nichts: Das lokale Ollama kann laufen oder nicht, und das ist
    /// eine Frage an den Server, nicht an die Konfiguration. Ein Wechsel, der
    /// scheitert, wäre hier nur eine Speicherung mit Umweg – die Oberfläche zeigt
    /// den Zustand ohnehin gleich an, wenn sie die Modelle lädt.
    async fn set_provider(&self, provider: Provider) -> Result<Provider, String> {
        let mut config = self.config.write().await;
        let mut candidate = config.clone();
        candidate.provider = provider;
        self.persist_config(&candidate)?;
        config.provider = provider;
        Ok(provider)
    }

    async fn set_base_url(&self, server_url: &str) -> Result<String, String> {
        let mut config = self.config.write().await;

        // Die Adresse gehört zum entfernten Provider. Solange das lokale läuft,
        // wäre eine Änderung hier etwas, das man einträgt und danach nicht mehr
        // bemerkt – der Knopf ist dann ausgeblendet, und ein Aufruf aus dem Chat
        // heraus bekommt dieselbe Antwort.
        if !config.provider.erlaubt_dateizugriff() {
            return Err(format!(
                "Die eingetragene Adresse gilt nur für das {}. Mit /provider remote kommst du \
                 zurück; lokal benutzt Mimir immer {DEFAULT_OLLAMA_BASE_URL}.",
                Provider::Remote.bezeichnung(),
            ));
        }

        let normalized_url = normalize_server_url(server_url)?;
        let mut candidate = config.clone();
        candidate.server_url = normalized_url.clone();
        self.persist_config(&candidate)?;
        config.server_url = normalized_url.clone();
        Ok(normalized_url)
    }

    async fn set_ssh_config(
        &self,
        target: &str,
        port: u16,
        identity_file: &str,
    ) -> Result<SshConfig, String> {
        let normalized_target = normalize_ssh_target(target)?;
        let port = validate_ssh_port(port)?;
        let identity_file = normalize_ssh_identity_file(identity_file)?;
        let mut config = self.config.write().await;
        let mut candidate = config.clone();
        candidate.ssh.target = normalized_target;
        candidate.ssh.port = port;
        candidate.ssh.identity_file = identity_file;
        self.persist_config(&candidate)?;
        config.ssh = candidate.ssh.clone();
        Ok(config.ssh.clone())
    }

    async fn set_chat_config(&self, chat: ChatConfig) -> Result<ChatConfig, String> {
        let chat = validate_chat_config(chat)?;
        let mut config = self.config.write().await;
        let mut candidate = config.clone();
        candidate.chat = chat.clone();
        self.persist_config(&candidate)?;
        config.chat = candidate.chat.clone();
        Ok(chat)
    }

    /// Merkt sich Adresse und bestätigtes Zertifikat, sonst aber nichts.
    ///
    /// Bewusst ohne Benutzernamen: Beim ersten Anmelden trägt der Benutzer den
    /// Namen erst in das Anmeldefenster ein, und das Zertifikat wird geprüft,
    /// bevor dieses Fenster überhaupt abgeschickt wird. Über
    /// `set_calendar_config` zu gehen, würde hier an einem leeren
    /// Benutzernamen scheitern und die Bestätigung unmöglich machen.
    async fn set_calendar_certificate(
        &self,
        server_url: &str,
        certificate: String,
    ) -> Result<(), String> {
        // Auch hier gilt: gespeichert wird die Basis der Instanz.
        let server_url = crate::calendar::CalendarConfig {
            server_url: crate::calendar::normalize_server_url(server_url)?,
            ..crate::calendar::CalendarConfig::default()
        }
        .instance_base();
        let mut config = self.config.write().await;
        let mut candidate = config.clone();
        candidate.calendar.server_url = server_url;
        candidate.calendar.server_certificate = Some(certificate);
        self.persist_config(&candidate)?;
        config.calendar = candidate.calendar.clone();
        Ok(())
    }

    async fn set_calendar_config(
        &self,
        calendar: crate::calendar::CalendarConfig,
    ) -> Result<crate::calendar::CalendarConfig, String> {
        let calendar = validate_calendar_config(calendar)?;
        let mut config = self.config.write().await;
        let mut candidate = config.clone();
        candidate.calendar = calendar.clone();
        self.persist_config(&candidate)?;
        config.calendar = candidate.calendar.clone();
        Ok(calendar)
    }

    async fn set_agent_config(&self, agent: AgentConfig) -> Result<AgentConfig, String> {
        // Der lokale Provider gibt keine Dateiwerkzeuge heraus. Das wird hier
        // abgelehnt und nicht erst in der Oberfläche: An dieser Stelle wird
        // gespeichert, und eine Prüfung, die nur beim Aufrufer steht, umgeht der
        // nächste Aufrufer.
        if agent.scope == Scope::Agent && !self.get_provider().await.erlaubt_dateizugriff() {
            return Err(format!(
                "Im Provider „{}“ gibt es nur den Terminumfang: Das Modell läuft auf diesem \
                 Rechner und bekommt dort keine Dateiwerkzeuge. Mit /provider remote kommst du \
                 zu den Dateiwerkzeugen zurück.",
                Provider::Local.bezeichnung(),
            ));
        }

        // `validate_agent_config` prüft die Struktur; zusätzlich muss das
        // Verzeichnis jetzt schon existieren, sonst wäre der Modus im Moment
        // der Freischaltung unbenutzbar.
        let agent = validate_agent_config(agent)?;

        if !agent.root.is_empty() {
            canonical_root(&agent.root)?;
        }

        let mut config = self.config.write().await;
        let mut candidate = config.clone();
        candidate.agent = agent.clone();
        self.persist_config(&candidate)?;
        config.agent = candidate.agent.clone();
        Ok(agent)
    }

    fn persist_config(&self, config: &OllamaConfig) -> Result<(), String> {
        let serialized = serde_json::to_string_pretty(config)
            .map_err(|error| format!("Ollama-Konfiguration nicht serialisierbar: {}", error))?;
        let parent = config_parent(&self.config_path);
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Konfigurationsverzeichnis nicht erstellbar: {}", error))?;

        let (temporary_path, mut temporary_file) =
            create_temporary_config_file(parent, &self.config_path)?;
        let result = (|| -> Result<(), String> {
            temporary_file
                .write_all(format!("{}\n", serialized).as_bytes())
                .map_err(|error| format!("Ollama-Konfiguration nicht speicherbar: {}", error))?;
            temporary_file.sync_all().map_err(|error| {
                format!("Ollama-Konfiguration nicht synchronisierbar: {}", error)
            })?;
            drop(temporary_file);
            std::fs::rename(&temporary_path, &self.config_path).map_err(|error| {
                format!("Ollama-Konfiguration nicht atomar ersetzbar: {}", error)
            })?;

            #[cfg(target_os = "linux")]
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| {
                    format!(
                        "Konfigurationsverzeichnis nicht synchronisierbar: {}",
                        error
                    )
                })?;

            Ok(())
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&temporary_path);
        }

        result
    }
}

fn config_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_temporary_config_file(
    parent: &Path,
    destination: &Path,
) -> Result<(PathBuf, std::fs::File), String> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Ollama-Konfiguration hat keinen gültigen Dateinamen".to_string())?;

    for _ in 0..MAX_CONFIG_TEMP_ATTEMPTS {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{file_name}.{}.{sequence}.tmp",
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }

        match options.open(&temporary_path) {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Temporäre Ollama-Konfiguration nicht erstellbar: {error}"
                ));
            }
        }
    }

    Err("Temporäre Ollama-Konfiguration konnte nicht eindeutig erstellt werden".to_string())
}

fn normalize_server_url(server_url: &str) -> Result<String, String> {
    let value = server_url.trim();

    if value.is_empty() {
        return Err("Die Server-URL darf nicht leer sein".to_string());
    }

    if value.len() > MAX_SERVER_URL_BYTES
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Die Server-URL ist ungültig".to_string());
    }

    if (value.starts_with("http:") || value.starts_with("https:"))
        && !value.starts_with("http://")
        && !value.starts_with("https://")
    {
        return Err("Ungültige Server-URL".to_string());
    }

    let candidate = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let raw_authority = candidate
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
        .unwrap_or_default();
    if raw_authority.contains('@') {
        return Err("Die Server-URL darf keine Zugangsdaten enthalten".to_string());
    }

    let url = reqwest::Url::parse(&candidate)
        .map_err(|error| format!("Ungültige Server-URL: {}", error))?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err("Die Server-URL muss http oder https verwenden".to_string());
    }

    let Some(host) = url.host_str() else {
        return Err("Die Server-URL muss einen Hostnamen enthalten".to_string());
    };

    if let Ok(address) = host.parse::<IpAddr>() {
        if !is_allowed_bind_ip(address) {
            return Err(
                "Die Server-URL muss auf eine loopback- oder private IP zeigen".to_string(),
            );
        }
    }

    let authority = url
        .as_str()
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
        .unwrap_or_default();
    if authority.contains('@') || !url.username().is_empty() || url.password().is_some() {
        return Err("Die Server-URL darf keine Zugangsdaten enthalten".to_string());
    }

    if url.port() == Some(0) {
        return Err("Der Server-Port muss zwischen 1 und 65535 liegen".to_string());
    }

    if url.query().is_some() || url.fragment().is_some() {
        return Err("Die Server-URL darf keine Query oder keinen Fragment enthalten".to_string());
    }

    let normalized = url.as_str().trim_end_matches('/').to_string();
    if normalized.len() > MAX_SERVER_URL_BYTES {
        return Err("Die Server-URL ist zu lang".to_string());
    }

    Ok(normalized)
}

fn validate_ssh_port(port: u16) -> Result<u16, String> {
    if port == 0 {
        return Err("Der SSH-Port muss zwischen 1 und 65535 liegen".to_string());
    }
    Ok(port)
}

/// Normalisiert den Pfad zu einem privaten SSH-Schlüssel. Leer ist erlaubt und
/// bedeutet "Standardpfade plus ssh-agent". Da Mimir mit `-F /dev/null` startet,
/// wird ein in `~/.ssh/config` konfiguriertes `IdentityFile` sonst ignoriert.
/// `~/` wird aufgelöst, damit der Befehl wie in der Shell geschrieben werden kann.
fn normalize_ssh_identity_file(identity_file: &str) -> Result<String, String> {
    let value = identity_file.trim();
    if value.is_empty() {
        return Ok(String::new());
    }

    if value.len() > MAX_SSH_IDENTITY_BYTES
        || value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Der Pfad zur SSH-Identitätsdatei ist ungültig".to_string());
    }

    // Platzhalter dürfen nicht durchgereicht werden, ssh würde sie selbst auflösen.
    if value.contains(['*', '?', '[', ']']) {
        return Err(
            "Der Pfad zur SSH-Identitätsdatei darf keine Platzhalter enthalten".to_string(),
        );
    }

    let expanded = match value.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) if !home.is_empty() && !home.contains('\\') && !home.contains('\0') => {
                format!("{home}/{rest}")
            }
            _ => {
                return Err("~ kann nicht aufgelöst werden, da HOME nicht verfügbar ist".to_string())
            }
        },
        None => value.to_string(),
    };

    if !expanded.starts_with('/') {
        return Err("Der Pfad zur SSH-Identitätsdatei muss absolut sein".to_string());
    }
    if expanded.split('/').any(|segment| segment == "..") {
        return Err("Der Pfad zur SSH-Identitätsdatei darf kein '..' enthalten".to_string());
    }

    Ok(expanded)
}

fn validate_ssh_host(host: &str) -> Result<(), String> {
    if host.is_empty() || host.len() > 253 {
        return Err("Das SSH-Ziel enthält ungültige Zeichen".to_string());
    }

    if host.starts_with('[') || host.ends_with(']') {
        if !(host.starts_with('[') && host.ends_with(']')) {
            return Err("Das SSH-Ziel enthält ungültige Zeichen".to_string());
        }
        let address = host
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or_default();
        if address.parse::<Ipv6Addr>().is_err() {
            return Err("Das SSH-Ziel enthält eine ungültige IPv6-Adresse".to_string());
        }
        return Ok(());
    }

    if host.contains(':') {
        return Err("Das SSH-Ziel darf keine ungeklammerten IPv6-Adressen enthalten".to_string());
    }

    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    let hostname = host.strip_suffix('.').unwrap_or(host);
    if hostname.is_empty() || hostname.len() > 253 {
        return Err("Das SSH-Ziel enthält ungültige Zeichen".to_string());
    }

    for label in hostname.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err("Das SSH-Ziel enthält ungültige Zeichen".to_string());
        }
    }

    Ok(())
}

fn normalize_ssh_target(target: &str) -> Result<String, String> {
    let value = target.trim();

    if value.is_empty() {
        return Err("Das SSH-Ziel darf nicht leer sein".to_string());
    }

    if value.len() > MAX_SSH_TARGET_BYTES
        || value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Das SSH-Ziel ist ungültig".to_string());
    }

    let mut parts = value.split('@');
    let first = parts.next();
    let second = parts.next();
    if parts.next().is_some() {
        return Err("Das SSH-Ziel ist ungültig".to_string());
    }

    let (username, host) = match second {
        Some(host) => (Some(first.unwrap_or_default()), host),
        None => (None, first.unwrap_or_default()),
    };
    if let Some(username) = username {
        if username.is_empty()
            || username.len() > 64
            || !username
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        {
            return Err("Das SSH-Ziel enthält ungültige Zeichen".to_string());
        }
    }

    validate_ssh_host(host)?;
    Ok(value.to_string())
}

fn is_private_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    first == 10 || (first == 172 && (16..=31).contains(&second)) || (first == 192 && second == 168)
}

fn mapped_ipv4(address: Ipv6Addr) -> Option<Ipv4Addr> {
    let octets = address.octets();
    if octets[..10].iter().all(|octet| *octet == 0) && octets[10] == 0xff && octets[11] == 0xff {
        Some(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ))
    } else {
        None
    }
}

fn is_allowed_bind_ipv4(address: Ipv4Addr) -> bool {
    address.is_loopback() || address.is_unspecified() || is_private_ipv4(address)
}

fn is_allowed_bind_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_allowed_bind_ipv4(address),
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || (address.octets()[0] & 0xfe) == 0xfc
                || mapped_ipv4(address).is_some_and(is_allowed_bind_ipv4)
        }
    }
}

fn validate_ssh_config(mut ssh: SshConfig) -> Result<SshConfig, String> {
    ssh.port = validate_ssh_port(ssh.port)?;
    if !ssh.target.is_empty() {
        ssh.target = normalize_ssh_target(&ssh.target)?;
    }
    ssh.identity_file = normalize_ssh_identity_file(&ssh.identity_file)?;
    Ok(ssh)
}

/// Prüft einen Verlauf, der von der Platte kommt. Dieselben Grenzen wie bei einer
/// Anfrage, damit eine manipulierte Datei nichts Unbegrenztes in den Prompt
/// bringen kann.
fn validate_history(messages: &[&ChatMessage]) -> Result<(), String> {
    if messages.len() > MAX_HISTORY_MESSAGES {
        return Err("Der gespeicherte Verlauf ist zu lang".to_string());
    }

    let mut total = 0usize;

    for message in messages {
        validate_chat_message(message)?;
        total += message.content.len();

        if total > MAX_HISTORY_FILE_BYTES {
            return Err("Der gespeicherte Verlauf ist zu groß".to_string());
        }
    }

    Ok(())
}

fn validate_chat_config(mut chat: ChatConfig) -> Result<ChatConfig, String> {
    chat.system_prompt = normalize_system_prompt(&chat.system_prompt)?;
    chat.context_tokens = validate_context_tokens(chat.context_tokens)?;
    Ok(chat)
}

/// Bringt die Kalendereinstellungen in eine benutzbare Form.
///
/// Diese Funktion läuft beim Laden der Konfiguration, also vor dem Start. Sie
/// darf deshalb nie einen Fehler nach oben geben: Ein halb fertiger Eintrag aus
/// einem abgebrochenen Anmeldevorgang – Adresse gesetzt, Benutzername leer –
/// darf die Anwendung nicht unstartbar machen. Was nicht brauchbar ist, wird
/// stillschweigend verworfen, und der Nutzer trägt es beim nächsten `/calendar`
/// neu ein. Der Benutzername wird hier ausdrücklich *nicht* verlangt: Beim
/// Zertifikatsschritt wird die Adresse gespeichert, lange bevor der Name aus
/// dem Fenster abgeschickt ist.
fn validate_calendar_config(
    mut config: crate::calendar::CalendarConfig,
) -> Result<crate::calendar::CalendarConfig, String> {
    config.server_url = crate::calendar::normalize_server_url(&config.server_url)
        .map(|url| {
            // Gespeichert wird die Basis der Instanz, nicht der kopierte
            // DAV-Pfad; Mimir hängt den Sammelpfad selbst an.
            let mut normalized = crate::calendar::CalendarConfig {
                server_url: url,
                ..config.clone()
            };
            normalized.server_url = normalized.instance_base();
            normalized.server_url
        })
        .unwrap_or_default();
    config.username = config.username.trim().to_string();

    if config.username.contains("..") || config.username.contains('/') {
        config.username.clear();
    }

    // Nicht mehr vorhandene Kalender dürfen nicht stehen bleiben, sonst schlägt
    // jeder Abruf mit einer Liste fehl, die niemand mehr korrigiert.
    config.calendars.retain(|value| {
        !value.is_empty() && !value.contains('/') && !value.chars().any(char::is_control)
    });
    config.calendars.sort();
    config.calendars.dedup();
    config.calendars.truncate(32);

    Ok(config)
}

fn validate_agent_config(mut agent: AgentConfig) -> Result<AgentConfig, String> {
    agent.max_steps = validate_agent_max_steps(agent.max_steps)?;
    if !agent.root.is_empty() {
        agent.root = normalize_agent_root(&agent.root)?;
    }
    Ok(agent)
}

fn validate_agent_max_steps(max_steps: usize) -> Result<usize, String> {
    if max_steps == 0 || max_steps > MAX_AGENT_STEPS_LIMIT {
        return Err(format!(
            "Die Schrittzahl muss zwischen 1 und {} liegen",
            MAX_AGENT_STEPS_LIMIT
        ));
    }
    Ok(max_steps)
}

/// Prüft das Arbeitsverzeichnis strukturell: absolut, ohne `..`, ohne
/// Steuerzeichen. Ob es existiert, wird bewusst erst beim Zugriff geprüft - ein
/// zwischenzeitlich gelöschtes Verzeichnis darf die Anwendung nicht unstartbar
/// machen.
fn normalize_agent_root(root: &str) -> Result<String, String> {
    let value = expand_home(root.trim())
        .ok_or_else(|| "~ kann nicht aufgelöst werden, da HOME nicht verfügbar ist".to_string())?;

    if value.is_empty() {
        return Err("Das Arbeitsverzeichnis darf nicht leer sein".to_string());
    }

    if value.len() > MAX_AGENT_ROOT_BYTES
        || value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Der Pfad zum Arbeitsverzeichnis ist ungültig".to_string());
    }

    if value.contains('\\') {
        return Err("Der Pfad zum Arbeitsverzeichnis darf keine Backslashes enthalten".to_string());
    }

    if !value.starts_with('/') {
        return Err("Der Pfad zum Arbeitsverzeichnis muss absolut sein".to_string());
    }

    if value.split('/').any(|segment| segment == "..") {
        return Err("Der Pfad zum Arbeitsverzeichnis darf kein '..' enthalten".to_string());
    }

    Ok(value)
}

fn expand_home(value: &str) -> Option<String> {
    match value.strip_prefix("~/") {
        Some(rest) => std::env::var("HOME")
            .ok()
            .filter(|home| !home.is_empty() && !home.contains('\\') && !home.contains('\0'))
            .map(|home| format!("{home}/{rest}")),
        None => Some(value.to_string()),
    }
}

/// Löst ein Verzeichnis auf und stellt sicher, dass es wirklich eines ist.
/// `canonicalize` löst Symlinks auf, damit der Vergleich nicht umgangen werden
/// kann.
fn canonical_root(root: &str) -> Result<PathBuf, String> {
    if root.is_empty() {
        return Err("Kein Arbeitsverzeichnis konfiguriert. Nutze /agent-dir <pfad>.".to_string());
    }

    let path = Path::new(root);
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Arbeitsverzeichnis nicht lesbar: {}", error))?;

    if !std::fs::metadata(&canonical)
        .map_err(|error| format!("Arbeitsverzeichnis nicht lesbar: {}", error))?
        .is_dir()
    {
        return Err("Das Arbeitsverzeichnis ist kein Verzeichnis".to_string());
    }

    Ok(canonical)
}

/// Löst einen vom Benutzer oder vom Modell genannten Pfad innerhalb des
/// Arbeitsverzeichnisses auf. Absolute Pfade, `..` und Symlinks, die aus dem
/// Arbeitsverzeichnis hinausführen, werden abgelehnt.
fn resolve_within_root(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let value = requested.trim();

    if value.is_empty() {
        return Ok(root.to_path_buf());
    }

    if value.len() > MAX_TOOL_PATH_BYTES
        || value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Der Pfad ist ungültig".to_string());
    }

    if value.contains('\\') {
        return Err("Der Pfad darf keine Backslashes enthalten".to_string());
    }

    if value.starts_with('/') {
        return Err("Es sind nur relative Pfade zum Arbeitsverzeichnis erlaubt".to_string());
    }

    if value.split('/').any(|segment| segment == "..") {
        return Err("Der Pfad darf kein '..' enthalten".to_string());
    }

    let mut candidate = root.to_path_buf();

    for segment in value.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        candidate.push(segment);
    }

    // `canonicalize` braucht einen existierenden Pfad und löst dabei alle
    // Symlinks auf. Erst danach wird geprüft, ob das Ergebnis wirklich im
    // Arbeitsverzeichnis liegt.
    let resolved = candidate
        .canonicalize()
        .map_err(|error| format!("Der Pfad wurde nicht gefunden: {}", error))?;

    if !resolved.starts_with(root) {
        return Err("Der Pfad zeigt aus dem Arbeitsverzeichnis heraus".to_string());
    }

    Ok(resolved)
}

fn relative_to_root(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rest) if rest.as_os_str().is_empty() => ".".to_string(),
        Ok(rest) => rest.to_string_lossy().into_owned(),
        Err(_) => "?".to_string(),
    }
}

fn string_argument(arguments: &serde_json::Value, name: &str) -> Result<String, String> {
    let value = arguments
        .get(name)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .ok_or_else(|| format!("Das Argument \"{}\" fehlt oder ist kein Text", name))?;

    if value.is_empty() {
        return Err(format!("Das Argument \"{}\" darf nicht leer sein", name));
    }

    if value.len() > MAX_TOOL_ARGUMENT_BYTES {
        return Err(format!("Das Argument \"{}\" ist zu lang", name));
    }

    Ok(value.to_string())
}

/// Liest ein Textargument, ohne es zu beschneiden. Für Dateiinhalte ist jedes
/// Leerzeichen significant: Ein `trim` würde Code und Daten still verändern.
/// Der Wert darf deshalb auch leer sein, etwa wenn ein Textabschnitt entfernt
/// werden soll.
fn raw_text_argument(
    arguments: &serde_json::Value,
    name: &str,
    max_bytes: usize,
) -> Result<String, String> {
    let value = arguments
        .get(name)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("Das Argument \"{}\" fehlt oder ist kein Text", name))?;

    if value.len() > max_bytes {
        return Err(format!(
            "Das Argument \"{}\" ist {} KiB groß. Erlaubt sind höchstens {} KiB.",
            name,
            value.len() / 1024,
            max_bytes / 1024
        ));
    }

    Ok(value.to_string())
}

/// Kürzt Werkzeugausgaben, damit eine einzelne Datei den Prompt nicht sprengt.
fn bounded_tool_output(mut content: String) -> (String, bool) {
    if content.len() <= MAX_TOOL_OUTPUT_BYTES {
        return (content, false);
    }

    let mut end = MAX_TOOL_OUTPUT_BYTES;

    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }

    content.truncate(end);
    content.push_str("\n[… gekürzt …]");
    (content, true)
}

/// Kürzt eine angehängte Datei auf das, was das Modell verträgt.
///
/// Der Anhang wird als **eine** Nachricht geschickt, und `validate_chat_message`
/// weist alles über `MAX_MESSAGE_BYTES` ab. Ein angehängtes Dokument darüber wäre
/// vorher mit „Angehängt" quittiert worden und hätte die Anfrage scheitern lassen –
/// ohne erkennbare Begründung. Deshalb wird hier gekürzt und die Kürzung im Text
/// benannt: Das Modell soll wissen, dass es nicht alles sieht, und der Benutzer
/// soll es im Chat lesen können.
///
/// Die Grenze folgt nicht dem Modell, sondern diesem einen Nachrichtenformat. Wer
/// ein Dokument vollständig lesen will, legt es ins Arbeitsverzeichnis: Dort liest
/// `read_file` es, und die Grenze pro Aufruf ist die großzügigere von 128 KiB – aber
/// sie wird nicht gekürzt, sondern verweigert, und ohne Absatzweise gibt es sie gar
/// nicht.
///
/// Gekürzt wird so, dass der **Kopf** mit hineinpasst. Begrenzt wird nur der
/// Dateitext; der Kopf – Name, Größe, der Hinweis auf die Kürzung – kommt in
/// `prepare_attachment` obendrauf, und ohne Abzug lief die fertige Nachricht über die
/// Grenze. Genau an dieser Stelle ist der Anhang vorher gescheitert, also muss hier
/// gerechnet werden, was am Ende wirklich dasteht.
fn kuerze_fuer_nachricht(kopf: &str, inhalt: &str) -> (String, bool) {
    if kopf.len() + inhalt.len() <= MAX_MESSAGE_BYTES {
        return (format!("{kopf}{inhalt}"), false);
    }

    const MARKE: &str = "\n\n[… ab hier gekürzt: Der Rest der Datei steht hier nicht. …]";
    let verbraucht = kopf.len() + MARKE.len();
    let mut end = MAX_MESSAGE_BYTES
        .saturating_sub(verbraucht)
        .min(inhalt.len());

    while end > 0 && !inhalt.is_char_boundary(end) {
        end -= 1;
    }

    if let Some(grenze) = inhalt[..end].rfind('\n') {
        end = grenze;
    }

    (format!("{kopf}{}{MARKE}", &inhalt[..end]), true)
}

/// Ergebnis eines lesenden Werkzeugs: der Text geht ans Modell, die Kurzfassung
/// in die Oberfläche.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct ToolOutput {
    content: String,
    summary: String,
    truncated: bool,
}

impl ToolOutput {
    fn new(content: String, summary: String) -> Self {
        let (content, truncated) = bounded_tool_output(content);
        Self {
            content,
            summary,
            truncated,
        }
    }

    /// Markiert eine Kürzung, die nicht aus der Bytegrenze stammt, etwa weil
    /// die Zahl der Fundstellen begrenzt war. Sonst wüsste die Oberfläche nicht,
    /// dass etwas fehlt.
    fn mark_truncated(mut self) -> Self {
        self.truncated = true;
        self
    }
}

/// Die drei Werkzeuge des Agentenmodus. Bewusst nur lesend: Es gibt keine
/// Funktion, die etwas anlegt, ändert, löscht oder ausführt. Neue Werkzeuge
/// gehören ausdrücklich in diese Liste, damit nichts hinzukommt, was der
/// Benutzer nicht sieht.
fn agent_tool_schemas() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_directory",
                "description": "Listet die Einträge eines Verzeichnisses unterhalb des Arbeitsverzeichnisses. Nur relative Pfade.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Verzeichnis relativ zum Arbeitsverzeichnis, \".\" für das Arbeitsverzeichnis selbst."
                        }
                    },
                    "required": []
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Liest eine Textdatei unterhalb des Arbeitsverzeichnisses vollständig ein. Nur relative Pfade.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Datei relativ zum Arbeitsverzeichnis, z. B. \"src/main.rs\"."
                        }
                    },
                    "required": ["path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Sucht eine Textstelle in Dateien unterhalb des Arbeitsverzeichnisses und liefert die Fundstellen mit Datei und Zeilennummer.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Gesuchte Textstelle, ohne Beachtung der Groß- und Kleinschreibung."
                        },
                        "path": {
                            "type": "string",
                            "description": "Optionaler Unterordner, in dem gesucht wird. Standard ist das Arbeitsverzeichnis."
                        }
                    },
                    "required": ["pattern"]
                }
            }
        }),
    ]
}

fn agent_system_prompt(root: &str) -> String {
    format!(
        "Du arbeitest im Agentenmodus von Mimir und kannst drei ausschließlich lesende \
Werkzeuge benutzen. Dein Arbeitsverzeichnis ist \"{}\". Alle Pfade, die du an die Werkzeuge \
reichst, müssen relativ dazu sein; absolute Pfade und \"..\" werden abgelehnt.

Werkzeuge:
- list_directory(path): zeigt Dateien und Ordner. Verzeichnisse enden auf \"/\".
- read_file(path): liefert den vollständigen Inhalt einer Textdatei.
- search_files(pattern, path): findet eine Textstelle und nennt Datei und Zeilennummer.

Arbeitsweise: Verschaffe dir zuerst einen Überblick mit list_directory, suche mit \
search_files nach der passenden Stelle und lies gezielt mit read_file. Rate nicht, wenn du \
die Datei noch nicht gelesen hast, und rate keine Dateinamen. Benenne im Verlauf knapp, \
welche Werkzeuge du benutzt hast und was du gefunden hast. Der Benutzer entscheidet über \
jeden Aufruf; wird ein Aufruf abgelehnt, arbeite mit dem, was du bereits hast, oder erkläre, \
was du dafür bräuchtest. Es gibt keine schreibenden Werkzeuge: Dateien kannst du nicht ändern, \
anlegen oder löschen, und Befehle kannst du nicht ausführen. Sag das klar, wenn eine Aufgabe \
das verlangt.",
        root
    )
}

fn tool_list_directory(root: &Path, arguments: &serde_json::Value) -> Result<ToolOutput, String> {
    let requested = arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let directory = resolve_within_root(root, requested)?;

    if !std::fs::metadata(&directory)
        .map_err(|error| format!("Verzeichnis nicht lesbar: {}", error))?
        .is_dir()
    {
        return Err("Der Pfad ist kein Verzeichnis".to_string());
    }

    let mut entries = Vec::new();
    let reader = std::fs::read_dir(&directory)
        .map_err(|error| format!("Verzeichnis nicht lesbar: {}", error))?;

    for entry in reader {
        let entry = match entry {
            Ok(entry) => entry,
            // Ein einzelner unlesbarer Eintrag darf die ganze Liste nicht kippen.
            Err(_) => continue,
        };

        // `file_type` folgt Symlinks nicht, ein Zielpfad wird also nicht berührt.
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };

        let name = entry.file_name().to_string_lossy().into_owned();
        let suffix = if kind.is_dir() {
            "/"
        } else if kind.is_symlink() {
            " ->"
        } else {
            ""
        };

        entries.push(format!("{name}{suffix}"));
    }

    entries.sort();

    let total = entries.len();
    entries.truncate(MAX_TOOL_DIRECTORY_ENTRIES);

    if total == 0 {
        return Ok(ToolOutput::new(
            "Das Verzeichnis ist leer.".to_string(),
            "leer".to_string(),
        ));
    }

    let mut content = entries.join("\n");

    if total > entries.len() {
        content.push_str(&format!(
            "\n[… {} weitere Einträge ausgelassen …]",
            total - entries.len()
        ));
    }

    let output = ToolOutput::new(
        content,
        format!(
            "{} Eintrag/Einträge in {}",
            total,
            relative_to_root(root, &directory)
        ),
    );

    Ok(if total > MAX_TOOL_DIRECTORY_ENTRIES {
        output.mark_truncated()
    } else {
        output
    })
}

fn tool_read_file(root: &Path, arguments: &serde_json::Value) -> Result<ToolOutput, String> {
    let requested = string_argument(arguments, "path")?;
    let file = resolve_within_root(root, &requested)?;

    let metadata =
        std::fs::metadata(&file).map_err(|error| format!("Datei nicht lesbar: {}", error))?;

    if metadata.is_dir() {
        return Err("Der Pfad ist ein Verzeichnis. Nutze list_directory.".to_string());
    }

    if !metadata.is_file() {
        return Err("Der Pfad ist keine reguläre Datei".to_string());
    }

    if metadata.len() > MAX_TOOL_FILE_BYTES {
        return Err(format!(
            "Die Datei ist {} KiB groß. Gelesen werden höchstens {} KiB.",
            metadata.len() / 1024,
            MAX_TOOL_FILE_BYTES / 1024
        ));
    }

    let bytes = std::fs::read(&file).map_err(|error| format!("Datei nicht lesbar: {}", error))?;

    let text = String::from_utf8(bytes).map_err(|_| {
        "Die Datei ist keine Textdatei (ungültiges UTF-8) und kann nicht gelesen werden".to_string()
    })?;

    let lines = text.lines().count();
    let relative = relative_to_root(root, &file);

    Ok(ToolOutput::new(
        text,
        format!("{} gelesen ({} Zeilen)", relative, lines),
    ))
}

fn tool_search_files(root: &Path, arguments: &serde_json::Value) -> Result<ToolOutput, String> {
    let pattern = string_argument(arguments, "pattern")?;

    if pattern.len() > MAX_TOOL_PATTERN_BYTES {
        return Err("Die Suchbegriffe sind zu lang".to_string());
    }

    let needle = pattern.to_lowercase();
    let requested = arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(".");
    let base = resolve_within_root(root, requested)?;

    if !std::fs::metadata(&base)
        .map_err(|error| format!("Verzeichnis nicht lesbar: {}", error))?
        .is_dir()
    {
        return Err("Der Pfad ist kein Verzeichnis".to_string());
    }

    let mut matches = Vec::new();
    let mut visited_files = 0usize;
    let mut stack = vec![(base.clone(), 0usize)];
    let mut truncated = false;

    'outer: while let Some((directory, depth)) = stack.pop() {
        let reader = match std::fs::read_dir(&directory) {
            Ok(reader) => reader,
            Err(_) => continue,
        };

        let mut children = Vec::new();

        for entry in reader.flatten() {
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => continue,
            };

            if kind.is_symlink() {
                // Symlinks werden übersprungen: Sie könnten aus dem
                // Arbeitsverzeichnis hinausführen.
                continue;
            }

            if kind.is_dir() {
                if depth < MAX_TOOL_SEARCH_DEPTH {
                    children.push(entry.path());
                }
                continue;
            }

            if !kind.is_file() {
                // Sockets, Gerätedateien und_fifo sind weder Text noch etwas, das
                // sich sinnvoll öffnen ließe: stillschweigend überspringen.
                continue;
            }

            if visited_files >= MAX_TOOL_SEARCH_FILES {
                // Ab hier wird nicht mehr gesucht. Das Ergebnis ist damit
                // unvollständig, und genau das wird dem Modell gemeldet.
                truncated = true;
                continue;
            }

            visited_files += 1;

            let path = entry.path();

            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };

            if metadata.len() > MAX_TOOL_SEARCH_FILE_BYTES {
                continue;
            }

            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };

            // Binärdateien überspringen, statt Müll in den Prompt zu geben.
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };

            let relative = relative_to_root(root, &path);

            for (index, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    let trimmed = line.trim();

                    // Die Fundstelle wird auf 200 Zeichen gekürzt: Es geht um das
                    // Auffinden der Stelle, nicht um die ganze Datei. Die Zeilen-
                    // nummer bleibt, damit die Stelle ohne erneutes Suchen
                    // wiedergefunden werden kann.
                    matches.push(format!(
                        "{}:{}: {}",
                        relative,
                        index + 1,
                        &trimmed[..trimmed.len().min(200)]
                    ));

                    if matches.len() >= MAX_TOOL_SEARCH_MATCHES {
                        truncated = true;
                        break 'outer;
                    }
                }
            }
        }

        for child in children {
            stack.push((child, depth + 1));
        }
    }

    if matches.is_empty() {
        return Ok(ToolOutput::new(
            format!("Keine Fundstelle für \"{}\".", pattern),
            "keine Fundstelle".to_string(),
        ));
    }

    let mut content = matches.join("\n");

    if truncated {
        content.push_str("\n[… weitere Fundstellen ausgelassen …]");
    }

    let output = ToolOutput::new(content, format!("{} Fundstelle/Fundstellen", matches.len()));

    Ok(if truncated {
        output.mark_truncated()
    } else {
        output
    })
}

/// Führt genau einen der drei erlaubten Lesezugriffe aus. Der Aufrufer kann den
/// Namen frei wählen, deshalb entscheidet ausschließlich dieses `match`, was
/// tatsächlich ausgeführt wird.
fn execute_read_only_tool(
    root: &str,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<ToolOutput, String> {
    let serialized = serde_json::to_string(arguments)
        .map_err(|error| format!("Werkzeugargumente nicht lesbar: {}", error))?;

    if serialized.len() > MAX_TOOL_ARGUMENT_BYTES {
        return Err("Die Werkzeugargumente sind zu groß".to_string());
    }

    let root = canonical_root(root)?;

    match name {
        "list_directory" => tool_list_directory(&root, arguments),
        "read_file" => tool_read_file(&root, arguments),
        "search_files" => tool_search_files(&root, arguments),
        other => Err(unknown_tool_message(other)),
    }
}

fn is_write_tool(name: &str) -> bool {
    matches!(
        name,
        "write_file"
            | "edit_file"
            | crate::calendar::write::EVENT_TOOL
            | crate::calendar::edit::UPDATE_TOOL
            | crate::calendar::edit::DELETE_TOOL
    )
}

/// Der Name des Werkzeugs, das Termine auflistet. Nur lesend, deshalb kein
/// Schreibwerkzeug und keine Bestätigung nötig.
const LIST_EVENTS_TOOL: &str = "list_calendar_events";

fn unknown_tool_message(name: &str) -> String {
    format!(
        "Unbekanntes Werkzeug \"{}\". Erlaubt sind list_directory, read_file und search_files.",
        name
    )
}

// ------------------------------------------------------------ Schreibwerkzeuge

/// Sitzungszustand des Schreibmodus. Bewusst nicht Teil der Konfiguration.
#[derive(Default)]
struct AgentState {
    write_enabled: std::sync::atomic::AtomicBool,
    budget: std::sync::Mutex<WriteBudget>,
    undo: std::sync::Mutex<Vec<UndoEntry>>,
    config_path: std::sync::Mutex<PathBuf>,
}

#[derive(Default)]
struct WriteBudget {
    writes: usize,
    bytes: usize,
}

/// Vorheriger Inhalt einer geänderten Datei, für „Rückgängig".
struct UndoEntry {
    path: PathBuf,
    /// `None`, wenn die Datei vorher nicht existierte.
    previous: Option<String>,
}

/// Ein geplanter Schreibvorgang. Vorschau und Ausführung benutzen dieselbe
/// Funktion, damit die angezeigte Wirkung garantiert die tatsächliche ist.
struct WritePlan {
    /// Kanonischer Pfad der Zieldatei.
    path: PathBuf,
    /// Kanonisches Elternverzeichnis, für die atomare Ablage.
    parent: PathBuf,
    /// Dateiname der letzten Komponente.
    file_name: String,
    /// Inhalt, der entstehen würde.
    next: String,
    /// Kurze Beschreibung der Änderung für die Oberfläche.
    summary: String,
}

impl AgentState {
    fn write_enabled(&self) -> bool {
        self.write_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_write_enabled(&self, enabled: bool) {
        self.write_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Zählt einen Schreibvorgang gegen das Budget des Zuges. Das Budget wird zu
    /// Beginn jedes Zugs zurückgesetzt, damit eine Modellschleife nicht
    /// unbegrenzt schreiben kann.
    fn charge_write(&self, bytes: usize) -> Result<(), String> {
        let mut budget = self
            .budget
            .lock()
            .map_err(|_| "Das Schreibbudget ist nicht verfügbar".to_string())?;

        if budget.writes + 1 > MAX_TOOL_WRITES_PER_TURN {
            return Err(format!(
                "In diesem Zug sind höchstens {} Schreibvorgänge erlaubt.",
                MAX_TOOL_WRITES_PER_TURN
            ));
        }

        if budget.bytes + bytes > MAX_TOOL_WRITE_TOTAL_BYTES {
            return Err(format!(
                "In diesem Zug sind höchstens {} KiB Schreibvorgänge erlaubt.",
                MAX_TOOL_WRITE_TOTAL_BYTES / 1024
            ));
        }

        budget.writes += 1;
        budget.bytes += bytes;
        Ok(())
    }

    fn remember_for_undo(&self, path: PathBuf, previous: Option<String>) {
        let Ok(mut entries) = self.undo.lock() else {
            return;
        };

        // Für denselben Pfad bleibt nur der jüngste Stand sinnvoll.
        entries.retain(|entry| entry.path != path);
        entries.push(UndoEntry { path, previous });

        while entries.len() > MAX_TOOL_UNDO_ENTRIES {
            entries.remove(0);
        }
    }

    fn take_undo(&self, path: &Path) -> Option<UndoEntry> {
        let mut entries = self.undo.lock().ok()?;
        let index = entries.iter().position(|entry| entry.path == path)?;
        Some(entries.remove(index))
    }

    fn is_protected(&self, path: &Path) -> bool {
        let Ok(config_path) = self.config_path.lock() else {
            return true;
        };

        if path == config_path.as_path() {
            return true;
        }

        // Alles im Konfigurationsverzeichnis und jeder .git-Ordner bleiben tabu.
        let repository = path.components().any(|part| part.as_os_str() == ".git");

        repository || config_path.parent() == path.parent()
    }
}

/// Löst das Ziel eines Schreibvorgangs auf. Anders als beim Lesen muss der Pfad
/// nicht existieren, deshalb wird das Elternverzeichnis kanonisiert und der
/// Dateiname angehängt. Damit kann kein `..` aus dem Arbeitsverzeichnis
/// herausführen, und der Dateiname selbst kann kein Sprung nach außen sein.
fn resolve_write_target(
    root: &Path,
    requested: &str,
) -> Result<(PathBuf, PathBuf, String), String> {
    let value = requested.trim();

    if value.is_empty() {
        return Err("Der Pfad darf nicht leer sein".to_string());
    }

    if value.len() > MAX_TOOL_PATH_BYTES
        || value.starts_with('-')
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("Der Pfad ist ungültig".to_string());
    }

    if value.contains('\\') {
        return Err("Der Pfad darf keine Backslashes enthalten".to_string());
    }

    if value.starts_with('/') {
        return Err("Es sind nur relative Pfade zum Arbeitsverzeichnis erlaubt".to_string());
    }

    let segments: Vec<&str> = value
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();

    if segments.is_empty() {
        return Err("Der Pfad zeigt auf kein Verzeichnis".to_string());
    }

    if segments.contains(&"..") {
        return Err("Der Pfad darf kein '..' enthalten".to_string());
    }

    let file_name = segments
        .last()
        .ok_or_else(|| "Der Pfad zeigt auf kein Verzeichnis".to_string())?
        .to_string();

    let parent_value = segments[..segments.len() - 1].join("/");
    let parent_requested = if parent_value.is_empty() {
        "."
    } else {
        &parent_value
    };
    let parent = resolve_within_root(root, parent_requested)?;

    if !std::fs::metadata(&parent)
        .map_err(|error| format!("Verzeichnis nicht lesbar: {}", error))?
        .is_dir()
    {
        return Err("Der Pfad ist kein Verzeichnis".to_string());
    }

    Ok((parent.join(&file_name), parent, file_name))
}

/// Liest den aktuellen Inhalt einer Zieldatei. `None`, wenn es sie nicht gibt.
fn read_current_content(path: &Path) -> Result<Option<String>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Datei nicht lesbar: {}", error)),
    };

    // Ein Symlink als Ziel wird nie geschrieben: Damit ließe sich ein Link
    // ausserhalb des Arbeitsverzeichnisses als Ziel angeben.
    if metadata.file_type().is_symlink() {
        return Err("Das Ziel ist ein Symlink und wird nicht überschrieben".to_string());
    }

    if !metadata.is_file() {
        return Err("Das Ziel ist keine reguläre Datei".to_string());
    }

    if metadata.len() > MAX_TOOL_FILE_BYTES {
        return Err(format!(
            "Die Datei ist {} KiB groß. Bearbeitet werden höchstens {} KiB.",
            metadata.len() / 1024,
            MAX_TOOL_FILE_BYTES / 1024
        ));
    }

    let bytes = std::fs::read(path).map_err(|error| format!("Datei nicht lesbar: {}", error))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "Die Datei ist keine Textdatei (ungültiges UTF-8)".to_string())?;

    Ok(Some(text))
}

fn bounded_preview(content: &str) -> (String, bool) {
    if content.len() <= MAX_TOOL_PREVIEW_BYTES {
        return (content.to_string(), false);
    }

    let mut end = MAX_TOOL_PREVIEW_BYTES;

    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }

    (format!("{}\n[… Vorschau gekürzt …]", &content[..end]), true)
}

/// Ermittelt, was ein Schreibaufruf ergeben würde, ohne etwas zu verändern.
/// Wird sowohl für die Diff-Vorschau als auch für die Ausführung benutzt.
fn plan_write(root: &Path, name: &str, arguments: &serde_json::Value) -> Result<WritePlan, String> {
    let requested = string_argument(arguments, "path")?;
    let (path, parent, file_name) = resolve_write_target(root, &requested)?;
    let current = read_current_content(&path)?;

    match name {
        "write_file" => {
            let content = raw_text_argument(arguments, "content", MAX_TOOL_WRITE_BYTES)?;

            // Bewusst kein Überschreiben: Wer eine bestehende Datei ändern will,
            // nennt die Stelle, die ersetzt werden soll.
            if current.is_some() {
                return Err(format!(
                    "{file_name} existiert bereits. Nutze edit_file, um eine Stelle gezielt zu ersetzen."
                ));
            }

            let added = content.lines().count();

            Ok(WritePlan {
                path,
                parent,
                file_name: file_name.clone(),
                next: content,
                summary: format!("neue Datei {file_name} mit {added} Zeilen"),
            })
        }
        "edit_file" => {
            // Beide Texte werden unbeschritten gelesen: Ein `trim` würde
            // Einrückung oder einen abschließenden Zeilenumbruch still entfernen.
            let old = raw_text_argument(arguments, "old_string", MAX_TOOL_WRITE_OLD_BYTES)?;
            let new = raw_text_argument(arguments, "new_string", MAX_TOOL_WRITE_BYTES)?;

            let Some(current) = current else {
                return Err(format!("{file_name} existiert nicht. Nutze write_file."));
            };

            let occurrences = current.matches(old.as_str()).count();

            if occurrences == 0 {
                return Err(format!(
                    "Der zu ersetzende Text wurde in {file_name} nicht gefunden. Nimm den exakten Wortlaut aus der gelesenen Datei."
                ));
            }

            if occurrences > 1 {
                return Err(format!(
                    "Der zu ersetzende Text kommt in {file_name} {} mal vor. Er muss eindeutig sein.",
                    occurrences
                ));
            }

            let next = current.replacen(old.as_str(), new.as_str(), 1);

            Ok(WritePlan {
                path,
                parent,
                file_name: file_name.clone(),
                next,
                summary: format!("{file_name}: eine Stelle ersetzt"),
            })
        }
        other => Err(unknown_tool_message(other)),
    }
}

/// Schreibt den Inhalt atomar: erst in eine neue Datei im selben Verzeichnis,
/// dann `rename`. Ein Absturz mitten im Schreiben lässt so keine halbe Datei
/// zurück. Das Muster entspricht dem der Konfigurationsdatei.
fn write_atomically(plan: &WritePlan) -> Result<(), String> {
    let mode = std::fs::symlink_metadata(&plan.path)
        .ok()
        .map(|metadata| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o7777
            }
            #[cfg(not(unix))]
            {
                let _ = metadata;
                0o644
            }
        })
        .unwrap_or(0o644);

    let temporary = plan.parent.join(format!(
        ".mimir-write-{}-{}.tmp",
        std::process::id(),
        TEMP_FILE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));

    let result = (|| -> Result<(), String> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // O_NOFOLLOW: Selbst wenn zwischen Prüfung und Schreiben jemand ein
            // Symlink anlegt, wird diesem nicht gefolgt. Der Zahlenwert ist
            // O_NOFOLLOW unter Linux; std::fs::bietet ihn nicht portabel an,
            // deshalb steht er hier ausgeschrieben.
            options.custom_flags(0o400000).mode(mode);
        }
        #[cfg(not(unix))]
        {
            options.mode(0o644);
        }

        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("Temporäre Datei nicht anlegbar: {}", error))?;
        file.write_all(plan.next.as_bytes())
            .map_err(|error| format!("Datei nicht schreibbar: {}", error))?;
        file.sync_all()
            .map_err(|error| format!("Datei nicht synchronisierbar: {}", error))?;
        drop(file);

        std::fs::rename(&temporary, &plan.path)
            .map_err(|error| format!("Datei nicht atomar ersetzbar: {}", error))?;

        #[cfg(target_os = "linux")]
        std::fs::File::open(&plan.parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("Verzeichnis nicht synchronisierbar: {}", error))?;

        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }

    result
}

fn execute_write_tool(
    state: &AgentState,
    root: &Path,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<ToolOutput, String> {
    if !state.write_enabled() {
        return Err(
            "Schreibende Werkzeuge sind nicht freigeschaltet. Der Benutzer muss sie im Chat freigeben."
                .to_string(),
        );
    }

    let serialized = serde_json::to_string(arguments)
        .map_err(|error| format!("Werkzeugargumente nicht lesbar: {}", error))?;

    if serialized.len() > MAX_TOOL_WRITE_ARGUMENT_BYTES {
        return Err(format!(
            "Die Werkzeugargumente sind {} KiB groß. Erlaubt sind höchstens {} KiB.",
            serialized.len() / 1024,
            MAX_TOOL_WRITE_ARGUMENT_BYTES / 1024
        ));
    }

    // Der Schutzcheck läuft vor der Planung: Eine gesperrte Datei soll auch
    // dann abgelehnt werden, wenn der Schreibaufruf inhaltlich noch gar nicht
    // ausführbar wäre.
    let requested = string_argument(arguments, "path")?;
    let (target, _parent, _file_name) = resolve_write_target(root, &requested)?;

    if state.is_protected(&target) {
        return Err(format!(
            "{} ist für den Agentenmodus gesperrt.",
            relative_to_root(root, &target)
        ));
    }

    let plan = plan_write(root, name, arguments)?;
    let previous = read_current_content(&plan.path)?;
    state.charge_write(plan.next.len())?;
    write_atomically(&plan)?;
    state.remember_for_undo(plan.path.clone(), previous);

    let mut content = String::new();
    content.push_str(&plan.file_name);
    content.push_str(" wurde geschrieben. Kehre zum Lesen zurück, um den Erfolg zu prüfen.");
    content.push_str(&format!("\n\n{}", plan.next));

    Ok(ToolOutput::new(content, plan.summary.clone()))
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    /// Dauerhafte Anweisungen an das Modell. Ollama dafür hat ein eigenes Feld,
    /// damit sie nicht als Nachricht im Verlauf landen.
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    /// Werkzeug-Schemata. Nur im Agentenmodus gesetzt; im Chat bleibt das Feld
    /// weg, damit Ollama den Weg unverändert sieht.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    /// Laufzeitoptionen. Nur gesetzt, wenn der Benutzer die Kontextgröße
    /// ausdrücklich gewählt hat.
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_ctx: usize,
}

#[derive(Clone, Serialize)]
struct ChatRetryNotice {
    attempt: usize,
    max_attempts: usize,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Deserialize)]
struct OllamaModelsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Serialize)]
struct ServerStatus {
    online: bool,
    server_url: String,
    /// Wann zuletzt eine Antwort ankam. Die Anzeige nutzt das, um eine
    /// Funkstelle von einem ausgefallenen Server zu unterscheiden.
    last_contact: Option<i64>,
}

#[derive(Serialize)]
#[serde(tag = "status", content = "message", rename_all = "snake_case")]
enum SshStartResult {
    Started,
    PasswordRequired(String),
}

#[derive(Deserialize, Debug, Default)]
struct OllamaResponseChunk {
    #[serde(default)]
    message: Option<ChatMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Reqwest aktiviert unter Linux standardmäßig `tcp_user_timeout = 30s` sowie
/// `tcp_keepalive = 15s` mit drei Wiederholungen. Beides lässt den Kernel die
/// Verbindung nach rund einer Minute mit ETIMEDOUT abbrechen, sobald Ollama
/// während einer Denkphase keine Daten sendet - also deutlich vor unseren
/// eigenen Warte- und Health-Check-Regeln. Für den Chat deaktivieren wir das
/// User-Timeout und setzen die Keepalive-Werte bewusst großzügiger.
fn ollama_client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        .tcp_user_timeout(None)
        .tcp_keepalive(Some(OLLAMA_TCP_KEEPALIVE))
        .tcp_keepalive_interval(Some(OLLAMA_TCP_KEEPALIVE_INTERVAL))
        .tcp_keepalive_retries(Some(OLLAMA_TCP_KEEPALIVE_RETRIES))
}

pub fn build_ollama_chat_client() -> Result<Client, String> {
    ollama_client_builder()
        .http1_only()
        .connect_timeout(OLLAMA_CHAT_CONNECT_TIMEOUT)
        .pool_idle_timeout(OLLAMA_POOL_IDLE_TIMEOUT)
        .pool_max_idle_per_host(OLLAMA_POOL_MAX_IDLE)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .map_err(|error| format!("Client-Fehler: {}", error))
}

/// Der eine Client für alles, was Ollama betrifft.
///
/// Vorher wurde für jede Anfrage ein neuer Client gebaut und damit jedes Mal
/// ein neuer Verbindungsaufbau. Auf einer schwankenden Verbindung ist genau das
/// der wacklige Teil: Der Handshake kann hängen, während eine bereits
/// bestehende Verbindung sofort nutzbar gewesen wäre. Ein gemeinsamer Client
/// hält die Verbindung warm, und die kleinen Prüfungen teilen sich den
/// Established-Request mit dem Chat.
///
/// Die Zeitgrenzen sitzen deshalb nicht mehr im Client, sondern an der einzelnen
/// Anfrage: Ein Chat darf sich Zeit nehmen, eine Statusanzeige nicht.
fn ollama_client() -> Result<&'static Client, String> {
    static CLIENT: std::sync::OnceLock<Result<Client, String>> = std::sync::OnceLock::new();

    CLIENT
        .get_or_init(build_ollama_chat_client)
        .as_ref()
        .map_err(|fehler| fehler.clone())
}

pub(crate) fn format_reqwest_error(error: &reqwest::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

fn validate_model_name(model: &str) -> Result<(), String> {
    if model.trim().is_empty()
        || model.len() > MAX_MODEL_NAME_BYTES
        || model.chars().any(char::is_control)
    {
        return Err("Ollama hat einen ungültigen Modellnamen zurückgegeben".to_string());
    }
    Ok(())
}

fn validate_chat_message(message: &ChatMessage) -> Result<(), String> {
    if !matches!(
        message.role.as_str(),
        "system" | "user" | "assistant" | "tool"
    ) || message.content.len() > MAX_MESSAGE_BYTES
        || message.thinking.len() > MAX_THINKING_BYTES
    {
        return Err("Die Chatnachricht ist ungültig oder zu groß".to_string());
    }
    Ok(())
}

/// Formuliert einen Verbindungsfehler so, dass daraus etwas folgt. Bisher kam
/// der englische Rohtext von `reqwest` durch, der zwar korrekt, aber ohne
/// Handlungshinweis war. Die technische Angabe bleibt als zweite Zeile erhalten,
/// weil sie bei der Fehlersuche hilft.
///
/// `beim_verbinden` unterscheidet die beiden Fälle, die reqwest gleich benennt
/// und die ganz verschiedene Ursachen haben: ein vergeblicher Verbindungsaufbau
/// und ein Server, der die Anfrage angenommen, aber nicht beantwortet hat.
pub fn connection_error_message(
    base_url: &str,
    details: &str,
    beim_verbinden: bool,
    versuche: usize,
) -> String {
    let lower = details.to_ascii_lowercase();
    // Nur nennen, wenn wirklich mehrfach versucht wurde; sonst klingt jede
    // einfache Abfrage nach einer Fehlersuche.
    let wiederholt = if versuche > 1 {
        format!(" Mimir hat es {versuche} Mal versucht.")
    } else {
        String::new()
    };

    let hint = if lower.contains("connection refused") || lower.contains("os error 111") {
        format!(
            "Unter {base_url} läuft kein Dienst. Läuft der Ollama-Server dort? \
Mit /server-start startet Mimir ihn über SSH."
        )
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        format!("Der Rechner mit {base_url} ist im Netz nicht erreichbar.")
    } else if lower.contains("timed out") || lower.contains("deadline") || lower.contains("timeout")
    {
        if beim_verbinden {
            // Genau dieser Fall trat auf: Der Rechner antwortete, der Dienst
            // lief, und doch lief der Verbindungsaufbau in eine Zeitgrenze. Die
            // alte Meldung sprach von Firewall und ausgefallenem Server und
            // traf damit ins Leere. Was sicher bekannt ist, sagt dieser Text:
            // Die Verbindung kam nicht zustande. Der Rest sind Ursachen, die man
            // der Reihe nach prüfen kann.
            format!(
                "Die Verbindung zu {base_url} kam beim Aufbau nicht zustande: Auf Port {} hat \
niemand geantwortet. Möglich sind ein Funkloch oder eine stark belastete Verbindung, \
eine Firewall, die den Port verwirft, oder ein Ollama, das nur auf dem Rechner selbst \
lauscht und dann mit OLLAMA_HOST=\"0.0.0.0\" gestartet werden muss.{wiederholt} \
/server-status zeigt, ob der Server gerade antwortet, /server-start startet ihn über SSH \
neu.",
                port_of(base_url)
            )
        } else {
            format!(
                "{base_url} antwortet nicht. Der Server hat die Anfrage bekommen, aber nicht \
geantwortet. Prüfe /server-status, und starte den Server mit /server-start."
            )
        }
    } else {
        format!("Die Verbindung zu {base_url} ist fehlgeschlagen.")
    };

    format!("{hint}\nTechnische Angabe: {details}")
}

/// Nimmt den Port aus einer Adresse wie `http://ollama.example.org:11434/api/chat`.
fn port_of(url: &str) -> String {
    let ohne_schema = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = ohne_schema.split('/').next().unwrap_or(ohne_schema);

    match host.rsplit_once(':') {
        Some((_, port)) if port.chars().all(|character| character.is_ascii_digit()) => {
            port.to_string()
        }
        _ => "der angegebenen".to_string(),
    }
}

/// Liest den Fehlertext aus einer Fehlerantwort. Ollama schreibt dorthin die
/// eigentliche Begründung, etwa dass die Eingabe länger ist als das
/// Kontextfenster. Ohne diesen Text bleibt nur "Status 400", was nicht weiterhilft.
async fn describe_http_error(response: reqwest::Response) -> String {
    let status = response.status();
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let Ok(chunk) = item else { break };

        if body.len().saturating_add(chunk.len()) > MAX_ERROR_BODY_BYTES {
            // Gekürzt lesen: Die Erklärung steht am Anfang.
            body.truncate(MAX_ERROR_BODY_BYTES);
            break;
        }

        body.extend_from_slice(&chunk);
    }

    let detail = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());

    if detail.is_empty() {
        format!("Ollama-Fehler: Status {status}")
    } else {
        format!("Ollama-Fehler: {status} - {detail}")
    }
}

pub fn validate_chat_input(model: &str, messages: &[ChatMessage]) -> Result<(), String> {
    validate_model_name(model)?;
    if messages.is_empty() || messages.len() > MAX_HISTORY_MESSAGES {
        return Err("Der Chatverlauf ist leer oder zu lang".to_string());
    }

    let mut total_bytes = model.len();
    for message in messages {
        validate_chat_message(message)?;
        total_bytes = total_bytes
            .checked_add(message.role.len())
            .and_then(|value| value.checked_add(message.content.len()))
            .ok_or_else(|| "Der Chatverlauf ist zu groß".to_string())?;
        if total_bytes > MAX_PROMPT_BYTES {
            return Err("Der Chatverlauf ist zu groß".to_string());
        }
    }

    Ok(())
}

fn truncate_string(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }

    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

async fn read_limited_response(
    response: reqwest::Response,
    max_bytes: usize,
    description: &str,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!("{description} ist zu groß"));
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk =
            item.map_err(|error| format!("Stream-Fehler: {}", format_reqwest_error(&error)))?;
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("{description} ist zu groß"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_stream_line(
    line: &[u8],
    line_count: &mut usize,
) -> Result<Option<OllamaResponseChunk>, String> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }

    let text = std::str::from_utf8(line)
        .map_err(|_| "Ollama-Stream enthält ungültiges UTF-8".to_string())?;
    if text.trim() == "[DONE]" {
        return Ok(Some(OllamaResponseChunk {
            done: true,
            ..OllamaResponseChunk::default()
        }));
    }

    let parsed = serde_json::from_str::<OllamaResponseChunk>(text)
        .map_err(|error| format!("Ungültige Ollama-Streamantwort: {}", error))?;
    if let Some(error) = parsed.error.as_deref() {
        return Err(if error.is_empty() {
            "Ollama-Fehler".to_string()
        } else {
            format!("Ollama-Fehler: {}", error)
        });
    }
    if let Some(message) = parsed.message.as_ref() {
        validate_chat_message(message)?;

        if message.tool_calls.len() > MAX_TOOL_CALLS_PER_MESSAGE {
            return Err(
                "Das Modell hat zu viele Werkzeugaufrufe auf einmal angefordert".to_string(),
            );
        }

        for call in &message.tool_calls {
            call.function.validated()?;
        }
    }

    *line_count += 1;
    if *line_count > MAX_STREAM_LINES {
        return Err("Ollama-Stream enthält zu viele Antwortzeilen".to_string());
    }

    Ok(Some(parsed))
}

struct OllamaStreamParser {
    buffer: Vec<u8>,
    total_bytes: usize,
    line_count: usize,
    done: bool,
}

impl OllamaStreamParser {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            total_bytes: 0,
            line_count: 0,
            done: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<Vec<OllamaResponseChunk>, String> {
        if self.done {
            return Ok(Vec::new());
        }

        self.total_bytes = self
            .total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| "Ollama-Stream ist zu groß".to_string())?;
        if self.total_bytes > MAX_STREAMED_RESPONSE_BYTES {
            return Err("Ollama-Stream ist zu groß".to_string());
        }
        let mut input = std::mem::take(&mut self.buffer);
        input.extend_from_slice(bytes);
        let mut parsed_chunks = Vec::new();
        let mut remainder = Vec::new();
        let mut line_start = 0;

        for (index, byte) in input.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            // Erste Grenze: Eine Zeile, die im aktuellen Stück vollständig
            // vorliegt, darf nicht länger sein als erlaubt. Geprüft wird hier
            // die Länge ohne den Zeilenumbruch selbst.
            if index - line_start > MAX_STREAM_LINE_BYTES {
                return Err("Ollama-Stream enthält eine zu lange Zeile".to_string());
            }

            let mut line = input[line_start..=index].to_vec();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(parsed) = parse_stream_line(&line, &mut self.line_count)? {
                let done = parsed.done;
                parsed_chunks.push(parsed);
                if done {
                    self.done = true;
                    self.buffer = remainder;
                    return Ok(parsed_chunks);
                }
            }
            line_start = index + 1;
        }

        if line_start < input.len() {
            remainder.extend_from_slice(&input[line_start..]);
        }
        self.buffer = remainder;
        // Zweite Grenze, für den Rest ohne Zeilenumbruch: Eine Zeile, die noch
        // nicht fertig angekommen ist, kann die erste Prüfung nicht sehen, und
        // genau darüber liefe der Speicher unbegrenzt auf.
        if !self.done && self.buffer.len() > MAX_STREAM_LINE_BYTES {
            return Err("Ollama-Stream enthält eine zu lange Zeile".to_string());
        }

        Ok(parsed_chunks)
    }

    fn finish(&mut self) -> Result<Vec<OllamaResponseChunk>, String> {
        if self.done || self.buffer.is_empty() {
            return Ok(Vec::new());
        }

        let mut line = std::mem::take(&mut self.buffer);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let parsed = parse_stream_line(&line, &mut self.line_count)?;
        let Some(parsed) = parsed else {
            return Ok(Vec::new());
        };
        self.done = parsed.done;
        Ok(vec![parsed])
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

/// Meldung, wenn die Antwortköpfe nicht rechtzeitig kommen. Der konfigurierte
/// Wert steht in der Meldung, damit sie nicht veralten kann.
fn header_timeout_message() -> String {
    format!(
        "Ollama hat nicht innerhalb von {} Sekunden auf die Anfrage geantwortet. Bei einem langen Prompt oder ausgelastetem Rechner kann das dauern.",
        OLLAMA_HEADER_TIMEOUT.as_secs()
    )
}

fn no_token_timeout_error() -> String {
    format!(
        "Ollama hat seit {} Minuten keinen Token gesendet",
        OLLAMA_NO_TOKEN_LIMIT.as_secs() / 60
    )
}

struct ChatWaitState {
    last_token_at: Instant,
    window_started_at: Instant,
    health_failures: usize,
}

impl ChatWaitState {
    fn new(now: Instant) -> Self {
        Self {
            last_token_at: now,
            window_started_at: now,
            health_failures: 0,
        }
    }

    fn on_token(&mut self, now: Instant) {
        self.last_token_at = now;
        self.window_started_at = now;
        self.health_failures = 0;
    }

    fn on_health_probe(&mut self, online: bool, now: Instant) -> bool {
        if online {
            self.health_failures = 0;
        } else {
            self.health_failures += 1;
        }
        self.window_started_at = now;
        self.health_failures < OLLAMA_HEALTH_FAILURE_LIMIT
    }

    fn remaining(&self, now: Instant) -> Option<(Duration, Duration)> {
        let no_token_elapsed = now.saturating_duration_since(self.last_token_at);
        if no_token_elapsed >= OLLAMA_NO_TOKEN_LIMIT {
            return None;
        }
        let idle_elapsed = now.saturating_duration_since(self.window_started_at);
        Some((
            OLLAMA_TOKEN_IDLE_TIMEOUT.saturating_sub(idle_elapsed),
            OLLAMA_NO_TOKEN_LIMIT.saturating_sub(no_token_elapsed),
        ))
    }

    fn silent_for(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_token_at)
    }
}

/// Prüft mehrfach, ob der Server erreichbar ist. Das WLAN bricht gelegentlich für
/// einige Sekunden komplett weg; ein einzelner Fehlversuch soll deshalb keinen
/// automatischen Neuversand verhindern.
async fn wait_for_server(base_url: &str) -> bool {
    for attempt in 0..OLLAMA_RETRY_HEALTH_ATTEMPTS {
        if is_ollama_online_within(base_url, OLLAMA_RETRY_PROBE_TIMEOUT).await {
            return true;
        }
        if attempt + 1 < OLLAMA_RETRY_HEALTH_ATTEMPTS {
            tokio::time::sleep(OLLAMA_RETRY_HEALTH_DELAY).await;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamFailureKind {
    Transport,
    Cancelled,
    Fatal,
}

struct StreamFailure {
    kind: StreamFailureKind,
    message: String,
    received_token: bool,
}

impl StreamFailure {
    fn new(kind: StreamFailureKind, message: String, received_token: bool) -> Self {
        Self {
            kind,
            message,
            received_token,
        }
    }

    fn transport(message: String, received_token: bool) -> Self {
        Self::new(StreamFailureKind::Transport, message, received_token)
    }

    fn cancelled() -> Self {
        Self::new(
            StreamFailureKind::Cancelled,
            "Chat abgebrochen".to_string(),
            true,
        )
    }

    fn fatal(message: String, received_token: bool) -> Self {
        Self::new(StreamFailureKind::Fatal, message, received_token)
    }
}

fn should_retry_chat(failure: &StreamFailure, attempt: usize) -> bool {
    failure.kind == StreamFailureKind::Transport
        && !failure.received_token
        && attempt < MAX_CHAT_ATTEMPTS
}

async fn stream_chat_response(
    response: reqwest::Response,
    on_chunk: &Channel<StreamChunk>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    base_url: &str,
    settings: &OllamaSettings,
) -> Result<(), StreamFailure> {
    let mut stream = response.bytes_stream();
    let mut parser = OllamaStreamParser::new();
    let mut chunk_count = 0usize;
    let mut wait_state = ChatWaitState::new(Instant::now());
    let mut received_token = false;
    let mut stall_failures = 0usize;
    let mut stall_interval = tokio::time::interval(OLLAMA_STALL_PROBE_INTERVAL);
    stall_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    stall_interval.tick().await;

    loop {
        let Some((idle_remaining, no_token_remaining)) = wait_state.remaining(Instant::now())
        else {
            return Err(StreamFailure::fatal(
                no_token_timeout_error(),
                received_token,
            ));
        };

        let item = tokio::select! {
            item = tokio::time::timeout(idle_remaining, stream.next()) => match item {
                Ok(item) => item,
                Err(_) => {
                    let online = is_ollama_online_within(base_url, OLLAMA_STALL_PROBE_TIMEOUT).await;
                    if !wait_state.on_health_probe(online, Instant::now()) {
                        return Err(StreamFailure::transport(
                            "Ollama ist während der Antwort nicht mehr erreichbar".to_string(),
                            received_token,
                        ));
                    }
                    continue;
                }
            },
            _ = tokio::time::sleep(no_token_remaining) => {
                return Err(StreamFailure::fatal(no_token_timeout_error(), received_token));
            }
            _ = stall_interval.tick() => {
                // Solange Tokens fließen, ist die Verbindung nachweislich intakt.
                // Erst bei Funkstille prüfen wir zusätzlich, ob der Server noch da
                // ist, damit ein Netzwerkausfall schnell in einen Neuversand mündet.
                if wait_state.silent_for(Instant::now()) >= OLLAMA_STALL_PROBE_MIN_SILENCE {
                    if is_ollama_online_within(base_url, OLLAMA_STALL_PROBE_TIMEOUT).await {
                        stall_failures = 0;
                    } else {
                        stall_failures += 1;
                        if stall_failures >= OLLAMA_STALL_FAILURE_LIMIT {
                            return Err(StreamFailure::transport(
                                "Die Verbindung zu Ollama wurde unterbrochen".to_string(),
                                received_token,
                            ));
                        }
                    }
                }
                continue;
            }
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Err(StreamFailure::cancelled());
                }
                continue;
            }
        };
        let Some(item) = item else {
            break;
        };
        chunk_count = chunk_count.checked_add(1).ok_or_else(|| {
            StreamFailure::fatal(
                "Ollama-Stream enthält zu viele Datenblöcke".to_string(),
                received_token,
            )
        })?;
        if chunk_count > MAX_STREAM_LINES {
            return Err(StreamFailure::fatal(
                "Ollama-Stream enthält zu viele Datenblöcke".to_string(),
                received_token,
            ));
        }
        let chunk = item.map_err(|error| {
            StreamFailure::transport(
                format!("Stream-Fehler: {}", format_reqwest_error(&error)),
                received_token,
            )
        })?;
        let parsed_chunks = parser
            .push(&chunk)
            .map_err(|error| StreamFailure::fatal(error, received_token))?;
        // Denktext zählt genauso wie Antworttext als Fortschritt: Solange das
        // Modell Tokens liefert, ist die Verbindung intakt.
        let chunk_had_token = parsed_chunks
            .iter()
            .any(|parsed| parsed.message.as_ref().is_some_and(|m| m.has_text()));
        for parsed in parsed_chunks {
            if let Some(message) = parsed.message {
                let piece = StreamChunk {
                    content: message.content,
                    thinking: message.thinking,
                    tool_calls: message.tool_calls,
                };
                if !piece.is_empty() {
                    on_chunk.send(piece).map_err(|error| {
                        StreamFailure::fatal(
                            format!("Stream-Kanal-Fehler: {}", error),
                            received_token,
                        )
                    })?;
                }
            }
            if parsed.done {
                return Ok(());
            }
        }
        if chunk_had_token {
            received_token = true;
            wait_state.on_token(Instant::now());
        }
    }

    let trailing = parser
        .finish()
        .map_err(|error| StreamFailure::transport(error, received_token))?;
    for parsed in trailing {
        if let Some(message) = parsed.message {
            let piece = StreamChunk {
                content: message.content,
                thinking: message.thinking,
                tool_calls: message.tool_calls,
            };
            if !piece.is_empty() {
                on_chunk.send(piece).map_err(|error| {
                    StreamFailure::fatal(format!("Stream-Kanal-Fehler: {}", error), received_token)
                })?;
                received_token = true;
            }
        }
    }

    if received_token {
        // Es ist etwas angekommen: Der Server lebt, auch wenn die Verbindung
        // danach abreißt.
        settings.mark_contact();
    }

    if parser.is_done() {
        Ok(())
    } else {
        Err(StreamFailure::transport(
            "Ollama-Stream endete ohne Abschluss".to_string(),
            received_token,
        ))
    }
}

async fn send_chat_attempt(
    client: &Client,
    base_url: &str,
    payload: &OllamaRequest,
    on_chunk: &Channel<StreamChunk>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    versuche: usize,
    settings: &OllamaSettings,
) -> Result<(), StreamFailure> {
    let response = tokio::time::timeout(
        OLLAMA_HEADER_TIMEOUT,
        client
            .post(format!("{base_url}/api/chat"))
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .json(payload)
            .send(),
    )
    .await
    .map_err(|_| StreamFailure::transport(header_timeout_message(), false))?
    .map_err(|error| {
        StreamFailure::transport(
            connection_error_message(
                &format!("{base_url}/api/chat"),
                &format_reqwest_error(&error),
                error.is_connect(),
                versuche,
            ),
            false,
        )
    })?;

    if !response.status().is_success() {
        return Err(StreamFailure::fatal(
            describe_http_error(response).await,
            false,
        ));
    }

    stream_chat_response(response, on_chunk, cancel, base_url, settings).await
}

/// Prüft die vom Frontend gelieferten Werkzeug-Schemata. Sie stammen zwar aus
/// dem Backend selbst, aber die Oberfläche ist kein vertrauenswürdiger Absender,
/// deshalb wird auch hier gedeckelt.
pub fn validate_tool_schemas(
    tools: Option<Vec<serde_json::Value>>,
) -> Result<Option<Vec<serde_json::Value>>, String> {
    let Some(tools) = tools else {
        return Ok(None);
    };

    if tools.is_empty() {
        return Ok(None);
    }

    if tools.len() > 16 {
        return Err("Es sind zu viele Werkzeugschemata übergeben worden".to_string());
    }

    let serialized = serde_json::to_string(&tools)
        .map_err(|error| format!("Werkzeugschemata nicht lesbar: {}", error))?;

    if serialized.len() > MAX_TOOL_SCHEMAS_BYTES {
        return Err("Die Werkzeugschemata sind zu groß".to_string());
    }

    Ok(Some(tools))
}

// Die Grenze bildet die IPC-Schnittstelle ab, die jedes Argument einzeln über
// Tauri übergibt. Ein Sammelobjekt würde hier nichts vereinfachen.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn send_chat_message(
    app: tauri::AppHandle,
    model: String,
    messages: Vec<ChatMessage>,
    on_chunk: Channel<StreamChunk>,
    settings: State<'_, OllamaSettings>,
    control: State<'_, ChatControl>,
    tools: Option<Vec<serde_json::Value>>,
    system: Option<String>,
) -> Result<(), String> {
    validate_chat_input(&model, &messages)?;
    let tools = validate_tool_schemas(tools)?;
    let system = system
        .as_deref()
        .map(normalize_system_prompt)
        .transpose()?
        .filter(|prompt| !prompt.is_empty());
    let _ = control.cancel.send(false);
    let base_url = settings.get_base_url().await;
    let chat = settings.get_config().await.chat;
    let client = ollama_client()?;
    let payload = OllamaRequest {
        model,
        messages: without_thinking(messages),
        stream: true,
        system,
        tools,
        options: chat.context_option(),
    };

    let mut attempt = 1usize;
    loop {
        let mut cancel = control.cancel.subscribe();
        let failure = match send_chat_attempt(
            client,
            &base_url,
            &payload,
            &on_chunk,
            &mut cancel,
            attempt,
            &settings,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(failure) => failure,
        };

        if !should_retry_chat(&failure, attempt) {
            return Err(failure.message);
        }
        if !wait_for_server(&base_url).await {
            return Err(failure.message);
        }

        attempt += 1;
        let _ = app.emit(
            "chat-retry",
            ChatRetryNotice {
                attempt,
                max_attempts: MAX_CHAT_ATTEMPTS,
            },
        );
    }
}

#[tauri::command]
async fn get_models(settings: State<'_, OllamaSettings>) -> Result<Vec<String>, String> {
    let base_url = settings.get_base_url().await;
    let client = ollama_client()?;
    let response = client
        .get(format!("{base_url}/api/tags"))
        .timeout(OLLAMA_MODELS_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            connection_error_message(
                &format!("{base_url}/api/tags"),
                &format_reqwest_error(&error),
                error.is_connect(),
                1,
            )
        })?;

    if !response.status().is_success() {
        return Err(describe_http_error(response).await);
    }

    settings.mark_contact();

    let body = tokio::time::timeout(
        OLLAMA_MODELS_TIMEOUT,
        read_limited_response(response, MAX_MODELS_RESPONSE_BYTES, "Modellantwort"),
    )
    .await
    .map_err(|_| "Modellantwort hat zu lange gedauert".to_string())??;
    let payload = serde_json::from_slice::<OllamaModelsResponse>(&body)
        .map_err(|error| format!("Ungültige Antwort von Ollama: {}", error))?;
    if payload.models.len() > MAX_MODEL_COUNT {
        return Err("Ollama hat zu viele Modelle zurückgegeben".to_string());
    }

    let mut models = Vec::with_capacity(payload.models.len());
    for model in payload.models {
        validate_model_name(&model.name)?;
        models.push(model.name);
    }
    models.sort();
    models.dedup();
    Ok(models)
}

async fn is_ollama_online(base_url: &str) -> bool {
    is_ollama_online_within(base_url, STATUS_TIMEOUT).await
}

/// Gleiche Prüfung mit eigenem Timeout.
///
/// Die Aufrufer wählen unterschiedliche Grenzen, je nachdem was der Test kosten
/// darf: `STATUS_TIMEOUT` (5 s) für die Anzeige im Kopf, `OLLAMA_RETRY_PROBE_TIMEOUT`
/// (5 s) vor einem Neuversand und `OLLAMA_STALL_PROBE_TIMEOUT` (10 s) als
/// Nebenprüfung neben einer laufenden Antwort. Dort ist der kurze Wert zu knapp:
/// Ist der Server gerade mit dem Erzeugen beschäftigt, gilt er sonst fälschlich als
/// ausgefallen und die noch gültige Verbindung wird weggeworfen.
async fn is_ollama_online_within(base_url: &str, timeout: Duration) -> bool {
    let Ok(client) = ollama_client() else {
        return false;
    };

    client
        .get(format!("{base_url}/api/tags"))
        .timeout(timeout)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

#[tauri::command]
async fn check_server(settings: State<'_, OllamaSettings>) -> Result<ServerStatus, String> {
    let server_url = settings.get_base_url().await;
    let online = is_ollama_online(&server_url).await;

    if online {
        settings.mark_contact();
    }

    Ok(ServerStatus {
        online,
        server_url,
        last_contact: settings.last_contact(),
    })
}

#[tauri::command]
async fn get_ssh_config(settings: State<'_, OllamaSettings>) -> Result<SshConfig, String> {
    Ok(settings.get_config().await.ssh)
}

#[tauri::command]
async fn set_ssh_config(
    target: String,
    port: u16,
    identity_file: Option<String>,
    settings: State<'_, OllamaSettings>,
) -> Result<SshConfig, String> {
    settings
        .set_ssh_config(&target, port, identity_file.as_deref().unwrap_or_default())
        .await
}

fn create_ssh_command() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("ssh");
    command
        .kill_on_drop(true)
        .arg("-F")
        .arg(SSH_CONFIG_FILE)
        .arg("-o")
        .arg("ConnectTimeout=8")
        .arg("-o")
        .arg("StrictHostKeyChecking=yes")
        .arg("-o")
        .arg("ForwardAgent=no")
        .arg("-o")
        .arg("ClearAllForwardings=yes")
        .arg("-o")
        .arg("ProxyCommand=none")
        .arg("-o")
        .arg("ProxyJump=none")
        // Die Verbindung lebt nur für einen Befehl. Multiplexing und
        // Keepalive-Intervalle wären hier wirkungslos, werden aber explizit
        // abgeschaltet, damit eine Systemkonfiguration nichts davon ändert.
        .arg("-o")
        .arg("ControlMaster=no")
        .arg("-o")
        .arg("ControlPath=none")
        .arg("-o")
        .arg("ServerAliveInterval=0");
    command
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn start_ollama_script() -> String {
    START_OLLAMA_TEMPLATE
        .replace("@OLLAMA_SERVE_ENV@", OLLAMA_SERVE_ENV)
        .replace("@OLLAMA_PORT_HEX@", &format!("{:04X}", OLLAMA_PORT))
        .replace("@OLLAMA_PORT_DEC@", &OLLAMA_PORT.to_string())
}

fn build_start_ollama_command() -> String {
    format!("sh -c {}", shell_quote(&start_ollama_script()))
}

fn finish_ssh_command(
    mut command: tokio::process::Command,
    config: &SshConfig,
) -> Result<tokio::process::Command, String> {
    let target = normalize_ssh_target(&config.target)?;
    let port = validate_ssh_port(config.port)?;
    let identity_file = normalize_ssh_identity_file(&config.identity_file)?;

    if !identity_file.is_empty() {
        // IdentitiesOnly=yes verhindert, dass zusätzlich alle Standardpfade
        // und der Agent versucht werden. Ohne das bricht der Host mit "Too many
        // authentication failures" ab, sobald mehrere Schlüssel vorhanden sind.
        command
            .arg("-i")
            .arg(&identity_file)
            .arg("-o")
            .arg("IdentitiesOnly=yes");
    }

    command
        .arg("-p")
        .arg(port.to_string())
        .arg(target)
        .arg(build_start_ollama_command());
    command.stdin(std::process::Stdio::null());
    Ok(command)
}

#[derive(Debug)]
enum SshCommandError {
    AuthenticationRequired(String),
    Other(String),
}

impl SshCommandError {
    fn into_message(self) -> String {
        match self {
            Self::AuthenticationRequired(message) | Self::Other(message) => message,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SshFailure {
    AuthenticationRequired,
    PublicKeyOverloaded,
    HostKeyUntrusted,
    Unreachable,
    RemoteCommand,
}

fn contains_any_marker(details: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| details.contains(marker))
}

/// OpenSSH beendet sich mit 255 bei Verbindungs-, Host-Key- und
/// Authentifizierungsfehlern, mit anderen Codes bei Fehlern des Remote-Skripts.
/// Über den Exit-Code lässt sich die beiden Gruppe zuverlässig trennen, ohne
/// sich nur auf den Fehlertext zu verlassen. Innerhalb der 255er-Gruppe wird
/// der Text verfeinert; was nicht erkannt wird, gilt als Authentifizierungs-
/// frage, weil genau dann ein Passwort helfen kann. Das ist bewusst eine
/// Rückfallregel: eine übersetzte OpenSSH-Meldung, die hier nicht erkannt wird,
/// führt dadurch weiterhin zur Passwortabfrage statt zu einer falschen Meldung.
fn classify_ssh_failure(details: &str, status: std::process::ExitStatus) -> SshFailure {
    if status.code() != Some(SSH_TRANSPORT_FAILURE_CODE) {
        return SshFailure::RemoteCommand;
    }

    let details = details.to_ascii_lowercase();

    // Die vier Texte sind keine vier Fehler, sondern derselbe in vier
    // Wortlauten: ein fehlender, ein abweichender, ein abgelehnter und ein
    // nicht auffindbarer Host-Key. Sie stehen trotzdem einzeln, weil OpenSSH
    // je nach Zustand des Known_hosts eine andere davon ausgibt.
    if contains_any_marker(
        &details,
        &[
            "host key verification failed",
            "remote host identification has changed",
            "host key for",
            "no matching host key",
        ],
    ) {
        return SshFailure::HostKeyUntrusted;
    }

    if contains_any_marker(
        &details,
        &["too many authentication failures", "too many keys"],
    ) {
        return SshFailure::PublicKeyOverloaded;
    }

    if contains_any_marker(
        &details,
        &[
            "connection refused",
            "connection reset",
            "connection timed out",
            "connection closed by",
            "could not resolve",
            "name or service not known",
            "no route to host",
            "network is unreachable",
            "operation timed out",
            "kex_exchange_identification",
        ],
    ) {
        return SshFailure::Unreachable;
    }

    SshFailure::AuthenticationRequired
}

fn ssh_failure_message(failure: SshFailure, message: String) -> String {
    let hint = match failure {
        SshFailure::AuthenticationRequired | SshFailure::RemoteCommand => None,
        SshFailure::PublicKeyOverloaded => Some(
            "Es wurden zu viele SSH-Schlüssel angeboten. Mit /ssh-key <pfad> genau einen Schlüssel festlegen oder den ssh-agent aufräumen.",
        ),
        SshFailure::HostKeyUntrusted => Some(
            "Der Host-Key ist nicht hinterlegt oder hat sich geändert. Einmalig von Hand mit ssh-keyscan prüfen und in ~/.ssh/known_hosts eintragen.",
        ),
        SshFailure::Unreachable => Some("Der SSH-Server war nicht erreichbar."),
    };

    match hint {
        Some(hint) => format!("{message} ({hint})"),
        None => message,
    }
}

fn bounded_ssh_error(details: &[u8], status: std::process::ExitStatus) -> String {
    let mut text = String::from_utf8_lossy(details).trim().to_string();
    truncate_string(&mut text, MAX_SSH_ERROR_BYTES);
    if text.is_empty() {
        format!("SSH-Start fehlgeschlagen: {}", status)
    } else {
        format!("SSH-Start fehlgeschlagen: {}", text)
    }
}

async fn run_ssh_command(mut command: tokio::process::Command) -> Result<(), SshCommandError> {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        SshCommandError::Other(format!(
            "SSH-Prozess konnte nicht gestartet werden: {}",
            error
        ))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        SshCommandError::Other("SSH-Fehlerkanal konnte nicht geöffnet werden".to_string())
    })?;
    let stderr_task = tokio::spawn(async move {
        let mut details = Vec::new();
        let mut limited = stderr.take((MAX_SSH_ERROR_BYTES + 1) as u64);
        let _ = limited.read_to_end(&mut details).await;
        details
    });

    let status = match tokio::time::timeout(SSH_COMMAND_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            stderr_task.abort();
            return Err(SshCommandError::Other(format!(
                "SSH-Prozess konnte nicht beendet werden: {}",
                error
            )));
        }
        Err(_) => {
            let _ = child.kill().await;
            stderr_task.abort();
            return Err(SshCommandError::Other(format!(
                "SSH-Verbindung oder Remote-Befehl hat nach {} Sekunden nicht geantwortet",
                SSH_COMMAND_TIMEOUT.as_secs()
            )));
        }
    };

    let details = stderr_task.await.map_err(|_| {
        SshCommandError::Other("SSH-Fehlerausgabe konnte nicht gelesen werden".to_string())
    })?;

    if status.success() {
        return Ok(());
    }

    let failure = classify_ssh_failure(&String::from_utf8_lossy(&details), status);
    let message = ssh_failure_message(failure, bounded_ssh_error(&details, status));
    if failure == SshFailure::AuthenticationRequired {
        Err(SshCommandError::AuthenticationRequired(message))
    } else {
        Err(SshCommandError::Other(message))
    }
}

async fn run_ssh_with_key(config: &SshConfig) -> Result<(), SshCommandError> {
    let mut command = create_ssh_command();
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("PreferredAuthentications=publickey")
        .arg("-o")
        .arg("PasswordAuthentication=no")
        .arg("-o")
        .arg("KbdInteractiveAuthentication=no");
    let command = finish_ssh_command(command, config).map_err(SshCommandError::Other)?;
    run_ssh_command(command).await
}

/// Legt ein eigenes Verzeichnis mit restriktiven Rechten an. Der Inhalt von
/// `std::env::temp_dir()` ist für andere Benutzer auflistbar, deshalb liegen
/// Helper und Passwortdatei in einem `0700`-Verzeichnis statt direkt in `/tmp`.
/// So bleiben sie selbst dann geschützt, wenn die Anwendung hart beendet wird
/// und `Drop` nicht mehr ausgeführt wird.
#[cfg(unix)]
fn create_askpass_directory() -> Result<PathBuf, String> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("Zeitstempel konnte nicht erstellt werden: {}", error))?
        .as_nanos();

    for _ in 0..MAX_CONFIG_TEMP_ATTEMPTS {
        let path = std::env::temp_dir().join(format!(
            "mimir-ssh-askpass-{}-{unique}-{}",
            std::process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::DirBuilder::new();
        options.mode(0o700);
        match options.create(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "SSH-Askpass-Verzeichnis nicht erstellbar: {}",
                    error
                ))
            }
        }
    }
    Err("SSH-Askpass-Verzeichnis konnte nicht eindeutig erstellt werden".to_string())
}

#[cfg(unix)]
fn write_private_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut file = options
        .open(path)
        .map_err(|error| format!("SSH-Askpass-Datei nicht erstellbar: {}", error))?;
    if let Err(error) = file.write_all(contents) {
        let _ = std::fs::remove_file(path);
        return Err(format!("SSH-Askpass-Datei nicht schreibbar: {}", error));
    }
    if let Err(error) = file.sync_all() {
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "SSH-Askpass-Datei nicht synchronisierbar: {}",
            error
        ));
    }
    Ok(())
}

#[cfg(unix)]
struct AskpassFiles {
    directory: PathBuf,
    helper_path: PathBuf,
    password_path: PathBuf,
}

#[cfg(unix)]
impl Drop for AskpassFiles {
    fn drop(&mut self) {
        remove_secret_file(&self.password_path);
        let _ = std::fs::remove_file(&self.helper_path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[cfg(unix)]
fn remove_secret_file(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(path) {
            let zeros = [0u8; 1024];
            let mut remaining = metadata.len();
            while remaining > 0 {
                let count = remaining.min(zeros.len() as u64) as usize;
                if file.write_all(&zeros[..count]).is_err() {
                    break;
                }
                remaining -= count as u64;
            }
            let _ = file.sync_all();
        }
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
fn create_askpass_files(password: &str) -> Result<AskpassFiles, String> {
    let directory = create_askpass_directory()?;
    let helper_path = directory.join("askpass.sh");
    let password_path = directory.join("password");

    // Der Helper enthält das Passwort nicht, er liest es nur aus der Datei, auf
    // die SSH_ASKPASS verweist. Damit steht das Passwort in keiner
    // Prozessumgebung, die ein anderer Benutzer auslesen könnte.
    let result = write_private_file(
        &helper_path,
        b"#!/bin/sh\nexec /bin/cat \"$MIMIR_SSH_PASSWORD_FILE\"\n",
        0o700,
    )
    .and_then(|()| write_private_file(&password_path, password.as_bytes(), 0o600));

    if let Err(error) = result {
        let _ = std::fs::remove_file(&password_path);
        let _ = std::fs::remove_file(&helper_path);
        let _ = std::fs::remove_dir(&directory);
        return Err(error);
    }

    Ok(AskpassFiles {
        directory,
        helper_path,
        password_path,
    })
}

#[cfg(unix)]
async fn run_ssh_with_password(config: &SshConfig, password: String) -> Result<(), String> {
    let password = Zeroizing::new(password);
    if password.is_empty() || password.len() > MAX_SSH_PASSWORD_BYTES {
        return Err("Das SSH-Passwort ist ungültig".to_string());
    }

    let files = create_askpass_files(password.as_str())?;
    let result: Result<(), String> = async {
        let mut command = create_ssh_command();
        command
            .env("SSH_ASKPASS", &files.helper_path)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("MIMIR_SSH_PASSWORD_FILE", &files.password_path)
            .env_remove("MIMIR_SSH_PASSWORD")
            .env(
                "DISPLAY",
                std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()),
            )
            .arg("-o")
            .arg("BatchMode=no")
            .arg("-o")
            .arg("PreferredAuthentications=password")
            .arg("-o")
            .arg("PubkeyAuthentication=no")
            .arg("-o")
            .arg("KbdInteractiveAuthentication=no")
            .arg("-o")
            .arg("PasswordAuthentication=yes")
            .arg("-o")
            .arg("NumberOfPasswordPrompts=1");
        let command = finish_ssh_command(command, config)?;
        run_ssh_command(command)
            .await
            .map_err(SshCommandError::into_message)
    }
    .await;
    drop(files);
    result
}

#[cfg(not(unix))]
async fn run_ssh_with_password(_config: &SshConfig, password: String) -> Result<(), String> {
    let _password = Zeroizing::new(password);
    Err("Der Passwort-Fallback wird nur unter Unix, also Linux und macOS, unterstützt".to_string())
}

#[tauri::command]
async fn start_ollama_via_ssh(
    settings: State<'_, OllamaSettings>,
    password: Option<String>,
) -> Result<SshStartResult, String> {
    let password = password.map(Zeroizing::new);
    let config = settings.get_config().await;

    // Ein Server auf diesem Rechner wird nicht über SSH gestartet: Dafür gibt es
    // hier weder ein Ziel noch einen Grund. Vorher wäre der Knopf im lokalen
    // Provider gedrückt worden und hätte an einem Rechner im Netz etwas gestartet,
    // das mit dem gerade benutzten Modell nichts zu tun hat.
    if !config.provider.erlaubt_dateizugriff() {
        return Err(format!(
            "Im Provider „{}“ wird kein Server über SSH gestartet. Läuft hier kein Ollama, wird es \
             mit `sudo pacman -S ollama` installiert und mit `ollama serve` gestartet. Mit \
             /provider remote steht der SSH-Weg wieder zur Verfügung.",
            config.provider.bezeichnung(),
        ));
    }

    if config.ssh.target.is_empty() {
        return Err(
            "Kein SSH-Ziel konfiguriert. Nutze /ssh-target <benutzer@host> [port].".to_string(),
        );
    }

    if is_ollama_online(config.provider.adresse(&config.server_url)).await {
        return Ok(SshStartResult::Started);
    }

    // Ein bereits bekanntes Passwort darf keinen Schlüsselversuch mehr
    // auslösen: das wäre eine zweite vollständige Verbindung mit eigenem
    // TCP- und Authentifizierungs-Handshake für dieselbe Anmeldung.
    if let Some(password) = password {
        return run_ssh_with_password(&config.ssh, password.as_str().to_owned())
            .await
            .map(|()| SshStartResult::Started);
    }

    match run_ssh_with_key(&config.ssh).await {
        Ok(()) => Ok(SshStartResult::Started),
        Err(SshCommandError::AuthenticationRequired(details)) => {
            Ok(SshStartResult::PasswordRequired(format!(
                "SSH-Authentifizierung erforderlich: {}",
                details
            )))
        }
        Err(SshCommandError::Other(error)) => Err(error),
    }
}

#[tauri::command]
async fn get_server_url(settings: State<'_, OllamaSettings>) -> Result<String, String> {
    Ok(settings.get_base_url().await)
}

/// Woher die Modelle kommen. Steht in der Rückgabe, damit die Oberfläche ihre
/// Auswahl nicht aus dem eingetragenen `server_url` erraten muss: Bei beiden
/// Providern kann dieselbe Adresse stehen.
#[tauri::command]
async fn get_provider(settings: State<'_, OllamaSettings>) -> Result<Provider, String> {
    Ok(settings.get_provider().await)
}

#[tauri::command]
async fn set_provider(
    provider: Provider,
    settings: State<'_, OllamaSettings>,
) -> Result<Provider, String> {
    settings.set_provider(provider).await
}

/// Wie weit Mimir eingerichtet ist.
///
/// Wird nur beim Start einmal abgefragt, um beim ersten Mal eine Anleitung im Chat
/// anzubieten. Die Rückgabe ist absichtlich knapp und enthält **keine** Geheimnisse:
/// nur, was schon eingetragen ist und was noch fehlt.
///
/// Ohne diesen Beflag hätte die Oberfläche nur raten können: Ein leerer Verlauf
/// bedeutet nicht, dass nichts eingestellt ist – der Verlauf ist optional und
/// standardmäßig aus. Ein leerer Kalender bedeutet auch nichts, weil der Kalender
/// freiwillig ist. Nur die Serveradresse ist etwas, ohne das Mimir nicht läuft.
#[derive(Serialize, Clone, Debug)]
struct Einrichtung {
    /// Stimmt, wenn die Serveradresse nicht mehr die Vorgabe ist – oder wenn gar
    /// keine gebraucht wird, weil das lokale Ollama ohne Eintragung auskommt.
    server_eingetragen: bool,
    /// Die Vorgabe, gegen die geprüft wird. Steht hier, damit die Oberfläche den
    /// Text nicht selbst nachbauen muss.
    server_vorgabe: String,
    /// Ob der Kalender eine Adresse hat. Freiwillig, nur ein Hinweis.
    kalender_eingetragen: bool,
}

#[tauri::command]
async fn get_einrichtung(settings: State<'_, OllamaSettings>) -> Result<Einrichtung, String> {
    let config = settings.get_config().await;
    let server = normalize_server_url(&config.server_url)?;

    Ok(Einrichtung {
        // Der Vergleich läuft über den normalisierten Wert: `localhost:11434` und
        // `http://localhost:11434/` sind derselbe Server, und wer die zweite Form
        // eingetragen hat, ist genauso gut eingerichtet wie jemand, der nichts
        // getan hat. Beim lokalen Provider zählt die Adresse gar nicht: Sie wird
        // nicht gebraucht, also gibt es auch nichts zu beanstanden.
        server_eingetragen: !config.provider.erlaubt_dateizugriff()
            || server != normalize_server_url(DEFAULT_OLLAMA_BASE_URL)?,
        server_vorgabe: DEFAULT_OLLAMA_BASE_URL.to_string(),
        kalender_eingetragen: !config.calendar.server_url.trim().is_empty(),
    })
}

#[tauri::command]
async fn set_server_url(
    server_url: String,
    settings: State<'_, OllamaSettings>,
) -> Result<String, String> {
    settings.set_base_url(&server_url).await
}

/// Der gespeicherte Umfang, zusammen mit dem, der tatsächlich gilt.
///
/// Beide stehen in der Antwort, weil sie im lokalen Provider auseinanderfallen
/// können: Dort bleibt der gespeicherte Umfang stehen, während gearbeitet wird
/// im Terminumfang. Die Oberfläche zeigt den wirksamen an, sonst behauptete sie
/// Dateiwerkzeuge, die es nicht gibt.
#[tauri::command]
async fn get_agent_config(settings: State<'_, OllamaSettings>) -> Result<AgentConfig, String> {
    let config = settings.get_config().await;
    let umfang = wirksamer_umfang(&config);
    Ok(AgentConfig {
        scope: umfang,
        ..config.agent
    })
}

#[tauri::command]
async fn set_agent_config(
    root: String,
    max_steps: Option<usize>,
    scope: Option<Scope>,
    settings: State<'_, OllamaSettings>,
) -> Result<AgentConfig, String> {
    let config = settings.get_config().await;

    // Ohne ausdrückliche Angabe bleibt die eingestellte erhalten: `/agent-dir`
    // setzt einen Pfad und darf den Umfang nicht nebenbei zurücksetzen.
    let agent = AgentConfig {
        root,
        max_steps: max_steps.unwrap_or(config.agent.max_steps),
        scope: scope.unwrap_or(config.agent.scope),
    };

    // Das Backend lehnt einen Umfang ab, den der Provider nicht hergibt; die
    // Meldung hier nennt den Grund, bevor überhaupt gespeichert wird.
    if agent.scope == Scope::Agent && !config.provider.erlaubt_dateizugriff() {
        return Err(format!(
            "Im Provider „{}“ gibt es nur den Terminumfang: Das Modell läuft auf diesem \
             Rechner und bekommt dort keine Dateiwerkzeuge. Mit /provider remote kommst du zu \
             den Dateiwerkzeugen zurück.",
            config.provider.bezeichnung(),
        ));
    }

    let agent = settings.set_agent_config(agent).await?;

    // Zurück kommt der Umfang, mit dem wirklich gearbeitet wird. Bei lokalem
    // Provider ist das der Terminumfang, auch wenn ein Agentenmodus gespeichert
    // war – sonst zeigte die Oberfläche Werkzeuge an, die es nicht gibt.
    let config = settings.get_config().await;
    Ok(AgentConfig {
        scope: wirksamer_umfang(&config),
        ..agent
    })
}

/// Schemata und Systemprompt des Agentenmodus. Beides wird im Backend
/// erzeugt, damit Schemata, Werkzeugimplementierung und Anweisung nicht
/// auseinanderlaufen können. Schreibende Werkzeuge werden erst angeboten, wenn
/// der Benutzer sie freigeschaltet hat: Ein nicht beworbenes Werkzeug kann das
/// Modell nicht verlangen.
#[derive(Serialize)]
pub struct AgentToolset {
    pub system_prompt: String,
    pub tools: Vec<serde_json::Value>,
}

fn write_tool_schemas() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Legt eine neue Textdatei unterhalb des Arbeitsverzeichnisses an. Bestehende Dateien werden damit nicht überschrieben.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Neue Datei, relativ zum Arbeitsverzeichnis." },
                        "content": { "type": "string", "description": "Vollständiger Inhalt der neuen Datei." }
                    },
                    "required": ["path", "content"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Ersetzt genau eine Stelle in einer bestehenden Textdatei. Der zu ersetzende Text muss wörtlich und eindeutig sein.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Datei, relativ zum Arbeitsverzeichnis." },
                        "old_string": { "type": "string", "description": "Wortlaut, der ersetzt werden soll. Muss genau einmal vorkommen." },
                        "new_string": { "type": "string", "description": "Text, der an diese Stelle tritt." }
                    },
                    "required": ["path", "old_string", "new_string"]
                }
            }
        }),
    ]
}

/// Setzt den Schreibmodus und meldet den tatsächlichen Stand zurück. Bewusst
/// keine Wunschmeldung: Die Oberfläche übernimmt diesen Wert, und ein
/// zurückgegebener Zustand, der von der Anfrage abweicht, wäre ein stiller
/// Fehler wie ein Schreibmodus, den es gar nicht gibt.
fn apply_write_mode(state: &AgentState, enabled: bool) -> bool {
    state.set_write_enabled(enabled);
    state.write_enabled()
}

/// Ob das Modell Termine anlegen, ändern und löschen darf.
///
/// Der Schreibmodus gilt für die ganze Sitzung und sperrt damit Datei **und**
/// Kalender. Im Terminumfang ist der eingestellte Umfang selbst die
/// Freischaltung: Er nimmt dem Modell die Dateiwerkzeuge und gibt ihm die
/// Terminwerkzeuge, und mehr als das kann er nicht. Deshalb genügt hier der
/// Umfang, statt zusätzlich `/agent` und `/agent-write` zu verlangen – drei
/// Schalter für eine Fähigkeit wären eine Hürde ohne Sicherheitsgewinn.
///
/// Jeder einzelne Vorgang bleibt davon unberührt: `preview_tool_call` zeigt ihn
/// dem Benutzer mit Titel, Zeit und Inhalt, und erst danach schreibt
/// `execute_tool`.
fn darf_kalender_schreiben(state: &AgentState, scope: Scope) -> bool {
    scope == Scope::Termine || state.write_enabled()
}

/// Prüft den schreibenden Kalenderzugriff und sagt bei Ablehnung, woran es liegt.
///
/// Der Text nennt den Umfang mit: Im Terminumfang kann eine Ablehnung gar nicht
/// erst entstehen, wer sie dort zu Gesicht bekommt, läuft also `/scope` nach,
/// statt `/agent-write` zu versuchen. An drei Stellen stand derselbe Text, und
/// eine der drei hätte ihn irgendwann anders formuliert.
fn pruefe_kalender_schreiben(state: &AgentState, scope: Scope) -> Result<(), String> {
    if darf_kalender_schreiben(state, scope) {
        return Ok(());
    }

    Err(format!(
        "Schreibende Werkzeuge sind nicht freigeschaltet. Der Umfang ist {}; /agent-write gibt \
         sie für diese Sitzung frei.",
        scope.bezeichnung()
    ))
}

/// Beschreibung des Termin-Werkzeugs für das Modell.
///
/// Bewusst knapp und ohne Beispieltermine aus der wirklichen Welt: Der Benutzer
/// nennt Datum und Uhrzeit, und das Modell muss sie unverändert weitergeben.
/// Ausgearbeitete Beispiele führen dazu, dass er erfundene Zeiten einsetzt.
///
/// Die Kalendernamen stehen **zweimal**: im Prompt und in der Beschreibung des
/// Feldes. Nach dem zweiten Validierungslauf war das nötig: Im Prompt genannt hat
/// das Modell sie noch zwei von fünf Sätzen weggelassen. Das Feld, das gefüllt
/// werden soll, hat offenbar die kürzeste Aufmerksamkeit – und genau dort muss
/// die Wahl stehen.
fn calendar_event_schema(kalender: &[String]) -> serde_json::Value {
    let kalender_text = kalender_hinweis(kalender);

    serde_json::json!({
        "type": "function",
        "function": {
            "name": crate::calendar::write::EVENT_TOOL,
            "description": "Legt einen Termin im Nextcloud-Kalender an. Gibt es mehrere \
    Kalender, muss calendar den Namen eines davon nennen. Gib die Zeiten in den Worten des \
    Benutzers weiter, nicht umgerechnet. Teilnehmer und Anlagen werden nicht verschickt; \
    eine Erinnerung und Kategorien kannst du angeben.",
            "parameters": {
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": titel_hinweis()
                    },
                    "start": {
                        "type": "string",
                        "description": start_hinweis()
                    },
                    "end": {
                        "type": "string",
                        "description": "Ende in denselben Worten. Ohne Angabe eine Stunde; \
    ohne Tagesangabe gehört es zum Tag von start."
                    },
                    "all_day": {
                        "type": "boolean",
                        "description": "true für einen Ganztagestermin, etwa Urlaub."
                    },
                    "location": {
                        "type": "string",
                        "description": "Ort, höchstens 200 Zeichen."
                    },
                    "description": {
                        "type": "string",
                        "description": "Beschreibung, höchstens 2000 Zeichen."
                    },
                    "calendar": {
                        "type": "string",
                        "description": kalender_text
                    },
                    "reminder": {
                        "type": "string",
                        "description": "Wie weit vorher erinnert werden soll, in den Worten des \
    Benutzers: „5 Minuten vorher“, „eine halbe Stunde vorher“, „eine Stunde vorher“, „am \
    Vorabend“. Ohne Erinnerung: „keine Erinnerung“. Lass das Feld weg, wenn der Benutzer \
    nichts gesagt hat – dann gibt es keine."
                    },
                    "category": {
                        "type": "string",
                        "description": "Kategorie, etwa „Arbeit“. Mehrere mit Komma trennen."
                    }
                },
                "required": ["summary", "start"]
            }
        }
    })
}

/// Schema des lesenden Kalenderwerkzeugs.
///
/// Bewusst ohne festen Zeitraum: Das Modell soll den Zeitraum nennen, den der
/// Benutzer genannt hat, oder gar keinen – und nicht aus dem Anlassdatum einen
/// Zeitraum von 92 Tagen bauen, wie es der Kalender für die Leiste tut.
fn list_events_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": LIST_EVENTS_TOOL,
            "description": "Listet Termine aus dem Nextcloud-Kalender mit ihrer Kennung. \
    Nur verwenden, wenn der Benutzer nach Terminen fragt oder ein Termin geändert \
    oder gelöscht werden soll.",
            "parameters": {
                "type": "object",
                "properties": {
                    "from": {
                        "type": "string",
                        "description": "Erster Tag des Zeitraums. Entweder als \
    JJJJ-MM-TT oder als Wort, so wie der Benutzer es gesagt hat. Nimm das immer, \
    wenn der Benutzer einen einzelnen Tag oder eine Woche nennt. Gelesen werden: \
    JJJJ-MM-TT, 4.10., 4.10.2026, 4. Oktober, „morgen“, „übermorgen“, „gestern“, \
    „heute“, „montag“ bis „sonntag“, „in drei Tagen“. Das Wort wird aus dem \
    heutigen Tag gerechnet, den das Systemfeld nennt."
                    },
                    "to": {
                        "type": "string",
                        "description": "Letzter Tag des Zeitraums, einschließlich. \
    Gleiche Schreibweisen wie from. Nur zusammen mit from."
                    },
                    "range": {
                        "type": "integer",
                        "description": "Wie viele Tage ab jetzt kommen sollen, \
    etwa 7 für „diese Woche“. Wird ignoriert, wenn from gesetzt ist. Ohne Angabe \
    kommen die nächsten 30 Tage. Für einen einzelnen Tag ist from genauer."
                    },
                    "search": {
                        "type": "string",
                        "description": "Wort aus dem Titel, falls der Benutzer einen \
    bestimmten Termin meint."
                    }
                },
                "required": []
            }
        }
    })
}

/// Was das Modell über das Lesen wissen muss.
///
/// Die Kennung ist das Entscheidende: Ohne sie kann das Modell keinen Termin
/// gezielt ändern oder löschen, und Raten wäre bei einem Löschvorgang nicht
/// verantwortbar.
fn calendar_read_prompt() -> String {
    format!(
        "\n\nDu kannst {} benutzen: Listet Termine aus dem Nextcloud-Kalender des Benutzers mit \
Titel, Zeit, Kalender und Kennung. Die Kennung brauchst du nur, wenn Titel und Zeit allein nicht \
eindeutig sind. Nenne im Chat immer Titel und Datum des Termins, den du änderst oder löschst, \
damit der Benutzer weiß, worum es geht.\n\
\n\
Zeiträume: Nennt der Benutzer einen Tag oder eine Woche, setz `from`, bei einer Woche zusätzlich \
`to`. Du darfst das Wort des Benutzers unverändert durchgeben – \"übermorgen\", \"montag\", \
\"in drei Tagen\" werden gelesen und aus dem heutigen Tag gerechnet. Nur der Parameter `range` zählt \
Tage ab jetzt und beginnt einen Tag vor heute; er taugt nicht für \"morgen\". Steht im Systemfeld \
kein HEUTE, rechne nichts selbst, sondern sage, dass das aktuelle Datum fehlt.",
        LIST_EVENTS_TOOL
    )
}

/// Die beiden schreibenden Kalenderwerkzeuge.
///
/// Zwei Schemata, darum eine Liste: `json!` fasst nur einen Wert.
fn calendar_change_schema(kalender: &[String]) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
        "type": "function",
        "function": {
            "name": crate::calendar::edit::UPDATE_TOOL,
            "description": "Ändert einen bestehenden Termin. Was du nicht nennst, bleibt stehen. \
        Serientermine und Termine mit Teilnehmern werden abgelehnt. Erinnerung und Kategorie \
        änderst du über reminder und category.",
            "parameters": {
                "type": "object",
                "properties": {
                    "uid": { "type": "string", "description": "Kennung aus list_calendar_events. Nicht nötig, wenn du title und on_date nennst." },
                    "title": { "type": "string", "description": "Aktueller Titel des Termins, so wie ihn der Benutzer gesagt hat." },
                    "on_date": { "type": "string", "description": "Wann der Termin JETZT ist, in den Worten des Benutzers: etwa „gestern 14:00“. Das grenzt Termine gleichen Titels voneinander ab. Steht in start schon eine neue Zeit, darfst du on_date weglassen." },
                    "calendar": { "type": "string", "description": format!("Kalendername, wenn der Benutzer einen genannt hat. Auswahl: {}. Ohne Angabe bleibt der bisherige Kalender.", kalender.join(", ")) },
                    "summary": { "type": "string", "description": "Neuer Titel." },
                    "start": { "type": "string", "description": "Neuer Beginn in den Worten des Benutzers, etwa „morgen 14:00“." },
                    "end": { "type": "string", "description": "Neues Ende. Ohne Angabe bleibt die bisherige Dauer." },
                    "all_day": { "type": "boolean", "description": "true, wenn es ein Ganztagestermin werden soll." },
                    "location": { "type": "string", "description": "Neuer Ort; leerer Text entfernt ihn." },
                    "description": { "type": "string", "description": "Neue Beschreibung; leerer Text entfernt sie. Lass das Feld weg, um sie zu behalten." },
                    "reminder": { "type": "string", "description": "Neue Erinnerung in den Worten des Benutzers: „5 Minuten vorher“, „eine halbe Stunde vorher“, „eine Stunde vorher“, „am Vorabend“. Mit „keine Erinnerung“ wird sie entfernt. Lass das Feld weg, um die vorhandene zu behalten." },
                    "category": { "type": "string", "description": "Neue Kategorie, etwa „Arbeit“. Mehrere mit Komma; leerer Text entfernt sie." }
                },
                "required": []
            }
        },
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": crate::calendar::edit::DELETE_TOOL,
        "description": "Löscht einen Termin aus dem Kalender. Das ist endgültig und nur für \
        Termine ohne Teilnehmer und ohne Serie möglich. Nenne title und on_date – Titel und Zeit des \
        Termins, wie der Benutzer sie gesagt hat –, und den Kalender, wenn er einen genannt hat.",
            "parameters": {
                "type": "object",
                "properties": {
                    "uid": { "type": "string", "description": "Kennung aus list_calendar_events. Nicht nötig, wenn du title und on_date nennst." },
                    "title": { "type": "string", "description": "Titel des Termins, so wie ihn der Benutzer gesagt hat." },
                    "on_date": { "type": "string", "description": "Wann der Termin ist, in den Worten des Benutzers: etwa „gestern 14:00“ oder „morgen 9 Uhr“. Das grenzt Termine gleichen Titels voneinander ab." },
                    "calendar": { "type": "string", "description": format!("Kalendername, wenn der Benutzer einen genannt hat. Auswahl: {}", kalender.join(", ")) }
                },
                "required": []
                }
            }
        }),
    ]
}

/// Was das Modell über die Terminwerkzeuge wissen muss, bevor es sie benutzt.
///
/// Die Uhrzeit steht bewusst hier: Ein Modell hat keine Uhr und rät sonst ein
/// Datum. Genau so ist es passiert – auf „heute 14 Uhr" kam ein Termin im Jahr
/// 2023 heraus, drei Jahre in der Vergangenheit. Wer die Worte des Benutzers
/// durchreichen kann, braucht sie nicht umzurechnen; das Werkzeug versteht
/// „heute", „morgen" und „in 3 Tagen" von sich aus.
///
/// Ebenso wichtig ist, was **nicht** geht: Anlagen, Teilnehmer, Serientermine. Über
/// die ersten beiden hat das Modell sonst nichts erfahren und hat in der Praxis
/// eine Erinnerung behauptet, die es nicht gab, und dafür den Termin verschoben. Ein
/// Satz, der es nicht kann, ist der Unterschied zwischen einer ehrlichen Antwort und
/// einer erfundenen.
fn calendar_event_prompt() -> String {
    // Kein Datum hier. Das heutige Datum steht in **jeder** Anfrage im Systemfeld,
    // und eine zweite Angabe konkurriert nur mit der ersten: Als HEUTE einmal hier
    // und einmal dort standen unterschiedliche Werte nebeneinander, und welcher
    // galt, war nicht mehr entscheidbar. Diese Anweisung verweist darauf.
    format!(
        "\n\nZusätzlich kannst du {} benutzen: Legt einen Termin im Kalender an, \
wobei der Benutzer den Zielkalender genannt hat. Der heutige Tag und die Uhrzeit stehen \
oben im Systemfeld bei HEUTE IST; daran misst du jede Angabe wie „heute“, „morgen“ oder \
„nächsten Dienstag“. Die Worte des Benutzers gibst du \
unverändert weiter und rechnest nichts um; das Werkzeug rechnet. Kannst du eine Angabe \
nicht auflösen, frag nach, statt zu raten. Fällt dir ein Widerspruch auf, etwa ein \
Termin, der schon mehrere Jahre zurückliegt, sag es und lege nichts an. Das Werkzeug \
verschickt keine Einladungen. Eine Erinnerung setzt du über reminder, in den Worten des \
Benutzers: „5 Minuten vorher“, „eine halbe Stunde vorher“, „eine Stunde vorher“, \
„am Vorabend“; mit „keine Erinnerung“ fällt sie weg. Bleibt das Feld leer, gibt es keine \
Erinnerung. Mit category setzt du die Kategorie, mehrere mit Komma. Anlagen kannst du \
nicht anhängen: Fragt der Benutzer danach, sag es ihm und hör auf, statt es zu behaupten \
oder den Termin dafür zu verschieben. Mit {} und {} änderst und löschst du Termine; \
beides ist endgültig und über Mimir nicht rückgängig zu machen, und beides wird dem \
Benutzer vorher mit Titel, Zeit, Kalender und dem vollständigen Dateiinhalt gezeigt. \
Dabei kennst du nur das, was der Benutzer gesagt hat: Titel, Uhrzeit und Kalender. Gib \
genau das als title und on_date weiter, im Klartext des Benutzers, und rate keine Kennung. \
Beim Ändern kommt das neue Datum und die neue Uhrzeit in start – on_date beschreibt nur, wo \
der Termin JETZT liegt, und bleibt weg, wenn start schon sagt, wohin er soll. Triffen Titel \
und Zeit mehrere Termine, nennt dir das Werkzeug die Kennungen; dann wiederholst du den \
Auftrag mit der passenden uid und fragst den Benutzer nicht. Frage den Benutzer nur dann, \
wenn er selbst nicht sagen kann, welcher Termin gemeint ist – nicht, um eine Kennung zu \
erfragen, denn die kennt er nicht. \
Serientermine und Termine mit Teilnehmern lässt du liegen: Eine Änderung oder Löschung \
gälte für die ganze Reihe oder schickte den Beteiligten eine Absage. Beides entscheidet \
der Benutzer selbst in Nextcloud.",
        crate::calendar::write::EVENT_TOOL,
        crate::calendar::edit::UPDATE_TOOL,
        crate::calendar::edit::DELETE_TOOL,
    )
}

fn agent_toolset_for(root: &Path, write_enabled: bool, kalender: &[String]) -> AgentToolset {
    // Ohne Anmeldung gibt es keine Namen und damit keine Kalenderwerkzeuge.
    let angemeldet = !kalender.is_empty();
    let mut tools = agent_tool_schemas();
    let mut prompt = agent_system_prompt(&root.to_string_lossy());

    if write_enabled {
        tools.extend(write_tool_schemas());
        prompt.push_str(
            "\n\nZusätzlich darfst du schreiben: write_file legt eine neue Datei an, \
edit_file ersetzt genau eine eindeutig auffindbare Stelle. Es gibt weiterhin keine \
Möglichkeit, Dateien zu löschen, umzubenennen oder Befehle auszuführen. Jeder \
Schreibvorgang wird dem Benutzer als Unterschied angezeigt und muss von ihm \
freigegeben werden.",
        );
    }

    // Das Lesen der Termine braucht nur die Anmeldung: Es verändert nichts und
    // kostet den Benutzer keine Bestätigung. Ohne Anmeldung gäbe es nichts zu
    // lesen, und das Modell würde es ankündigen und an jedem Aufruf scheitern.
    if angemeldet {
        tools.push(list_events_schema());
        prompt.push_str(&calendar_read_prompt());
    }

    // Die schreibenden Kalenderwerkzeuge brauchen beides: die Freigabe für
    // Schreibvorgänge und eine Anmeldung.
    if write_enabled && angemeldet {
        tools.push(calendar_event_schema(kalender));
        tools.extend(calendar_change_schema(kalender));
        prompt.push_str(&calendar_event_prompt());
    }

    AgentToolset {
        system_prompt: prompt,
        tools,
    }
}

/// Das Werkzeugangebot im Terminumfang: die vier Kalenderwerkzeuge, sonst nichts.
///
/// Eine eigene Funktion und kein Schalter in `agent_toolset_for`, weil dort die
/// Anweisung über das Arbeitsverzeichnis erzeugt wird – im Terminumfang gibt es
/// keins, und eine leere Adresse darin zu nennen wäre eine Aussage, die nicht
/// stimmt. Die Dateiwerkzeuge sind hier nicht abgeschaltet, sondern gar nicht
/// erst vorhanden: Ein nicht angebotenes Werkzeug kann das Modell nicht
/// verlangen, und ohne Anmeldung gäbe es hier gar nichts zu tun.
pub fn termine_toolset_for(kalender: &[String]) -> Result<AgentToolset, String> {
    if kalender.is_empty() {
        // Ohne Anmeldung oder ohne Kalenderauswahl. Beides heißt dasselbe für
        // das Modell: Es gäbe nichts zu tun, und eine leere Liste sähe aus wie
        // ein Kalender mit nichts drin.
        return Err(
            "Der Terminumfang braucht eine Anmeldung: Mit /calendar anmelden, sonst \
             hat das Modell kein Werkzeug."
                .to_string(),
        );
    }

    let mut tools = vec![list_events_schema()];
    tools.push(calendar_event_schema(kalender));
    tools.extend(calendar_change_schema(kalender));

    Ok(AgentToolset {
        system_prompt: termine_anweisung(kalender),
        tools,
    })
}

/// Die Anweisung des Terminumfangs, für sich genommen.
///
/// Aus `termine_toolset_for` herausgezogen, weil die Validierung mit einem echten
/// Modell dieselben Sätze schicken muss, die im Betrieb gehen. An zwei Stellen
/// gelesen zu sein ist hier kein Duplikat: Es ist derselbe Text, derselbe Weg.
pub fn termine_anweisung(kalender_namen: &[String]) -> String {
    let mut prompt = calendar_read_prompt();
    prompt.push_str(&kalender_aufgabe(kalender_namen));
    prompt.push_str(&calendar_event_prompt());
    prompt
}

/// Was das Modell über die **ausgewählten** Kalender wissen muss.
///
/// Ohne diesen Absatz rät das Modell. Das Schema sagt nur, `calendar` sei „Name
/// des Zielkalenders, wenn mehrere ausgewählt sind" – welche Kalender das sind,
/// stand nirgends. Im ersten Validierungslauf nannte qwen2.5:7b daraufhin
/// „Arbeitskalender", „WorkCalendar" und ließ das Feld in anderen Fällen ganz
/// weg. `waehle_kalender` vergleicht absichtlich exakt, also lehnt es zu Recht ab
/// und der Benutzer sieht eine Fehlermeldung statt eines Termins.
fn kalender_aufgabe(namen: &[String]) -> String {
    if namen.len() == 1 {
        return format!(
            "\n\nEs ist ein Kalender ausgewählt: {}. Lass `calendar` leer – das Werkzeug trägt \
             dann selbst ein.",
            namen[0]
        );
    }

    format!(
        "\n\nEs sind mehrere Kalender ausgewählt: {}. Trage `calendar` **nur** ein, wenn der \
         Benutzer selbst einen Kalender genannt hat – dann genau diesen Namen, unverändert. \
         Hat er keinen genannt, lass das Feld leer und **frag ihn, in welchen Kalender es soll**. \
         Rate keinen: Ein erfundener Name wird abgelehnt, und eine Frage kostet den Benutzer \
         weniger als ein Termin im falschen Kalender.",
        namen.join(", ")
    )
}

/// Der Text für das Feld `calendar` im Schéma.
///
/// Bei einem Kalender: Das Feld soll leer bleiben. Bei mehreren: Die Namen – und
/// die ausdrückliche Erlaubnis, das Feld leer zu lassen und nachzufragen.
///
/// Das ist die Stelle, an der es entschieden wird. Ein Absatz im Prompt allein hat
/// nicht gereicht: Der Prompt sagte „nenn einen dieser Namen“, und die Felder
/// waren `Pflicht`. Beides zusammen las das Modell als Auftrag, das Feld zu füllen,
/// und es erfand einen Namen. Erst als die Feldbeschreibung selbst sagte, dass ein
/// leerer Wert richtig ist, blieb es leer.
fn kalender_hinweis(namen: &[String]) -> String {
    match namen.len() {
        0 => "Nicht gesetzt: Ohne Anmeldung gibt es dieses Werkzeug nicht.".to_string(),
        1 => format!(
            "Nicht nötig, es ist nur ein Kalender ausgewählt ({}). Lass das Feld leer.",
            namen[0]
        ),
        _ => format!(
            "Nur wenn der Benutzer einen Kalender genannt hat – dann genau einen von {}, \
             unverändert. Sonst **leer lassen und ihn fragen**, welchen Kalender er meint. \
             Erfinde keinen Namen: Ein Name, der nicht stimmt, wird abgelehnt.",
            namen.join(", ")
        ),
    }
}

/// Der Text für das Feld `start` im Anlegen-Schema.
///
/// `start` ist zusammen mit `summary` das einzige Pflichtfeld. Wird es hier
/// allein genannt, weiß das Modell am Ort des Ausfüllens, dass ein fehlender
/// Zeitpunkt eine Nachfrage ist – die allgemeine Regel im Prompt steht weiter
/// weg und wird eher überlesen.
fn start_hinweis() -> &'static str {
    "Beginn in den Worten des Benutzers, etwa „heute 14:00“ oder „morgen um 9“. Auch \
     JJJJ-MM-TTThh:mm wird verstanden. Ohne Versatz gilt die Zeit des Rechners."
}

/// Der Text für das Feld `summary` im Anlegen-Schema.
///
/// Steht an derselben Stelle wie der Kalenderhinweis und aus demselben Grund:
/// Nach dem zweiten Lauf war der Titel in drei von sechs Sätzen „Termin" oder eine
/// Umformulierung dessen, was der Benutzer gesagt hatte. Das Feld bekommt den
/// Klartext des Benutzers, nicht einen Titel, den das Modell erfunden hat.
fn titel_hinweis() -> &'static str {
    "Überschrift des Termins, höchstens 200 Zeichen."
}

/// Das Werkzeugangebot für den eingestellten Umfang.
///
/// Der Umfang entscheidet hier, nicht der Schalter im Kopf: Im Terminumfang
/// läuft die Werkzeugschleife ohne weiteres Zutun des Benutzers, weil er die
/// Kalenderwerkzeuge mitbringt. Deshalb prüft dieser Aufruf das
/// Arbeitsverzeichnis nur im Agentenmodus – sonst müsste man es setzen, obwohl
/// nichts gelesen wird.
///
/// Der Umfang kommt als eigener Parameter und wird nicht aus `agent` gelesen:
/// Er ist der wirksame Umfang, der vom Provider abweichen kann. Aus der
/// Konfiguration gelesen wäre hier die eine Stelle, an der das lokale Modell
/// doch Dateiwerkzeuge bekäme.
fn toolset_for(
    agent: &AgentConfig,
    scope: Scope,
    write_enabled: bool,
    kalender: &[String],
) -> Result<AgentToolset, String> {
    match scope {
        Scope::Termine => termine_toolset_for(kalender),
        Scope::Agent => {
            if agent.root.is_empty() {
                return Err(
                    "Kein Arbeitsverzeichnis konfiguriert. Nutze /agent-dir <pfad>.".to_string(),
                );
            }

            Ok(agent_toolset_for(
                &canonical_root(&agent.root)?,
                write_enabled,
                kalender,
            ))
        }
    }
}

/// Die Namen der Kalender, in die geschrieben werden darf.
///
/// Es sind die **ausgewählten**, nicht alle vorhandenen: `waehle_kalender` sucht
/// nur unter den ausgewählten, und ein Name aus dem übrigen Bestand würde vom
/// Modell als gültig gelesen und vom Werkzeug abgelehnt.
///
/// Leer heißt: keine Anmeldung oder keine Auswahl. Dann gibt es kein Werkzeug,
/// und der Grund dafür steht im Aufrufer – eine leere Liste hier einzusetzen
/// hieße, dem Modell eine Wahl zu geben, die es nicht hat.
fn gewaehlte_kalender(
    config: &crate::calendar::CalendarConfig,
    session: &crate::calendar::CalendarSession,
) -> Vec<String> {
    if config.calendars.is_empty() {
        return known_calendars(session)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
    }

    // Die Auswahl nennt Pfade. Die Namen holt der Sitzungszustand, weil nur er
    // sie kennt; ein Pfad ohne Namen im Sitzungszustand fällt weg, weil er sich
    // nicht nennen ließe.
    known_calendars(session)
        .into_iter()
        .filter(|(href, name)| !name.is_empty() && config.calendars.iter().any(|wahl| wahl == href))
        .map(|(_, name)| name)
        .collect()
}

#[tauri::command]
async fn list_tools(
    settings: State<'_, OllamaSettings>,
    state: State<'_, AgentState>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<AgentToolset, String> {
    let config = settings.get_config().await;
    toolset_for(
        &config.agent,
        wirksamer_umfang(&config),
        state.write_enabled(),
        &gewaehlte_kalender(&config.calendar, &session),
    )
}

pub mod termine_validierung;
pub mod termine_validierung_pruefungen;

/// Vorschau eines Schreibvorgangs. Sie entsteht aus derselben Planungsfunktion
/// wie der Schreibvorgang selbst, kann also nicht von der tatsächlichen Wirkung
/// abweichen.
#[derive(Serialize)]
struct ToolPreview {
    /// Kanonischer Zielpfad, relativ zum Arbeitsverzeichnis für die Anzeige.
    relative_path: String,
    /// Bisheriger Inhalt, `None`, wenn es die Datei noch nicht gibt.
    current: Option<String>,
    /// Inhalt nach der Änderung.
    next: String,
    /// Kurze Beschreibung der Wirkung.
    summary: String,
    /// Zeilenzahl vor und nachher.
    lines_before: usize,
    lines_after: usize,
    /// Ob die Vorschau gekürzt wurde.
    truncated: bool,
}

fn preview_write(
    root: &Path,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<ToolPreview, String> {
    if !is_write_tool(name) {
        return Err("Vorschauen sind nur für schreibende Werkzeuge vorgesehen".to_string());
    }

    let plan = plan_write(root, name, arguments)?;
    let current = read_current_content(&plan.path)?;
    let exists = current.is_some();
    let lines_before = current
        .as_ref()
        .map(|content| content.lines().count())
        .unwrap_or(0);
    let (current_preview, truncated) = match &current {
        Some(content) => bounded_preview(content),
        None => (String::new(), false),
    };
    let (next_preview, next_truncated) = bounded_preview(&plan.next);

    Ok(ToolPreview {
        relative_path: relative_to_root(root, &plan.path),
        current: if exists { Some(current_preview) } else { None },
        next: next_preview,
        summary: plan.summary,
        lines_before,
        lines_after: plan.next.lines().count(),
        truncated: truncated || next_truncated,
    })
}

/// Baut die Vorschau eines Termins aus derselben Planung, aus der der Termin
/// entsteht. Das Bestätigungsfenster zeigt damit die tatsächliche Wirkung: Titel,
/// Zeit, Kalender und den Inhalt, der geschrieben wird.
fn preview_event(plan: &crate::calendar::write::EventPlan) -> ToolPreview {
    ToolPreview {
        relative_path: format!("Kalender {}", plan.calendar_display),
        current: None,
        next: plan.ics.clone(),
        summary: plan.summary.clone(),
        lines_before: 0,
        lines_after: plan.ics.lines().count(),
        truncated: false,
    }
}

/// Holt die Kalenderpaare Pfad und Name aus dem Sitzungszustand.
fn known_calendars(session: &crate::calendar::CalendarSession) -> Vec<(String, String)> {
    session
        .calendars()
        .into_iter()
        .map(|entry| (entry.href, entry.display_name))
        .collect()
}

/// Führt das Anlegen eines Termins aus. Der Aufbau ist derselbe wie beim
/// Schreiben einer Datei: erst die Freigabe prüfen, dann planen, dann das Budget
/// belasten, dann schreiben.
async fn execute_calendar_event(
    state: &AgentState,
    config: &crate::calendar::CalendarConfig,
    session: &crate::calendar::CalendarSession,
    arguments: &serde_json::Value,
    scope: Scope,
    benutzertext: &str,
) -> Result<ToolOutput, String> {
    pruefe_kalender_schreiben(state, scope)?;

    let Some(password) = session.password() else {
        return Err("Für Termine muss Mimir angemeldet sein. Mit /calendar anmelden.".to_string());
    };

    let plan = crate::calendar::write::plan_event(
        config,
        &known_calendars(session),
        arguments,
        benutzertext,
    )?;

    // Wie beim Lesen: Das Zertifikat wird geprüft, bevor das Passwort die Maschine
    // verlässt.
    let client = crate::calendar::client::build_client(config.uses_tls())?;
    crate::calendar::ensure_certificate(config).await?;

    state.charge_write(plan.ics.len())?;
    let url = crate::calendar::client::create_event(&client, config, &password, &plan).await?;
    session.mark_calendar_write();

    let mut content = String::new();
    content.push_str(&plan.summary);
    content.push_str("\n\nGespeichert unter ");
    content.push_str(&url);
    content.push('\n');
    content.push_str(&format!("Kennung {}.", plan.uid));
    content.push_str("Prüfe das Ergebnis mit /termine.");

    // Was verworfen wurde, gehört auch in das Werkzeugergebnis: Das Modell
    // erfährt damit, dass seine Angabe nicht angekommen ist, und kann im
    // nächsten Schritt nachfragen, statt zu glauben, der Ort stehe drin.
    if !plan.verworfen.is_empty() {
        content.push_str(&format!(
            "\nNicht übernommen, weil der Benutzer es nicht genannt hat: {}.",
            plan.verworfen.join(", ")
        ));
    }

    Ok(ToolOutput::new(content, plan.summary))
}

/// Baut die Vorschau für das Ändern oder Löschen eines Termins.
///
/// Beim Ändern stehen alter und neuer Inhalt nebeneinander, damit der Benutzer
/// genau die eine Zeile sieht, die sich ändert. Beim Löschen steht der
/// vollständige Termin im Fenster – er verschwindet, und nichts davon kommt
/// zurück.
fn preview_change(
    aktueller_inhalt: &str,
    neuer_inhalt: &str,
    zusammenfassung: &str,
    kalender: &str,
) -> ToolPreview {
    ToolPreview {
        relative_path: format!("Kalender {kalender}"),
        current: (!aktueller_inhalt.is_empty()).then(|| aktueller_inhalt.to_string()),
        next: neuer_inholt_zuruecksetzen(neuer_inhalt),
        summary: zusammenfassung.to_string(),
        lines_before: aktueller_inhalt.lines().count(),
        lines_after: neuer_inhalt.lines().count(),
        truncated: false,
    }
}

/// Beim Löschen gibt es keinen neuen Inhalt; dann bleibt das Feld leer, und das
/// Fenster zeigt genau das, was verschwindet.
fn neuer_inholt_zuruecksetzen(neuer_inhalt: &str) -> String {
    neuer_inhalt.to_string()
}

/// Sucht den Termin, dessen jetziger Titel genannt wurde.
///
/// Ohne diese Möglichkeit passiert Folgendes: Das Modell nennt den Titel, weil
/// es die Kennung nicht kennt, und der Vorgang scheitert. Nennt der Benutzer
/// „den Termin vom Dienstag“, ist der Titel ohnehin die Angabe, die vorliegt.
///
/// Bei mehreren Treffern wird nicht geraten: Es folgt die Liste der Kandidaten
/// mit Datum, Titel und Kennung, damit das Modell im nächsten Schritt eindeutig
/// ist.
async fn finde_termin_nach_titel(
    client: &reqwest::Client,
    config: &crate::calendar::CalendarConfig,
    password: &str,
    arguments: &serde_json::Value,
) -> Result<String, String> {
    use crate::calendar::edit;

    let gesucht = arguments
        .get("title")
        .and_then(|wert| wert.as_str())
        .map(crate::calendar::write::clean)
        .filter(|wert| !wert.is_empty())
        .ok_or_else(|| {
            "Welcher Termin? Nenne Titel und Zeit – zum Beispiel Titel und „gestern 14:00“ – \
             oder die Kennung aus list_calendar_events."
                .to_string()
        })?
        .to_lowercase();

    // Der Benutzer kennt Titel, Uhrzeit und Kalender. Die Kennung kennt er nicht,
    // und er soll sie auch nicht kennen müssen: Sie wird hier aus genau diesen
    // drei Angaben ermittelt.
    //
    // `on_date` beschreibt im Werkzeugschema den **jetzigen** Termin. Wer aber
    // sagt „verschiebe den Termin auf heute 15 Uhr“, meint mit der Zeit das
    // **Ziel**, und das Modell schickt sie folgerichtig als `on_date`. Würde die
    // Suche darauf einschränken, fände sie den Termin nicht, den der Benutzer
    // gerade verschieben will – der läge ja gerade woanders.
    //
    // Deshalb gilt: Sobald ein neues `start` dasteht, ist `on_date` das Ziel
    // und darf die Suche nicht eingrenzen. Die Zeit des Termins zu kennen ist
    // dann nicht nötig, um ihn zu finden.
    let auswahl = auswahl_aus_argumenten(arguments);

    let kalender = arguments
        .get("calendar")
        .and_then(|wert| wert.as_str())
        .map(crate::calendar::write::clean)
        .filter(|wert| !wert.is_empty());

    // Weit genug, um auch einen alten Termin zu finden: Wer „verschiebe meinen
    // Termin“ sagt, meint den nächsten, aber ein zurückliegender ist nicht
    // ausgeschlossen.
    let jetzt = crate::calendar::client::now();
    let fenster = crate::calendar::events::Window {
        start: jetzt - chrono::Duration::days(60),
        end: jetzt + chrono::Duration::days(365),
    };
    let termine = crate::calendar::client::fetch_events(client, config, password, &fenster).await?;

    let treffer: Vec<&crate::calendar::CalendarEvent> = termine
        .iter()
        .filter(|termin| edit::passt_zu_termin(termin, &gesucht, &auswahl, kalender.as_deref()))
        .collect();

    if treffer.is_empty() {
        // Der Fehlertext nennt die Angaben, die tatsächlich ankamen. Ein Modell
        // kann nur korrigieren, was es nachlesen kann.
        let mut beschreibung = format!("Einen Termin mit dem Titel „{gesucht}“");

        if let Some(tag) = auswahl.tag {
            beschreibung.push_str(&format!(" am {}", tag.format("%d.%m.%Y")));
        }

        if let Some(uhrzeit) = auswahl.uhrzeit {
            beschreibung.push_str(&format!(" um {}", uhrzeit.format("%H:%M")));
        }

        if let Some(kalender) = &kalender {
            beschreibung.push_str(&format!(" im Kalender „{kalender}“"));
        }

        return Err(format!(
            "{beschreibung} gibt es nicht. Suche mit list_calendar_events nach einer ähnlichen \
             Bezeichnung, oder frage den Benutzer nach dem genauen Titel."
        ));
    }

    // Genau ein Treffer: das ist die Kennung.
    if treffer.len() == 1 {
        return Ok(treffer[0].uid.clone());
    }

    // Mehrere Treffer werden nicht geraten. Gerade beim Löschen und Ändern wäre
    // ein Fehlgriff stiller Datenverlust, deshalb nennt Mimir die Kandidaten
    // vollständig und lässt den Benutzer wählen.
    let kandidaten: Vec<String> = treffer
        .iter()
        .take(6)
        .map(|termin| {
            let zeit = chrono::TimeZone::timestamp_opt(&chrono::Local, termin.start, 0)
                .single()
                .map(|wann| wann.format("%d.%m.%Y %H:%M").to_string())
                .unwrap_or_else(|| "unbekannt".to_string());
            // Die Kennung steht mit drin: Das Modell kann den Auftrag sonst
            // nicht eindeutig machen und müsste den Benutzer mit einer Frage
            // behelligen, die dieser mangels Kennung nicht beantworten kann.
            format!(
                "- {zeit}  {}  (Kalender {}  –  Kennung {})",
                termin.summary, termin.calendar, termin.uid
            )
        })
        .collect();

    Err(format!(
        "Auf diese Angabe fallen {} Termine, und sie unterscheiden sich nicht. Nenne den \
         Kalender dazu, eine andere Uhrzeit oder den genaueren Titel – oder entscheide dich \
         mit der Kennung eines der folgenden. Welcher ist gemeint?\n{}",
        treffer.len(),
        kandidaten.join("\n")
    ))
}

/// Sagt, ob der Auftrag eine neue Uhrzeit nennt.
///
/// Dann meint `on_date` das Ziel des Termins und nicht seine jetzige Lage.
/// Ohne diese Unterscheidung suchte Mimir einen Termin am Tag, den der Benutzer
/// gerade verlassen will, statt den, den er meint.
fn name_der_aenderung_setzt_die_zeit(arguments: &serde_json::Value) -> bool {
    ["start", "end"].iter().any(|feld| {
        arguments
            .get(*feld)
            .and_then(|wert| wert.as_str())
            .map(crate::calendar::write::clean)
            .is_some_and(|wert| !wert.is_empty())
    })
}

/// Die Auswahl, die aus den Werkzeugargumenten folgt.
///
/// Getrennt von `finde_termin_nach_titel`, weil sie ohne Server ausprüfbar ist:
/// Ob ein Termin gefunden wird, hängt an dieser einen Regel, und sie ist im
/// Betrieb mehrfach falsch gewesen.
fn auswahl_aus_argumenten(arguments: &serde_json::Value) -> crate::calendar::edit::Auswahl {
    use crate::calendar::edit;

    if name_der_aenderung_setzt_die_zeit(arguments) {
        // Eine neue Uhrzeit ist genannt: on_date meint dann das Ziel und darf
        // die Suche nicht auf den heutigen Tag einschränken.
        return edit::Auswahl {
            tag: None,
            uhrzeit: None,
        };
    }

    arguments
        .get("on_date")
        .and_then(|wert| wert.as_str())
        .map(edit::auswahl_aus)
        .unwrap_or(edit::Auswahl {
            tag: None,
            uhrzeit: None,
        })
}

/// Holt den Termin aus dem Kalender und legt den Plan für die Vorschau an.
///
/// Der Termin wird **nur gelesen**. Erst die Vorschau entsteht, und geschrieben
/// wird erst, wenn der Benutzer sie freigegeben hat.
async fn lade_fuer_aenderung(
    client: &reqwest::Client,
    config: &crate::calendar::CalendarConfig,
    password: &str,
    session: &crate::calendar::CalendarSession,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<GeaenderterTermin, String> {
    use crate::calendar::edit;

    edit::pruefe_auswahl(arguments)?;
    let kalender = known_calendars(session);

    // Erst die Kennung, sonst der jetzige Titel des Termins. Der Titel ist für
    // ein Modell leichter zu treffen – im Betrieb wurde ein Titel als Kennung
    // geschickt, und der Vorgang scheiterte daran.
    let uid = match edit::kennung_aus(arguments) {
        Some(uid) => uid,
        None => finde_termin_nach_titel(client, config, password, arguments).await?,
    };

    // Der Kalender wird über die Auswahl des Benutzers bestimmt – mit derselben
    // Regel wie in der Planung, damit Abruf und Schreibvorgang nicht an
    // verschiedenen Orten landen.
    let zielkalender = crate::calendar::edit::zielkalender(
        config,
        &kalender,
        arguments.get("calendar").and_then(|wert| wert.as_str()),
        &uid,
    )?;
    let (ziel_pfad, _) = zielkalender;

    let auf_dem_server =
        crate::calendar::client::fetch_event(client, config, password, &ziel_pfad, &uid).await?;

    if name == edit::UPDATE_TOOL {
        let plan = edit::plan_update(config, &kalender, arguments, &auf_dem_server.ics, &uid)?;

        Ok(GeaenderterTermin {
            url: auf_dem_server.url,
            etag: auf_dem_server.etag,
            inhalt: plan.nachher,
            vorher: plan.vorher,
            zusammenfassung: plan.summary,
            kalender: plan.calendar_display,
            geloescht: false,
        })
    } else {
        let plan = edit::plan_delete(config, &kalender, arguments, &auf_dem_server.ics, &uid)?;

        Ok(GeaenderterTermin {
            url: auf_dem_server.url,
            etag: auf_dem_server.etag,
            inhalt: String::new(),
            vorher: plan.vorher,
            zusammenfassung: plan.summary,
            kalender: plan.calendar_display,
            geloescht: true,
        })
    }
}

/// Alles, was für einen Schreibvorgauf am Termin nötig ist.
struct GeaenderterTermin {
    url: String,
    etag: String,
    /// Der neue Inhalt. Beim Löschen leer.
    inhalt: String,
    vorher: String,
    zusammenfassung: String,
    kalender: String,
    geloescht: bool,
}

/// Listet Termine mit ihrer Kennung. Nur gelesen, also ohne Bestätigung.
///
/// Die Kennung ist das Einzige, was ein gezieltes Ändern oder Löschen möglich
/// macht. Sie zu raten wäre bei einem Löschvorgang nicht verantwortbar, deshalb
/// steht sie in der Antwort.
/// In welcher Woche ein Zeitraum liegt – in Worten, wie der Benutzer sie sagt.
///
/// Ohne diese Zeile stand in der Antwort nur ein Datum. Der 4. Oktober 2026 ist
/// ein Sonntag, und ein Sonntag ohne Jahr passt auf jede Woche – das Modell
/// nannte ihn daraufhin die „übermächste Woche“, obwohl er in der laufenden lag,
/// die genau an ihm endet. Die Zuordnung ist eine einfache Rechnung, an der das
/// Modell nicht zweifeln kann.
///
/// „Diese Woche“ meint Montag bis Sonntag, wie in Deutschland üblich.
fn wochenzuordnung(erster: &chrono::NaiveDate, letzter: &chrono::NaiveDate) -> String {
    use chrono::Datelike;

    // Montag der Woche, in der der **letzte** Tag liegt. Bei einem Zeitraum über
    // mehrere Wochen hinweg wäre der des Anfangs irreführend.
    let montag = *letzter - chrono::Duration::days(letzter.weekday().num_days_from_monday() as i64);
    let heute = chrono::Local::now().date_naive();
    let diese_woche = heute - chrono::Duration::days(heute.weekday().num_days_from_monday() as i64);

    let name = match (montag - diese_woche).num_days() / 7 {
        0 => "die laufende Woche",
        1 => "die nächste Woche",
        2 => "die übernächste Woche",
        -1 => "die letzte Woche",
        n if n < -1 => "eine frühere Woche",
        _ => "eine spätere Woche",
    };

    let von = erster.format("%a, %d.%m.%Y");
    let bis = letzter.format("%a, %d.%m.%Y");

    let woche = format!("{name}, Montag bis Sonntag");

    if erster == letzter {
        return format!(" Das ist {woche}.");
    }

    // „gehört zu“ verlangt den Dativ, „ist“ den Nominativ. Beide Formen stehen
    // hier, weil ein Satz mit Präposition sonst falsch klingt.
    format!(" Der Zeitraum läuft von {von} bis {bis}; das ist {woche}.")
}

async fn list_calendar_events(
    config: &crate::calendar::CalendarConfig,
    session: &crate::calendar::CalendarSession,
    arguments: &serde_json::Value,
) -> Result<ToolOutput, String> {
    let Some(password) = session.password() else {
        return Err("Für Termine muss Mimir angemeldet sein. Mit /calendar anmelden.".to_string());
    };

    crate::calendar::ensure_certificate(config).await?;
    let client = crate::calendar::client::build_client(config.uses_tls())?;

    // Der Zeitraum kommt in zwei Formen. `from`/`to` sind ausgeschriebene
    // Datumsgrenzen (`JJJJ-MM-TT`) und treffen einen einzelnen Tag genau; `range`
    // ist eine Zahl Tage ab jetzt und deshalb für „morgen“ zu ungenau – die
    // früheste damit erreichbare Grenze ist „jetzt minus einen Tag“, wer den
    // nächsten Tag meint, bekommt den heutigen mit und nennt daraufhin den
    // falschen.
    //
    // Ohne beides die nächsten 30 Tage: weit genug für „was steht an“ und eng
    // genug, um den Kontext nicht zu fluten.
    let jetzt = crate::calendar::client::now();
    let tage = arguments
        .get("range")
        .and_then(|wert| wert.as_i64().or_else(|| wert.as_str()?.trim().parse().ok()))
        .unwrap_or(30);
    // Datumsgrenzen **und** Worte. Nur `JJJJ-MM-TT` zu lesen war zu streng: Das
    // Modell schickte `from: übermorgen`, das Parsen scheiterte, und ohne Fehler
    // fielen 30 Tage an. Die Antwort sah damit nach einer leeren Agenda aus,
    // war aber eine Abfrage nach einem völlig anderen Zeitraum. Die Worte sind
    // hier dieselben wie beim Schreiben, damit es nur eine Regel gibt.
    let grenze = |name: &str| -> Result<Option<chrono::NaiveDate>, String> {
        let roh = arguments
            .get(name)
            .and_then(|wert| wert.as_str())
            .map(str::trim)
            .filter(|wert| !wert.is_empty());

        match roh {
            None => Ok(None),
            // Ein ISO-Datum geht immer vor, damit „2026-10-02“ nicht als Wort
            // gelesen wird.
            Some(wert) => match chrono::NaiveDate::parse_from_str(wert, "%Y-%m-%d") {
                Ok(datum) => Ok(Some(datum)),
                Err(_) => {
                    crate::calendar::write::parse_zeitraum_tag(wert, jetzt.date_naive()).map(Some)
                }
            },
        }
    };
    let von = grenze("from")?;
    let fenster = match von {
        Some(von) => crate::calendar::events::window_from_days(jetzt, von, grenze("to")?),
        None => crate::calendar::events::window_from_now(jetzt, tage),
    };
    let termine =
        crate::calendar::client::fetch_events(&client, config, &password, &fenster).await?;
    session.mark_success(crate::calendar::client::now().timestamp());

    // Das Fenster beginnt einen Tag früher, damit eine über den Rand laufende
    // Serie nicht fehlt. Für die Antwort zählt aber nur der gewünschte Zeitraum –
    // sonst stünde in der Antwort zu „morgen“ auch der heutige Tag.
    let antwort_ab = fenster.start + chrono::Duration::days(1);
    let termine: Vec<_> = termine
        .into_iter()
        .filter(|termin| termin.start >= antwort_ab.timestamp())
        .collect();

    let suchbegriff = arguments
        .get("search")
        .and_then(|wert| wert.as_str())
        .map(|wert| wert.trim().to_lowercase())
        .filter(|wert| !wert.is_empty());

    let mut treffer: Vec<String> = Vec::new();

    for termin in termine
        .iter()
        .filter(|termin| !termin.canceled)
        .filter(|termin| {
            suchbegriff
                .as_ref()
                .map(|wort| termin.summary.to_lowercase().contains(wort))
                .unwrap_or(true)
        })
    {
        let beginn = chrono::TimeZone::timestamp_opt(&chrono::Local, termin.start, 0)
            .single()
            .map(|zeit| {
                if termin.all_day {
                    zeit.format("%d.%m.%Y (ganztägig)").to_string()
                } else {
                    zeit.format("%d.%m.%Y %H:%M").to_string()
                }
            })
            .unwrap_or_else(|| "unbekannt".to_string());

        // Die Erinnerung und die Kategorien mitnennen: Das Modell soll vor einer
        // Änderung wissen, dass es da sind – sonst „erfindet“ es eine, weil es
        // keine kennt.
        let merkmale = merkmale_text(termin);

        treffer.push(format!(
            "- {beginn}  {}  (Kalender {}, Kennung {}{merkmale})",
            termin.summary, termin.calendar, termin.uid
        ));
    }

    // Der abgefragte Zeitraum steht in **jeder** Antwort, ob Treffer da sind oder
    // nicht. Ohne ihn entstand die Antwort „in diesem Zeitraum von übermorgen
    // keine Termine“ – ohne jedes Datum, und ohne die Möglichkeit zu erkennen,
    // dass da etwas anderes abgefragt wurde als gemeint war.
    // Der erste gewünschte Tag: Das Fenster beginnt einen Tag früher, damit eine
    // über den Rand laufende Serie nicht fehlt.
    let erster_tag_zeit = (fenster.start + chrono::Duration::days(1)).with_timezone(&chrono::Local);
    // Und der letzte: Das Fenster endet exklusiv.
    let letzter_tag_zeit = (fenster.end - chrono::Duration::days(1)).with_timezone(&chrono::Local);
    let erster_tag = erster_tag_zeit.format("%a, %d.%m.%Y").to_string();
    let letzter_tag = letzter_tag_zeit.format("%a, %d.%m.%Y").to_string();
    let erster_tag_local = erster_tag_zeit.date_naive();
    let letzter_tag_local = letzter_tag_zeit.date_naive();
    // Welche Woche das ist, steht in der Antwort. Aus dem Datum allein geht das
    // nicht hervor: Der 4.10.2026 ist ein Sonntag, und „Sonntag“ ohne Jahr
    // passt auf jede Woche. Das Modell nannte ihn deshalb die „übermächste
    // Woche“ – er liegt in der laufenden, die am 4.10. endet.
    let wochenzeile = wochenzuordnung(&erster_tag_local, &letzter_tag_local);
    let zeitraum = format!("Abgefragter Zeitraum: {erster_tag} bis {letzter_tag}.{wochenzeile}");

    let inhalt = if treffer.is_empty() {
        format!("{zeitraum}\nIn diesem Zeitraum stehen keine Termine.")
    } else {
        let mut text = format!("{zeitraum}\n{} Termin(e):\n", treffer.len());
        text.push_str(&treffer.join("\n"));
        text.push_str(
            "\n\nZum Ändern oder Löschen die Kennung unverändert in das Feld uid übernehmen, \
             oder den jetzigen Titel in title nennen. Beides wird als Werkzeug aufgerufen, nicht \
             als Text geschrieben.",
        );
        text
    };

    let zusammenfassung = if treffer.is_empty() {
        "Keine Termine gefunden".to_string()
    } else {
        format!("{} Termin(e) gefunden", treffer.len())
    };

    Ok(ToolOutput::new(inhalt, zusammenfassung))
}

/// Der Zusatz hinter einem Termin im Text von `list_calendar_events`.
///
/// Steht dort, weil das Modell sonst keine Erinnerung und keine Kategorie
/// kennt – und eine nicht gekannte Erinnerung legt es erfunden an. Leer heißt:
/// der Termin hat beides nicht, und es steht kein „, “ herum.
fn merkmale_text(termin: &crate::calendar::CalendarEvent) -> String {
    let mut teile: Vec<String> = Vec::new();

    if !termin.reminder_text.is_empty() {
        teile.push(termin.reminder_text.clone());
    }

    for kategorie in &termin.categories {
        teile.push(format!("Kategorie {kategorie}"));
    }

    if teile.is_empty() {
        return String::new();
    }

    format!(", {}", teile.join(", "))
}

/// Was der Benutzer im Fenster geändert hat.
///
/// Alle Felder kommen immer, auch die unveränderten: Das Formular schickt den
/// ganzen Termin, und Mimir vergleicht ihn mit dem, was im Kalender steht. So
/// entsteht kein Vorgang, bei dem eine halbe Änderung fehlt.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminAenderung {
    uid: String,
    /// Der Pfad des Kalenders, wie ihn die Leiste mitliefert. Nicht der Name:
    /// Zwei Kalender können ähnlich heißen, und ein Schreibvorgang im falschen
    /// Kalender wäre stiller Datenverlust.
    calendar_href: String,
    summary: String,
    start: String,
    /// Leer heißt: die bisherige Dauer bleibt stehen.
    #[serde(default)]
    end: String,
    #[serde(default)]
    all_day: bool,
    #[serde(default)]
    location: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    categories: String,
    /// In den Worten des Benutzers, etwa „15 Minuten vorher“. Leer oder „keine
    /// Erinnerung“ nimmt sie weg.
    #[serde(default)]
    reminder: String,
}

impl TerminAenderung {
    /// Baut die Argumente, die auch das Werkzeug des Agentenmodums bekommt.
    ///
    /// Bewusst derselbe Aufbau: Dadurch greifen dieselben Prüfungen und dieselbe
    /// Zeilenbearbeitung. Ein zweiter Weg zum Ändern eines Termins wäre eine
    /// zweite Stelle, an der die beiden auseinanderlaufen.
    fn als_argumente(&self) -> serde_json::Value {
        let mut end = serde_json::Value::Null;

        if !self.end.trim().is_empty() {
            end = serde_json::Value::String(self.end.trim().to_string());
        }

        serde_json::json!({
            "uid": self.uid,
            "summary": self.summary,
            "start": self.start,
            "end": end,
            "all_day": self.all_day,
            // Leer heißt hier: entfernen. `null` würde „stehen lassen“ heißen und
            // das wäre beim leeren Feld genau das Gegenteil von dem, was der
            // Benutzer getippt hat.
            "location": self.location,
            "description": self.description,
            "category": self.categories,
            "reminder": self.reminder,
            // Der Pfad, nicht der Anzeigename. `plan_update` löst den Zielkalender
            // über dieses Feld auf, und ein Name kann doppelt vorkommen – der
            // Pfad nicht. Für das Werkzeug des Agentenmodus bleibt es beim Namen,
            // weil dort der Benutzer spricht.
            "calendar": self.calendar_href,
        })
    }
}

/// Holt die Felder eines Termins für das Fenster.
///
/// Nur gelesen, also ohne jede Freigabe: Es wird nichts am Kalender berührt.
/// Der Termin wird frisch vom Server geholt und nicht aus der Liste der Leiste
/// genommen – die kann Stunden alt sein.
#[tauri::command]
async fn calendar_event_open(
    uid: String,
    calendar_href: String,
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<crate::calendar::edit::TerminDetails, String> {
    let config = settings.get_config().await.calendar;
    let Some(password) = session.password() else {
        return Err("Für Termine muss Mimir angemeldet sein. Mit /calendar anmelden.".to_string());
    };

    let kalender = kalender_zu_href(&known_calendars(&session), &calendar_href)?;
    crate::calendar::ensure_certificate(&config).await?;
    let client = crate::calendar::client::build_client(config.uses_tls())?;
    let termin =
        crate::calendar::client::fetch_event(&client, &config, &password, &calendar_href, &uid)
            .await?;

    session.mark_success(crate::calendar::client::now().timestamp());
    crate::calendar::edit::termin_details(&termin.ics, &kalender, &calendar_href)
}

/// Schreibt einen Termin, wie ihn das Fenster zusammengestellt hat.
///
/// `nur_ansicht` bildet den Vorgang nur ab und gibt den Unterschied zurück. Das
/// ist der erste der beiden Schritte: Der Benutzer sieht, was gespeichert würde,
/// und entscheidet dann. Der zweite Schritt nimmt denselben Aufbau und dieselbe
/// Planung – es kann also nicht etwas anderes schreiben als gezeigt.
#[tauri::command]
async fn calendar_event_save(
    aenderung: TerminAenderung,
    nur_ansicht: Option<bool>,
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<SpeichernErgebnis, String> {
    let nur_ansicht = nur_ansicht.unwrap_or(false);
    let config = settings.get_config().await.calendar;
    let Some(password) = session.password() else {
        return Err("Für Termine muss Mimir angemeldet sein. Mit /calendar anmelden.".to_string());
    };

    // Nur die Prüfung, dass es diesen Kalender noch gibt. Der Anzeigename kommt
    // aus der Planung, weil sie den Zielkalender auflöst.
    kalender_zu_href(&known_calendars(&session), &aenderung.calendar_href)?;
    crate::calendar::ensure_certificate(&config).await?;
    let client = crate::calendar::client::build_client(config.uses_tls())?;

    // Unmittelbar vor dem Schreiben holen: Zwischen der Vorschau und dem Klick
    // können Sekunden liegen, in denen Nextcloud den Termin ändert. Genau dafür
    // gibt es die Änderungskennung.
    let auf_dem_server = crate::calendar::client::fetch_event(
        &client,
        &config,
        &password,
        &aenderung.calendar_href,
        &aenderung.uid,
    )
    .await?;

    let plan = crate::calendar::edit::plan_update(
        &config,
        &known_calendars(&session),
        &aenderung.als_argumente(),
        &auf_dem_server.ics,
        &aenderung.uid,
    )?;

    if nur_ansicht {
        return Ok(SpeichernErgebnis {
            vorher: plan.vorher,
            nachher: plan.nachher,
            zusammenfassung: plan.summary,
            geschrieben: false,
        });
    }

    crate::calendar::client::update_event(
        &client,
        &config,
        &password,
        &auf_dem_server.url,
        &auf_dem_server.etag,
        &plan.nachher,
    )
    .await?;

    session.mark_calendar_write();

    Ok(SpeichernErgebnis {
        vorher: plan.vorher,
        nachher: plan.nachher,
        zusammenfassung: plan.summary,
        geschrieben: true,
    })
}

/// Was ein Speichervorgang bewirkt hat, und was er bewirken würde.
#[derive(Serialize, Clone, Debug)]
struct SpeichernErgebnis {
    vorher: String,
    nachher: String,
    zusammenfassung: String,
    /// `false`, wenn nur die Vorschau gemeint war.
    geschrieben: bool,
}

/// Findet den Anzeigenamen zu einem Kalenderpfad.
///
/// Nur über den Pfad, nie über den Namen: Der Pfad kommt aus der Leiste und
/// ist damit eindeutig. Ein Name kann doppelt vorkommen, und dann wäre nicht
/// mehr zu sagen, welcher Kalender gemeint war.
fn kalender_zu_href(kalender: &[(String, String)], href: &str) -> Result<String, String> {
    let href = href.trim();

    match kalender
        .iter()
        .find(|(pfad, _)| pfad == href)
        .map(|(_, name)| name.clone())
    {
        Some(name) => Ok(name),
        None => Err(format!(
            "Den Kalender {href} gibt es nicht mehr. Die Leiste neu laden und den Termin \
             erneut öffnen."
        )),
    }
}

/// Führt das Ändern oder Löschen eines Termins aus.
///
/// Der Ablauf ist derselbe wie beim Anlegen: Freigabe prüfen, Termin lesen,
/// Plan bauen, Budget belasten, schreiben. Neu ist die Änderungskennung: Sie wird
/// erst beim Lesen geholt und beim Schreiben mitgeschickt, damit ein Termin, der
/// zwischenzeitlich in Nextcloud verändert wurde, unangetastet bleibt.
async fn execute_calendar_change(
    state: &AgentState,
    settings: &OllamaSettings,
    session: &crate::calendar::CalendarSession,
    name: &str,
    arguments: &serde_json::Value,
) -> Result<ToolOutput, String> {
    let config = settings.get_config().await;
    pruefe_kalender_schreiben(state, wirksamer_umfang(&config))?;

    let calendar = config.calendar;
    let Some(password) = session.password() else {
        return Err("Für Termine muss Mimir angemeldet sein. Mit /calendar anmelden.".to_string());
    };

    crate::calendar::ensure_certificate(&calendar).await?;
    let client = crate::calendar::client::build_client(calendar.uses_tls())?;

    // Der Termin wird unmittelbar vor dem Schreiben geholt: Zwischen Vorschau
    // und Freigabe können Sekunden liegen, in denen Nextcloud den Termin ändert.
    let termin =
        lade_fuer_aenderung(&client, &calendar, &password, session, name, arguments).await?;

    state.charge_write(termin.inhalt.len().max(1))?;

    if termin.geloescht {
        crate::calendar::client::delete_event(
            &client,
            &calendar,
            &password,
            &termin.url,
            &termin.etag,
        )
        .await?;
    } else {
        crate::calendar::client::update_event(
            &client,
            &calendar,
            &password,
            &termin.url,
            &termin.etag,
            &termin.inhalt,
        )
        .await?;
    }

    session.mark_calendar_write();

    let mut content = termin.zusammenfassung.clone();
    content.push_str("\n\nPrüfe das Ergebnis mit /termine. In Nextcloud lässt sich der Termin auch wieder ändern oder anlegen.");

    Ok(ToolOutput::new(content, termin.zusammenfassung))
}

#[tauri::command]
async fn preview_tool_call(
    name: String,
    arguments: serde_json::Value,
    benutzertext: String,
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
    state: State<'_, AgentState>,
) -> Result<ToolPreview, String> {
    let config = settings.get_config().await;
    let scope = wirksamer_umfang(&config);

    if name == crate::calendar::write::EVENT_TOOL {
        if !darf_kalender_schreiben(&state, scope) {
            return Err(
                "Schreibende Werkzeuge sind nicht freigeschaltet. Der Benutzer muss sie im \
                 Chat freigeben."
                    .to_string(),
            );
        }

        let plan = crate::calendar::write::plan_event(
            &config.calendar,
            &known_calendars(&session),
            &arguments,
            &benutzertext,
        )?;

        return Ok(preview_event(&plan));
    }

    if name == crate::calendar::edit::UPDATE_TOOL || name == crate::calendar::edit::DELETE_TOOL {
        pruefe_kalender_schreiben(&state, scope)?;

        let calendar = config.calendar;
        let Some(password) = session.password() else {
            return Err(
                "Für Termine muss Mimir angemeldet sein. Mit /calendar anmelden.".to_string(),
            );
        };

        crate::calendar::ensure_certificate(&calendar).await?;
        let client = crate::calendar::client::build_client(calendar.uses_tls())?;
        let termin =
            lade_fuer_aenderung(&client, &calendar, &password, &session, &name, &arguments).await?;

        return Ok(preview_change(
            &termin.vorher,
            &termin.inhalt,
            &termin.zusammenfassung,
            &termin.kalender,
        ));
    }

    let root = canonical_root(&config.agent.root)?;
    preview_write(&root, &name, &arguments)
}

#[tauri::command]
async fn execute_tool(
    name: String,
    arguments: serde_json::Value,
    benutzertext: String,
    settings: State<'_, OllamaSettings>,
    state: State<'_, AgentState>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<ToolOutput, String> {
    if name.len() > MAX_TOOL_NAME_BYTES {
        return Err("Der Werkzeugname ist ungültig".to_string());
    }

    if name == crate::calendar::write::EVENT_TOOL {
        let config = settings.get_config().await;
        return execute_calendar_event(
            &state,
            &config.calendar,
            &session,
            &arguments,
            wirksamer_umfang(&config),
            &benutzertext,
        )
        .await;
    }

    if name == crate::calendar::edit::UPDATE_TOOL || name == crate::calendar::edit::DELETE_TOOL {
        return execute_calendar_change(&state, &settings, &session, &name, &arguments).await;
    }

    if name == LIST_EVENTS_TOOL {
        let config = settings.get_config().await.calendar;
        return list_calendar_events(&config, &session, &arguments).await;
    }

    let agent = settings.get_config().await.agent;
    let root = canonical_root(&agent.root)?;

    if is_write_tool(&name) {
        return execute_write_tool(&state, &root, &name, &arguments);
    }

    execute_read_only_tool(&agent.root, &name, &arguments)
}

/// Stellt den vorherigen Inhalt einer Datei wieder her. Für Dateien, die es
/// vorher nicht gab, wird die Datei wieder entfernt.
fn undo_last_change(state: &AgentState, root: &Path, path: &str) -> Result<ToolOutput, String> {
    let (target, _parent, file_name) = resolve_write_target(root, path)?;

    if state.is_protected(&target) {
        return Err("Dieses Verzeichnis ist für den Agentenmodus gesperrt.".to_string());
    }

    let Some(entry) = state.take_undo(&target) else {
        return Err(format!(
            "Für {file_name} gibt es keinen gespeicherten Vorgänger."
        ));
    };

    let parent = target
        .parent()
        .ok_or_else(|| "Kein Elternverzeichnis".to_string())?
        .to_path_buf();
    let summary = format!("{file_name} wiederhergestellt");
    let restored = format!("{file_name} wurde auf den Stand vor der Änderung zurückgesetzt.");
    let short = format!("{file_name} zurückgesetzt");

    match entry.previous {
        Some(content) => {
            write_atomically(&WritePlan {
                path: target,
                parent,
                file_name,
                next: content,
                summary,
            })?;
        }
        None => {
            std::fs::remove_file(&target)
                .map_err(|error| format!("Datei nicht entfernbar: {}", error))?;
        }
    }

    Ok(ToolOutput::new(restored, short))
}

#[tauri::command]
async fn undo_write(
    path: String,
    settings: State<'_, OllamaSettings>,
    state: State<'_, AgentState>,
) -> Result<ToolOutput, String> {
    let agent = settings.get_config().await.agent;
    let root = canonical_root(&agent.root)?;
    undo_last_change(&state, &root, &path)
}

/// Schaltmodus für schreibende Werkzeuge. Gilt nur für diese Sitzung und wird
/// bewusst nicht gespeichert.
#[tauri::command]
async fn set_write_enabled(enabled: bool, state: State<'_, AgentState>) -> Result<bool, String> {
    Ok(apply_write_mode(&state, enabled))
}

#[derive(Serialize)]
struct WriteState {
    enabled: bool,
    writes: usize,
    max_writes: usize,
    max_bytes: usize,
    used_bytes: usize,
}

#[tauri::command]
async fn get_write_state(state: State<'_, AgentState>) -> Result<WriteState, String> {
    let budget = state
        .budget
        .lock()
        .map(|budget| (budget.writes, budget.bytes))
        .map_err(|_| "Das Schreibbudget ist nicht verfügbar".to_string())?;

    Ok(WriteState {
        enabled: state.write_enabled(),
        writes: budget.0,
        max_writes: MAX_TOOL_WRITES_PER_TURN,
        max_bytes: MAX_TOOL_WRITE_TOTAL_BYTES,
        used_bytes: budget.1,
    })
}

/// Setzt das Schreibbudget zu Beginn eines Agentenzugs zurück.
#[tauri::command]
async fn reset_write_budget(state: State<'_, AgentState>) -> Result<(), String> {
    let mut budget = state
        .budget
        .lock()
        .map_err(|_| "Das Schreibbudget ist nicht verfügbar".to_string())?;
    budget.writes = 0;
    budget.bytes = 0;
    Ok(())
}

#[tauri::command]
async fn get_chat_config(settings: State<'_, OllamaSettings>) -> Result<ChatConfig, String> {
    Ok(settings.get_config().await.chat)
}

/// Bereitet eine angehängte Datei für das Modell auf.
///
/// Zwei Fehler werden hier behoben, die vorher im Chat sichtbar waren:
///
/// - Das Frontend akzeptierte bis 128 KiB, das Backend ließ aber nur 64 KiB zu.
///   Eine Datei dazwischen wurde als „Angehängt" quittiert und ließ anschließend
///   die Anfrage scheitern.
/// - Der Benutzer sah nicht, dass ein Teil der Datei fehlte.
///
/// Die Kürzung passiert deshalb **hier** und nicht erst im Modellaufruf, damit die
/// Quittung im Chat das wahre Ergebnis zeigt.
#[tauri::command]
fn prepare_attachment(name: String, content: String) -> Result<PreparedAttachment, String> {
    if name.trim().is_empty() {
        return Err("Die Datei hat keinen Namen".to_string());
    }

    if content.len() > MAX_ATTACHMENT_BYTES {
        return Err(format!(
            "{} ist {} KiB groß; angehängt werden höchstens {} KiB.",
            name,
            content.len() / 1024,
            MAX_ATTACHMENT_BYTES / 1024
        ));
    }

    // Die Kürzung entscheidet, ob der Kopf den Hinweis bekommt. Deshalb wird er
    // zweimal gebildet: einmal, um zu messen, und einmal mit dem Zusatz. Der
    // Platz für den Zusatz ist im zweiten Fall durch die Kürzung wieder frei.
    let kopf = |gekuerzt: bool| {
        format!(
            "Datei: {} ({} KiB){}\n\n",
            name,
            content.len() / 1024,
            if gekuerzt { " – gekürzt" } else { "" }
        )
    };

    let (_, gedacht_gekuerzt) = kuerze_fuer_nachricht(&kopf(true), &content);
    let (message, truncated) = kuerze_fuer_nachricht(&kopf(gedacht_gekuerzt), &content);

    Ok(PreparedAttachment {
        message,
        truncated,
        original_bytes: content.len(),
    })
}

/// Eine angehängte Datei, fertig für den Verlauf.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
struct PreparedAttachment {
    message: String,
    truncated: bool,
    original_bytes: usize,
}

/// Ändert die Chat-Einstellungen. Nicht gesetzte Felder bleiben unverändert, damit
/// die Oberfläche nicht alles mitschicken muss.
#[tauri::command]
async fn set_chat_config(
    system_prompt: Option<String>,
    context_tokens: Option<usize>,
    save_history: Option<bool>,
    settings: State<'_, OllamaSettings>,
) -> Result<ChatConfig, String> {
    let current = settings.get_config().await.chat;
    let chat = ChatConfig {
        system_prompt: system_prompt.unwrap_or(current.system_prompt),
        context_tokens: context_tokens.unwrap_or(current.context_tokens),
        save_history: save_history.unwrap_or(current.save_history),
    };
    settings.set_chat_config(chat).await
}

/// Gespeicherter Verlauf. Ohne Freischaltung kommt immer `None`, damit sich die
/// Datei nicht auslesen lässt.
#[tauri::command]
async fn load_chat_history(
    settings: State<'_, OllamaSettings>,
) -> Result<Option<Vec<ChatMessage>>, String> {
    Ok(settings.load_history().await)
}

#[tauri::command]
async fn save_chat_history(
    messages: Vec<ChatMessage>,
    settings: State<'_, OllamaSettings>,
) -> Result<bool, String> {
    validate_chat_input("verlauf", &messages)?;
    settings.persist_history(&messages).await?;
    Ok(settings.get_config().await.chat.save_history)
}

#[tauri::command]
async fn delete_chat_history(settings: State<'_, OllamaSettings>) -> Result<(), String> {
    settings.delete_history()
}

// ------------------------------------------------------------ Kalenderleiste
// Bewusst nur lesend: diese Befehle rufen PROPFIND und REPORT auf und schreiben
// nichts in den Kalender. Anlegen, Ändern und Löschen kommen später und brauchen
// dann dieselbe Vorschau und Bestätigung wie die Schreibwerkzeuge des
// Agentenmodus.

#[tauri::command]
async fn get_calendar_config(
    settings: State<'_, OllamaSettings>,
) -> Result<crate::calendar::CalendarConfig, String> {
    Ok(settings.get_config().await.calendar)
}

/// Merkt sich Adresse, Benutzer und Kalenderauswahl. Ohne Adresse wird die
/// Einstellung gelöscht, damit die Leiste wieder ihren Ausgangszustand zeigt.
#[tauri::command]
async fn set_calendar_config(
    server_url: Option<String>,
    username: Option<String>,
    calendars: Option<Vec<String>>,
    settings: State<'_, OllamaSettings>,
) -> Result<crate::calendar::CalendarConfig, String> {
    let current = settings.get_config().await.calendar;
    let requested = crate::calendar::CalendarConfig {
        server_url: server_url.unwrap_or(current.server_url.clone()),
        username: username.unwrap_or(current.username.clone()),
        calendars: calendars.unwrap_or(current.calendars.clone()),
        // Das Zertifikat wird nicht über diesen Befehl gesetzt, sondern nur über
        // `calendar_trust_certificate` nach ausdrücklicher Bestätigung.
        server_certificate: current.server_certificate.clone(),
    };

    // Leere Adresse heißt: Einrichtung zurücknehmen, dann auch das Passwort weg.
    if requested.server_url.trim().is_empty() {
        settings.forget_calendar_credential()?;
        let cleared = settings
            .set_calendar_config(crate::calendar::CalendarConfig::default())
            .await?;

        return Ok(cleared);
    }

    // Gehört das gespeicherte Passwort zu einem anderen Benutzer, nützt es
    // nichts mehr und wird entfernt.
    if requested.username != current.username {
        settings.forget_calendar_credential()?;
    }

    settings.set_calendar_config(requested).await
}

/// Meldet an, prüft die Anmeldedaten und merkt sich die gefundenen Kalender.
/// Das App-Passwort bleibt im Arbeitsspeicher.
#[tauri::command]
async fn calendar_login(
    server_url: String,
    username: String,
    app_password: String,
    remember: Option<bool>,
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<crate::calendar::CalendarStatus, String> {
    let gespeichert = settings.get_config().await.calendar;
    let eingetragen = crate::calendar::normalize_server_url(&server_url)?;
    let mut name = username.trim().to_string();

    // Die aus den Nextcloud-Einstellungen kopierte WebDAV-Adresse trägt den
    // Benutzernamen schon im Pfad. Dann muss er nicht zusätzlich getippt werden.
    if name.is_empty() {
        let mit_adresse = crate::calendar::CalendarConfig {
            server_url: eingetragen.clone(),
            ..gespeichert.clone()
        };

        if let Some(aus_pfad) = mit_adresse.username_from_dav_path() {
            name = aus_pfad;
        }
    }

    let config = crate::calendar::CalendarConfig {
        // Gespeichert wird die Basis, nicht der kopierte DAV-Pfad.
        server_url: crate::calendar::CalendarConfig {
            server_url: eingetragen.clone(),
            ..gespeichert.clone()
        }
        .instance_base(),
        username: crate::calendar::validate_username(&name)?,
        calendars: gespeichert.calendars,
        server_certificate: gespeichert.server_certificate,
    };

    // Ein kopiertes App-Passwort bringt oft ein Leerzeichen oder einen
    // Zeilenumbruch mit; getrimmt wird einmal, derselbe Wert wird überall
    // verwendet.
    let app_password = crate::calendar::trimmed_app_password(&app_password);

    // Erst merken, wenn der Server die Anmeldedaten auch bestätigt hat. Sonst
    // bliebe eine unbrauchbare Adresse in der Konfiguration stehen.
    // Das Zertifikat wird geprüft, bevor das Passwort die Maschine verlässt.
    crate::calendar::client::build_client(config.uses_tls())?;
    crate::calendar::ensure_certificate(&config).await?;

    session.set_password(&app_password)?;
    let client = crate::calendar::client::build_client(config.uses_tls())?;
    let result = match crate::calendar::client::login(&client, &config, &app_password).await {
        Ok(result) => result,
        Err(error) => {
            session.forget();
            return Err(error);
        }
    };
    session.set_calendars(result.calendars.clone(), result.version.clone());

    let remember = remember.unwrap_or(false);
    settings.persist_calendar_credential(&config.username, &app_password, remember)?;
    settings.set_calendar_config(config.clone()).await?;

    let mut status = crate::calendar::build_status(&config, &session, None);
    status.remembered = remember;

    Ok(status)
}

/// Meldet ab und vergisst das App-Passwort. Die Adresse bleibt stehen, damit die
/// Leiste den letzten Stand zeigt und ein erneutes Anmelden nur ein Feld braucht.
#[tauri::command]
async fn calendar_logout(
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<crate::calendar::CalendarStatus, String> {
    session.forget();
    // Abmelden entfernt auch das gespeicherte Passwort, sonst käme die
    // Anmeldung beim nächsten Start von selbst zurück.
    settings.forget_calendar_credential()?;
    let config = settings.get_config().await.calendar;
    let mut status = crate::calendar::build_status(&config, &session, None);
    status.remembered = settings.has_stored_calendar_credential(&config.username);

    Ok(status)
}

/// Zustand der Leiste, ohne Netzabruf. Die Oberfläche fragt das beim Öffnen und
/// nach jeder Änderung, damit der Text nicht von einem Abruf abhängt.
#[tauri::command]
async fn calendar_status(
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<crate::calendar::CalendarStatus, String> {
    let config = settings.get_config().await.calendar;
    let mut status = crate::calendar::build_status(&config, &session, None);
    // `build_status` kennt den Pfad zur Ablage nicht, deshalb wird das Feld hier
    // gefüllt. Ohne das könnte die Leiste nie anzeigen, dass das Passwort
    // gemerkt wurde – es wäre nach der Anmeldung wieder verschwunden.
    status.remembered = settings.has_stored_calendar_credential(&config.username);

    Ok(status)
}

/// Die Kalender der Instanz. Wird beim Anmelden und beim Ändern der Auswahl
/// gebraucht.
#[tauri::command]
async fn calendar_calendars(
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<Vec<crate::calendar::client::CalendarInfo>, String> {
    let config = settings.get_config().await.calendar;
    let Some(password) = session.password() else {
        return Err("Nicht angemeldet".to_string());
    };

    crate::calendar::ensure_certificate(&config).await?;
    let client = crate::calendar::client::build_client(config.uses_tls())?;
    let calendars = crate::calendar::client::list_calendars(&client, &config, &password).await?;
    session.set_calendars(calendars.clone(), None);

    Ok(calendars)
}

/// Die nächsten Termine im Zeitraum der Leiste. Nur gelesen.
#[tauri::command]
async fn calendar_events(
    settings: State<'_, OllamaSettings>,
    session: State<'_, crate::calendar::CalendarSession>,
) -> Result<Vec<crate::calendar::CalendarEvent>, String> {
    let config = settings.get_config().await.calendar;
    let Some(password) = session.password() else {
        return Err("Nicht angemeldet".to_string());
    };

    crate::calendar::ensure_certificate(&config).await?;
    let client = crate::calendar::client::build_client(config.uses_tls())?;
    let window = crate::calendar::events::default_window(crate::calendar::client::now());
    let events =
        crate::calendar::client::fetch_events(&client, &config, &password, &window).await?;
    session.mark_success(crate::calendar::client::now().timestamp());

    Ok(events)
}

/// Prüft das Zertifikat der Instanz, ohne Zugangsdaten zu senden, und nennt den
/// Fingerabdruck. Das Aufrufen bestätigt nichts: Dafür gibt es
/// `calendar_trust_certificate`.
#[tauri::command]
async fn calendar_certificate(
    server_url: Option<String>,
    settings: State<'_, OllamaSettings>,
) -> Result<crate::calendar::CertificateStatus, String> {
    let config =
        crate::calendar::config_for_address(server_url, &settings.get_config().await.calendar)?;

    crate::calendar::certificate_status(&config).await
}

/// Nimmt das Zertifikat der Instanz an, nachdem der Benutzer den Fingerabdruck
/// geprüft hat. Ohne diesen Schritt verweigert Mimir bei `https` jeden Zugang.
#[tauri::command]
async fn calendar_trust_certificate(
    server_url: Option<String>,
    settings: State<'_, OllamaSettings>,
) -> Result<crate::calendar::CertificateStatus, String> {
    let config =
        crate::calendar::config_for_address(server_url, &settings.get_config().await.calendar)?;

    if config.server_url.is_empty() {
        return Err("Es ist keine Instanz eingetragen".to_string());
    }

    let client = crate::calendar::client::build_client(config.uses_tls())?;
    let certificate = match crate::calendar::client::probe_certificate(&client, &config).await? {
        crate::calendar::client::CertificateCheck::NeedsApproval(certificate) => certificate,
        crate::calendar::client::CertificateCheck::Matches => {
            return Ok(crate::calendar::CertificateStatus {
                state: "bestätigt".to_string(),
                fingerprint: config.server_certificate_fingerprint(),
                host: config.server_url.clone(),
                detail: None,
            });
        }
        crate::calendar::client::CertificateCheck::Changed { expected, found } => {
            return Ok(crate::calendar::CertificateStatus {
                state: "geändert".to_string(),
                fingerprint: Some(found.fingerprint),
                host: config.server_url.clone(),
                detail: Some(format!(
                    "Bisher bestätigt war {expected}. Wurde das Zertifikat auf der \
                     Instanz erneuert, darf es hier neu angenommen werden."
                )),
            });
        }
        crate::calendar::client::CertificateCheck::NotApplicable => {
            return Ok(crate::calendar::CertificateStatus {
                state: "ohne".to_string(),
                fingerprint: None,
                host: config.server_url.clone(),
                detail: Some(
                    "Die Adresse läuft über HTTP, es wird kein Zertifikat benötigt.".to_string(),
                ),
            });
        }
    };

    settings
        .set_calendar_certificate(&config.server_url, certificate.der.clone())
        .await?;

    Ok(crate::calendar::CertificateStatus {
        state: "bestätigt".to_string(),
        fingerprint: Some(certificate.fingerprint),
        host: config.server_url.clone(),
        detail: None,
    })
}

#[tauri::command]
fn cancel_chat(control: State<'_, ChatControl>) {
    let _ = control.cancel.send(true);
}

#[tauri::command]
fn render_markdown(markdown: String) -> String {
    let mut markdown = markdown;
    truncate_string(&mut markdown, MAX_MARKDOWN_BYTES);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(&markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    let mut cleaner = Builder::new();
    cleaner.rm_tags(["img"]).url_relative(UrlRelative::Deny);
    let mut cleaned = cleaner.clean(&html_output).to_string();
    truncate_string(&mut cleaned, MAX_RENDERED_MARKDOWN_BYTES);
    cleaned
}

#[cfg(target_os = "linux")]
fn is_hyprland_session() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
            desktop
                .split(':')
                .any(|name| name.eq_ignore_ascii_case("hyprland"))
        })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Setzt den Kryptografie-Anbieter von rustls, und zwar genau einmal.
///
/// rustls bringt absichtlich keinen Anbieter mit, und reqwest braucht einen
/// schon beim Bauen *jedes* Clients – auch für reine Klartext-Verbindungen wie
/// die zu Ollama. Deshalb steht das ganz vorn beim Start und nicht erst im
/// Kalenderteil. `ring` statt der Vorgabe, weil es ohne zusätzliche Werkzeuge
/// baut.
pub fn install_crypto_provider() {
    use std::sync::Once;

    static EINMAL: Once = Once::new();
    EINMAL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub fn run() {
    // Muss vor jedem Clientbau stehen, sonst bricht schon die Prüfung des
    // Ollama-Servers beim Start ab.
    install_crypto_provider();

    tauri::Builder::default()
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("ollama.json");
            let settings =
                OllamaSettings::load(config_path.clone()).map_err(std::io::Error::other)?;
            // Das App-Passwort gehört in den Sitzungszustand; ein auf der Platte
            // abgelegtes wird beim Start wieder eingelesen, damit die Anmeldung
            // nach einem Neustart nicht neu gemacht werden muss.
            let session = crate::calendar::CalendarSession::default();
            if settings.restore_calendar_session(&session) {
                app.emit(
                    "kalender-angemeldet",
                    "Ein gespeichertes App-Passwort wurde wiederhergestellt",
                )
                .ok();
            }

            app.manage(session);
            app.manage(settings);
            app.manage(ChatControl::new());

            // Der Schreibmodus kennt die Konfigurationsdatei, um sie selbst
            // vor dem Agentenmodus zu schützen.
            let agent_state = AgentState::default();
            if let Ok(mut guarded) = agent_state.config_path.lock() {
                *guarded = config_path;
            }
            app.manage(agent_state);

            #[cfg(target_os = "linux")]
            if is_hyprland_session() {
                if let Some(window) = app.get_webview_window("main") {
                    window.set_decorations(false)?;
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_chat_message,
            cancel_chat,
            get_models,
            check_server,
            get_ssh_config,
            set_ssh_config,
            start_ollama_via_ssh,
            get_server_url,
            get_provider,
            set_provider,
            get_einrichtung,
            set_server_url,
            get_agent_config,
            set_agent_config,
            get_chat_config,
            prepare_attachment,
            set_chat_config,
            get_calendar_config,
            set_calendar_config,
            calendar_status,
            calendar_login,
            calendar_logout,
            calendar_calendars,
            calendar_event_open,
            calendar_event_save,
            calendar_events,
            calendar_certificate,
            calendar_trust_certificate,
            load_chat_history,
            save_chat_history,
            delete_chat_history,
            list_tools,
            execute_tool,
            preview_tool_call,
            undo_write,
            set_write_enabled,
            get_write_state,
            reset_write_budget,
            render_markdown
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der Tauri-Anwendung");
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::create_askpass_files;
    use super::ChatMessage;
    use super::{
        agent_system_prompt,
        agent_tool_schemas,
        agent_toolset_for,
        apply_write_mode,
        auswahl_aus_argumenten,
        build_start_ollama_command,
        calendar_change_schema,
        calendar_event_prompt,
        calendar_event_schema,
        canonical_root,
        classify_ssh_failure,
        connection_error_message,
        create_ssh_command,
        describe_http_error,
        execute_calendar_change,
        execute_calendar_event,
        execute_read_only_tool,
        execute_write_tool,
        finish_ssh_command,
        gewaehlte_kalender,
        is_write_tool,
        known_calendars,
        kuerze_fuer_nachricht,
        list_events_schema,
        name_der_aenderung_setzt_die_zeit,
        no_token_timeout_error,
        normalize_agent_root,
        normalize_server_url,
        normalize_ssh_identity_file,
        normalize_ssh_target,
        normalize_system_prompt,
        ollama_client_builder,
        port_of,
        prepare_attachment,
        preview_change,
        preview_event,
        preview_write,
        render_markdown,
        should_retry_chat,
        ssh_failure_message,
        start_ollama_script,
        termine_toolset_for,
        toolset_for,
        undo_last_change,
        validate_agent_max_steps,
        validate_chat_config,
        validate_chat_input,
        validate_chat_message,
        validate_context_tokens,
        validate_history,
        validate_tool_schemas,
        wirksamer_umfang,
        without_thinking,
        AgentConfig,
        AgentState,
        ChatConfig,
        ChatWaitState,
        OllamaConfig,
        OllamaOptions,
        OllamaRequest,
        OllamaSettings,
        OllamaStreamParser,
        Provider,
        Scope,
        SshConfig,
        SshFailure,
        StreamChunk,
        StreamFailure,
        ToolCall,
        ToolOutput,
        LIST_EVENTS_TOOL,
        MAX_AGENT_ROOT_BYTES,
        MAX_AGENT_STEPS_LIMIT,
        MAX_ATTACHMENT_BYTES,
        MAX_CHAT_ATTEMPTS,
        MAX_CONTEXT_TOKENS,
        MAX_ERROR_BODY_BYTES,
        MAX_HISTORY_MESSAGES,
        MAX_MESSAGE_BYTES,
        MAX_SYSTEM_PROMPT_BYTES,
        MAX_THINKING_BYTES,
        MAX_TOOL_CALLS_PER_MESSAGE,
        MAX_TOOL_FILE_BYTES,
        MAX_TOOL_OUTPUT_BYTES,
        MAX_TOOL_SCHEMAS_BYTES,
        MAX_TOOL_SEARCH_MATCHES,
        MAX_TOOL_WRITES_PER_TURN,
        MAX_TOOL_WRITE_TOTAL_BYTES,
        MIN_CONTEXT_TOKENS,
        OLLAMA_NO_TOKEN_LIMIT,
        // Für die Prüfungen am festcodierten Startbefehl.
        OLLAMA_PORT,
        OLLAMA_SERVE_ENV,
        OLLAMA_TOKEN_IDLE_TIMEOUT,
        SSH_COMMAND_TIMEOUT,
        SSH_TRANSPORT_FAILURE_CODE,
    };
    use std::time::{Duration, Instant};

    /// Erzeugt einen ExitStatus, wie ihn `std::process::Command` meldet.
    fn exit_status(code: i32) -> std::process::ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code << 8)
        }
        #[cfg(not(unix))]
        {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code as u32)
        }
    }

    fn ssh_config(identity_file: &str) -> SshConfig {
        SshConfig {
            target: "mimir@localhost".to_string(),
            port: 2222,
            identity_file: identity_file.to_string(),
        }
    }

    fn test_config_path(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mimir-{label}-{}-{unique}.json",
            std::process::id()
        ))
    }

    #[test]
    fn renders_common_markdown() {
        let html = render_markdown(
            "# Überschrift\n\n**Fett** und `Code`\n\n- Punkt\n\n```rust\nfn main() {}\n```"
                .to_string(),
        );

        assert!(html.contains("<h1>Überschrift</h1>"));
        assert!(html.contains("<strong>Fett</strong>"));
        assert!(html.contains("<code>Code</code>"));
        assert!(html.contains("<li>Punkt</li>"));
        assert!(html.contains("fn main() {}"));
    }

    #[test]
    fn sanitizes_unsafe_html_and_links() {
        let html = render_markdown(
            "<script>alert('xss')</script>\n\n[unsafe](javascript:alert(1))".to_string(),
        );

        assert!(!html.contains("<script"));
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn bounds_markdown_input_and_output() {
        let markdown = "a".repeat(super::MAX_MARKDOWN_BYTES + 1);
        let html = render_markdown(markdown);
        assert!(html.len() <= super::MAX_RENDERED_MARKDOWN_BYTES);
    }

    #[test]
    fn normalizes_server_urls() {
        assert_eq!(
            normalize_server_url("192.168.178.42:11434/").unwrap(),
            "http://192.168.178.42:11434"
        );
        assert_eq!(
            normalize_server_url("https://ollama.example.com/").unwrap(),
            "https://ollama.example.com"
        );
    }

    #[test]
    fn rejects_invalid_server_urls() {
        assert!(normalize_server_url("").is_err());
        assert!(normalize_server_url("ftp://ollama.example.com").is_err());
        assert!(normalize_server_url("http://user:pass@ollama.example.com").is_err());
        assert!(normalize_server_url("http://@ollama.example.com").is_err());
        assert!(normalize_server_url("http://ollama.example.com?token=value").is_err());
        assert!(normalize_server_url("http://ollama.example.com:0").is_err());
        assert!(normalize_server_url("http://8.8.8.8:11434").is_err());
        assert!(normalize_server_url("http://169.254.169.254").is_err());
    }

    #[test]
    fn validates_ssh_targets() {
        assert_eq!(
            normalize_ssh_target("mimir@192.168.178.42").unwrap(),
            "mimir@192.168.178.42"
        );
        assert_eq!(normalize_ssh_target("localhost").unwrap(), "localhost");
        assert_eq!(
            normalize_ssh_target("mimir@[2001:db8::1]").unwrap(),
            "mimir@[2001:db8::1]"
        );
        assert!(normalize_ssh_target("").is_err());
        assert!(normalize_ssh_target("-oProxyCommand=bad").is_err());
        assert!(normalize_ssh_target("user@host;rm -rf /").is_err());
        assert!(normalize_ssh_target("user@host:2222").is_err());
        assert!(normalize_ssh_target("user@@host").is_err());
    }

    #[test]
    fn ssh_options_precede_remote_command() {
        let config = ssh_config("");
        let mut command = create_ssh_command();
        command.arg("-o").arg("BatchMode=no");
        let command = finish_ssh_command(command, &config).unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let target_index = args.iter().position(|arg| arg == &config.target).unwrap();
        let command_index = args
            .iter()
            .position(|arg| arg.contains("sh -c") && arg.contains(OLLAMA_SERVE_ENV))
            .unwrap();
        let batch_mode_index = args.iter().position(|arg| arg == "BatchMode=no").unwrap();
        let strict_index = args
            .iter()
            .position(|arg| arg == "StrictHostKeyChecking=yes")
            .unwrap();
        let forward_index = args
            .iter()
            .position(|arg| arg == "ForwardAgent=no")
            .unwrap();
        let clear_index = args
            .iter()
            .position(|arg| arg == "ClearAllForwardings=yes")
            .unwrap();
        let control_index = args
            .iter()
            .position(|arg| arg == "ControlMaster=no")
            .unwrap();

        assert!(batch_mode_index < target_index);
        assert!(strict_index < target_index);
        assert!(forward_index < target_index);
        assert!(clear_index < target_index);
        assert!(control_index < target_index);
        assert!(target_index < command_index);
    }

    #[test]
    fn no_multiplexing_or_keepalive_is_requested() {
        // Die Verbindung besteht nur für einen einzigen Befehl. Ohne diese
        // Abschaltung würde eine Systemkonfiguration die Annahme "einmal
        // anmelden, wiederverwenden" vortäuschen.
        let args = create_ssh_command()
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"ControlMaster=no".to_string()));
        assert!(args.contains(&"ControlPath=none".to_string()));
        assert!(args.contains(&"ServerAliveInterval=0".to_string()));
        assert!(!args.iter().any(|arg| arg.contains("ServerAliveCountMax")));
    }

    #[test]
    fn configured_identity_is_passed_before_the_target() {
        let config = ssh_config("/home/mimir/.ssh/id_ed25519");
        let command = finish_ssh_command(create_ssh_command(), &config).unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        let identity_index = args
            .iter()
            .position(|arg| arg == "/home/mimir/.ssh/id_ed25519")
            .unwrap();
        let dash_i = args.iter().position(|arg| arg == "-i").unwrap();
        let only_index = args
            .iter()
            .position(|arg| arg == "IdentitiesOnly=yes")
            .unwrap();
        let target_index = args.iter().position(|arg| arg == &config.target).unwrap();

        assert!(dash_i < identity_index);
        assert!(identity_index < only_index);
        assert!(only_index < target_index);
    }

    #[test]
    fn without_identity_only_default_identities_are_tried() {
        // Ohne ausdrücklich gesetzten Schlüssel muss der ssh-agent nutzbar
        // bleiben. IdentitiesOnly=yes würde ihn hier aussperren.
        let config = ssh_config("");
        let command = finish_ssh_command(create_ssh_command(), &config).unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(!args.iter().any(|arg| arg == "-i"));
        assert!(!args.iter().any(|arg| arg == "IdentitiesOnly=yes"));
    }

    #[test]
    fn normalizes_ssh_identity_files() {
        assert_eq!(normalize_ssh_identity_file("").unwrap(), "");
        assert_eq!(normalize_ssh_identity_file("   ").unwrap(), "");
        assert_eq!(
            normalize_ssh_identity_file(" /home/mimir/.ssh/id_ed25519 ").unwrap(),
            "/home/mimir/.ssh/id_ed25519"
        );
        assert!(normalize_ssh_identity_file("-i").is_err());
        assert!(normalize_ssh_identity_file("~/.ssh/*").is_err());
        assert!(normalize_ssh_identity_file("~/.ssh/id_ed25519?").is_err());
        assert!(normalize_ssh_identity_file("~/.ssh/[abc]").is_err());
        assert!(normalize_ssh_identity_file("id_ed25519").is_err());
        assert!(normalize_ssh_identity_file("~/.ssh/../../etc/shadow").is_err());
        assert!(normalize_ssh_identity_file("/keys/\nid_ed25519").is_err());
        assert!(normalize_ssh_identity_file("/keys/id\u{7}ed25519").is_err());
        assert!(normalize_ssh_identity_file(&format!("/keys/{}", "a".repeat(4096))).is_err());
    }

    #[test]
    fn expands_tilde_in_ssh_identity_files() {
        let Ok(home) = std::env::var("HOME") else {
            return;
        };
        assert!(!home.is_empty());
        assert_eq!(
            normalize_ssh_identity_file("~/.ssh/id_ed25519").unwrap(),
            format!("{home}/.ssh/id_ed25519")
        );
    }

    #[test]
    fn remote_start_command_binds_to_all_interfaces() {
        // Der Server wird fest mit OLLAMA_HOST="0.0.0.0" und freigegebenen
        // CORS-Origins gestartet; die Adresse wird vom Benutzer eingegeben.
        let command = build_start_ollama_command();
        let script = start_ollama_script();

        assert!(command.contains("sh -c "));
        assert!(command.contains(OLLAMA_SERVE_ENV));
        assert!(script.contains(&format!("env {OLLAMA_SERVE_ENV} \"$ollama_bin\" serve")));
        assert!(!script.contains("@OLLAMA_SERVE_ENV@"));
        assert_eq!(command, build_start_ollama_command());
    }

    #[test]
    fn remote_start_command_is_the_required_invocation() {
        // Der auf dem Server laufende Befehl ist exakt
        // OLLAMA_HOST="0.0.0.0" OLLAMA_ORIGINS="*" ollama serve
        assert_eq!(
            format!("{OLLAMA_SERVE_ENV} ollama serve"),
            r#"OLLAMA_HOST="0.0.0.0" OLLAMA_ORIGINS="*" ollama serve"#
        );
        // Ohne die feste Bindung wäre der Server aus dem LAN nicht erreichbar.
        assert!(OLLAMA_SERVE_ENV.contains("OLLAMA_HOST=\"0.0.0.0\""));
        assert!(OLLAMA_SERVE_ENV.contains("OLLAMA_ORIGINS=\"*\""));
        assert!(!OLLAMA_SERVE_ENV.contains("$"));
    }

    #[test]
    fn remote_start_command_is_posix_sh() {
        let script = start_ollama_script();
        // Bashismen würden unter dash, der Bourne-Shell vieler Systeme, brechen.
        for bashism in [
            "[[",
            "]]",
            "function ",
            "source ",
            "$RANDOM",
            "=~",
            ">&-",
            "`",
            "<(",
        ] {
            assert!(!script.contains(bashism), "Bashism im Skript: {bashism}");
        }
        assert!(script.contains("nohup env "));
        assert!(script.contains("</dev/null"));
        // Nur ein einziger ausführbarer Startvorgang, damit der Start
        // idempotent bleibt.
        assert_eq!(script.matches("nohup env ").count(), 1);
    }

    #[test]
    fn remote_start_command_detects_non_reachable_running_server() {
        // Ein Prozessnamens-Treffer allein genügt nicht: die Desktop-App
        // lauscht auf 127.0.0.1 und wäre aus dem LAN nicht erreichbar.
        let script = start_ollama_script();
        assert!(script.contains("listening_on_all_interfaces"));
        assert!(script.contains("/proc/net/tcp"));
        assert!(script.contains("reachable"));
    }

    #[test]
    fn remote_start_command_uses_the_right_port_in_hex() {
        // /proc/net/tcp fuehrt Ports hexadezimal. 11434 ist 0x2CAA; 0x2C9A
        // waere 11418 und wuerde einen laufenden Server nie erkennen.
        let script = start_ollama_script();
        assert!(script.contains("0.0.0.0:11434"));
        assert!(script.contains("hexadezimal"));
        assert!(script.contains(r#"address[2] == "2CAA""#));
        assert!(!script.contains("2C9A"));
        assert_eq!(format!("{:04X}", OLLAMA_PORT), "2CAA");
        assert_eq!(OLLAMA_PORT, 11434);
        assert!(!script.contains("@OLLAMA_PORT_HEX@"));
        assert!(!script.contains("@OLLAMA_PORT_DEC@"));
    }

    #[test]
    fn classifies_ssh_failures_by_exit_code_first() {
        let transport = exit_status(SSH_TRANSPORT_FAILURE_CODE);
        let remote = exit_status(2);

        assert_eq!(
            classify_ssh_failure("user@host: Permission denied (publickey).", transport),
            SshFailure::AuthenticationRequired
        );
        assert_eq!(
            classify_ssh_failure("user@host: Host key verification failed.", transport),
            SshFailure::HostKeyUntrusted
        );
        assert_eq!(
            classify_ssh_failure("user@host: Too many authentication failures.", transport),
            SshFailure::PublicKeyOverloaded
        );
        assert_eq!(
            classify_ssh_failure(
                "ssh: connect to host 192.168.178.42 port 22: Connection refused",
                transport
            ),
            SshFailure::Unreachable
        );
        assert_eq!(
            classify_ssh_failure(
                "ssh: connect to host ollama port 22: Could not resolve hostname: ollama",
                transport
            ),
            SshFailure::Unreachable
        );
        // Unbekannte 255er-Fehler werden als Authentifizierungsfrage gewertet,
        // damit eine übersetzte Meldung nicht in einer Sackgasse endet.
        assert_eq!(
            classify_ssh_failure("ssh: Vibrierender Fehler", transport),
            SshFailure::AuthenticationRequired
        );
        // Das Skript selbst meldet 2, wenn Ollama falsch gebunden laeuft.
        assert_eq!(
            classify_ssh_failure(
                "Ollama laeuft bereits, lauscht aber nicht auf 0.0.0.0",
                remote
            ),
            SshFailure::RemoteCommand
        );
        assert_eq!(
            classify_ssh_failure("user@host: Permission denied (publickey).", remote),
            SshFailure::RemoteCommand
        );
    }

    #[test]
    fn key_overload_does_not_ask_for_a_password() {
        // "Too many authentication failures" ist ein Publickey-Problem. Als
        // Authentifizierungsfehler gemeldet, öffnete Mimir nutzlos den
        // Passwortdialog.
        let message = ssh_failure_message(
            SshFailure::PublicKeyOverloaded,
            "SSH-Start fehlgeschlagen: Too many authentication failures".to_string(),
        );
        assert!(message.contains("/ssh-key"));
        assert!(!SshFailure::PublicKeyOverloaded.eq(&SshFailure::AuthenticationRequired));
        assert!(ssh_failure_message(
            SshFailure::HostKeyUntrusted,
            "SSH-Start fehlgeschlagen".to_string()
        )
        .contains("known_hosts"));
        assert_eq!(
            ssh_failure_message(
                SshFailure::AuthenticationRequired,
                "SSH-Start fehlgeschlagen".to_string()
            ),
            "SSH-Start fehlgeschlagen"
        );
    }

    #[test]
    fn timeout_message_uses_the_configured_limit() {
        assert_eq!(SSH_COMMAND_TIMEOUT.as_secs(), 30);
        assert!(SSH_COMMAND_TIMEOUT < OLLAMA_NO_TOKEN_LIMIT);
    }

    #[test]
    fn parses_stream_incrementally_and_stops_at_done() {
        let line =
            b"{\"message\":{\"role\":\"assistant\",\"content\":\"\xC3\xA4\"},\"done\":false}";
        let split = line
            .windows(2)
            .position(|window| window == [0xc3, 0xa4])
            .unwrap()
            + 1;
        let mut parser = OllamaStreamParser::new();
        assert!(parser.push(&line[..split]).unwrap().is_empty());
        let mut chunks = parser.push(&line[split..]).unwrap();
        assert!(chunks.is_empty());
        chunks = parser.push(b"\n").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].message.as_ref().unwrap().content, "ä");
        chunks = parser.push(b"{\"done\":true}\n{\"message\":{\"role\":\"assistant\",\"content\":\"ignored\"}}\n").unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].done);
        assert!(parser.is_done());
        assert!(parser.push(b"{\"done\":true}\n").unwrap().is_empty());
    }

    #[test]
    fn rejects_stream_errors_and_invalid_utf8() {
        let mut parser = OllamaStreamParser::new();
        assert!(parser.push(b"{\"error\":\"model failed\"}\n").is_err());
        let mut parser = OllamaStreamParser::new();
        assert!(parser.push(&[0xc3]).is_ok());
        assert!(parser.push(&[0x28]).is_ok());
        assert!(parser.finish().is_err());
    }

    #[test]
    fn validates_prompt_limits() {
        let message = ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            ..Default::default()
        };
        assert!(validate_chat_input("model", std::slice::from_ref(&message)).is_ok());
        assert!(validate_chat_input("", std::slice::from_ref(&message)).is_err());
        let oversized = ChatMessage {
            role: "user".to_string(),
            content: "x".repeat(MAX_MESSAGE_BYTES + 1),
            ..Default::default()
        };
        assert!(validate_chat_input("model", &[oversized]).is_err());
        let history = vec![message; MAX_HISTORY_MESSAGES + 1];
        assert!(validate_chat_input("model", &history).is_err());
    }

    #[test]
    fn reads_reasoning_text_from_the_stream() {
        // Ollama liefert bei Denkmodellen Denktext zusätzlich im message-Objekt.
        // Er muss den Stream passieren, ohne den Antworttext zu verändern.
        let mut parser = OllamaStreamParser::new();
        let chunks = parser
            .push(
                b"{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"thinking\":\"Der Nutzer fragt\"}}\n\
                  {\"message\":{\"role\":\"assistant\",\"content\":\"Vier\",\"thinking\":\"\"}}\n",
            )
            .unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks[0].message.as_ref().unwrap().thinking,
            "Der Nutzer fragt"
        );
        assert_eq!(chunks[0].message.as_ref().unwrap().content, "");
        assert_eq!(chunks[1].message.as_ref().unwrap().content, "Vier");
        // Beide Nachrichten gelten als Fortschritt, sonst würde die Wartezeit
        // während einer langen Denkphase ablaufen.
        assert!(chunks
            .iter()
            .all(|c| c.message.as_ref().unwrap().has_text()));
    }

    #[test]
    fn pure_metadata_chunks_are_no_progress() {
        let mut parser = OllamaStreamParser::new();
        let chunks = parser
            .push(b"{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"thinking\":\"\"}}\n")
            .unwrap();
        assert!(!chunks[0].message.as_ref().unwrap().has_text());
        assert!(StreamChunk {
            content: String::new(),
            thinking: String::new(),
            tool_calls: Vec::new(),
        }
        .is_empty());
    }

    #[test]
    fn stream_chunk_omits_empty_thinking_in_the_payload() {
        // Das Frontend unterscheidet die Felder an ihrer Anwesenheit; ein leerer
        // Denktext darf daher nicht als leeres Feld mitgeschickt werden.
        let json = serde_json::to_string(&StreamChunk {
            content: "Antwort".to_string(),
            thinking: String::new(),
            tool_calls: Vec::new(),
        })
        .unwrap();
        assert_eq!(json, r#"{"content":"Antwort"}"#);

        let json = serde_json::to_string(&StreamChunk {
            content: String::new(),
            thinking: "Gedanke".to_string(),
            tool_calls: Vec::new(),
        })
        .unwrap();
        assert_eq!(json, r#"{"content":"","thinking":"Gedanke"}"#);
    }

    #[test]
    fn thinking_text_is_never_sent_back_to_ollama() {
        // Der Denktext wird nur angezeigt. Ein Folgeauftrag besteht aus Rolle und
        // Antworttext, damit der Kontext nicht um den Denktext wächst.
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "Frage".to_string(),
            thinking: "Gedanke zur Frage".to_string(),
            ..Default::default()
        }];
        let request = OllamaRequest {
            model: "test".to_string(),
            messages: without_thinking(messages),
            stream: true,
            system: None,
            options: None,
            tools: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("thinking"), "Denktext im Request: {json}");
        assert!(json.contains(r#""content":"Frage""#), "{json}");
    }

    #[test]
    fn rejects_oversized_thinking_text() {
        let message = ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            thinking: "x".repeat(MAX_THINKING_BYTES + 1),
            ..Default::default()
        };
        assert!(validate_chat_message(&message).is_err());
        let message = ChatMessage {
            role: "assistant".to_string(),
            content: String::new(),
            thinking: "x".repeat(MAX_THINKING_BYTES),
            ..Default::default()
        };
        assert!(validate_chat_message(&message).is_ok());
    }

    #[tokio::test]
    async fn persists_server_and_ssh_config_atomically() {
        let config_path = test_config_path("persist");
        let settings = OllamaSettings::load(config_path.clone()).unwrap();

        settings.set_base_url("localhost:11434").await.unwrap();
        settings
            .set_ssh_config("mimir@localhost", 2222, "/home/mimir/.ssh/id_ed25519")
            .await
            .unwrap();
        let reloaded = OllamaSettings::load(config_path.clone()).unwrap();
        let config = reloaded.get_config().await;
        assert_eq!(config.server_url, "http://localhost:11434");
        assert_eq!(config.ssh.target, "mimir@localhost");
        assert_eq!(config.ssh.port, 2222);
        assert_eq!(config.ssh.identity_file, "/home/mimir/.ssh/id_ed25519");

        // Ohne identity_file in der Datei muss eine bestehende Konfiguration
        // weiterhin geladen werden.
        let legacy_path = test_config_path("persist-legacy");
        std::fs::write(
            &legacy_path,
            br#"{"server_url":"http://localhost:11434","ssh":{"target":"mimir@localhost","port":22}}"#,
        )
        .unwrap();
        let legacy = OllamaSettings::load(legacy_path.clone()).unwrap();
        assert_eq!(legacy.get_config().await.ssh.identity_file, String::new());
        std::fs::remove_file(legacy_path).unwrap();

        let file_name = config_path.file_name().unwrap().to_string_lossy();
        let temporary_prefix = format!(".{file_name}.");
        let temporary_files = std::fs::read_dir(config_path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&temporary_prefix)
            })
            .count();
        assert_eq!(temporary_files, 0);

        std::fs::remove_file(config_path).unwrap();
    }

    #[tokio::test]
    async fn failed_persistence_does_not_change_memory() {
        let parent_path = test_config_path("not-a-directory");
        std::fs::write(&parent_path, b"file").unwrap();
        let settings = OllamaSettings {
            config: tokio::sync::RwLock::new(OllamaConfig {
                server_url: "http://localhost:11434".to_string(),
                provider: Provider::Remote,
                ssh: SshConfig::default(),
                agent: AgentConfig::default(),
                chat: ChatConfig::default(),
                calendar: crate::calendar::CalendarConfig::default(),
            }),
            config_path: parent_path.join("ollama.json"),
            history_path: parent_path.join("chat-history.json"),
            credential_path: parent_path.join("calendar-secret.json"),
            last_contact: std::sync::Mutex::new(None),
        };
        let original = settings.get_base_url().await;
        assert!(settings.set_base_url("localhost:11435").await.is_err());
        assert_eq!(settings.get_base_url().await, original);
        std::fs::remove_file(parent_path).unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_persisted_ssh_values() {
        let ssh_path = test_config_path("invalid-ssh");
        std::fs::write(
            &ssh_path,
            br#"{"server_url":"http://localhost:11434","ssh":{"target":"user@host;bad","port":22}}"#,
        )
        .unwrap();
        assert!(OllamaSettings::load(ssh_path.clone()).is_err());
        std::fs::remove_file(ssh_path).unwrap();

        let port_path = test_config_path("invalid-port");
        std::fs::write(
            &port_path,
            br#"{"server_url":"http://localhost:11434","ssh":{"target":"user@host","port":0}}"#,
        )
        .unwrap();
        assert!(OllamaSettings::load(port_path.clone()).is_err());
        std::fs::remove_file(port_path).unwrap();

        let identity_path = test_config_path("invalid-identity");
        std::fs::write(
            &identity_path,
            br#"{"server_url":"http://localhost:11434","ssh":{"target":"user@host","port":22,"identity_file":"~/.ssh/*"}}"#,
        )
        .unwrap();
        assert!(OllamaSettings::load(identity_path.clone()).is_err());
        std::fs::remove_file(identity_path).unwrap();
    }

    // --------------------------------------------------------- Agentenmodus
    /// Legt ein Verzeichnis mit Dateien an und liefert seinen Pfad zurück.
    fn test_tree(label: &str) -> std::path::PathBuf {
        let root = test_config_path(label).with_extension("dir");
        std::fs::create_dir_all(root.join("unterordner")).unwrap();
        std::fs::write(root.join("hallo.txt"), "Zeile eins\ngeheim: wert42\n").unwrap();
        std::fs::write(root.join("unterordner/tief.txt"), "tief GEFUNDEN\n").unwrap();
        std::fs::write(root.join("bild.bin"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn reads_only_what_the_allowlist_permits() {
        let root = test_tree("agent-werkzeuge");
        let root = root.to_string_lossy().to_string();

        let listing =
            execute_read_only_tool(&root, "list_directory", &serde_json::json!({})).unwrap();
        assert!(listing.content.contains("hallo.txt"), "{}", listing.content);
        assert!(
            listing.content.contains("unterordner/"),
            "{}",
            listing.content
        );

        let file = execute_read_only_tool(
            &root,
            "read_file",
            &serde_json::json!({ "path": "hallo.txt" }),
        )
        .unwrap();
        assert!(file.content.contains("geheim: wert42"), "{}", file.content);
        assert!(!file.truncated);

        let found = execute_read_only_tool(
            &root,
            "search_files",
            &serde_json::json!({ "pattern": "gefunden" }),
        )
        .unwrap();
        assert!(
            found.content.contains("unterordner/tief.txt:1"),
            "{}",
            found.content
        );

        // Nichts außerhalb der drei Namen wird ausgeführt - auch dann nicht,
        // wenn das Modell einen anderen Namen erfindet.
        for name in [
            "write_file",
            "delete_file",
            "run_command",
            "fetch_url",
            "read_file ",
            "",
            "READ_FILE",
        ] {
            let error = execute_read_only_tool(&root, name, &serde_json::json!({})).unwrap_err();
            assert!(error.contains("Unbekanntes Werkzeug"), "{name}: {error}");
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_every_escape_from_the_working_directory() {
        let root = test_tree("agent-sandbox");
        let root = root.to_string_lossy().to_string();

        for path in [
            "/etc/passwd",
            "/",
            "..",
            "../",
            "../../etc/passwd",
            "unterordner/../../..",
            "./../../a",
            "hallo.txt/../../a",
            "unterordner\\hallo.txt",
        ] {
            let error =
                execute_read_only_tool(&root, "read_file", &serde_json::json!({ "path": path }))
                    .unwrap_err();
            assert!(!error.is_empty(), "Pfad wurde angenommen: {path}");
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_may_not_lead_out_of_the_working_directory() {
        let root = test_tree("agent-symlink");
        let outside = test_config_path("agent-aussen");
        // Eigenes Kennwort, damit die Suche nicht an den Testdaten des
        // Verzeichnisbaums scheitern kann.
        std::fs::write(&outside, "ausserhalb-vertraulich").unwrap();
        let link = root.join("verweis.txt");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let root = root.to_string_lossy().to_string();

        // Der Lesezugriff löst den Link auf und lehnt das Ziel ab.
        let error = execute_read_only_tool(
            &root,
            "read_file",
            &serde_json::json!({ "path": "verweis.txt" }),
        )
        .unwrap_err();
        assert!(error.contains("aus dem Arbeitsverzeichnis"), "{error}");

        // Die Suche überspringt Symlinks vollständig.
        let found = execute_read_only_tool(
            &root,
            "search_files",
            &serde_json::json!({ "pattern": "ausserhalb-vertraulich" }),
        )
        .unwrap();
        assert!(
            found.content.contains("Keine Fundstelle"),
            "{}",
            found.content
        );

        std::fs::remove_file(&outside).ok();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bounds_file_reads_and_search_results() {
        let root = test_tree("agent-grenzen");
        let root = root.to_string_lossy().to_string();

        // Binärdatei wird nicht als Text ausgegeben.
        let error = execute_read_only_tool(
            &root,
            "read_file",
            &serde_json::json!({ "path": "bild.bin" }),
        )
        .unwrap_err();
        assert!(error.contains("keine Textdatei"), "{error}");

        // Zu große Datei wird abgelehnt statt gelesen.
        std::fs::write(
            root_path(&root, "gross.txt"),
            "x".repeat((MAX_TOOL_FILE_BYTES + 1) as usize),
        )
        .unwrap();
        let error = execute_read_only_tool(
            &root,
            "read_file",
            &serde_json::json!({ "path": "gross.txt" }),
        )
        .unwrap_err();
        assert!(error.contains("KiB"), "{error}");

        // Fehlende Argumente werden benannt statt still ignoriert.
        let error = execute_read_only_tool(&root, "read_file", &serde_json::json!({})).unwrap_err();
        assert!(error.contains("\"path\""), "{error}");
        let error = execute_read_only_tool(
            &root,
            "search_files",
            &serde_json::json!({ "pattern": "  " }),
        )
        .unwrap_err();
        assert!(error.contains("leer"), "{error}");

        std::fs::remove_dir_all(&root).ok();
    }

    fn root_path(root: &str, name: &str) -> std::path::PathBuf {
        std::path::Path::new(root).join(name)
    }

    #[test]
    fn search_result_stays_within_its_limit() {
        let root = test_tree("agent-suchlimit");
        // Deutlich mehr Treffer als erlaubt.
        std::fs::write(
            root_path(&root.to_string_lossy(), "viele.txt"),
            "TREFFER\n".repeat(MAX_TOOL_SEARCH_MATCHES + 50),
        )
        .unwrap();

        let root = root.to_string_lossy().to_string();
        let found = execute_read_only_tool(
            &root,
            "search_files",
            &serde_json::json!({ "pattern": "treffer" }),
        )
        .unwrap();

        assert!(found.truncated, "Ergebnis wurde nicht gekürzt");
        assert!(found.content.matches("TREFFER").count() <= MAX_TOOL_SEARCH_MATCHES);
        assert!(found.content.contains("ausgelassen"), "{}", found.content);
        assert!(
            found.content.len() <= MAX_TOOL_OUTPUT_BYTES + 32,
            "{} Bytes",
            found.content.len()
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn requires_a_working_directory() {
        let error =
            execute_read_only_tool("", "read_file", &serde_json::json!({ "path": "a.txt" }))
                .unwrap_err();
        assert!(error.contains("/agent-dir"), "{error}");

        let error = canonical_root("/gibt/es/nicht/xyz").unwrap_err();
        assert!(error.contains("nicht lesbar"), "{error}");
    }

    #[test]
    fn validates_agent_configuration() {
        assert!(validate_agent_max_steps(0).is_err());
        assert!(validate_agent_max_steps(MAX_AGENT_STEPS_LIMIT + 1).is_err());
        assert_eq!(validate_agent_max_steps(4).unwrap(), 4);

        assert!(normalize_agent_root("").is_err());
        assert!(normalize_agent_root("relativ/pfad").is_err());
        assert!(normalize_agent_root("/a/../b").is_err());
        assert!(normalize_agent_root("/a\nb").is_err());
        assert!(normalize_agent_root("/a\\b").is_err());
        assert!(normalize_agent_root(&format!("/{}", "a".repeat(MAX_AGENT_ROOT_BYTES))).is_err());
        assert_eq!(
            normalize_agent_root("/tmp/agenten").unwrap(),
            "/tmp/agenten"
        );
    }

    #[tokio::test]
    async fn persists_the_agent_configuration() {
        let config_path = test_config_path("agent-persist");
        let settings = OllamaSettings::load(config_path.clone()).unwrap();

        let root = test_tree("agent-persist-dir");
        let stored = settings
            .set_agent_config(AgentConfig {
                root: root.to_string_lossy().to_string(),
                max_steps: 5,
                scope: Scope::Agent,
            })
            .await
            .unwrap();
        assert_eq!(stored.max_steps, 5);
        assert_eq!(stored.scope, Scope::Agent);

        let reloaded = OllamaSettings::load(config_path.clone()).unwrap();
        let agent = reloaded.get_config().await.agent;
        assert_eq!(agent.max_steps, 5);
        assert_eq!(agent.root, root.to_string_lossy());
        assert_eq!(agent.scope, Scope::Agent);

        // Der Umfang überlebt einen Neustart und kommt als lesbarer Wert zurück,
        // nicht als Zahlencode: Eine Konfigurationsdatei wird auch von Hand
        // gelesen.
        settings
            .set_agent_config(AgentConfig {
                root: root.to_string_lossy().to_string(),
                max_steps: 5,
                scope: Scope::Termine,
            })
            .await
            .unwrap();
        let text = std::fs::read_to_string(&config_path).unwrap();
        assert!(text.contains("\"scope\": \"termine\""), "{text}");

        let neu = OllamaSettings::load(config_path.clone()).unwrap();
        assert_eq!(neu.get_config().await.agent.scope, Scope::Termine);

        // Ein nicht existierendes Verzeichnis darf nicht gespeichert werden, ein
        // relatives ebenfalls nicht.
        assert!(settings
            .set_agent_config(AgentConfig {
                root: "/gibt/es/nicht/xyz".to_string(),
                max_steps: 5,
                scope: Scope::Agent,
            })
            .await
            .is_err());
        assert!(settings
            .set_agent_config(AgentConfig {
                root: "relativ".to_string(),
                max_steps: 5,
                scope: Scope::Agent,
            })
            .await
            .is_err());
        assert_eq!(settings.get_config().await.agent.max_steps, 5);

        std::fs::remove_file(&config_path).unwrap();
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn toolset_and_system_prompt_mention_the_working_directory() {
        let root = test_tree("agent-prompt");
        let root = canonical_root(&root.to_string_lossy()).unwrap();
        let prompt = agent_system_prompt(&root.to_string_lossy());
        let schemas = agent_tool_schemas();

        assert!(prompt.contains(root.to_string_lossy().as_ref()), "{prompt}");
        assert!(prompt.contains("lesende"), "{prompt}");
        assert_eq!(schemas.len(), 3);
        for schema in &schemas {
            let name = schema["function"]["name"].as_str().unwrap();
            assert!(
                ["list_directory", "read_file", "search_files"].contains(&name),
                "unerwartetes Werkzeug: {name}"
            );
            assert!(schema["function"]["description"].is_string());
            assert!(schema["function"]["parameters"]["type"] == "object");
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reads_tool_calls_from_the_stream() {
        let mut parser = OllamaStreamParser::new();
        let chunks = parser
            .push(
                b"{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":\
                  [{\"function\":{\"name\":\"read_file\",\"arguments\":{\"path\":\"a.txt\"}}}]},\"done\":false}\n",
            )
            .unwrap();
        assert_eq!(chunks.len(), 1);
        let call = &chunks[0].message.as_ref().unwrap().tool_calls[0];
        assert_eq!(call.function.validated().unwrap().0, "read_file");
        assert_eq!(call.function.validated().unwrap().1["path"], "a.txt");

        // Manche Modelle schicken die Argumente als Zeichenkette.
        let mut parser = OllamaStreamParser::new();
        let chunks = parser
            .push(
                b"{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":\
                  [{\"function\":{\"name\":\"search_files\",\"arguments\":\"{\\\"pattern\\\":\\\"x\\\"}\"}}]}}\n",
            )
            .unwrap();
        let (name, arguments) = chunks[0].message.as_ref().unwrap().tool_calls[0]
            .function
            .validated()
            .unwrap();
        assert_eq!(name, "search_files");
        assert_eq!(arguments["pattern"], "x");
    }

    #[test]
    fn rejects_bogus_tool_calls_from_the_model() {
        let mut parser = OllamaStreamParser::new();
        let too_many: Vec<String> = (0..MAX_TOOL_CALLS_PER_MESSAGE + 1)
            .map(|index| {
                format!(
                    "{{\"function\":{{\"name\":\"read_file\",\"arguments\":{{\"path\":\"a{index}.txt\"}}}}}}"
                )
            })
            .collect();
        let line = format!(
            "{{\"message\":{{\"role\":\"assistant\",\"tool_calls\":[{}]}}}}\n",
            too_many.join(",")
        );
        assert!(
            parser.push(line.as_bytes()).is_err(),
            "zu viele Aufrufe wurden akzeptiert"
        );

        for name in ["read-file", "read file", "read/../file", ""] {
            let mut parser = OllamaStreamParser::new();
            let line = format!(
                "{{\"message\":{{\"role\":\"assistant\",\"tool_calls\":[{{\"function\":{{\"name\":\"{name}\",\"arguments\":{{}}}}}}]}}}}\n"
            );
            assert!(
                parser.push(line.as_bytes()).is_err(),
                "Werkzeugname wurde akzeptiert: {name}"
            );
        }
    }

    #[test]
    fn tool_calls_and_results_are_sent_back_but_thinking_is_not() {
        // Anders als der Denktext gehören Werkzeugaufrufe und ihr Ergebnis in
        // den Verlauf, sonst kann das Modell nicht weiterarbeiten.
        let call = ToolCall {
            function: super::ToolCallFunction {
                name: "read_file".to_string(),
                arguments: super::ToolArguments::from_value(serde_json::json!({ "path": "a.txt" })),
            },
        };
        let request = OllamaRequest {
            model: "test".to_string(),
            messages: without_thinking(vec![ChatMessage {
                role: "assistant".to_string(),
                content: String::new(),
                thinking: "Gedanke".to_string(),
                tool_calls: vec![call],
                tool_name: String::new(),
            }]),
            stream: true,
            system: None,
            options: None,
            tools: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("thinking"), "Denktext im Request: {json}");
        assert!(json.contains("read_file"), "{json}");

        let request = OllamaRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "tool".to_string(),
                content: "Inhalt".to_string(),
                tool_name: "read_file".to_string(),
                ..Default::default()
            }],
            stream: true,
            system: None,
            options: None,
            tools: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"tool_name\":\"read_file\""), "{json}");
        assert!(json.contains("\"role\":\"tool\""), "{json}");
    }

    #[test]
    fn bounds_the_tool_schemas_from_the_frontend() {
        assert!(validate_tool_schemas(None).unwrap().is_none());
        assert!(validate_tool_schemas(Some(Vec::new())).unwrap().is_none());
        let two = vec![
            serde_json::json!({"type": "function"}),
            serde_json::json!({}),
        ];
        assert!(validate_tool_schemas(Some(two)).unwrap().is_some());
        let many: Vec<serde_json::Value> = (0..17)
            .map(|_| serde_json::json!({"type": "function"}))
            .collect();
        assert!(validate_tool_schemas(Some(many)).is_err());
        let huge = vec![serde_json::json!({
            "type": "function",
            "function": { "description": "x".repeat(MAX_TOOL_SCHEMAS_BYTES) }
        })];
        assert!(validate_tool_schemas(Some(huge)).is_err());
    }

    #[test]
    fn tool_output_is_bounded_and_marked() {
        let output = ToolOutput::new("x".repeat(MAX_TOOL_OUTPUT_BYTES + 1000), "viel".to_string());
        assert!(output.truncated);
        assert!(output.content.len() <= MAX_TOOL_OUTPUT_BYTES + 32);

        let output = ToolOutput::new("kurz".to_string(), "wenig".to_string());
        assert!(!output.truncated);
        assert_eq!(output.content, "kurz");
    }

    // ------------------------------------------------- Schreibende Werkzeuge
    /// Legt ein Arbeitsverzeichnis mit Agenten-Daten an.
    fn test_write_tree(label: &str) -> std::path::PathBuf {
        let root = test_config_path(label).with_extension("write");
        std::fs::create_dir_all(root.join("unterordner")).unwrap();
        std::fs::write(root.join("notiz.txt"), "erste Zeile\nzweite Zeile\n").unwrap();
        root.canonicalize().unwrap()
    }

    fn write_state(config_path: &std::path::Path) -> AgentState {
        let state = AgentState::default();
        *state.config_path.lock().unwrap() = config_path.to_path_buf();
        state.set_write_enabled(true);
        state
    }

    #[test]
    fn writes_only_when_the_mode_is_enabled() {
        let root = test_write_tree("agent-write-gesperrt");
        let state = write_state(&test_config_path("agent-write-config.json"));
        let arguments = serde_json::json!({ "path": "neu.txt", "content": "x" });

        state.set_write_enabled(false);
        let error = execute_write_tool(&state, &root, "write_file", &arguments).unwrap_err();
        assert!(error.contains("nicht freigeschaltet"), "{error}");
        assert!(
            !root.join("neu.txt").exists(),
            "Es wurde trotzdem geschrieben"
        );

        state.set_write_enabled(true);
        execute_write_tool(&state, &root, "write_file", &arguments).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("neu.txt")).unwrap(), "x");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn create_does_not_overwrite_and_edit_needs_the_file() {
        let root = test_write_tree("agent-write-ueberschreiben");
        let state = write_state(&test_config_path("agent-write-config2.json"));

        execute_write_tool(
            &state,
            &root,
            "write_file",
            &serde_json::json!({ "path": "notiz.txt", "content": "neu" }),
        )
        .unwrap_err();
        assert_eq!(
            std::fs::read_to_string(root.join("notiz.txt")).unwrap(),
            "erste Zeile\nzweite Zeile\n",
            "bestehende Datei wurde überschrieben"
        );

        execute_write_tool(
            &state,
            &root,
            "edit_file",
            &serde_json::json!({ "path": "gibtsnicht.txt", "old_string": "a", "new_string": "b" }),
        )
        .unwrap_err();

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn edit_replaces_exactly_one_occurrence() {
        let root = test_write_tree("agent-write-edit");
        let state = write_state(&test_config_path("agent-write-config3.json"));

        // Zweimal vorhanden: nichts ändern.
        let error = execute_write_tool(
            &state,
            &root,
            "edit_file",
            &serde_json::json!({ "path": "doppelt.txt", "old_string": "Zeile", "new_string": "X" }),
        );
        assert!(error.is_err(), "mehrdeutige Ersetzung wurde ausgeführt");

        std::fs::write(root.join("doppelt.txt"), "Zeile\nZeile\n").unwrap();
        let error = execute_write_tool(
            &state,
            &root,
            "edit_file",
            &serde_json::json!({ "path": "doppelt.txt", "old_string": "Zeile", "new_string": "X" }),
        )
        .unwrap_err();
        assert!(error.contains("mal vor"), "{error}");
        assert_eq!(
            std::fs::read_to_string(root.join("doppelt.txt")).unwrap(),
            "Zeile\nZeile\n"
        );

        // Genau einmal: wird ersetzt.
        execute_write_tool(
            &state,
            &root,
            "edit_file",
            &serde_json::json!({
                "path": "notiz.txt",
                "old_string": "zweite Zeile",
                "new_string": "zweite geänderte Zeile"
            }),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("notiz.txt")).unwrap(),
            "erste Zeile\nzweite geänderte Zeile\n"
        );

        // Wortlaut passt nicht: Fehler, Datei unverändert.
        let error = execute_write_tool(
            &state,
            &root,
            "edit_file",
            &serde_json::json!({
                "path": "notiz.txt",
                "old_string": "dritte Zeile",
                "new_string": "egal"
            }),
        )
        .unwrap_err();
        assert!(error.contains("nicht gefunden"), "{error}");
        assert_eq!(
            std::fs::read_to_string(root.join("notiz.txt")).unwrap(),
            "erste Zeile\nzweite geänderte Zeile\n"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn writes_never_leave_the_working_directory() {
        let root = test_write_tree("agent-write-sandbox");
        let state = write_state(&test_config_path("agent-write-config4.json"));

        for path in [
            "/tmp/agent-ausbruch.txt",
            "../agent-ausbruch.txt",
            "unterordner/../../agent-ausbruch.txt",
        ] {
            let error = execute_write_tool(
                &state,
                &root,
                "write_file",
                &serde_json::json!({ "path": path, "content": "x" }),
            )
            .unwrap_err();
            assert!(!error.is_empty(), "Pfad wurde angenommen: {path}");
        }

        // Ein Symlink als Ziel wird nicht beschrieben.
        #[cfg(unix)]
        {
            let outside = test_config_path("agent-write-aussen");
            std::fs::write(&outside, "unverändert").unwrap();
            std::os::unix::fs::symlink(&outside, root.join("verweis.txt")).unwrap();
            let error = execute_write_tool(
                &state,
                &root,
                "edit_file",
                &serde_json::json!({
                    "path": "verweis.txt",
                    "old_string": "unverändert",
                    "new_string": "geändert"
                }),
            )
            .unwrap_err();
            assert!(error.contains("Symlink"), "{error}");
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "unverändert");
            std::fs::remove_file(&outside).ok();
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn writes_are_atomic_and_leave_no_temporary_files() {
        let root = test_write_tree("agent-write-atomar");
        let state = write_state(&test_config_path("agent-write-config5.json"));

        execute_write_tool(
            &state,
            &root,
            "write_file",
            &serde_json::json!({ "path": "unterordner/neu.txt", "content": "hallo\n" }),
        )
        .unwrap();

        let leftovers: Vec<String> = std::fs::read_dir(root.join("unterordner"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".mimir-write"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporäre Dateien übrig: {leftovers:?}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("unterordner/neu.txt")).unwrap(),
            "hallo\n"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn undo_restores_the_previous_state() {
        let root = test_write_tree("agent-write-undo");
        let state = write_state(&test_config_path("agent-write-config6.json"));

        execute_write_tool(
            &state,
            &root,
            "edit_file",
            &serde_json::json!({
                "path": "notiz.txt",
                "old_string": "erste Zeile",
                "new_string": "geänderte erste Zeile"
            }),
        )
        .unwrap();
        assert!(std::fs::read_to_string(root.join("notiz.txt"))
            .unwrap()
            .contains("geänderte"));

        undo_last_change(&state, &root, "notiz.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("notiz.txt")).unwrap(),
            "erste Zeile\nzweite Zeile\n"
        );

        // Zweites Mal: nichts mehr gespeichert.
        assert!(undo_last_change(&state, &root, "notiz.txt").is_err());

        // Eine neu angelegte Datei verschwindet wieder.
        execute_write_tool(
            &state,
            &root,
            "write_file",
            &serde_json::json!({ "path": "weg.txt", "content": "weg" }),
        )
        .unwrap();
        assert!(root.join("weg.txt").exists());
        undo_last_change(&state, &root, "weg.txt").unwrap();
        assert!(
            !root.join("weg.txt").exists(),
            "neue Datei wurde nicht entfernt"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_timeout_while_connecting_is_not_reported_as_a_dead_server() {
        // Gemeldet war: "operation timed out" beim Verbindungsaufbau, während
        // Rechner und Dienst sehr wohl erreichbar waren. Die alte Meldung
        // sprach von Firewall und nicht erreichbarem Rechner; beides traf nicht
        // zu und schickte auf die falsche Fährte.
        let meldung = connection_error_message(
            "http://192.168.178.42:11434/api/chat",
            "error sending request for url (http://192.168.178.42:11434/api/chat): \
             client error (Connect): operation timed out",
            true,
            3,
        );

        // Der Port steht drin, sonst hilft der Hinweis auf 0.0.0.0 nicht.
        assert!(meldung.contains("11434"), "{meldung}");
        assert!(meldung.contains("OLLAMA_HOST"), "{meldung}");
        assert!(meldung.contains("Funkloch"), "{meldung}");
        assert!(
            meldung.contains("3 Mal versucht"),
            "die Wiederholung fehlt: {meldung}"
        );
        // Und nichts davon behauptet, der Rechner sei unerreichbar.
        assert!(!meldung.contains("nicht erreichbar"), "{meldung}");
        assert!(!meldung.contains("läuft kein Dienst"), "{meldung}");
    }

    #[test]
    fn a_timeout_while_waiting_is_told_apart_from_a_failed_connect() {
        // Der Server hat die Anfrage bekommen und antwortet nicht: Eine andere
        // Ursache, ein anderer nächster Schritt.
        let meldung = connection_error_message(
            "http://192.168.178.42:11434/api/chat",
            "error decoding response body: operation timed out",
            false,
            1,
        );

        assert!(meldung.contains("bekommen, aber nicht"), "{meldung}");
        assert!(!meldung.contains("OLLAMA_HOST"), "{meldung}");
        // Ohne Wiederholung steht auch keine Zahl im Text.
        assert!(!meldung.contains("Mal versucht"), "{meldung}");
    }

    #[test]
    fn the_port_is_read_out_of_the_address() {
        assert_eq!(port_of("http://192.168.178.42:11434/api/chat"), "11434");
        assert_eq!(port_of("https://cloud.example.org:8080/x"), "8080");
        assert_eq!(port_of("http://127.0.0.1/api/chat"), "der angegebenen");
    }

    #[test]
    fn connection_errors_name_a_next_step() {
        // Der rohe englische Text von reqwest sagt nichts darüber, was zu tun
        // ist. Diese Meldungen stehen dem Benutzer im Chat gegenüber.
        let refused = connection_error_message(
            "http://192.168.178.42:11434",
            "error sending request for url (http://192.168.178.42:11434/api/tags): \
             client error (Connect): tcp connect error: Connection refused (os error 111)",
            true,
            1,
        );
        assert!(refused.contains("/server-start"), "{refused}");
        assert!(refused.contains("192.168.178.42:11434"), "{refused}");
        assert!(refused.contains("Technische Angabe"), "{refused}");

        let timed_out = connection_error_message(
            "http://192.168.178.42:11434/api/chat",
            "error sending request for url (http://192.168.178.42:11434/api/chat): \
             client error (Connect): tcp connect error: deadline has elapsed",
            true,
            3,
        );
        assert!(timed_out.contains("/server-status"), "{timed_out}");
        assert!(timed_out.contains("/server-start"), "{timed_out}");
        assert!(
            timed_out.contains("deadline has elapsed"),
            "Technische Angabe fehlt"
        );

        let unreachable = connection_error_message(
            "http://192.168.178.42:11434",
            "error sending request: no route to host",
            true,
            1,
        );
        assert!(unreachable.contains("nicht erreichbar"), "{unreachable}");

        // Und für alles Unbekannte bleibt eine verständliche Grundform.
        let unknown =
            connection_error_message("http://127.0.0.1:11434", "sonstiges Problem", true, 1);
        assert!(
            unknown.starts_with("Die Verbindung zu http://127.0.0.1:11434"),
            "{unknown}"
        );

        // Keine englischen Rohmeldungen mehr in der ersten Zeile.
        for message in [&refused, &timed_out, &unreachable, &unknown] {
            let first_line = message.lines().next().unwrap();
            assert!(
                !first_line.contains("error sending request"),
                "{first_line}"
            );
            assert!(!first_line.contains("tcp connect error"), "{first_line}");
        }
    }

    #[test]
    fn every_ollama_call_shares_one_connection() {
        // Vorher wurde für jede Anfrage ein Client gebaut und damit jedes Mal
        // neu verbunden. Auf einer schwankenden Strecke ist der Handshake der
        // wacklige Teil; eine bestehende Verbindung wäre sofort nutzbar.
        let erster = super::ollama_client().expect("Client 1");
        let zweiter = super::ollama_client().expect("Client 2");

        assert!(
            std::ptr::eq(erster, zweiter),
            "es werden verschiedene Clients gebaut, die Verbindung wird jedes Mal neu aufgebaut"
        );
    }

    #[test]
    fn the_last_contact_is_remembered_for_the_status_line() {
        // Ohne diese Notiz springt die Anzeige bei jedem kurzen Aussetzer auf
        // „Offline“ und bietet einen Neustart an, den niemand braucht.
        let settings = super::test_settings();

        assert_eq!(
            settings.last_contact(),
            None,
            "vor dem ersten Kontakt gibt es nichts"
        );

        settings.mark_contact();
        let vermerkt = settings.last_contact();
        assert!(vermerkt.is_some(), "der Kontakt wurde nicht vermerkt");
        assert!(
            super::now_seconds() - vermerkt.unwrap() < 5,
            "der Zeitstempel stimmt nicht"
        );
    }

    #[test]
    fn the_status_probe_waits_longer_than_three_seconds() {
        // Drei Sekunden galten bei einem beschäftigten Server als Ausfall.
        assert!(super::STATUS_TIMEOUT >= Duration::from_secs(5));
        // Die Anzeige soll aber auch nicht kleben.
        assert!(super::STATUS_TIMEOUT < super::OLLAMA_MODELS_TIMEOUT);
    }

    #[test]
    fn the_calendar_schemas_offer_the_reminder_and_the_category() {
        // Fehlten die beiden Felder im Schema, hätte das Modell sie nicht
        // gefunden und stattdessen behauptet, es hätte sie gesetzt.
        for schema in [
            super::calendar_event_schema(&zwei_kalender()),
            super::calendar_change_schema(&zwei_kalender())
                .into_iter()
                .find(|schema| schema["function"]["name"] == super::calendar::edit::UPDATE_TOOL)
                .expect("das Ändern-Werkzeug fehlt im Angebot"),
        ] {
            let werkzeug = schema["function"]["name"].as_str().unwrap_or_default();

            for feld in ["reminder", "category"] {
                assert!(
                    !schema["function"]["parameters"]["properties"][feld].is_null(),
                    "{werkzeug} bietet {feld} nicht an: {schema}"
                );
            }
        }
    }

    #[test]
    fn the_status_says_whether_the_app_password_was_kept() {
        // Ohne diese Angabe kann die Leiste nicht anzeigen, dass das Passwort
        // gemerkt wurde – es wäre nach der Anmeldung spurlos verschwunden.
        let settings = super::test_settings();
        let path = settings.credential_path.clone();
        std::fs::remove_file(&path).ok();

        assert!(
            !settings.has_stored_calendar_credential("kai"),
            "ohne Datei ist nichts gemerkt"
        );
        assert!(
            !settings.has_stored_calendar_credential(""),
            "ohne Benutzer kann nichts gemerkt sein"
        );

        crate::calendar::write_stored_credential(&path, "kai", "app-passwort").unwrap();
        assert!(settings.has_stored_calendar_credential("kai"));
        assert!(
            !settings.has_stored_calendar_credential("jemand-anderes"),
            "ein Passwort gehört immer zu genau einem Konto"
        );

        crate::calendar::delete_stored_credential(&path).unwrap();
        assert!(!settings.has_stored_calendar_credential("kai"));
    }

    #[test]
    fn the_header_timeout_outlasts_a_busy_machine() {
        // Gemessen: 2,6 s bis zu den Antwortköpfen bei freier Maschine. Ein
        // laufender Übersetzungsvorgang verlängert das um ein Vielfaches, und ein
        // solcher Abbruch ist ein Fehlalarm. Die Grenze muss deutlich über der
        // Verbindungszeit liegen, aber unter der Abbruchgrenze ohne Token.
        assert!(super::OLLAMA_HEADER_TIMEOUT > super::OLLAMA_CHAT_CONNECT_TIMEOUT * 4);
        assert!(super::OLLAMA_HEADER_TIMEOUT < OLLAMA_NO_TOKEN_LIMIT);
        assert!(super::OLLAMA_HEADER_TIMEOUT < OLLAMA_TOKEN_IDLE_TIMEOUT * 2);
        assert!(super::header_timeout_message()
            .contains(&super::OLLAMA_HEADER_TIMEOUT.as_secs().to_string()));
    }

    #[tokio::test]
    async fn the_error_message_contains_what_ollama_said() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        // Ein kleiner HTTP-Server im Test: er schickt genau die Antwort, die
        // Ollama im Fehlerfall liefert, damit der Nachbau nichts erfindet.
        let serve = |status_line: &'static str, body: Vec<u8>, keep_open: bool| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                let mut stream = listener.accept().ok().map(|(client, _)| client);
                let Some(mut client) = stream.take() else {
                    return;
                };

                let mut buffer = [0u8; 2048];
                let _ = client.read(&mut buffer);
                let head = format!(
                    "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = client.write_all(head.as_bytes());
                let _ = client.write_all(&body);

                if keep_open {
                    // Der Körper ist länger als die Grenze: der Rest darf weder
                    // gelesen noch blockiert werden.
                    let _ = client.write_all(&vec![b'x'; MAX_ERROR_BODY_BYTES * 2]);
                    let _ = client.flush();
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            address
        };

        // 1) Ollamas übliches Fehlerformat.
        let address = serve(
            "400 Bad Request",
            br#"{"error":"model \"falsch\" not found, try pulling it first"}"#.to_vec(),
            false,
        );
        let response = ollama_client_builder()
            .build()
            .unwrap()
            .get(format!("http://{address}/api/chat"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let message = describe_http_error(response).await;
        assert!(message.contains("400"), "{message}");
        assert!(
            message.contains("model \"falsch\" not found"),
            "Begründung von Ollama fehlt: {message}"
        );

        // 2) Reines JSON ohne Fehlerfeld: der Rohtext wird genannt.
        let address = serve("404 Not Found", br#"{"detail":"weg"}"#.to_vec(), false);
        let response = ollama_client_builder()
            .build()
            .unwrap()
            .get(format!("http://{address}/api/tags"))
            .send()
            .await
            .unwrap();
        let message = describe_http_error(response).await;
        assert!(message.contains("weg"), "{message}");

        // 3) Kein Körper: wenigstens der Status.
        let address = serve("500 Internal Server Error", Vec::new(), false);
        let response = ollama_client_builder()
            .build()
            .unwrap()
            .get(format!("http://{address}/api/chat"))
            .send()
            .await
            .unwrap();
        let message = describe_http_error(response).await;
        assert_eq!(message, "Ollama-Fehler: Status 500 Internal Server Error");

        // 4) Sehr langer Körper: die Meldung bleibt handhabbar und es hängt
        //    nicht, weil nicht der ganze Rest gelesen wird.
        let address = serve("400 Bad Request", b"viele Details ".repeat(4096), true);
        let response = ollama_client_builder()
            .build()
            .unwrap()
            .get(format!("http://{address}/api/chat"))
            .send()
            .await
            .unwrap();
        let message = describe_http_error(response).await;
        assert!(
            message.len() < MAX_ERROR_BODY_BYTES + 200,
            "Meldung zu lang: {}",
            message.len()
        );
        assert!(message.contains("400"), "{message}");
    }

    #[test]
    fn validates_system_prompt_and_context_size() {
        assert_eq!(normalize_system_prompt("  ").unwrap(), "");
        assert_eq!(
            normalize_system_prompt(" Antworte auf Deutsch ").unwrap(),
            "Antworte auf Deutsch"
        );
        assert!(
            normalize_system_prompt("a\nb\tc").is_ok(),
            "Zeilenumbrüche müssen erlaubt sein"
        );
        assert!(normalize_system_prompt("mit\u{7}Steuerzeichen").is_err());
        assert!(normalize_system_prompt(&"a".repeat(MAX_SYSTEM_PROMPT_BYTES + 1)).is_err());

        // 0 heißt: Vorgabe des Modells.
        assert_eq!(validate_context_tokens(0).unwrap(), 0);
        assert!(validate_context_tokens(1).is_err());
        assert!(validate_context_tokens(MIN_CONTEXT_TOKENS - 1).is_err());
        assert!(validate_context_tokens(MAX_CONTEXT_TOKENS + 1).is_err());
        // Auf ein Vielfaches von 256 gerundet, damit der Wert zur Größe passt.
        assert_eq!(validate_context_tokens(5000).unwrap(), 4864);
        assert_eq!(validate_context_tokens(8192).unwrap(), 8192);

        let config = validate_chat_config(ChatConfig {
            system_prompt: "Antworte kurz".to_string(),
            context_tokens: 9000,
            save_history: true,
        })
        .unwrap();
        assert_eq!(config.context_tokens, 8960);
        assert!(config.save_history);
        assert_eq!(config.context_option().unwrap().num_ctx, 8960);
        assert!(ChatConfig::default().context_option().is_none());
    }

    #[test]
    fn the_system_prompt_goes_into_its_own_field() {
        // Es darf keine Nachricht im Verlauf werden: Sonst tauchte sie in jedem
        // Folgeauftrag erneut als Teil des Chatverlaufs auf.
        let request = OllamaRequest {
            model: "test".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hallo".to_string(),
                ..Default::default()
            }],
            stream: true,
            system: Some("Antworte auf Deutsch".to_string()),
            tools: None,
            options: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(
            json.contains(r#""system":"Antworte auf Deutsch""#),
            "{json}"
        );

        // Ohne Anweisung bleibt das Feld weg.
        let request = OllamaRequest {
            model: "test".to_string(),
            messages: Vec::new(),
            stream: true,
            system: None,
            tools: None,
            options: Some(OllamaOptions { num_ctx: 8192 }),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("system"), "{json}");
        assert!(json.contains(r#""options":{"num_ctx":8192}"#), "{json}");
    }

    #[tokio::test]
    async fn the_history_is_only_written_with_consent() {
        let config_path = test_config_path("verlauf");
        std::fs::remove_file(config_path.parent().unwrap().join("chat-history.json")).ok();
        let settings = OllamaSettings::load(config_path.clone()).unwrap();
        let history = vec![ChatMessage {
            role: "user".to_string(),
            content: "geheime Frage".to_string(),
            ..Default::default()
        }];

        // Ohne Freischaltung entsteht keine Datei und nichts wird gelesen.
        settings.persist_history(&history).await.unwrap();
        assert!(
            !settings.history_path.exists(),
            "Verlauf ohne Freischaltung geschrieben"
        );
        assert!(
            settings.load_history().await.is_none(),
            "Verlauf ohne Freischaltung gelesen"
        );

        // Nach der Freischaltung wird er geschrieben und wieder gelesen.
        settings
            .set_chat_config(ChatConfig {
                save_history: true,
                ..Default::default()
            })
            .await
            .unwrap();
        settings.persist_history(&history).await.unwrap();
        assert!(settings.history_path.exists());
        let loaded = settings.load_history().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "geheime Frage");

        // Rechte: nur der Benutzer selbst.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&settings.history_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "Verlauf ist zu weit lesbar: {mode:o}");
        }

        // Werkzeugreste gehören nicht in die gespeicherte Unterhaltung.
        let with_tool = vec![
            history[0].clone(),
            ChatMessage {
                role: "assistant".to_string(),
                content: String::new(),
                tool_name: String::new(),
                ..Default::default()
            },
            ChatMessage {
                role: "tool".to_string(),
                content: "Inhalt einer Datei".to_string(),
                tool_name: "read_file".to_string(),
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Antwort".to_string(),
                ..Default::default()
            },
        ];
        settings.persist_history(&with_tool).await.unwrap();
        let loaded = settings.load_history().await.unwrap();
        assert!(
            !loaded.iter().any(|message| message.role == "tool"),
            "Werkzeugergebnis gespeichert: {loaded:?}"
        );
        assert_eq!(loaded.len(), 2, "unerwarteter Verlauf: {loaded:?}");
        assert_eq!(loaded[1].content, "Antwort");

        // Löschen geht immer.
        settings.delete_history().unwrap();
        assert!(!settings.history_path.exists());

        std::fs::remove_file(&config_path).ok();
    }

    #[test]
    fn a_manipulated_history_file_is_refused() {
        let too_big = ChatMessage {
            role: "user".to_string(),
            content: "x".repeat(MAX_MESSAGE_BYTES + 1),
            ..Default::default()
        };
        let small = ChatMessage {
            role: "user".to_string(),
            content: "ok".to_string(),
            ..Default::default()
        };
        let messages = vec![&too_big, &small];
        assert!(
            validate_history(&messages).is_err(),
            "zu große Nachricht wurde akzeptiert"
        );

        let too_many: Vec<&ChatMessage> = (0..=MAX_HISTORY_MESSAGES).map(|_| &small).collect();
        assert!(
            validate_history(&too_many).is_err(),
            "zu viele Nachrichten wurden akzeptiert"
        );

        let valid: Vec<&ChatMessage> = vec![&small];
        assert!(validate_history(&valid).is_ok());
    }

    #[tokio::test]
    async fn an_incomplete_calendar_entry_does_not_prevent_the_start() {
        // Nach dem Abbruch einer Anmeldung kann die Konfiguration eine Adresse
        // ohne Benutzernamen enthalten. Das darf den Start nicht abwürgen, sonst
        // startet die Anwendung nicht mehr und lässt sich nur von Hand
        // reparieren.
        let config_path = test_config_path("start-mit-angebrochenem-kalender");
        std::fs::remove_file(&config_path).ok();
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            r#"{
                "server_url": "http://192.168.178.42:11434",
                "calendar": {
                    "server_url": "https://cloud.example.org/remote.php/dav/principals/users/benutzer",
                    "username": ""
                }
            }"#,
        )
        .unwrap();

        let settings = OllamaSettings::load(config_path.clone())
            .expect("eine angebrochene Kalendereintragung hat den Start verhindert");
        let calendar = settings.get_config().await.calendar;

        // Die Adresse wird auf die Instanz zurückgeführt und der Name steckt im
        // kopierten Pfad, ist aber noch nicht eingetragen.
        assert_eq!(calendar.server_url, "https://cloud.example.org");
        assert!(calendar.username.is_empty());
        assert_eq!(
            calendar.dav_root().unwrap(),
            "https://cloud.example.org/remote.php/dav"
        );

        // Auch eine völlig unbrauchbare Adresse darf den Start nicht verhindern.
        std::fs::write(
            &config_path,
            r#"{ "server_url": "http://192.168.178.42:11434", "calendar": { "server_url": "ftp://quatsch" } }"#,
        )
        .unwrap();
        let settings = OllamaSettings::load(config_path.clone())
            .expect("eine unbrauchbare Adresse hat den Start verhindert");
        assert!(settings.get_config().await.calendar.server_url.is_empty());

        // Und ein kaputter Benutzername wird verworfen statt abgelehnt.
        std::fs::write(
            &config_path,
            r#"{ "server_url": "http://192.168.178.42:11434",
                "calendar": { "server_url": "https://cloud.example.org", "username": "../../admin" } }"#,
        )
        .unwrap();
        let settings = OllamaSettings::load(config_path.clone())
            .expect("ein kaputter Benutzername hat den Start verhindert");
        let calendar = settings.get_config().await.calendar;
        assert_eq!(calendar.server_url, "https://cloud.example.org");
        assert!(calendar.username.is_empty());

        std::fs::remove_file(&config_path).ok();
    }

    #[tokio::test]
    async fn the_certificate_can_be_accepted_before_any_username_exists() {
        // Ablauf beim ersten Anmelden über https: Adresse und App-Passwort
        // stehen im Fenster, gespeichert ist noch nichts. Das Zertifikat wird
        // geprüft, bevor das Fenster abgeschickt wird, und muss sich dabei
        // speichern lassen. Über die normale Konfiguration ginge das nicht, weil
        // dort ein Benutzername verlangt wird.
        let config_path = test_config_path("zertifikat-ohne-benutzer");
        std::fs::remove_file(&config_path).ok();
        let settings = OllamaSettings::load(config_path.clone()).unwrap();

        settings
            .set_calendar_certificate("https://cloud.example.org", "MIIBadenFqADAg".to_string())
            .await
            .expect("das Zertifikat ließ sich ohne Benutzernamen nicht merken");

        let config = settings.get_config().await.calendar;
        assert_eq!(config.server_url, "https://cloud.example.org");
        assert!(
            config.uses_tls(),
            "die Adresse sollte als https erkannt sein"
        );
        assert_eq!(config.server_certificate.as_deref(), Some("MIIBadenFqADAg"));
        // Der Benutzername bleibt leer, bis die Anmeldung ihn nachträgt.
        assert!(config.username.is_empty());

        // Und die Auswahl der Kalender ist unberührt geblieben.
        settings
            .set_calendar_config(crate::calendar::CalendarConfig {
                server_url: "https://cloud.example.org".to_string(),
                username: "kai".to_string(),
                calendars: vec!["persoenlich".to_string()],
                server_certificate: config.server_certificate.clone(),
            })
            .await
            .expect("die Konfiguration mit Benutzername wurde abgelehnt");

        let config = settings.get_config().await.calendar;
        assert_eq!(config.username, "kai");
        assert_eq!(config.calendars, vec!["persoenlich".to_string()]);
        assert_eq!(config.server_certificate.as_deref(), Some("MIIBadenFqADAg"));
        assert!(config.server_certificate_fingerprint().is_some());

        std::fs::remove_file(&config_path).ok();
    }

    #[tokio::test]
    async fn a_client_can_be_built_on_a_worker_thread() {
        // Der Absturz ereignete sich in einem Tokio-Worker und nicht im
        // Hauptthread: Der Client wird dort gebaut. Deshalb wird hier genau das
        // nachgestellt.
        crate::install_crypto_provider();
        let ergebnis = tokio::spawn(async {
            super::ollama_client().is_ok() && crate::calendar::client::build_client(true).is_ok()
        })
        .await
        .expect("der Worker ist abgestürzt");

        assert!(ergebnis, "der Clientbau auf dem Worker ist gescheitert");
    }

    #[test]
    fn every_client_can_be_built_without_a_network() {
        // Ohne den Kryptografie-Anbieter bricht reqwest schon beim Bauen jedes
        // Clients ab, auch für reine Klartext-Verbindungen. Das ist passiert,
        // nachdem der TLS-Stapel für den Kalender dazukam, und hat die ganze
        // Anwendung beim Start mitgenommen.
        crate::install_crypto_provider();
        assert!(super::build_ollama_chat_client().is_ok());
        // Und der gemeinsame Client, aus dem jede Anfrage ihre Verbindung holt.
        assert!(super::ollama_client().is_ok());
        assert!(
            crate::calendar::client::build_client(false).is_ok(),
            "der Kalender-Client ohne TLS ließ sich nicht bauen"
        );
        assert!(
            crate::calendar::client::build_client(true).is_ok(),
            "der Kalender-Client mit TLS ließ sich nicht bauen"
        );
        // Und zweimal: Der Anbieter darf beim zweiten Mal nicht meckern.
        crate::install_crypto_provider();
    }

    fn kalender_konfig() -> crate::calendar::CalendarConfig {
        crate::calendar::CalendarConfig {
            server_url: "https://cloud.example.org".to_string(),
            username: "kai".to_string(),
            calendars: vec!["/remote.php/dav/calendars/kai/persoenlich/".to_string()],
            server_certificate: None,
        }
    }

    fn angemeldet() -> crate::calendar::CalendarSession {
        let session = crate::calendar::CalendarSession::default();
        session.set_password("geheim").expect("Passwort");
        session.set_calendars(
            vec![crate::calendar::client::CalendarInfo {
                href: "/remote.php/dav/calendars/kai/persoenlich/".to_string(),
                display_name: "Persönlich".to_string(),
                ctag: "ctag-1".to_string(),
                color: "#2D55FFAA".to_string(),
            }],
            Some("35.0.0.10".to_string()),
        );
        session
    }

    #[test]
    fn ein_termin_aendern_und_loeschen_sind_ebenfalls_schreibvorgaenge() {
        // Beides verändert den Kalender des Benutzers und braucht dieselbe
        // Freigabe wie eine Datei.
        assert!(is_write_tool(crate::calendar::edit::UPDATE_TOOL));
        assert!(is_write_tool(crate::calendar::edit::DELETE_TOOL));
    }

    #[test]
    fn die_werkzeuge_fuer_die_kalenderlinie_werden_angeboten() {
        let root = std::env::temp_dir();

        // Anmelden genügt fürs Lesen, Schreiben braucht beides.
        let nur_lesen = agent_toolset_for(&root, false, &zwei_kalender());
        let namen_lesen = namen(&nur_lesen.tools);
        assert!(
            namen_lesen.contains(&LIST_EVENTS_TOOL.to_string()),
            "{namen_lesen:?}"
        );
        assert!(!namen_lesen.contains(&crate::calendar::edit::UPDATE_TOOL.to_string()));
        assert!(!namen_lesen.contains(&crate::calendar::edit::DELETE_TOOL.to_string()));

        let mit_schreiben = agent_toolset_for(&root, true, &zwei_kalender());
        let namen_schreiben = namen(&mit_schreiben.tools);
        assert!(namen_schreiben.contains(&crate::calendar::edit::UPDATE_TOOL.to_string()));
        assert!(namen_schreiben.contains(&crate::calendar::edit::DELETE_TOOL.to_string()));
        // Und das Modell erfährt, dass es nicht rückgängig machen kann.
        assert!(
            mit_schreiben
                .system_prompt
                .contains("nicht rückgängig zu machen"),
            "{}",
            mit_schreiben.system_prompt
        );
        assert!(
            mit_schreiben.system_prompt.contains("Serientermine"),
            "{}",
            mit_schreiben.system_prompt
        );
    }

    #[test]
    fn ohne_anmeldung_gibt_es_auch_das_lesen_nicht() {
        let root = std::env::temp_dir();
        let ohne = agent_toolset_for(&root, true, &[]);

        assert!(!namen(&ohne.tools).contains(&LIST_EVENTS_TOOL.to_string()));
    }

    #[test]
    fn das_loeschwerkzeug_beschreibt_seine_grenzen() {
        // Die Grenzen stehen in der Beschreibung, die das Modell liest: Es soll
        // gar nicht erst danach scheitern.
        // Zwei Schemata; deshalb wird der Text aus allen zusammengesetzt.
        let text = calendar_change_schema(&zwei_kalender())
            .iter()
            .map(|schema| schema.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        assert!(text.contains(crate::calendar::edit::DELETE_TOOL), "{text}");
        assert!(text.contains("Serientermine"), "{text}");
        assert!(text.contains("Teilnehmern"), "{text}");
        assert!(text.contains("endgültig"), "{text}");
        // Und das Ändern sagt, dass nicht Genanntes stehen bleibt.
        assert!(text.contains("bleibt stehen"), "{text}");
    }

    /// Ein Termin, wie Nextcloud ihn schreibt: mit Zeitzonenbezug, Erinnerung,
    /// Kategorie und Beschreibung. Dient nur als Datenbasis für die Prüfung am
    /// Fenster.
    const TERMIN_MIT_ALLEN: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n",
        "UID:termin-1@nextcloud\r\nDTSTAMP:20260901T120000Z\r\nSEQUENCE:3\r\n",
        "DTSTART;TZID=Europe/Berlin:20261005T090000\r\n",
        "DTEND;TZID=Europe/Berlin:20261005T100000\r\n",
        "SUMMARY:Teammeeting\r\nLOCATION:Raum 2\r\nDESCRIPTION:Kurze Vorbereitung\r\n",
        "CATEGORIES:Arbeit\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\n",
        "END:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    #[test]
    fn das_fenster_baut_dieselben_argumente_wie_das_werkzeug() {
        // Zwei Wege zum Ändern eines Termins dürfen nicht auseinanderlaufen: Das
        // Fenster schickt den Kalender als Pfad, das Werkzeug den Namen, weil der
        // Benutzer spricht. Alles andere muss gleich sein.
        let aenderung = super::TerminAenderung {
            uid: "termin-1@nextcloud".to_string(),
            calendar_href: "/calendars/kai/arbeit/".to_string(),
            summary: "Zahnarzt".to_string(),
            start: "2026-10-05T09:00".to_string(),
            end: String::new(),
            all_day: false,
            location: "Praxis".to_string(),
            description: String::new(),
            categories: "Gesundheit".to_string(),
            reminder: "15 Minuten vorher".to_string(),
        };
        let argumente = aenderung.als_argumente();

        assert_eq!(argumente["uid"], "termin-1@nextcloud");
        assert_eq!(argumente["calendar"], "/calendars/kai/arbeit/");
        assert_eq!(argumente["summary"], "Zahnarzt");
        assert_eq!(argumente["reminder"], "15 Minuten vorher");
        assert_eq!(argumente["category"], "Gesundheit");
        // Ohne eigenes Ende bleibt die bisherige Dauer stehen – dafür steht hier
        // `null` und nicht ein leerer Text.
        assert!(argumente["end"].is_null(), "{argumente}");
        // Ein geleertes Feld entfernt den Inhalt; `null` würde ihn stehen lassen.
        assert_eq!(argumente["description"], "", "{argumente}");

        // Und das Werkzeug nimmt dieselben Argumente an.
        let plan = crate::calendar::edit::plan_update(
            &Default::default(),
            &[("/calendars/kai/arbeit/".to_string(), "Arbeit".to_string())],
            &argumente,
            TERMIN_MIT_ALLEN,
            "termin-1@nextcloud",
        );

        assert!(plan.is_ok(), "{:?}", plan.err());
    }

    #[test]
    fn die_liste_nennt_erinnerung_und_kategorien_mit() {
        // Sonst kennt das Modell sie nicht und legt beim Ändern eine erfundene
        // an – genau das ist im Betrieb passiert.
        let mut termin = crate::calendar::CalendarEvent {
            uid: "a@mimir".to_string(),
            summary: "Zahnarzt".to_string(),
            location: String::new(),
            start: 0,
            end: 0,
            all_day: false,
            floating: false,
            calendar: "Persönlich".to_string(),
            calendar_href: "/calendars/kai/persoenlich/".to_string(),
            reminder: Some(15),
            reminder_text: "Erinnerung 15 Minuten vorher".to_string(),
            categories: vec!["Gesundheit".to_string()],
            canceled: false,
        };

        let text = super::merkmale_text(&termin);
        assert!(text.contains("Erinnerung 15 Minuten vorher"), "{text}");
        assert!(text.contains("Kategorie Gesundheit"), "{text}");

        // Ohne beides bleibt die Zeile unverändert – kein „, “ ohne Inhalt.
        termin.reminder = None;
        termin.reminder_text = String::new();
        termin.categories.clear();
        assert_eq!(super::merkmale_text(&termin), "");
    }

    #[tokio::test]
    async fn ein_termin_wird_ohne_freigabe_weder_geaendert_noch_geloescht() {
        let state = AgentState::default();
        let session = angemeldet();

        for name in [
            crate::calendar::edit::UPDATE_TOOL,
            crate::calendar::edit::DELETE_TOOL,
        ] {
            let fehler = execute_calendar_change(
                &state,
                &OllamaSettings::load(std::env::temp_dir().join("mimir-test-agent.json"))
                    .expect("Einstellungen für den Test"),
                &session,
                name,
                &serde_json::json!({"uid": "termin-1@nextcloud"}),
            )
            .await
            .expect_err("ohne Freigabe wird nichts angefasst");

            assert!(fehler.contains("nicht freigeschaltet"), "{fehler}");
        }
    }

    #[test]
    fn die_vorschau_eines_loeschvorgangs_zeigt_was_verschwindet() {
        let vorher = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\n\
UID:t1\r\nDTSTAMP:20260901T120000Z\r\nDTSTART:20260920T070000Z\r\n\
DTEND:20260920T073000Z\r\nSUMMARY:Teammeeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let vorschau = preview_change(vorher, "", "Termin wird gelöscht.", "Arbeit");

        // Der alte Inhalt steht im Fenster, der neue ist leer: so steht im
        // Fenster genau das, was verschwindet.
        assert_eq!(vorschau.current.as_deref(), Some(vorher));
        assert_eq!(vorschau.next, "");
        assert_eq!(vorschau.relative_path, "Kalender Arbeit");
        assert!(vorschau.lines_before > 5, "{}", vorschau.lines_before);
    }

    #[test]
    fn die_vorschau_eines_aenderns_zeigt_alt_und_neu() {
        let vorher = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:t1\r\n\
DTSTART:20260920T070000Z\r\nDTEND:20260920T073000Z\r\n\
SUMMARY:Teammeeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let nachher = vorher.replace("Teammeeting", "Teammeeting Montag");
        let vorschau = preview_change(vorher, &nachher, "Titel wird geändert.", "Arbeit");

        assert!(vorschau
            .current
            .unwrap()
            .contains("SUMMARY:Teammeeting\r\n"));
        assert!(vorschau.next.contains("SUMMARY:Teammeeting Montag"));
        assert_eq!(vorschau.summary, "Titel wird geändert.");
    }

    #[test]
    fn a_termin_is_a_write_that_needs_confirmation() {
        // Das Werkzeug schreibt in den Kalender des Benutzers. Es gehört
        // deshalb hinter dieselbe Freigabe wie eine Datei.
        assert!(is_write_tool(crate::calendar::write::EVENT_TOOL));
        assert!(!is_write_tool("read_file"));
    }

    #[test]
    fn the_termin_tool_only_shows_up_with_a_calendar() {
        let root = std::env::temp_dir();

        // Ohne Anmeldung bekommt das Modell das Werkzeug nicht: Es würde es
        // ankündigen und an jedem Aufruf scheitern.
        let ohne = agent_toolset_for(&root, true, &[]);
        assert!(!namen(&ohne.tools).contains(&crate::calendar::write::EVENT_TOOL.to_string()));
        assert!(!ohne.system_prompt.contains("create_calendar_event"));

        let mit = agent_toolset_for(&root, true, &zwei_kalender());
        assert!(namen(&mit.tools).contains(&crate::calendar::write::EVENT_TOOL.to_string()));
        assert!(mit.system_prompt.contains("create_calendar_event"));
        assert!(mit.system_prompt.contains("nicht rückgängig zu machen"));
    }

    #[test]
    fn the_termin_tool_needs_both_the_calendar_and_the_write_switch() {
        let root = std::env::temp_dir();

        // Kalender da, Schreiben aus: nur die lesenden Werkzeuge.
        assert!(
            !namen(&agent_toolset_for(&root, false, &zwei_kalender()).tools)
                .contains(&crate::calendar::write::EVENT_TOOL.to_string())
        );
    }

    #[test]
    fn the_model_is_pointed_at_the_date_in_the_system_field() {
        // Das heutige Datum steht in **jeder** Anfrage im Systemfeld, nicht in
        // dieser Anweisung. Als es hier noch einmal stand, standen zwei Angaben
        // nebeneinander, und welche galt, war nicht mehr entscheidbar.
        let anweisung = calendar_event_prompt();

        assert!(anweisung.contains("HEUTE IST"), "{anweisung}");
        assert!(
            !anweisung.contains(&chrono::Local::now().format("%d.%m.%Y").to_string()),
            "hier steht noch ein eigener Zeitstempel: {anweisung}"
        );
    }

    /// Die Anleitung erscheint genau dann, wenn noch nichts eingetragen ist.
    ///
    /// Sie wird nach dem Speichern der Adresse nicht mehr gezeigt. Ohne diese
    /// Prüfung bliebe sie nach dem Einrichten stehen und wäre ein Text, den man
    /// wegscrollen muss, obwohl alles fertig ist.
    #[tokio::test]
    async fn die_anleitung_erscheint_nur_vor_der_einrichtung() {
        use super::Einrichtung;

        fn stand(provider: Provider, server_url: &str, kalender: &str) -> Einrichtung {
            let normalisiert = normalize_server_url(server_url).expect("gültige Adresse");

            Einrichtung {
                server_eingetragen: !provider.erlaubt_dateizugriff()
                    || normalisiert
                        != normalize_server_url(super::DEFAULT_OLLAMA_BASE_URL).expect("Vorgabe"),
                server_vorgabe: super::DEFAULT_OLLAMA_BASE_URL.to_string(),
                kalender_eingetragen: !kalender.trim().is_empty(),
            }
        }

        let fern = Provider::Remote;
        // Ohne jede Eingabe: Der Vorgabewert steht noch, die Anleitung gehört hin.
        assert!(!stand(fern, super::DEFAULT_OLLAMA_BASE_URL, "").server_eingetragen);
        assert!(!stand(fern, "localhost:11434", "").server_eingetragen);
        // Beide Schreibweisen desselben Servers zählen als eingerichtet.
        assert!(!stand(fern, "http://localhost:11434/", "").server_eingetragen);
        // Eine eigene Adresse zählt.
        assert!(stand(fern, "ollama.example.org:11434", "").server_eingetragen);
        // Eine Adresse im LAN. `192.0.2.10` wäre nicht zulässig – das ist
        // TEST-NET-1 und damit gerade keine private Adresse, und Mimir lässt nur
        // loopback, unspezifiziert und privat zu.
        assert!(stand(fern, "http://192.168.1.50:11434", "").server_eingetragen);
        // Und eine Ablehnung, weil sie ins Netz zeigt.
        assert!(normalize_server_url("http://192.0.2.10:11434").is_err());
        // Der Kalender ändert daran nichts.
        assert!(
            !stand(
                fern,
                super::DEFAULT_OLLAMA_BASE_URL,
                "https://cloud.example.org"
            )
            .server_eingetragen
        );

        // Lokal braucht keine eingetragene Adresse und wird deshalb nicht mit einer
        // Anleitung begrüßt, die nach einer Adresse fragt, die es dort nicht gibt.
        // Das ist der ganze Grund für den Provider.
        assert!(stand(Provider::Local, super::DEFAULT_OLLAMA_BASE_URL, "").server_eingetragen);
        // Und eine eingetragene Adresse ändert daran nichts: Sie wird lokal nicht
        // benutzt, also ist auch nichts zu beanstanden.
        assert!(stand(Provider::Local, "ollama.example.org:11434", "").server_eingetragen);
    }

    /// Ein Hostname ist erlaubt, eine Adresse ins Internet nicht.
    ///
    /// Für die Anleitung wichtig: Sie nennt `ollama.example.org` als Beispiel. Das
    /// muss auch tatsächlich annehmbar sein, sonst zeigt der erste Schritt in den
    /// Fehler. Die Prüfung greift nur bei einer **IP-Adresse**; ein Name wird
    /// aufgelöst und nicht geprüft.
    #[test]
    fn ein_hostname_wird_angenommen_und_ein_fremder_server_nicht() {
        // Der Name aus der Anleitung.
        assert_eq!(
            normalize_server_url("ollama.example.org:11434").unwrap(),
            "http://ollama.example.org:11434"
        );
        assert_eq!(
            normalize_server_url("https://cloud.example.org").unwrap(),
            "https://cloud.example.org"
        );
        // Eine Adresse, die nicht loopback und nicht privat ist.
        assert!(normalize_server_url("http://8.8.8.8:11434").is_err());
        assert!(normalize_server_url("http://192.0.2.10:11434").is_err());
        // Loopback und privat dagegen.
        assert!(normalize_server_url("http://127.0.0.1:11434").is_ok());
        assert!(normalize_server_url("http://192.168.1.50:11434").is_ok());
    }

    /// Der Kalender ist ein Hinweis, kein Erfordernis.
    ///
    /// Er gehört in die Anleitung, damit der Benutzer weiß, dass es ihn gibt –
    /// die Anleitung darf aber nicht an ihm hängen, sonst bleibt sie bei einem
    /// Benutzer ohne Kalender für immer stehen.
    #[tokio::test]
    async fn der_kalender_blockiert_die_einrichtung_nicht() {
        use super::Einrichtung;

        let ohne = Einrichtung {
            server_eingetragen: false,
            server_vorgabe: "x".to_string(),
            kalender_eingetragen: false,
        };
        let mit = Einrichtung {
            server_eingetragen: false,
            server_vorgabe: "x".to_string(),
            kalender_eingetragen: true,
        };

        // In beiden Fällen wird die Anleitung gebraucht; der Kalender ändert nur
        // den Text, nicht die Entscheidung.
        assert!(!ohne.server_eingetragen);
        assert!(!mit.server_eingetragen);
    }

    #[test]
    fn der_vorgabewert_zeigt_auf_diesen_rechner() {
        // Keine fremde Adresse als Vorgabe. Der Wert landet als Zeichenkette im
        // ausgelieferten Binary und wird beim ersten Start in die Konfiguration
        // geschrieben: Mit einer Adresse des Entwicklers würde jeder, der Mimir
        // bekommt, auf dessen Server starten – und `strings` zeigt sie jedem.
        // Geprüft wird zusätzlich über `src/tests/binary-pruefen.mjs` am
        // Artefakt, denn im Quelltext genügt es nicht.
        const VORGABE: &str = super::DEFAULT_OLLAMA_BASE_URL;

        assert_eq!(VORGABE, "http://localhost:11434");
        assert!(!VORGABE.contains("192.168."), "{VORGABE}");
        assert!(!VORGABE.contains("10."), "{VORGABE}");
    }

    #[test]
    fn die_antwort_sagt_in_welcher_woche_der_zeitraum_liegt() {
        // Der Befund aus dem Betrieb: Der 4. Oktober 2026 ist ein Sonntag, und
        // das Modell nannte ihn die „übermächste Woche“. Er liegt in der laufenden,
        // die genau an ihm endet. Aus dem Datum allein geht das nicht hervor –
        // ein Sonntag ohne Jahr passt auf jede Woche.
        let heute = chrono::Local::now().date_naive();
        use chrono::Datelike;
        let montag = heute - chrono::Duration::days(heute.weekday().num_days_from_monday() as i64);
        let sonntag = montag + chrono::Duration::days(6);
        let vergangener_sonntag = sonntag - chrono::Duration::days(7);

        // Sonntag, mit dem die Unterhaltung begann: gehört in die laufende Woche.
        assert!(
            super::wochenzuordnung(&sonntag, &sonntag).contains("laufende Woche"),
            "{}",
            super::wochenzuordnung(&sonntag, &sonntag)
        );
        // Montag derselben Woche ebenso.
        assert!(
            super::wochenzuordnung(&montag, &montag).contains("laufende Woche"),
            "{}",
            super::wochenzuordnung(&montag, &montag)
        );
        // Der Montag danach ist die nächste, der übernächste Montag die
        // übernächste – und **nicht** mehr die laufende.
        assert!(
            super::wochenzuordnung(
                &(montag + chrono::Duration::days(7)),
                &(montag + chrono::Duration::days(7))
            )
            .contains("nächste Woche"),
            "eine Woche später ist nicht mehr die laufende"
        );
        assert!(
            super::wochenzuordnung(
                &(montag + chrono::Duration::days(14)),
                &(montag + chrono::Duration::days(14))
            )
            .contains("übernächste Woche"),
            "zwei Wochen später ist die übernächste"
        );
        // Der vorige Sonntag gehört in die letzte Woche, nicht in die laufende.
        assert!(
            super::wochenzuordnung(&vergangener_sonntag, &vergangener_sonntag)
                .contains("letzte Woche"),
            "{}",
            super::wochenzuordnung(&vergangener_sonntag, &vergangener_sonntag)
        );
    }

    #[test]
    fn die_antwort_nennt_den_abgefragten_zeitraum() {
        // Ohne den Zeitraum in der Antwort entstand „in diesem Zeitraum von
        // übermorgen keine Termine“ – ohne Datum, und ohne die Möglichkeit zu
        // erkennen, dass etwas anderes abgefragt wurde als gemeint war.
        let schema = list_events_schema();
        let text = schema.to_string();

        assert!(
            text.contains("JJJJ-MM-TT") && text.contains("übermorgen"),
            "das Schema nennt weder Datumsgrenzen noch Worte: {schema}"
        );
        assert!(
            text.contains("montag"),
            "die Wochentage fehlen im Schema: {schema}"
        );
    }

    #[test]
    fn the_read_tool_takes_a_calendar_date_not_a_day_count() {
        // Der gemeldete Fehler: „Termine von morgen“ lieferte den heutigen Tag.
        // Der Zeitraum war eine Zahl Tage ab jetzt, deren frühester Start „jetzt
        // minus ein Tag" ist – den morgigen Tag konnte man damit nicht treffen.
        let schema = list_events_schema();
        let eigenschaften = &schema["function"]["parameters"]["properties"];

        assert!(eigenschaften.get("from").is_some(), "from fehlt: {schema}");
        assert!(eigenschaften.get("to").is_some(), "to fehlt: {schema}");
        assert!(
            !eigenschaften["range"]["description"]
                .as_str()
                .unwrap_or_default()
                .contains("morgen"),
            "range wird noch für „morgen“ angeboten: {schema}"
        );
    }

    #[test]
    fn the_termin_tool_is_described_without_fabricated_examples() {
        let schema = calendar_event_schema(&zwei_kalender());
        let text = schema.to_string();

        assert!(text.contains("create_calendar_event"));
        assert!(text.contains("summary"));
        assert!(text.contains("start"));
        // Keine Teilnehmer: Mimir lädt niemanden ein.
        assert!(!text.contains("attendees"), "{text}");
    }

    fn namen(tools: &[serde_json::Value]) -> Vec<String> {
        tools
            .iter()
            .filter_map(|tool| tool.get("function")?.get("name")?.as_str())
            .map(|name| name.to_string())
            .collect()
    }

    #[tokio::test]
    async fn a_termin_is_not_written_while_writing_is_switched_off() {
        let state = AgentState::default();
        let session = angemeldet();

        let fehler = execute_calendar_event(
            &state,
            &kalender_konfig(),
            &session,
            &serde_json::json!({"summary": "Zahnarzt", "start": "morgen 09:00"}),
            Scope::Agent,
            "",
        )
        .await
        .expect_err("ohne Freigabe wird nichts geschrieben");

        assert!(fehler.contains("nicht freigeschaltet"), "{fehler}");
        // Der Text nennt den Umfang, damit im Terminumfang niemand einen Befehl
        // sucht, der dort gar nicht nötig ist.
        assert!(fehler.contains("Agentenmodus"), "{fehler}");
    }

    #[tokio::test]
    async fn a_termin_needs_a_login() {
        let state = AgentState::default();
        state.set_write_enabled(true);
        let session = crate::calendar::CalendarSession::default();

        let fehler = execute_calendar_event(
            &state,
            &kalender_konfig(),
            &session,
            &serde_json::json!({"summary": "Zahnarzt", "start": "morgen 09:00"}),
            Scope::Agent,
            "",
        )
        .await
        .expect_err("ohne Anmeldung gibt es keine Adresse");

        assert!(fehler.contains("angemeldet"), "{fehler}");
    }

    #[tokio::test]
    async fn the_termin_scope_writes_without_the_write_switch() {
        // Der Umfang ist die Freischaltung: Ohne /agent-write wird im
        // Terminumfang trotzdem geschrieben, und der Aufruf scheitert erst an
        // der fehlenden Anmeldung – das ist der nächste Fehler, nicht der
        // Schreibschalter.
        let state = AgentState::default();
        let session = crate::calendar::CalendarSession::default();

        let fehler = execute_calendar_event(
            &state,
            &kalender_konfig(),
            &session,
            &serde_json::json!({"summary": "Zahnarzt", "start": "morgen 09:00"}),
            Scope::Termine,
            "",
        )
        .await
        .expect_err("ohne Anmeldung gibt es keine Adresse");

        assert!(!fehler.contains("nicht freigeschaltet"), "{fehler}");
        assert!(fehler.contains("angemeldet"), "{fehler}");
    }

    #[tokio::test]
    async fn a_broken_request_costs_nothing() {
        // Ein Aufruf, den die Planung ablehnt, darf das Budget nicht belasten:
        // Sonst könnte das Modell das Budget ausschöpfen, ohne etwas zu
        // erreichen.
        let state = AgentState::default();
        state.set_write_enabled(true);
        let session = angemeldet();

        for schlecht in [
            serde_json::json!({"start": "2026-09-14T09:00"}),
            serde_json::json!({"summary": "A", "start": "keine Zeit"}),
            serde_json::json!({"summary": "A", "start": "2023-10-05T14:00"}),
            serde_json::json!({"summary": "A", "start": "2026-09-14T09:00", "attendees": ["a@b.de"]}),
        ] {
            let _ = execute_calendar_event(
                &state,
                &kalender_konfig(),
                &session,
                &schlecht,
                Scope::Agent,
                "",
            )
            .await;
        }

        // Ohne Server bleibt die Prüfung des Zertifikats als Fehlerquelle; das
        // Budget ist trotzdem unberührt.
        state.charge_write(1).expect("das Budget ist noch frei");
    }

    #[test]
    fn the_preview_shows_what_will_be_written() {
        let session = angemeldet();
        let plan = crate::calendar::write::plan_event(
            &kalender_konfig(),
            &known_calendars(&session),
            &serde_json::json!({
                "summary": "Zahnarzt",
                "start": format!("{}T09:00", (chrono::Local::now() + chrono::Duration::days(7)).format("%Y-%m-%d")),
                "location": "Praxis Dr. Klein"
            }),
            "",
        )
        .expect("der Plan muss sich bilden lassen");

        let vorschau = preview_event(&plan);

        assert_eq!(vorschau.relative_path, "Kalender Persönlich");
        assert!(
            vorschau.current.is_none(),
            "Ein neuer Termin hat keinen Vorgänger"
        );
        assert_eq!(
            vorschau.next, plan.ics,
            "Die Vorschau zeigt den echten Inhalt"
        );
        assert!(vorschau.summary.contains("Zahnarzt"));
        assert!(vorschau.next.contains("LOCATION:Praxis Dr. Klein"));
        assert!(vorschau.lines_after > 5, "Die Vorschau zählt die Zeilen");
    }

    #[test]
    fn the_reported_bug_chain_takes_effect_end_to_end() {
        // Der gemeldete Ablauf: Schreibmodus einschalten, das Modell um eine
        // Änderung bitten. Vorher blieb der Schreibmodus im Backend aus, die
        // Oberfläche zeigte ihn aber an, und das Modell bekam keine Werkzeuge.
        let root = test_write_tree("agent-kette");
        let root_string = root.to_string_lossy().to_string();
        let state = AgentState::default();
        let canonical = canonical_root(&root_string).unwrap();

        // 1. Ausgangszustand: nur lesende Werkzeuge, Schreiben abgelehnt.
        assert_eq!(
            agent_toolset_for(&canonical, state.write_enabled(), &[])
                .tools
                .len(),
            3
        );
        let denied = execute_write_tool(
            &state,
            &canonical,
            "write_file",
            &serde_json::json!({ "path": "neu.txt", "content": "x" }),
        )
        .unwrap_err();
        assert!(denied.contains("nicht freigeschaltet"), "{denied}");

        // 2. Einschalten und nachsehen, ob es wirklich wirkt.
        assert!(apply_write_mode(&state, true));
        assert!(state.write_enabled());
        assert_eq!(
            agent_toolset_for(&canonical, state.write_enabled(), &[])
                .tools
                .len(),
            5
        );

        // 3. Dasselbe Arbeitsverzeichnis freischalten, damit der Schutz greift.
        *state.config_path.lock().unwrap() = test_config_path("agent-kette-config.json");

        // 4. Die Änderung, um die das Modell gebeten wurde, ist jetzt möglich.
        std::fs::write(
            root.join("projekt.md"),
            "# Testprojekt\n\nZeile eins\nZeile zwei\n",
        )
        .unwrap();
        let edit = serde_json::json!({
            "path": "projekt.md",
            "old_string": "Zeile zwei",
            "new_string": "Zeile zwei, geändert"
        });
        let preview = preview_write(&canonical, "edit_file", &edit).unwrap();
        assert!(preview.current.as_ref().unwrap().contains("Zeile zwei"));

        execute_write_tool(&state, &canonical, "edit_file", &edit).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("projekt.md")).unwrap(),
            "# Testprojekt\n\nZeile eins\nZeile zwei, geändert\n"
        );

        // 5. Und wieder abschalten: Das Angebot verschwindet wieder.
        assert!(!apply_write_mode(&state, false));
        assert_eq!(
            agent_toolset_for(&canonical, state.write_enabled(), &[])
                .tools
                .len(),
            3
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn enabling_the_write_mode_actually_takes_effect() {
        // Dieser Pfad wird von der Oberfläche angezeigt: Sie übernimmt den
        // Rückgabewert. Ein Fehler hier hieß bisher, dass die Anzeige
        // "Schreiben: an" zeigte, das Modell aber keine Werkzeuge bekam.
        let state = AgentState::default();
        assert!(
            !state.write_enabled(),
            "Schreibmodus ist ab Werk eingeschaltet"
        );

        assert!(apply_write_mode(&state, true), "Einschalten wirkte nicht");
        assert!(
            state.write_enabled(),
            "Zustand im Backend ist nicht gesetzt"
        );

        assert!(!apply_write_mode(&state, false), "Ausschalten wirkte nicht");
        assert!(
            !state.write_enabled(),
            "Zustand im Backend ist nicht zurückgesetzt"
        );
    }

    #[test]
    fn write_tools_are_offered_only_when_enabled() {
        let root = test_tree("agent-werkzeugliste");
        let root = canonical_root(&root.to_string_lossy()).unwrap();

        let read_only = agent_toolset_for(&root, false, &[]);
        assert_eq!(
            read_only.tools.len(),
            3,
            "schreibende Werkzeuge ohne Freischaltung"
        );
        // Der Standardsatz "es gibt keine schreibenden Werkzeuge" bleibt
        // erhalten; gemeint ist nur, dass keine der Werkzeugnamen auftaucht.
        assert!(
            !read_only.system_prompt.contains("write_file"),
            "{}",
            read_only.system_prompt
        );
        assert!(
            !read_only.system_prompt.contains("edit_file"),
            "{}",
            read_only.system_prompt
        );
        assert!(!read_only
            .system_prompt
            .contains("Zusätzlich darfst du schreiben"));

        let with_write = agent_toolset_for(&root, true, &[]);
        assert_eq!(
            with_write.tools.len(),
            5,
            "Schreibwerkzeuge fehlen nach Freischaltung"
        );
        let names: Vec<&str> = with_write
            .tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        for expected in [
            "list_directory",
            "read_file",
            "search_files",
            "write_file",
            "edit_file",
        ] {
            assert!(names.contains(&expected), "fehlt: {expected} ({names:?})");
        }
        assert!(with_write.system_prompt.contains("write_file"));
        assert!(with_write.system_prompt.contains("Jeder Schreibvorgang"));
        // Und weiterhin keine Lösch- oder Ausführungsmöglichkeit.
        for forbidden in ["delete", "run_command", "remove_file"] {
            assert!(
                !names.contains(&forbidden),
                "unerwartetes Werkzeug: {forbidden}"
            );
        }

        std::fs::remove_dir_all(&root).ok();
    }

    /// Zwei ausgewählte Kalender, wie sie aus der Anmeldung kommen.
    fn zwei_kalender() -> Vec<String> {
        vec!["Persönlich".to_string(), "Arbeit".to_string()]
    }

    #[test]
    fn the_termin_scope_offers_exactly_the_calendar_tools() {
        let toolset =
            termine_toolset_for(&zwei_kalender()).expect("mit Anmeldung gibt es Werkzeuge");

        let names: Vec<&str> = toolset
            .tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        for expected in [
            LIST_EVENTS_TOOL,
            crate::calendar::write::EVENT_TOOL,
            crate::calendar::edit::UPDATE_TOOL,
            crate::calendar::edit::DELETE_TOOL,
        ] {
            assert!(names.contains(&expected), "fehlt: {expected} ({names:?})");
        }

        // Ohne Dateiwerkzeuge, an keiner Stelle: Nicht in der Liste und nicht im
        // Prompt. Ein Schalter, der sie nur abschaltet, ließe die Anweisung
        // stehen und damit das Modell glauben, es gäbe sie doch.
        for verboten in ["write_file", "edit_file", "read_file", "list_directory"] {
            assert!(
                !names.contains(&verboten),
                "unerwartetes Werkzeug: {verboten}"
            );
            assert!(
                !toolset.system_prompt.contains(verboten),
                "{}",
                toolset.system_prompt
            );
        }

        // Auch das Arbeitsverzeichnis kommt nicht vor: Es gäbe in diesem Umfang
        // nichts, wofür es eine Grenze wäre.
        for pfad_stueck in ["Arbeitsverzeichnis", "/tmp/", "read_file"] {
            assert!(
                !toolset.system_prompt.contains(pfad_stueck),
                "{}",
                toolset.system_prompt
            );
        }

        // Die Kalenderanweisungen sind beide drin, samt der Werkzeugnamen.
        assert!(
            toolset
                .system_prompt
                .contains(crate::calendar::write::EVENT_TOOL),
            "{}",
            toolset.system_prompt
        );
        assert!(
            toolset
                .system_prompt
                .contains(crate::calendar::edit::UPDATE_TOOL),
            "{}",
            toolset.system_prompt
        );
    }

    /// Die Kalendernamen gehören in die Anweisung.
    ///
    /// Aus dem ersten Validierungslauf: Das Schéma sagt nur, `calendar` sei „Name
    /// des Zielkalenders, wenn mehrere ausgewählt sind" – welche Kalender das sind,
    /// stand nirgends. Das Modell riet daraufhin „Arbeitskalender" und „WorkCalendar",
    /// und `waehle_kalender` lehnte zu Recht ab, weil es absichtlich exakt vergleicht.
    #[test]
    fn the_prompt_names_the_selected_calendars() {
        let toolset = termine_toolset_for(&zwei_kalender()).expect("mit Kalendern");
        let prompt = &toolset.system_prompt;

        for name in ["Persönlich", "Arbeit"] {
            assert!(
                prompt.contains(name),
                "„{name}“ fehlt in der Anweisung:\n{prompt}"
            );
        }

        // Das Modell soll wissen, dass ein genannter Name übernommen wird – und
        // dass es bei einem **nicht** genannten nachfragen soll, statt zu raten.
        assert!(
            prompt.contains("**nur** ein, wenn der Benutzer selbst einen Kalender genannt hat")
                && prompt.contains("frag ihn, in welchen Kalender es soll"),
            "die Anweisung sagt nicht, wann ein Name hingesetzt wird:\n{prompt}"
        );

        // Die Namen stehen auch im Schéma, und zwar im Feld, das gefüllt werden
        // soll. Nach dem zweiten Lauf war das der Unterschied: Im Prompt genannt hat
        // das Modell sie noch weggelassen; am Feld selbst nicht mehr.
        let schema = toolset.tools[1]["function"]["parameters"]["properties"]["calendar"]
            ["description"]
            .as_str()
            .expect("das Feld calendar hat eine Beschreibung");
        for name in ["Persönlich", "Arbeit"] {
            assert!(
                schema.contains(name),
                "„{name}“ fehlt in der Feldbeschreibung: {schema}"
            );
        }
        // Und sie sagt ausdrücklich, dass ein leerer Wert richtig ist. Das ist
        // der Punkt: Solange die Felder „Pflicht" waren und der Prompt zum
        // Füllen aufforderte, erfand das Modell einen Namen.
        assert!(
            schema.contains("leer lassen") && schema.contains("frag"),
            "die Feldbeschreibung erlaubt kein leeres Feld: {schema}"
        );
        assert!(
            !schema.contains("Pflicht"),
            "die Feldbeschreibung macht den Kalender wieder zur Pflicht: {schema}"
        );

        // Und der Prompt sagt dasselbe, statt zum Füllen aufzufordern.
        assert!(
            prompt.contains("frag ihn, in welchen Kalender"),
            "der Prompt sagt nicht, dass nach dem Kalender gefragt wird:\n{prompt}"
        );
        assert!(
            !prompt.contains("Nenn für jeden Termin"),
            "der Prompt fordert weiterhin zum Füllen auf:\n{prompt}"
        );

        // Bei einem einzigen Kalender genügt die Nennung nicht als Pflicht: Dann
        // trägt das Werkzeug selbst ein, und ein erzwungener Name wäre eine
        // erfundene Möglichkeit.
        let einer = termine_toolset_for(&["Persönlich".to_string()]).expect("ein Kalender");
        assert!(
            einer.system_prompt.contains("Lass `calendar` leer"),
            "{}",
            einer.system_prompt
        );
        assert!(
            !einer.system_prompt.contains("Nenn für jeden Termin"),
            "{}",
            einer.system_prompt
        );
    }

    /// Die Anweisung nennt die **ausgewählten** Kalender, nicht alle vorhandenen.
    ///
    /// `waehle_kalender` sucht nur unter den ausgewählten. Ein Name aus dem übrigen
    /// Bestand im Prompt wäre eine Wahl, die das Werkzeug ablehnt.
    #[test]
    fn only_the_selected_calendars_are_named() {
        let alle = [
            ("privat", "Privat"),
            ("arbeit", "Arbeit"),
            ("familie", "Familie"),
        ];

        let session = crate::calendar::CalendarSession::default();
        session.set_calendars(
            alle.iter()
                .map(|(href, name)| crate::calendar::client::CalendarInfo {
                    href: href.to_string(),
                    display_name: name.to_string(),
                    ctag: String::new(),
                    color: String::new(),
                })
                .collect(),
            None,
        );

        let mut config = crate::calendar::CalendarConfig {
            server_url: "https://kalender.example.org".to_string(),
            username: "benutzer".to_string(),
            ..Default::default()
        };

        // Ohne Auswahl zählt, was da ist.
        assert_eq!(
            gewaehlte_kalender(&config, &session),
            vec![
                "Privat".to_string(),
                "Arbeit".to_string(),
                "Familie".to_string()
            ]
        );

        config.calendars = vec!["privat".to_string(), "arbeit".to_string()];
        assert_eq!(
            gewaehlte_kalender(&config, &session),
            vec!["Privat".to_string(), "Arbeit".to_string()]
        );

        // Ein Pfad ohne Namen im Sitzungszustand fällt weg: Er ließe sich dem Modell
        // nicht nennen, also nennt die Anweisung ihn auch nicht.
        let namenlos = crate::calendar::CalendarSession::default();
        namenlos.set_calendars(
            vec![crate::calendar::client::CalendarInfo {
                href: "privat".to_string(),
                display_name: String::new(),
                ctag: String::new(),
                color: String::new(),
            }],
            None,
        );
        config.calendars = vec!["privat".to_string()];
        assert!(gewaehlte_kalender(&config, &namenlos).is_empty());
    }

    #[test]
    fn the_termin_scope_needs_a_calendar_login() {
        // Ohne Anmeldung gäbe es kein einziges Werkzeug: Das Modell würde nichts
        // ankündigen und nichts erreichen. Eine leere Liste wäre die schlechtere
        // Antwort, weil sie aussieht, als gäbe es nichts zu tun.
        let Err(fehler) = termine_toolset_for(&[]) else {
            panic!("ohne Anmeldung gibt es nichts");
        };
        assert!(fehler.contains("/calendar"), "{fehler}");

        let agent = AgentConfig {
            scope: Scope::Termine,
            ..Default::default()
        };
        // Und über den Umfang dasselbe – auch ohne Arbeitsverzeichnis, das hier
        // gar nicht gebraucht wird.
        let Err(ueber_umfang) = toolset_for(&agent, Scope::Termine, false, &[]) else {
            panic!("ohne Anmeldung gibt es nichts");
        };
        assert!(ueber_umfang.contains("/calendar"), "{ueber_umfang}");
    }

    #[test]
    fn the_agent_scope_still_needs_a_working_directory() {
        let ohne_root = AgentConfig {
            scope: Scope::Agent,
            ..AgentConfig {
                root: String::new(),
                max_steps: 8,
                scope: Scope::Agent,
            }
        };
        let Err(fehler) = toolset_for(&ohne_root, Scope::Agent, false, &[]) else {
            panic!("ohne Verzeichnis gibt es kein Werkzeugangebot");
        };
        assert!(fehler.contains("/agent-dir"), "{fehler}");

        // Mit Verzeichnis läuft der Agentenpfad unverändert weiter.
        let root = test_tree("scope-agent-dir");
        let agent = AgentConfig {
            root: root.to_string_lossy().to_string(),
            ..Default::default()
        };
        let toolset = toolset_for(&agent, Scope::Agent, false, &[]).expect("mit Verzeichnis");
        assert_eq!(toolset.tools.len(), 3);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_scope_that_is_misspelled_stops_the_start_up() {
        // Ein Tippfehler von Hand darf nicht stillschweigend als breiter
        // Umfang gelesen werden. Der Umfang steuert, ob das Modell Dateiwerkzeuge
        // bekommt; ein stilles Zurückfallen auf den Vorgabeumfang wäre eine
        // Rechteausweitung ohne den Benutzer. Deshalb bricht das Laden hier ab
        // und nennt die beiden erlaubten Werte.
        let fehler = serde_json::from_str::<OllamaConfig>(
            r#"{"server_url": "http://localhost:11434", "agent": {"scope": "gibtesnicht"}}"#,
        )
        .expect_err("ein unbekannter Umfang darf nicht durchrutschen");
        let text = fehler.to_string();
        assert!(text.contains("agent"), "{text}");
        assert!(text.contains("termine"), "{text}");

        // Und über `load` ist der Fehler als Lesefehler erkennbar, nicht als
        // irgendein anderer Zustand.
        let config_path = test_config_path("scope-typo.json");
        std::fs::write(
            &config_path,
            r#"{"server_url": "http://localhost:11434", "agent": {"scope": "gibtesnicht"}}"#,
        )
        .unwrap();
        let Err(fehler) = OllamaSettings::load(config_path.clone()) else {
            panic!("ein unbekannter Umfang muss den Start abbrechen");
        };
        assert!(fehler.contains("Konfiguration"), "{fehler}");
        std::fs::remove_file(&config_path).ok();
    }

    #[test]
    fn the_scope_can_never_widen_the_access() {
        // Ein gespeichertes Arbeitsverzeichnis wird im Terminumfang nicht
        // ausgewertet, weil es dort nichts zu lesen gibt: Das Werkzeugangebot
        // entsteht, ohne den Pfad zu prüfen oder zu nennen.
        let termine = AgentConfig {
            root: "/gibt/es/nicht/xyz".to_string(),
            scope: Scope::Termine,
            ..Default::default()
        };
        let toolset = toolset_for(&termine, Scope::Termine, false, &zwei_kalender())
            .expect("der Pfad wird nicht gebraucht");
        assert!(
            !toolset.system_prompt.contains("xyz"),
            "{}",
            toolset.system_prompt
        );

        // Und im anderen Umfang gilt der Pfad weiterhin, samt seiner Prüfung.
        let agent = AgentConfig {
            root: "/gibt/es/nicht/xyz".to_string(),
            scope: Scope::Agent,
            ..Default::default()
        };
        let Err(fehler) = toolset_for(&agent, Scope::Agent, false, &zwei_kalender()) else {
            panic!("der Pfad wird im Agentenmodus geprüft");
        };
        assert!(fehler.contains("Arbeitsverzeichnis"), "{fehler}");
    }

    /// Ein Provider, der fehlt, ist das entfernte Ollama.
    ///
    /// Jede bis jetzt geschriebene Konfiguration hat kein Provider-Feld. Ohne
    /// Vorgabe würde sie beim Start entweder abbrechen oder – schlimmer – auf das
    /// lokale Ollama zeigen, also genau das, was der Benutzer nicht eingestellt
    /// hat. Der alte Stand muss sich also von selbst einstellen.
    #[test]
    fn ein_fehlender_provider_bleibt_das_entfernte_ollama() {
        let config: OllamaConfig =
            serde_json::from_str(r#"{"server_url": "http://192.168.1.50:11434"}"#).unwrap();
        assert_eq!(config.provider, Provider::Remote);
        assert_eq!(
            config.provider.adresse(&config.server_url),
            "http://192.168.1.50:11434"
        );

        // Und auch als Zeichenkette, wie es in der Datei steht.
        let gespeichert = serde_json::to_string(&config).unwrap();
        assert!(
            gespeichert.contains("\"provider\":\"remote\""),
            "{gespeichert}"
        );

        // Ausgeschrieben wird der Wert als Wort, nicht als Zahlencode: Eine
        // Konfigurationsdatei wird auch von Hand gelesen.
        let lokal: OllamaConfig = serde_json::from_str(
            r#"{"server_url": "http://192.168.1.50:11434", "provider": "local"}"#,
        )
        .unwrap();
        assert_eq!(lokal.provider, Provider::Local);
        assert_eq!(
            lokal.provider.adresse(&lokal.server_url),
            super::DEFAULT_OLLAMA_BASE_URL,
            "lokal sieht die eingetragene Adresse nicht"
        );

        // Und ein Tippfehler bricht den Start ab, statt auf das entfernte Ollama
        // zurückzufallen: Genau das wäre die stille Umstellung, die hier nicht
        // passieren soll.
        assert!(serde_json::from_str::<OllamaConfig>(
            r#"{"server_url": "http://localhost:11434", "provider": "hieslokal"}"#
        )
        .is_err());
    }

    /// Lokal ist eine feste Adresse auf diesem Rechner.
    ///
    /// Der eingetragene Server bleibt unangetastet: Wer zurückwechselt, muss die
    /// Adresse nicht noch einmal eintippen. Und es gibt keine andere Adresse, auf
    /// die ein Ollama auf diesem Rechner hörte.
    #[tokio::test]
    async fn der_lokale_provider_zeigt_auf_diesen_rechner() {
        let config_path = test_config_path("provider-lokal");
        let settings = OllamaSettings::load(config_path.clone()).unwrap();

        settings
            .set_base_url("http://192.168.1.50:11434")
            .await
            .unwrap();

        settings.set_provider(Provider::Local).await.unwrap();

        assert_eq!(
            settings.get_base_url().await,
            super::DEFAULT_OLLAMA_BASE_URL,
            "lokal ist immer die Vorgabeadresse"
        );
        // Die eingetragene Adresse ist noch da, nur nicht mehr in Gebrauch.
        assert_eq!(
            settings.get_config().await.server_url,
            "http://192.168.1.50:11434"
        );

        // Und sie kommt nach dem Zurückwechseln wieder genau so zurück.
        settings.set_provider(Provider::Remote).await.unwrap();
        assert_eq!(settings.get_base_url().await, "http://192.168.1.50:11434");

        // Der Wechsel übersteht einen Neustart.
        let reloaded = OllamaSettings::load(config_path.clone()).unwrap();
        assert_eq!(reloaded.get_provider().await, Provider::Remote);
        assert_eq!(reloaded.get_base_url().await, "http://192.168.1.50:11434");
        std::fs::remove_file(&config_path).ok();
    }

    /// Im lokalen Provider gibt es keine Dateiwerkzeuge.
    ///
    /// Geprüft wird der Umfang, aus dem die Werkzeuge entstehen – nicht die
    /// Oberfläche. Ein Anzeigefehler wäre lästig, ein Werkzeugangebot mit
    /// `read_file` dagegen ein Fehler, der das Modell in ein Verzeichnis greifen
    /// ließe, in dem der Assistent selbst läuft.
    #[tokio::test]
    async fn das_lokale_modell_bekommt_nur_die_kalenderwerkzeuge() {
        let config_path = test_config_path("provider-umfang");
        let settings = OllamaSettings::load(config_path.clone()).unwrap();
        let root = test_tree("provider-umfang-dir");

        // Der gespeicherte Umfang ist der volle Agentenmodus – mit
        // Arbeitsverzeichnis, damit die Prüfung nicht zufällig daran hängen
        // bleibt.
        settings
            .set_agent_config(AgentConfig {
                root: root.to_string_lossy().to_string(),
                max_steps: 5,
                scope: Scope::Agent,
            })
            .await
            .unwrap();

        settings.set_provider(Provider::Local).await.unwrap();

        // Wirksam ist der Terminumfang, obwohl der Agentenmodus gespeichert ist.
        let config = settings.get_config().await;
        assert_eq!(config.agent.scope, Scope::Agent, "gespeichert bleibt er");
        assert_eq!(
            wirksamer_umfang(&config),
            Scope::Termine,
            "wirksam ist er es nicht"
        );

        // Und deshalb braucht das Werkzeugangebot kein Arbeitsverzeichnis: Der
        // Pfad hier ist ungültig, und es stünde nicht einmal mehr im Weg.
        let kaputt = AgentConfig {
            root: "/gibt/es/nicht/xyz".to_string(),
            scope: Scope::Agent,
            ..Default::default()
        };
        let toolset = toolset_for(&kaputt, wirksamer_umfang(&config), false, &zwei_kalender())
            .expect("lokal wird der Pfad nicht gebraucht");
        let namen: Vec<&str> = toolset
            .tools
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        for erwartet in [
            crate::calendar::write::EVENT_TOOL,
            crate::calendar::edit::UPDATE_TOOL,
            crate::calendar::edit::DELETE_TOOL,
            LIST_EVENTS_TOOL,
        ] {
            assert!(namen.contains(&erwartet), "{erwartet} fehlt: {namen:?}");
        }
        for verboten in [
            "read_file",
            "write_file",
            "edit_file",
            "list_files",
            "search_files",
        ] {
            assert!(
                !namen.contains(&verboten),
                "{verboten} gehört nicht dazu: {namen:?}"
            );
        }

        std::fs::remove_file(&config_path).ok();
    }

    /// Der lokale Provider nimmt keinen Umfang an, den er nicht halten kann.
    ///
    /// Ohne diese Ablehnung würde `/scope agent` eine Wahl anzeigen, die nicht
    /// gilt, und der Fehler käme erst beim Senden – nach der ersten Antwort des
    /// Modells, die auf Dateiwerkzeuge hinausläuft.
    #[tokio::test]
    async fn das_lokale_modell_lässt_sich_nicht_zum_agenten_erweitern() {
        let config_path = test_config_path("provider-erweiterung");
        let settings = OllamaSettings::load(config_path.clone()).unwrap();
        let root = test_tree("provider-erweiterung-dir");

        settings.set_provider(Provider::Local).await.unwrap();

        let fehler = settings
            .set_agent_config(AgentConfig {
                root: root.to_string_lossy().to_string(),
                max_steps: 5,
                scope: Scope::Agent,
            })
            .await
            .unwrap_err();
        assert!(fehler.contains("/provider remote"), "{fehler}");

        // Der Terminumfang lässt sich setzen, und er bleibt gültig.
        let gesetzt = settings
            .set_agent_config(AgentConfig {
                root: root.to_string_lossy().to_string(),
                max_steps: 5,
                scope: Scope::Termine,
            })
            .await
            .unwrap();
        assert_eq!(gesetzt.scope, Scope::Termine);

        std::fs::remove_file(&config_path).ok();
    }

    /// Die eingetragene Adresse gehört zum entfernten Provider.
    ///
    /// Solange das lokale Ollama läuft, wäre eine neue Adresse ein Eintrag, den
    /// niemand sieht: Die Oberfläche zeigte weiter `localhost`, und der Knopf ist
    /// ausgeblendet. Die Ablehnung nennt deshalb den Weg zurück.
    #[tokio::test]
    async fn lokal_kann_die_adresse_nicht_umstellen() {
        let config_path = test_config_path("provider-adresse");
        let settings = OllamaSettings::load(config_path.clone()).unwrap();

        settings.set_provider(Provider::Local).await.unwrap();

        let fehler = settings
            .set_base_url("http://192.168.1.50:11434")
            .await
            .unwrap_err();
        assert!(fehler.contains("/provider remote"), "{fehler}");
        assert_eq!(
            settings.get_base_url().await,
            super::DEFAULT_OLLAMA_BASE_URL
        );

        // Und remote ist sie wieder gültig – genau derselbe Aufruf.
        settings.set_provider(Provider::Remote).await.unwrap();
        assert_eq!(
            settings
                .set_base_url("http://192.168.1.50:11434")
                .await
                .unwrap(),
            "http://192.168.1.50:11434"
        );

        std::fs::remove_file(&config_path).ok();
    }

    #[test]
    fn a_scope_that_is_missing_stays_the_default() {
        // Eine bestehende Konfiguration ohne das Feld lädt, und der alte Stand
        // bleibt: Der Agentenmodus mit Arbeitsverzeichnis, wie er war.
        let config: OllamaConfig = serde_json::from_str(
            r#"{"server_url": "http://localhost:11434", "agent": {"root": "/srv", "max_steps": 5}}"#,
        )
        .unwrap();
        assert_eq!(config.agent.scope, Scope::Agent);
        assert_eq!(config.agent.root, "/srv");
        assert_eq!(config.agent.max_steps, 5);

        // Und ohne ganzen `agent`-Abschnitt gilt dasselbe.
        let ohne =
            serde_json::from_str::<OllamaConfig>(r#"{"server_url": "http://localhost:11434"}"#)
                .unwrap();
        assert_eq!(ohne.agent.scope, Scope::Agent);
        assert_eq!(ohne.agent.scope, AgentConfig::default().scope);
    }

    #[test]
    fn the_write_budget_of_a_turn_is_limited() {
        let root = test_write_tree("agent-write-budget");
        let state = write_state(&test_config_path("agent-write-config7.json"));

        for index in 0..MAX_TOOL_WRITES_PER_TURN {
            execute_write_tool(
                &state,
                &root,
                "write_file",
                &serde_json::json!({ "path": format!("datei{index}.txt"), "content": "x" }),
            )
            .unwrap_or_else(|error| panic!("Schreibvorgang {index}: {error}"));
        }

        let error = execute_write_tool(
            &state,
            &root,
            "write_file",
            &serde_json::json!({ "path": "zuviel.txt", "content": "x" }),
        )
        .unwrap_err();
        assert!(error.contains("höchstens"), "{error}");
        assert!(!root.join("zuviel.txt").exists());

        // Ein neuer Zug bekommt das Budget zurück.
        *state.budget.lock().unwrap() = super::WriteBudget::default();
        execute_write_tool(
            &state,
            &root,
            "write_file",
            &serde_json::json!({ "path": "nachher.txt", "content": "x" }),
        )
        .unwrap();
        assert!(root.join("nachher.txt").exists());

        // Auch die Gesamtmenge je Zug ist begrenzt.
        let fresh = write_state(&test_config_path("agent-write-config8.json"));
        let mut written = 0usize;

        for index in 0..12 {
            match execute_write_tool(
                &fresh,
                &root,
                "write_file",
                &serde_json::json!({
                    "path": format!("gross{index}.txt"),
                    "content": "x".repeat(MAX_TOOL_WRITE_TOTAL_BYTES / 2)
                }),
            ) {
                Ok(_) => written += 1,
                Err(error) => {
                    assert!(error.contains("KiB"), "{error}");
                    break;
                }
            }
        }

        assert!(written > 0, "kein Schreibvorgang möglich");
        let error = execute_write_tool(
            &fresh,
            &root,
            "write_file",
            &serde_json::json!({ "path": "zu-gross.txt", "content": "x" }),
        )
        .unwrap_err();
        assert!(error.contains("KiB"), "{error}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_configuration_file_and_git_are_protected() {
        let root = test_write_tree("agent-write-geschuetzt");
        let config_path = root.join("ollama.json");
        std::fs::write(&config_path, "{}\n").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let state = write_state(&config_path);

        for path in ["ollama.json", ".git/config"] {
            let error = execute_write_tool(
                &state,
                &root,
                "write_file",
                &serde_json::json!({ "path": path, "content": "x" }),
            )
            .unwrap_err();
            assert!(error.contains("gesperrt"), "{path}: {error}");
        }
        assert_eq!(std::fs::read_to_string(&config_path).unwrap(), "{}\n");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_preview_matches_what_would_be_written() {
        let root = test_write_tree("agent-write-vorschau");
        let state = write_state(&test_config_path("agent-write-config9.json"));
        let arguments = serde_json::json!({
            "path": "notiz.txt",
            "old_string": "zweite Zeile",
            "new_string": "ersetzt"
        });

        let preview = preview_write(&root, "edit_file", &arguments).unwrap();
        assert!(preview.current.is_some(), "Vorher-Stand fehlt");
        assert!(preview.current.as_ref().unwrap().contains("zweite Zeile"));
        assert!(preview.next.contains("ersetzt"));
        assert_eq!(preview.lines_before, 2);
        assert_eq!(preview.lines_after, 2);

        // Die Vorschau verändert nichts.
        assert_eq!(
            std::fs::read_to_string(root.join("notiz.txt")).unwrap(),
            "erste Zeile\nzweite Zeile\n"
        );

        // Und sie beschreibt genau das, was der Schreibvorgang erzeugt.
        execute_write_tool(&state, &root, "edit_file", &arguments).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("notiz.txt")).unwrap(),
            preview.next,
            "Vorschau und Ergebnis weichen ab"
        );

        // Neue Datei: kein Vorher-Stand.
        let preview = preview_write(
            &root,
            "write_file",
            &serde_json::json!({ "path": "neu.txt", "content": "neu" }),
        )
        .unwrap();
        assert!(preview.current.is_none());
        assert_eq!(preview.next, "neu");

        // Lesewerkzeuge haben keine Vorschau.
        assert!(preview_write(
            &root,
            "read_file",
            &serde_json::json!({ "path": "notiz.txt" })
        )
        .is_err());

        // Ein Fehlerfall wird schon in der Vorschau sichtbar.
        assert!(preview_write(
            &root,
            "edit_file",
            &serde_json::json!({ "path": "notiz.txt", "old_string": "gibtsnicht", "new_string": "x" })
        )
        .is_err());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn oversized_writes_are_refused() {
        let root = test_write_tree("agent-write-gross");
        let state = write_state(&test_config_path("agent-write-config10.json"));

        let huge = "x".repeat(super::MAX_TOOL_WRITE_BYTES + 1);
        let error = execute_write_tool(
            &state,
            &root,
            "write_file",
            &serde_json::json!({ "path": "riesig.txt", "content": huge }),
        )
        .unwrap_err();
        assert!(error.contains("KiB"), "{error}");
        assert!(!root.join("riesig.txt").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn askpass_helper_contains_no_password() {
        use std::os::unix::fs::PermissionsExt;

        let files = create_askpass_files("geheim123").unwrap();
        let content = std::fs::read_to_string(&files.helper_path).unwrap();
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

        assert_eq!(
            content,
            "#!/bin/sh\nexec /bin/cat \"$MIMIR_SSH_PASSWORD_FILE\"\n"
        );
        assert_eq!(mode(&files.helper_path), 0o700);
        assert_eq!(mode(&files.password_path), 0o600);
        // Der Inhalt von /tmp ist auflistbar, deshalb 0700 auf dem Verzeichnis.
        assert_eq!(mode(&files.directory), 0o700);
        assert!(files.directory.starts_with(std::env::temp_dir()));
        assert!(!content.contains("geheim123"));
        assert!(!content.contains("MIMIR_SSH_PASSWORD="));
        assert_eq!(
            std::fs::read_to_string(&files.password_path).unwrap(),
            "geheim123"
        );

        let directory = files.directory.clone();
        drop(files);
        assert!(
            !directory.exists(),
            "Askpass-Verzeichnis wurde nicht entfernt"
        );
    }

    #[test]
    fn chat_wait_window_resets_on_every_token() {
        let start = Instant::now();
        let mut state = ChatWaitState::new(start);

        let (idle, no_token) = state.remaining(start + Duration::from_secs(299)).unwrap();
        assert_eq!(idle, Duration::from_secs(1));
        assert_eq!(no_token, Duration::from_secs(601));

        state.on_token(start + Duration::from_secs(299));
        let (idle, no_token) = state.remaining(start + Duration::from_secs(299)).unwrap();
        assert_eq!(idle, OLLAMA_TOKEN_IDLE_TIMEOUT);
        assert_eq!(no_token, OLLAMA_NO_TOKEN_LIMIT);
    }

    #[test]
    fn chat_wait_aborts_without_token_after_hard_limit() {
        let start = Instant::now();
        let state = ChatWaitState::new(start);

        assert!(state
            .remaining(start + OLLAMA_NO_TOKEN_LIMIT - Duration::from_secs(1))
            .is_some());
        assert!(state.remaining(start + OLLAMA_NO_TOKEN_LIMIT).is_none());
        assert!(no_token_timeout_error().contains("15 Minuten"));
    }

    #[test]
    fn chat_wait_health_probe_extends_idle_but_not_hard_limit() {
        let start = Instant::now();
        let mut state = ChatWaitState::new(start);

        assert!(state.on_health_probe(true, start + OLLAMA_TOKEN_IDLE_TIMEOUT));
        let (idle, no_token) = state.remaining(start + OLLAMA_TOKEN_IDLE_TIMEOUT).unwrap();
        assert_eq!(idle, OLLAMA_TOKEN_IDLE_TIMEOUT);
        assert_eq!(no_token, OLLAMA_NO_TOKEN_LIMIT - OLLAMA_TOKEN_IDLE_TIMEOUT);

        let probe_at = start + OLLAMA_TOKEN_IDLE_TIMEOUT;
        assert!(state.on_health_probe(false, probe_at));
        assert!(!state.on_health_probe(false, probe_at + OLLAMA_TOKEN_IDLE_TIMEOUT));
        assert!(state
            .remaining(probe_at + OLLAMA_NO_TOKEN_LIMIT - OLLAMA_TOKEN_IDLE_TIMEOUT)
            .is_none());
    }

    #[test]
    fn chat_wait_silence_tracks_the_last_token_not_the_probe() {
        let start = Instant::now();
        let mut state = ChatWaitState::new(start);

        assert_eq!(
            state.silent_for(start + Duration::from_secs(30)),
            Duration::from_secs(30)
        );

        state.on_token(start + Duration::from_secs(30));
        assert_eq!(
            state.silent_for(start + Duration::from_secs(45)),
            Duration::from_secs(15)
        );

        // Ein Health-Probe startet nur das Idle-Fenster neu, nicht die Stille-Zeit.
        state.on_health_probe(true, start + Duration::from_secs(60));
        assert_eq!(
            state.silent_for(start + Duration::from_secs(90)),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn retries_only_transport_failures_before_the_first_token() {
        let transport_before_token = StreamFailure::transport("Verbindung weg".to_string(), false);
        assert!(should_retry_chat(&transport_before_token, 1));
        assert!(should_retry_chat(
            &transport_before_token,
            MAX_CHAT_ATTEMPTS - 1
        ));
        assert!(!should_retry_chat(
            &transport_before_token,
            MAX_CHAT_ATTEMPTS
        ));

        let transport_after_token = StreamFailure::transport("Verbindung weg".to_string(), true);
        assert!(!should_retry_chat(&transport_after_token, 1));

        let fatal = StreamFailure::fatal("Ollama-Fehler: Status 404".to_string(), false);
        assert!(!should_retry_chat(&fatal, 1));

        let cancelled = StreamFailure::cancelled();
        assert!(!should_retry_chat(&cancelled, 1));

        let timeout = StreamFailure::fatal(no_token_timeout_error(), false);
        assert!(!should_retry_chat(&timeout, 1));
    }

    /// "verschiebe den Termin auf heute 15 Uhr" heißt: on_date nennt das
    /// **Ziel**, nicht die jetzige Lage des Termins. Der Auftrag sah im
    /// Betrieb so aus, und Mimir suchte daraufhin den Termin am heutigen Tag –
    /// also gerade nicht den, den der Benutzer verschieben wollte.
    #[test]
    fn eine_neue_zeit_macht_on_date_zum_ziel() {
        assert!(name_der_aenderung_setzt_die_zeit(
            &serde_json::json!({"title": "test", "on_date": "heute", "start": "heute 15 Uhr"})
        ));
        assert!(name_der_aenderung_setzt_die_zeit(
            &serde_json::json!({"title": "test", "end": "morgen 16 Uhr"})
        ));

        // Ohne neue Zeit bleibt on_date die jetzige Lage – so, wie es im
        // Schema beschrieben ist.
        assert!(!name_der_aenderung_setzt_die_zeit(
            &serde_json::json!({"title": "test", "on_date": "gestern 14:30"})
        ));
        assert!(!name_der_aenderung_setzt_die_zeit(
            &serde_json::json!({"title": "test", "summary": "Neuer Titel"})
        ));
        // Leere Werte zählen nicht als neue Zeit.
        assert!(!name_der_aenderung_setzt_die_zeit(
            &serde_json::json!({"title": "test", "start": "  "})
        ));
    }

    /// Der Vorgang aus dem Screenshot: „verschiebe den Termin ‚angelegt durch KI'
    /// auf heute 15 Uhr". Das Modell schickt on_date mit, weil es das Feld als
    /// Beschreibung des Termins liest. Mimir suchte daraufhin am heutigen Tag –
    /// also gerade nicht den Termin, der woanders liegt und verschoben werden
    /// soll. Mit gesetztem start darf on_date die Suche nicht mehr einschränken.
    #[test]
    fn ein_ziel_darf_die_suche_nicht_einschraenken() {
        let auftrag = serde_json::json!({
            "title": "angelegt durch KI",
            "on_date": "heute",
            "start": "heute 15 Uhr",
            "calendar": "Familienkalender",
        });

        let auswahl = auswahl_aus_argumenten(&auftrag);

        // Weder Tag noch Uhrzeit dürfen die Suche eingrenzen …
        assert_eq!(auswahl.tag, None, "{auswahl:?}");
        assert_eq!(auswahl.uhrzeit, None, "{auswahl:?}");

        // … der Kalender aber weiterhin, denn er ist eine Angabe zum jetzigen
        // Termin und keine Angabe zum Ziel.
        assert_eq!(
            auswahl,
            crate::calendar::edit::Auswahl {
                tag: None,
                uhrzeit: None
            }
        );
    }

    /// Umgekehrt: Ohne neue Zeit bleibt on_date die jetzige Lage des Termins.
    #[test]
    fn ohne_neue_zeit_grenzt_on_date_ein() {
        let auftrag = serde_json::json!({"title": "angelegt durch KI", "on_date": "heute"});
        let auswahl = auswahl_aus_argumenten(&auftrag);

        assert_eq!(
            auswahl.tag,
            Some(chrono::Local::now().date_naive()),
            "{auswahl:?}"
        );
    }

    /// Der Fehler aus dem Betrieb: Eine Datei zwischen 64 und 128 KiB wurde als
    /// „Angehängt" quittiert und ließ anschließend die Anfrage scheitern, weil
    /// `validate_chat_message` alles über 64 KiB abweist. Der Anhang wird jetzt
    /// gekürzt, und der Benutzer erfährt davon.
    #[test]
    fn ein_zu_grosser_anhang_wird_gekuerzt_statt_abgewiesen() {
        let inhalt = "Absatz mit Nutzen.\n".repeat(6000);
        let fertig = prepare_attachment("handbuch.txt".to_string(), inhalt.clone())
            .expect("eine große Datei muss sich aufbereiten lassen");

        assert!(fertig.truncated, "die Kürzung muss benannt werden");
        assert!(
            fertig.message.len() <= MAX_MESSAGE_BYTES,
            "die Nachricht muss die Grenze einhalten: {} > {MAX_MESSAGE_BYTES}",
            fertig.message.len()
        );
        assert!(
            fertig.message.contains("gekürzt"),
            "{}",
            &fertig.message[..200]
        );
        assert!(fertig.message.contains("Absatz mit Nutzen."));

        // Und die Nachricht besteht die Prüfung, an der es vorher scheiterte.
        let laenge = fertig.message.len();
        let nachricht = ChatMessage {
            role: "user".to_string(),
            content: fertig.message,
            thinking: String::new(),
            tool_calls: Vec::new(),
            tool_name: String::new(),
        };
        validate_chat_message(&nachricht).expect("gekürzt muss durchgehen");
        assert_eq!(nachricht.content.len(), laenge);
    }

    /// Eine kleine Datei bleibt unangetastet.
    #[test]
    fn ein_kleiner_anhang_bleibt_vollstaendig() {
        let fertig =
            prepare_attachment("notiz.txt".to_string(), "nur ein Satz".to_string()).unwrap();

        assert!(!fertig.truncated);
        assert!(!fertig.message.contains("gekürzt"));
        assert!(fertig.message.contains("nur ein Satz"));
        assert!(fertig.message.starts_with("Datei: notiz.txt"));
    }

    /// Die Kürzung darf nicht mitten in einem Zeichen oder Wort enden.
    /// Die Kürzung muss auch die Kopfzeile rechnen. Vorher lief die fertige
    /// Nachricht um 28 Byte über die Grenze, weil der Kopf erst danach dazukam.
    #[test]
    fn die_kuerzung_zaehlt_den_kopf_mit() {
        let inhalt = "Zeile.\n".repeat(20_000);
        let fertig = prepare_attachment("a.txt".to_string(), inhalt).unwrap();

        assert!(fertig.truncated);
        assert!(
            fertig.message.len() <= MAX_MESSAGE_BYTES,
            "{} > {MAX_MESSAGE_BYTES}",
            fertig.message.len()
        );
        assert!(
            fertig.message.contains("– gekürzt"),
            "{}",
            &fertig.message[..120]
        );
    }

    /// Ein langer Dateiname frisst Platz – auch er muss mitgerechnet werden.
    #[test]
    fn ein_langer_dateiname_wird_mitgezaehlt() {
        let name = "a".repeat(500);
        let inhalt = "Zeile.\n".repeat(20_000);
        let fertig = prepare_attachment(format!("{name}.txt"), inhalt).unwrap();

        assert!(fertig.truncated);
        assert!(
            fertig.message.len() <= MAX_MESSAGE_BYTES,
            "{} > {MAX_MESSAGE_BYTES}",
            fertig.message.len()
        );
    }

    #[test]
    fn die_kuerzung_endet_an_einer_zeichengrenze() {
        // Umlaute und ein langes Wort erzwingen eine Grenze im Mehrbyte-Bereich.
        let inhalt = "Ü".repeat(40_000);
        let (gekuerzt, markiert) = kuerze_fuer_nachricht("", &inhalt);

        assert!(markiert);
        assert!(gekuerzt.len() <= MAX_MESSAGE_BYTES);
        assert!(
            gekuerzt.ends_with("steht hier nicht. …]"),
            "{}",
            &gekuerzt[gekuerzt.len().saturating_sub(80)..]
        );
        // Der Inhalt selbst muss gültiges UTF-8 ohne Ersatzzeichen sein.
        assert!(
            !gekuerzt.contains('\u{fffd}'),
            "mitten im Zeichen abgeschnitten"
        );
    }

    /// Eine Datei ganz ohne Zeilenumbruch wird trotzdem brauchbar gekürzt.
    #[test]
    fn ein_einzeiliger_anhang_wird_gekuerzt() {
        let inhalt = "Wort ".repeat(30_000);
        let (gekuerzt, markiert) = kuerze_fuer_nachricht("", &inhalt);

        assert!(markiert);
        assert!(gekuerzt.len() <= MAX_MESSAGE_BYTES, "{}", gekuerzt.len());
        assert!(gekuerzt.contains("gekürzt"));
    }

    /// Zu große Dateien werden abgewiesen, mit einer Meldung, die die Grenze nennt.
    #[test]
    fn eine_riesige_datei_wird_abgewiesen() {
        let inhalt = "x".repeat(MAX_ATTACHMENT_BYTES + 1);
        let fehler = prepare_attachment("riesig.txt".to_string(), inhalt).unwrap_err();

        assert!(fehler.contains("1024"), "{fehler}");
        assert!(fehler.contains("riesig.txt"), "{fehler}");
    }

    /// Ein Anhang ohne Namen ergibt keinen Verlaufseintrag.
    #[test]
    fn ein_anhang_ohne_namen_wird_abgewiesen() {
        assert!(prepare_attachment("   ".to_string(), "Text".to_string()).is_err());
    }
}
