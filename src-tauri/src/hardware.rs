//! Was Mimir über den Rechner weiß, auf dem es läuft.
//!
//! Zweck: In Variante A bringt Mimir die Engine mit, das Modell wählt der Benutzer
//! selbst. Damit muss Mimir sagen können, **welches** Modell in diese Maschine
//! passt – sonst steht da eine Liste mit sieben Möglichkeiten und der Benutzer
//! ratet. Die Antwort ist eine Eigenschaft des Rechners, keine des Benutzers.
//!
//! Drei Quellen, aus zwei Gründen getrennt:
//!
//! * **`sysinfo` für Prozessor und Arbeitsspeicher.** Die kommen über die
//!   Schnittstellen des Betriebssystems, damit sie auch auf Windows und macOS
//!   da sind. Ein eigenes Lesen von `/proc` wäre nur auf Linux richtig – und Mimir
//!   soll nicht auf Linux beschränkt sein.
//! * **CPUID für die Befehlssätze.** Ob die Engine ihre schnellen Kerne benutzen
//!   kann, entscheidet die CPU, und das weiß sie über CPUID, nicht über eine
//!   Textdatei. Auf Linux ginge es auch über `/proc/cpuinfo`; das gäbe es nur auf
//!   Linux.
//! * **`PATH` für die Grafik.** [`Grafik`] sagt, was *nachweisbar* ist. Mehr geht
//!   ohne zusätzliche Bibliothek nicht, und mehr wäre hier geraten.
//!
//! [`Hardware::einordnung`] ist die ableitbare Grobeinteilung. Sie ist noch keine
//! Empfehlung: Der Katalog der konkreten Modelle kommt später dazu, und diese
//! Stufen sind die Eingabe dafür.

use serde::Serialize;

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

/// Ein GiB in Bytes. Der Umrechnungskoeffizient für alle RAM-Angaben.
const GIB: u64 = 1024 * 1024 * 1024;

/// Die Angaben zum Rechner, aus denen ein Modell gewählt wird.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Hardware {
    /// Modellname der CPU, soweit der Rechner ihn nennt.
    pub prozessor: String,
    /// Gleichzeitig nutzbare Rechenthreads. Das ist die Zahl, die die
    /// Geschwindigkeit bestimmt; getrennte Kerne zu zählen gäbe nur eine Zahl
    /// mehr, die niemand braucht.
    pub threads: usize,
    /// Gesamter Arbeitsspeicher in GiB, abgerundet.
    pub ram_gib: Option<u32>,
    /// Noch freier Arbeitsspeicher in GiB, abgerundet. Das ist die Zahl, die
    /// tatsächlich zählt: Ein Modell passt in den freien Speicher, nicht in den
    /// gesamten.
    pub ram_frei_gib: Option<u32>,
    /// Die Befehlssätze der CPU. `None` heißt: nicht ermittelbar – nicht
    /// „vorhanden“.
    pub befehlssaetze: Option<Befehlssaetze>,
    /// Was an Grafik nachweisbar ist.
    pub grafik: Grafik,
    /// Was aus diesen Angaben folgt.
    pub einordnung: Einordnung,
}

/// Die SIMD-Befehlssätze, auf die es ankommt.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Befehlssaetze {
    /// 256-Bit-Gleitkommaregister.
    pub avx: bool,
    /// 256-Bit mit ganzzahligen Befehlen. Die Grundlage, auf der die schnellen
    /// Kerne der Engine entstehen.
    pub avx2: bool,
    /// 512-Bit.
    pub avx512: bool,
    /// ARM-Vektorbefehle. Auf `aarch64` immer vorhanden, sonst nie.
    pub neon: bool,
}

impl Befehlssaetze {
    /// Ob die Engine hier schnell rechnen kann.
    ///
    /// Falsch heißt nicht „etwas langsamer“, sondern „die Engine fällt auf ihre
    /// Skalarkerne zurück“. Das ist ein Unterschied, den man dem Benutzer sagen
    /// muss, statt es im Kleingedruckten zu verstecken.
    pub fn schnell(&self) -> bool {
        self.avx2 || self.neon
    }
}

/// Die Grafik, und wie weit sie bekannt ist.
///
/// Die Grenze ist Absicht: Mehr lässt sich ohne weitere Bibliothek nicht
/// belastbar sagen, und eine erfundene Angabe wäre hier schädlich – sie würde
/// ein Modell empfehlen, das auf dieser Maschine nicht läuft.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Grafik {
    /// Eine NVIDIA-Karte ist nachweisbar.
    Nvidia,
    /// Apple Silicon: eine Grafik ist vorhanden, gleich mit dem Prozessor.
    Apple,
    /// Vorhanden, aber ohne Nachweis. Bleibt so, statt zu raten.
    Unbekannt,
}

impl Grafik {
    /// Ob Mimir angenommen darf, dass die Engine die Grafik benutzt.
    ///
    /// Das ist eine **Bauentscheidung**, keine Eigenschaft des Rechners: Eine
    /// Engine greift nur dann auf die Grafik zu, wenn sie entsprechend übersetzt
    /// wurde. Deshalb steht hier „nein“ – Mimir liefert sich ohne CUDA und kann
    /// es nicht nachrüsten. Sobald die Engine mit Grafikunterstützung gebaut wird,
    /// ist das hier die eine Stelle, die es umzustellen gilt.
    pub fn nutzbar(&self) -> bool {
        false
    }
}

/// Wie gut der Rechner für ein Modell geeignet ist.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stufe {
    /// Zu klein, um ein Modell zu tragen, das Werkzeugargumente zuverlässig
    /// liefert. Lieber gar keines als eines, das jedes zweite Mal danebenliegt.
    ZuKlein,
    Klein,
    Mittel,
    Gross,
    SehrGross,
}

/// Die ableitbare Grobeinteilung des Rechners.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Einordnung {
    pub stufe: Stufe,
    /// Größtes sinnvolles Modell in Milliarden Parametern. Ein Richtwert, keine
    /// Grenze: Der Katalog nennt später die konkreten Modelle.
    pub max_milliarden: u32,
    /// Was der Benutzer wissen muss, bevor er ein Modell wählt. Leer heißt: nichts
    /// Besonderes.
    pub hinweis: String,
}

/// Der kleinste Arbeitsspeicher, bei dem ein Modell überhaupt sinnvoll läuft.
///
/// Die Grenze kommt aus der Rechnung, nicht aus dem Gefühl: Unter zwei GiB passt
/// neben Mimir und der WebView kein Modell mit nennenswertem Kontext. Das kleinste
/// brauchbare GGUF liegt bei rund 400 MB, und davon bleiben nach Kontextpuffer und
/// Anwendungen zu wenig übrig, um noch sinnvoll zu arbeiten.
const RAM_MINDESTENS_GIB: u32 = 2;

/// Liest den Rechner aus.
///
/// Erwartet wird nichts: Ein unlesbarer Wert führt zu `None` und damit zu „unbekannt“,
/// nicht zu einem Abbruch. Mimir soll auch ohne die Erkennung starten – sie ist eine
/// Hilfe beim Wählen, keine Voraussetzung dafür, den Server zu erreichen.
pub fn erkenne() -> Hardware {
    let (prozessor, threads, ram_gib, ram_frei_gib) = rechner_angaben();
    let befeehlssaetze = Some(befehlssaetze());
    let grafik = bestimme_grafik();

    Hardware {
        einordnung: einordnung(ram_gib, ram_frei_gib, befeehlssaetze),
        prozessor,
        threads,
        ram_gib,
        ram_frei_gib,
        befehlssaetze: befeehlssaetze,
        grafik,
    }
}

/// Was bei der Erkennung abgefragt wird.
///
/// Steht in einer eigenen Funktion, weil hier ein Fehler einmal passiert ist und
/// unsichtbar blieb: `MemoryRefreshKind::nothing()` heißt *nichts* auffrischen,
/// nicht *alles ohne Umweg*. Damit blieb `total_memory()` bei null stehen, und eine
/// Erkennung, die auf jedem Rechner „0 GB gesamt“ meldet, schlug für jedes Modell
/// die Empfehlung aus – es sah aus wie ein zu schwacher Rechner, war aber eine
/// fehlende Abfrage.
///
/// Der Tausch wird nicht geholt; er wird nirgends gelesen. Für den Prozessor
/// genügt `nothing()`: Der Name kommt aus der CPUID und wird ohnehin gelesen.
fn abfrage_art() -> RefreshKind {
    RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::nothing())
        .with_memory(MemoryRefreshKind::nothing().with_ram())
}

/// Prozessorname, Threads und Arbeitsspeicher.
///
/// `System::new_all()` würde zusätzlich jede Prozessliste aufbauen – bei einer
/// Anwendung, die das einmal beim Start macht, Verschwendung von Zeit und
/// Arbeitsspeicher.
fn rechner_angaben() -> (String, usize, Option<u32>, Option<u32>) {
    let art = abfrage_art();

    let mut system = System::new_with_specifics(art);
    system.refresh_specifics(art);

    let prozessor = system
        .cpus()
        .iter()
        .map(|cpu| cpu.brand().trim())
        .find(|name| !name.is_empty())
        .unwrap_or_default()
        .to_string();

    let threads = system.cpus().len().max(1);
    let ram_gib = gib(system.total_memory());
    let ram_frei_gib = gib(system.available_memory());

    (prozessor, threads, ram_gib, ram_frei_gib)
}

/// Bytes in GiB, **abgerundet**.
///
/// Abgerundet wird deshalb, weil hier eine zu kleine Zahl nur eine zu vorsichtige
/// Empfehlung bedeutet, eine zu große aber ein Modell, das nicht hineinpasst und
/// anfängt, auf die Festplatte auszulagern. Ein Rechner mit 3,07 GiB frei darf nicht
/// wie einer mit 4 aussehen und dadurch eine Stufe zu hoch landen.
fn gib(bytes: u64) -> Option<u32> {
    (bytes / GIB).try_into().ok()
}

/// Die Befehlssätze der CPU.
///
/// Die Abfrage läuft über CPUID, also zur Laufzeit auf dem Rechner selbst. Deshalb
/// ist sie hier nicht prüfbar: Sie kann nur sagen, was der Rechner kann, auf dem sie
/// läuft. Geprüft wird weiter unten die Entscheidung, die daraus folgt.
fn befehlssaetze() -> Befehlssaetze {
    #[cfg(target_arch = "x86_64")]
    {
        Befehlssaetze {
            avx: std::arch::is_x86_feature_detected!("avx"),
            avx2: std::arch::is_x86_feature_detected!("avx2"),
            avx512: std::arch::is_x86_feature_detected!("avx512f"),
            neon: false,
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Auf ARM ist NEON kein Merkmal, sondern Teil der Architektur: Es gibt
        // keine aarch64-CPU ohne. Ein Aufzählen nach dem Muster von x86 würde hier
        // immer `false` liefern und damit jeden ARM-Rechner für zu langsam halten.
        Befehlssaetze {
            avx: false,
            avx2: false,
            avx512: false,
            neon: true,
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        Befehlssaetze::default()
    }
}

/// Die Grafik, soweit sie sich ohne weitere Bibliothek nachweisen lässt.
///
/// Gesucht wird nur nach *Belegen*, nicht nach Bestätigung: Fehlt `nvidia-smi`,
/// kann daraus nicht „keine Grafik“ folgen – es könnte eine AMD- oder Intel-Karte
/// sein, oder ein Apple-Chip. Deshalb `Unbekannt` statt `Keine`: Eine fehlende
/// Karte ist eine schlechte Nachricht, eine nicht gefundene ist keine.
fn bestimme_grafik() -> Grafik {
    if cfg!(target_arch = "aarch64") && cfg!(target_os = "macos") {
        return Grafik::Apple;
    }

    if liegt_im_pfad("nvidia-smi") || std::path::Path::new("/dev/nvidiactl").exists() {
        return Grafik::Nvidia;
    }

    Grafik::Unbekannt
}

/// Ob im `PATH` eine Datei dieses Namens liegt.
///
/// Selbst gesucht statt `which` oder `where` aufzurufen: Das spart einen
/// Kindprozess bei einer Auskunft, die der Benutzer beim Start abwartet.
fn liegt_im_pfad(datei: &str) -> bool {
    let Some(pfad) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&pfad).any(|verzeichnis| verzeichnis.join(datei).is_file())
}

/// Ordnet RAM und Befehlssätze einer Stufe zu.
///
/// Aufgeteilt nach dem **freien** Speicher, weil das die Grenze ist, an der ein
/// Modell anfängt, in den Auslagerungsspeicher zu gehen – und von dort ist die
/// Antwort um Größenordnungen langsamer. Nur wenn der freie Speicher unbekannt
/// ist, wird der gesamte verwendet, mit derselben Skala: Das ist die schwächere
/// Zahl, aber bessere als gar keine.
fn einordnung(
    ram_gib: Option<u32>,
    ram_frei_gib: Option<u32>,
    befeehlssaetze: Option<Befehlssaetze>,
) -> Einordnung {
    let speicher = ram_frei_gib.or(ram_gib);

    let (stufe, max_milliarden) = match speicher {
        None => (Stufe::ZuKlein, 0),
        Some(0..RAM_MINDESTENS_GIB) => (Stufe::ZuKlein, 0),
        // Ein 1,5B-Modell passt mit einem kleinen Kontext daneben; mehr würde den
        // Speicher beim ersten langen Verlauf auffressen.
        Some(2..4) => (Stufe::Klein, 2),
        // 3B in Q4_K_M sind rund 1,9 GB und brauchen etwa ein GiB Kontextpuffer.
        Some(4..8) => (Stufe::Mittel, 3),
        // 7B in Q4_K_M sind rund 4,0 GB – der übliche Punkt, an dem sich ein
        // kleines Modell noch flüssig anfühlt.
        Some(8..16) => (Stufe::Gross, 7),
        Some(_) => (Stufe::SehrGross, 13),
    };

    Einordnung {
        stufe,
        max_milliarden,
        hinweis: hinweis(speicher, befeehlssaetze, max_milliarden),
    }
}

/// Der Text, der beim Modellwählen dazustehen muss.
fn hinweis(
    speicher: Option<u32>,
    befeehlssaetze: Option<Befehlssaetze>,
    max_milliarden: u32,
) -> String {
    let mut teile: Vec<String> = Vec::new();

    // Der wichtigste Fall zuerst: Ein Rechner ohne die nötigen Befehlssätze
    // bekommt kein Modell angeboten, sondern eine Begründung.
    if let Some(saetze) = befeehlssaetze {
        if !saetze.schnell() {
            teile.push(
                "Dieser Prozessor kann die schnellen Kerne der Engine nicht benutzen – \
                 er kann rechnen, aber jede Antwort würde Minuten brauchen."
                    .to_string(),
            );
        }
    }

    if speicher.is_none() {
        teile.push("Der Arbeitsspeicher ließ sich nicht auslesen.".to_string());
    } else if max_milliarden == 0 {
        teile.push(
            "Für ein Modell, das Termine zuverlässig anlegt, reicht der Arbeitsspeicher nicht."
                .to_string(),
        );
    }

    teile.join(" ")
}

#[cfg(test)]
mod tests;
