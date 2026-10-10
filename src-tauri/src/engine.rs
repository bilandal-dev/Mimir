//! Die eingebaute Engine: llama.cpp im eigenen Prozess.
//!
//! **Warum dieses Modul die Form von Ollamas Antworten nachbildet.** Mimir
//! spricht seit jeher die HTTP-Schnittstelle von Ollama an: `/api/tags` für die
//! Modellliste, `/api/chat` für den Strom aus Antwortzeilen. Diese beiden Wege
//! tragen den ganzen Rest der Anwendung – den Werkzeugkreis, die Termine, das
//! Zählen der Kontextbelegung, den Abbruch und die Wiederholung. Eine eigene
//! Engine zurückzubauen, hieße, all das zweimal zu schreiben.
//!
//! Deshalb erzeugt dieses Modul **dasselbe Format**: Newline-getrenntes JSON mit
//! `message.content` und `done`. Der obere Teil von `send_chat_message` weiß
//! nicht, ob die Zeilen aus einem Ollama im Netz kommen oder von hier. Das ist der
//! ganze Trick, und er ist der Grund, warum diese Datei so kurz ausfällt.
//!
//! **Was hier nicht liegt.** Kein HTTP-Server: Der Aufruf von außen ist ein
//! Tauri-Befehl (`chat_engine`), und der Stream läuft über denselben Kanal wie beim
//! Server. Und keine Werkzeugschemata in llama.cpp-eigenem Format – die
//! Werkzeugbeschreibung geht als Anweisung in die Systemnachricht, und der
//! Aufruf kommt als JSON im Text zurück. Das ist weniger elegant als eine
//! Grammatik, aber es braucht kein Modell mit eingebautem Werkzeugsupport, und die
//! Katalogeinträge sind Qwen-Instruct-Modelle, die diese Form gut beherrschen.

pub mod werkzeug;

use std::path::Path;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

/// Wie die Rechenquelle in der Kopfzeile heißt.
///
/// Sie steht in der Statuszeile an der Stelle, an der sonst eine Adresse steht, und
/// an den Stellen, an denen eine Fehlermeldung sonst eine Adresse nennen würde.
/// „Eingebaut“ ist das Wort, das der Provider in der Oberfläche schon führt – die
/// Anzeige soll nicht zwei Namen für dieselbe Sache erfinden.
///
/// Der Name ist nicht `BEZEICHNUNG`, weil `Provider` eines mit diesem Namen führt und
/// es dort etwas anderes meint: den Provider, nicht die Rechenquelle.
pub const QUELLE: &str = "eingebaute Engine (dieser Rechner)";

/// Der Zustand, in dem die Engine das Modell in den Speicher holt.
///
/// Nur für die Anzeige, und deshalb ein Wort statt eines Satzes: Die Kopfzeile
/// setzt „Lokal: … …" davor, und ein ganzer Satz darin sähe aus wie eine
/// Fehlermeldung.
///
/// Der Vorgang ist der längste Schweigepunkt des ganzen Ablaufs und der einzige,
/// in dem wirklich nichts angezeigt wird – danach wächst der Text Token für
/// Token. Ohne diese Meldung stünde in dieser Zeit eine leere Blase im Fenster.
///
/// **Die Wörter stehen unverändert in der Anzeige.** „Wird geladen" und nicht
/// „Modell wird in die Engine geladen": Der Platz hat eine Zeile, und der Benutzer
/// liest während des Wartens keinen Nebensatz, sondern ein Stichwort.
pub fn ladezustand() -> &'static str {
    "wird geladen"
}

/// Der Zustand, in dem die Engine rechnet.
pub fn rechenzustand() -> &'static str {
    "rechnet"
}

/// Das Kontextfenster, wenn nichts eingestellt ist.
///
/// Klein, weil der Platz auf einem Rechner mit vier GiB begrenzt ist und ein
/// Fenster nichts kostet, solange es nicht benutzt wird: Der KV-Speicher wächst mit
/// der eingestellten Länge, nicht mit der benutzten.
pub const STANDARDFENSTER: usize = 2048;

/// Die Obergrenze für das Fenster.
///
/// Sie ist eine ehrliche Rechengrenze, keine Vorsicht: Der Speicherbedarf liegt bei
/// etwa 0,5 MiB je Token und hängt von der Modellgröße ab. 16384 Token sind auf
/// einem 3B-Modell rund 8 GiB – mehr, als die Katalogeinträge auf einem Rechner
/// belegen, auf dem sie laufen. Wer mehr will, bekommt die Meldung und keine
/// Absturzmeldung später.
pub const MAX_FENSTER: usize = 16384;

/// Die Nachrichten des Verlaufs als Nachrichten für die Engine.
///
/// Die Rollen werden unverändert übernommen: Eine Engine kennt `system`, `user` und
/// `assistant`, und eine Nachricht mit einer Rolle, die sie nicht kennt, gehört
/// nicht umgeschrieben – dann sieht der Benutzer im Verlauf etwas anderes, als das
/// Modell bekommen hat. Der Denktext fällt weg, weil er bei Ollama auch nicht
/// mitgeht.
pub fn nachrichten(verlauf: &[crate::ChatMessage]) -> Result<Vec<LlamaChatMessage>, String> {
    verlauf
        .iter()
        .map(|nachricht| {
            let rolle = match nachricht.role.as_str() {
                // Ollamas Werkzeugantworten kommen als `tool` an. Für die Engine ist
                // das eine Antwort des Werkzeugs, und sie wird auch so gesendet –
                // sonst wüsste das Modell nicht, was es zuletzt gefragt hat.
                "tool" => "user",
                andere => andere,
            };

            LlamaChatMessage::new(rolle.to_string(), nachricht.content.clone()).map_err(|_| {
                "Eine Nachricht enthält Zeichen, die die Engine nicht lesen kann".to_string()
            })
        })
        .collect()
}

/// Wie viele Worker-Threads die Engine für die Mathe benutzt.
///
/// Ohne Angabe nimmt llama.cpp die Zahl der logischen Kerne. Das ist auf einem
/// Rechner richtig, auf dem Mimir das einzige Programm ist – aber es kann den
/// Rechner vollständig belegen, weil der WebView dieselben Kerne braucht. Es
/// wird deshalb eine Kernezahl weniger genommen, und mindestens einer bleibt es.
pub fn standard_threads() -> i32 {
    let kerne = std::thread::available_parallelism()
        .map(|zahl| zahl.get())
        .unwrap_or(1);

    (kerne.saturating_sub(1)).clamp(1, 32) as i32
}

/// Die Vorlage, wenn das Modell keine mitbringt.
///
/// llama.cpp hat für die üblichen Familien Namen hinterlegt, und „chatml" ist der
/// Name, den auch die Katalogeinträge hier benutzen. Fehlt die Vorlage ganz, wird
/// genau diese hier gesetzt – sie zu verwenden ist besser, als dem Modell eine
/// Antwort abzuringen, die keine Chat-Struktur hat.
const ERSATZ_VORLAGE: &str = "chatml";

/// Wie viele Token in einem Durchgang gerechnet werden.
///
/// 512 ist der Wert, den llama.cpp selbst für kleine Modelle nennt. Er ist keine
/// Geschwindigkeitsgrenze: Ein größerer Stapel rechnet mehr auf einmal, belegt
/// aber auch mehr Speicher, und bei einem kleinen Modell ist der Unterschied
/// gering.
const STAPEL_GROESSE: u32 = 512;

/// Die Antwort braucht mindestens so viele Token, um ein Satz zu sein.
///
/// Darunter ist keine Antwort möglich, sondern nur ein Wort – und eine abgebrochene
/// Antwort, die mitten im ersten Satz endet, ist für den Benutzer dasselbe wie ein
/// Fehler.
const MIN_ANTWORT_TOKEN: usize = 64;

/// Die Obergrenze für eine neue Antwort.
///
/// Sie begrenzt nicht das Modell, sondern die Arbeit einer einzelnen Sitzung: Ein
/// entlaufenes Modell, das endlos weiterschreibt, würde sonst den Rechner belegen,
/// ohne dass der Benutzer etwas eingeleitet hätte. 8192 Token sind für eine
/// Antwort im Chat weit mehr, als jemand lesen würde.
const MAX_ANTWORT_TOKEN: usize = 8192;

/// Ein Abschnitt aus Zeichen, für den Vergleich.
fn abschnitt(text: &[char], ab: usize, laenge: usize) -> &[char] {
    &text[ab..ab + laenge]
}

/// Wie viele Durchgänge ein Auftrag von dieser Länge braucht.
///
/// Nur eine Rechnung, aber sie entscheidet, ob das Programm überhaupt läuft:
/// llama.cpp verlangt je Durchgang höchstens `n_batch` Token und ruft sonst eine
/// Zusicherung auf, die das Programm beendet. Die Antwort wird deshalb in Blöcke
/// zerlegt, und diese Funktion sagt, wie viele es werden.
pub fn durchgaenge(token: usize, stapel: usize) -> usize {
    token.div_ceil(stapel)
}

/// Der Text, mit dem ein abgebrochener Zug zurückkommt.
///
/// Er ist kein Fehlerfall, sondern ein vom Benutzer gewählter Abbruch – deshalb
/// steht er als eigener Satz da und nicht als Zeichenkette aus null und einem
/// Zeichen, wie es technisch ginge. Der Aufrufer erkennt ihn wieder.
/// Rechnet das Fenster aus, in dem ein Auftrag Platz hat.
///
/// **Der Fehler, den diese Funktion verhindert.** Vorher stand die Bedingung als
/// `auftrag + MAX_ANTWORT_TOKEN <= fenster`. Das ist bei jedem Fenster, das kleiner
/// ist als die Obergrenze von 8192 Token, immer `false` – also bei jedem normalen
/// Fenster. Die Engine hat daraufhin **jede** Anfrage mit „Fenster zu klein"
/// abgelehnt, noch bevor der erste Token gerechnet war. Der Auftrag passte immer,
/// der Platz für die Antwort nie.
///
/// Jetzt ist es umgekehrt gerechnet: Der Auftrag nimmt, was er braucht, und die
/// Antwort bekommt das, was übrig bleibt – höchstens `MAX_ANTWORT_TOKEN`.
///
/// Gibt zurück: Der Platz für den Auftrag und der Platz für die Antwort, beide in
/// Token. Beides kann `0` sein, und dann sagt der Aufrufer dem Benutzer, warum.
pub fn fenster_aufteilen(fenster: usize, auftrag: usize) -> (usize, usize) {
    // **Der Auftrag darf das Fenster nicht auffressen.** Ohne diese Grenze bekäme
    // ein Auftrag, der genau den ganzen Platz belegt, null Token für die Antwort
    // und die Anfrage scheiterte – obwohl ein Fenster, das für nichts reicht, das
    // Richtige ist. Deshalb wird für die Antwort immer mindestens
    // `MIN_ANTWORT_TOKEN` Platz von unten geschützt.
    let hoechstens = fenster.saturating_sub(MIN_ANTWORT_TOKEN + 8);
    let auftrag = auftrag.min(hoechstens);
    let uebrig = fenster - auftrag;
    let antwort = MAX_ANTWORT_TOKEN.min(uebrig.saturating_sub(8));

    (auftrag, antwort)
}

pub const ABBRUCH_TEXT: &str = "Der Zug wurde abgebrochen.";

/// Wie viele Bytes die Vorlage haben darf, bevor sie abgelehnt wird.
///
/// Eine Vorlage ist eine Datei neben dem Modell, und die Datei muss kein
/// Gesprächspartner sein. Das Modell lädt sie ungeprüft; hier wird sie geprüft,
/// und ein Muster ist für die Prüfung nicht nötig, weil das Modell sie selbst anwendet.
const MAX_VORLAGE_BYTES: usize = 64 * 1024;

/// Das geladene Modell mit seinem Kontext.
///
/// Der Kontext wird nicht über [`std::cell::RefCell`] gehalten, sondern über einen
/// einfachen Schreibschloss in der aufrufenden Stelle: Das Modell wird über einen
/// Zug gehalten, der über Tokio läuft und mehrere Anfragen bedienen kann.
pub struct Engine {
    /// Nur zum Erhalten des Modells nötig: llama.cpp verlangt die Instanz, solange
    /// ein Modell von ihr geladen wurde.
    _backend: LlamaBackend,
    model: LlamaModel,
    vorlage: String,
    threads: i32,
    n_ctx: usize,
}

impl Engine {
    /// Lädt ein Modell von der Platte.
    ///
    /// Der Rückgabewert nennt den geladenen Pfad zurück, weil der Aufrufer ihn für
    /// die Meldung braucht und weil es der einzige Ort ist, an dem sichergestellt
    /// werden muss, dass die geladene Datei auch die gemeinte ist.
    pub fn laden(pfad: &Path, n_ctx: usize, threads: i32) -> Result<Engine, String> {
        if !pfad.exists() {
            return Err(format!("Die Modelldatei gibt es nicht: {}", pfad.display()));
        }

        let backend = LlamaBackend::init()
            .map_err(|fehler| format!("Die Engine ließ sich nicht starten: {fehler}"))?;

        let parameter = LlamaModelParams::default()
            .with_n_gpu_layers(0)
            .with_use_mmap(true)
            .with_use_mlock(false);

        let model = LlamaModel::load_from_file(&backend, pfad, &parameter)
            .map_err(|fehler| format!("Das Modell ließ sich nicht laden: {fehler}"))?;

        let vorlage = Self::vorlage_lesen(&model, pfad)?;

        Ok(Engine {
            _backend: backend,
            model,
            vorlage,
            threads: threads.max(1),
            n_ctx: n_ctx.max(256),
        })
    }

    /// Die Chat-Vorlage, die im Modell steckt, mit dem Ersatz daneben.
    ///
    /// Der Pfad ist nur für die Fehlermeldung da: Es soll unterscheidbar bleiben,
    /// ob ein Modell keine Vorlage mitbringt (das ist ein Hinweis auf die
    /// Herkunft der Datei) oder ob die Vorlage unlesbar ist (das ist ein Fehler
    /// auf der Platte).
    fn vorlage_lesen(model: &LlamaModel, pfad: &Path) -> Result<String, String> {
        let im_modell = model
            .chat_template(None)
            .ok()
            .and_then(|vorlage| vorlage.to_string().ok())
            .filter(|text| !text.trim().is_empty());

        if let Some(text) = im_modell {
            if text.len() <= MAX_VORLAGE_BYTES {
                return Ok(text);
            }

            return Err(format!(
                "Die Chat-Vorlage in {} ist größer als {} KiB. Mimir verwendet sie nicht.",
                pfad.display(),
                MAX_VORLAGE_BYTES / 1024
            ));
        }

        Ok(ERSATZ_VORLAGE.to_string())
    }

    /// Rechnet eine Antwort.
    ///
    /// Der Aufruf ist blockierend: Er rechnet auf den Worker-Threads der Engine und
    /// gibt den Text erst ganz am Ende zurück. Das ist bewusst so – siehe
    /// [`antwort_als_zeilen`], das daraus den Strom macht, den die Oberfläche
    /// erwartet.
    pub fn antwort(
        &mut self,
        nachrichten: &[LlamaChatMessage],
        system: Option<&str>,
        abbrechen: &(dyn Fn() -> bool + Send + Sync),
        ausgabe: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<String, String> {
        // Der Systemtext kommt als eigene Nachricht voran: Das ist der Ort, an dem
        // auch die Werkzeugbeschreibung steht, und beide gehören in dieselbe Ecke.
        let mut voll: Vec<LlamaChatMessage> = Vec::with_capacity(nachrichten.len() + 1);
        if let Some(text) = system.filter(|text| !text.trim().is_empty()) {
            voll.push(
                LlamaChatMessage::new("system".to_string(), text.to_string())
                    .map_err(|fehler| format!("Die Systemanweisung ist unbrauchbar: {fehler}"))?,
            );
        }
        voll.extend_from_slice(nachrichten);

        let vorlage = LlamaChatTemplate::new(&self.vorlage)
            .map_err(|_| "Die Chat-Vorlage des Modells ist unbrauchbar".to_string())?;

        let prompt = self
            .model
            .apply_chat_template(&vorlage, &voll, true)
            .map_err(|fehler| format!("Die Vorlage ließ sich nicht anwenden: {fehler}"))?;

        self.erzeugen(&prompt, abbrechen, ausgabe)
    }

    /// Erzeugt Token für einen fertigen Prompt.
    ///
    /// Der Kontext entsteht hier und nicht im Modell: Er hält den KV-Speicher für
    /// das ganze Fenster, und das sind bei 2048 Token mehrere hundert MiB. Hielte
    /// das Modell ihn, bliebe er auch dann belegt, wenn gerade keine Frage läuft –
    /// und bei vier GiB Arbeitsspeicher ist das der Unterschied zwischen „läuft"
    /// und „läuft nicht".
    ///
    /// `abbrechen` wird zwischen zwei Token geprüft. Ein Token dauert auf einem
    /// schwachen Rechner gut eine Zehntelsekunde, das reicht für einen Abbruch, der
    /// sich sofort anfühlt.
    fn erzeugen(
        &mut self,
        prompt: &str,
        abbrechen: &(dyn Fn() -> bool + Send + Sync),
        ausgabe: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<String, String> {
        let token = self.model.vocab().tokenize(prompt.as_bytes(), true, true);
        if token.is_empty() {
            return Err("Der Auftrag kam ohne ein einziges Token an".to_string());
        }

        // Passt der Auftrag nicht in das Fenster, wird gekürzt – von hinten, denn
        // dort stehen die Nachrichten, und vorn steht die Systemanweisung mit den
        // Werkzeugen. Ein Auftrag ohne Werkzeugbeschreibung liefert keine
        // Termine, also geht die Anweisung zuletzt verloren.
        //
        // **Das Budget der Antwort ist das, was übrig bleibt, nicht das Maximum.**
        // Vorher stand hier `token.len() + MAX_ANTWORT_TOKEN <= platz`, und damit
        // scheiterte **jede** Anfrage bei jedem Fenster, das kleiner war als die
        // 8192 Token Obergrenze – also bei jedem normalen Fenster. Der Auftrag
        // passte immer, der Platz für die Antwort nie. Die Grenze wird deshalb
        // genommen als das Kleinere aus dem Höchstwert und dem, was das Fenster
        // übrig lässt.
        let platz = self.n_ctx;
        let (auftrag_budget, antwort_budget) = fenster_aufteilen(platz, token.len());

        // Zu wenig Platz für eine brauchbare Antwort: Das ist kein Auftrag, den man
        // kürzen kann – es ist ein zu kleines Fenster, und das sagt man so.
        if antwort_budget < MIN_ANTWORT_TOKEN {
            return Err(format!(
                "Das Fenster von {} Token ist zu klein: Der Auftrag braucht {} Token, \
                 für eine Antwort bleiben {}.",
                platz, auftrag_budget, antwort_budget
            ));
        }

        let einstieg = if token.len() <= auftrag_budget {
            token.as_slice()
        } else {
            &token[token.len() - auftrag_budget..]
        };

        // `u32`, weil llama.cpp diese Zahlen dort als `uint32` führt. Das Fenster ist
        // gegen `MAX_FENSTER` begrenzt, der Rest wäre ein Rechner mit mehr als vier
        // Milliarden Token im Fenster.
        let fenster = u32::try_from(self.n_ctx).unwrap_or(u32::MAX);

        // Wie viele Token in einen Durchgang gehen.
        //
        // **Der Auftrag wird in Blöcke zerlegt, und das ist keine Optimierung.**
        // llama.cpp verlangt für einen Durchgang höchstens `n_batch` Token und
        // beendet das Programm mit einer Zusicherung, wenn mehr kommen
        // (`GGML_ASSERT(n_tokens_all <= cparams.n_batch)`). Ein Auftrag mit
        // Systemanweisung und Werkzeugbeschreibung hat gut 1200 Token – bei 512
        // also einen Abbruch im ersten Durchgang. Ein Absturz mitten in einer
        // Anfrage, deren Text man im Chat sieht, ist der denkbar schlechteste
        // Ausgang: Der Benutzer hat getippt und erfährt nur „Abgebrochen".
        // `clamp` statt `min().max()`: Die Reihenfolge ist hier wichtig – erst
        // nach oben begrenzen, dann nach unten – und genau das macht `clamp`.
        let stapel = (fenster as usize).clamp(1, STAPEL_GROESSE as usize);

        let parameter = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(fenster))
            .with_n_batch(fenster.min(STAPEL_GROESSE))
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads);

        let mut kontext = self
            .model
            .new_context(&self._backend, parameter)
            .map_err(|fehler| format!("Die Engine ließ sich nicht starten: {fehler}"))?;

        // Logits braucht nur das letzte Token des Auftrags: Daraus wird der erste
        // erzeugte Token gezogen. Für alle davor wären sie ein Ergebnis, das
        // niemand liest.
        let mut position = 0i32;
        let durchgaenge = durchgaenge(einstieg.len(), stapel);
        debug_assert!(durchgaenge > 0, "der Auftrag war leer");

        for (nummer, block) in einstieg.chunks(stapel).enumerate() {
            let letzter = (nummer + 1) * stapel >= einstieg.len();
            let mut batch = LlamaBatch::new(block.len(), 1);

            for (versatz, id) in block.iter().enumerate() {
                batch
                    .add(
                        *id,
                        position + versatz as i32,
                        &[0],
                        letzter && versatz + 1 == block.len(),
                    )
                    .map_err(|fehler| format!("Der Auftrag ließ sich nicht einreihen: {fehler}"))?;
            }

            kontext
                .decode(&mut batch)
                .map_err(|fehler| format!("Die Engine rechnete nicht: {fehler}"))?;

            position += block.len() as i32;
        }

        let mut text = String::new();
        let mut sampler = self.sampler();

        // Die Position wandert mit dem Token: Der erste erzeugte Token ist der
        // nächste nach dem Auftrag, und jeder weitere hängt sich dahinter.
        for _ in 0..antwort_budget {
            if abbrechen() {
                return Err(ABBRUCH_TEXT.to_string());
            }

            let mut daten = kontext.token_data_array();
            sampler.apply(&mut daten);

            let Some(id) = daten.selected_token() else {
                break;
            };
            sampler.accept(id);

            let vokabular = self.model.vocab();
            if vokabular.is_eog(id) {
                break;
            }

            // `from_utf8` kann scheitern, wenn das Stueck mitten in einem
            // mehr Byte grossen Zeichen abgeschnitten ist. Dann wandert nichts in
            // die Anzeige – der Text waere sonst ein Ersatzzeichen, das der
            // Benutzer zu Gesicht bekommt.
            if let Ok(teil) = String::from_utf8(vokabular.token_to_piece(id, true, None)) {
                text.push_str(&teil);

                // **Jedes Token geht sofort raus.** Das ist der Unterschied zwischen
                // „es passiert etwas" und „es ist still": Auf einem Rechner ohne
                // Grafik braucht ein Token gut eine Zehntelsekunde, eine ganze
                // Antwort also Minuten – und ohne diese Meldung stünde in dieser
                // Zeit nichts auf dem Bildschirm. Der Benutzer säbe eine leere
                // Blase und wüsste nicht, ob Mimir hängt oder arbeitet.
                ausgabe(&teil);
            }

            // Jeder Schritt bekommt einen eigenen Stapel mit genau einem Token: Der
            // erste Stapel trägt noch alle Positionen, und llama.cpp verlangt für
            // den nächsten Schritt eine Position, die dahinter liegt.
            let mut naechster = LlamaBatch::new(1, 1);
            naechster
                .add(id, position, &[0], true)
                .map_err(|fehler| format!("Die Antwort ließ sich nicht fortsetzen: {fehler}"))?;
            kontext
                .decode(&mut naechster)
                .map_err(|fehler| format!("Die Engine rechnete nicht: {fehler}"))?;

            // Der nächste Token hängt an das Ende. Ohne das schickt der zweite
            // Schritt dieselbe Position wie der erste, und llama.cpp verlangt
            // aufeinanderfolgende – es lehnt den Stapel ab und die Antwort endet
            // nach einem einzigen Token.
            position += 1;

            if wiederholt(&text) {
                break;
            }
        }

        if text.is_empty() {
            return Err("Das Modell hat ohne ein einziges Textzeichen geantwortet".to_string());
        }

        Ok(text)
    }

    /// Die Reihenfolge der Stichprobe.
    ///
    /// Bewusst konservativ und ohne Zufall: Ein Hilfsmodell soll antworten wie ein
    /// Modell, nicht würdeln. `top_k` und `top_p` schneiden die Auswahl ein, die
    /// Temperatur erniedrigt sie, und `dist` wählt daraus – ohne einen festen Seed,
    /// weil zwei gleiche Fragen nicht bitgleich antworten müssen.
    fn sampler(&self) -> LlamaSampler {
        LlamaSampler::chain_simple([
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|dauer| dauer.as_nanos() as u32)
                    .unwrap_or(0),
            ),
        ])
    }
}

/// Die kürzeste und längste Länge, die als Wiederholung gilt.
///
/// Unten, weil ein Loop bei einem einzelnen Zeichen noch kein Text ist, und oben,
/// weil eine Schleife, die mehr als diesen Umfang braucht, nicht mehr auffällt: Sie
/// liest sich wie ein sehr gründlicher Benutzer.
const MIN_WIEDERHOLUNG: usize = 12;
const MAX_WIEDERHOLUNG: usize = 160;

/// Wie oft hintereinander derselbe Abschnitt stehen muss.
///
/// Zweimal wäre der Normalfall: Ein Satz, der sich wiederholt, ist meistens Absicht.
const MAX_WIEDERHOLUNG_HALT: usize = 2;

/// Ob das Ende der Antwort aus einem wiederholten Abschnitt besteht.
///
/// Gesucht wird die **Periode**, nicht eine feste Blocklänge: Ein Modell, das in
/// eine Schleife gerät, wiederholt seinen letzten Abschnitt Token für Token, und
/// dessen Länge hängt vom Text darin ab. Ein Vergleich fester Blöcke fände eine
/// Wiederholung nur, wenn ihre Länge gerade passte – der Fehler wäre still, und eine
/// Schleife, die unbegrenzt Rechenzeit verbraucht, sieht man nicht.
///
/// Geprüft wird das Ende, nicht der ganze Text: Eine lange Antwort mit einem
/// wiederholten Satz irgendwo ist keine Schleife.
fn wiederholt(text: &str) -> bool {
    let zeichen: Vec<char> = text.chars().collect();
    let ende = zeichen.len();

    for periode in MIN_WIEDERHOLUNG..=MAX_WIEDERHOLUNG {
        if ende < periode * (MAX_WIEDERHOLUNG_HALT + 1) {
            break;
        }

        // Drei gleiche Abschnitte hintereinander: Der erste Vergleich sähe auch bei
        // zufälliger Übereinstimmung richtig aus, der zweite fängt den Zufall weg.
        if abschnitt(&zeichen, ende - periode, periode)
            == abschnitt(&zeichen, ende - 2 * periode, periode)
            && abschnitt(&zeichen, ende - 2 * periode, periode)
                == abschnitt(&zeichen, ende - 3 * periode, periode)
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests;
