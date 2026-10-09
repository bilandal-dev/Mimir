//! Die Modelle, die Mimir anbietet, und die auf diesem Rechner liegen.
//!
//! Zweck: In Variante A bringt Mimir die Engine mit, das Modell wählt der Benutzer.
//! Damit diese Wahl eine ist und kein Ratespiel, braucht Mimir zweierlei: eine
//! kuratierte Liste dessen, was es anbietet, und den Abgleich dieser Liste mit dem
//! Rechner aus [`crate::hardware`].
//!
//! **Warum eine feste Liste und nicht die ganze Registry.** Ein Katalog, der
//! alles anbietet, was jemand veröffentlicht, ist keine Empfehlung mehr, sondern
//! eine Suchmaschine. Mimir steht aber gerade für wenige Modelle ein: die kleinen
//! Qwen2.5-Instruct-Ausgaben von Qwen selbst, Apache-2.0, als GGUF in einer
//! Quantisierung, die auf schwacher Hardware überhaupt erst läuft. Zu jedem Eintrag
//! steht eine ehrliche Anmerkung dabei, denn gerade die kleinsten Modelle sind im
//! Werkzeugaufruf die schwächsten – wer das nicht dazusagt, verkauft eine
//! Enttäuschung.
//!
//! **Der Katalog ist eine Konstante**, keine Tabelle in einer Datenbank. Das ist
//! Absicht: Er ändert sich nur mit einer neuen Mimir-Fassung, und dann soll er mit
//! ihr ausgeliefert werden. Ein heruntergeladenes Modell merkt sich Mimir an seinem
//! Dateinamen – es braucht keinen Eintrag, der auseinanderlaufen könnte.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sysinfo::Disks;

use crate::hardware::Hardware;

/// Ein MiB in Bytes, für die Größenangaben.
const MIB: u64 = 1024 * 1024;

/// Endung, in der alle angebotenen Modelle kommen.
const ENDUNG: &str = "gguf";

/// Endung der Teil-Datei während eines Ladens.
const ENDUNG_TEIL: &str = "teil";

/// Wie lange ein Download laufen darf, ohne ein einziges Byte zu liefern.
///
/// Ohne diese Grenze bliebe ein Verbindungsproblem ein hängender Ladevorgang. Der
/// Wert ist weit größer als bei einer Abfrage: Ein Download, der Daten liefert,
/// darf langsam sein.
const LADEN_LEISE_GRENZE_SECS: u64 = 60;

/// Ob eine geladene Datei als vollständig gilt.
///
/// **Warum hier eine Spanne und keine Gleichheit.** `groesse_mib` im Katalog ist
/// aufgerundet, weil 468,64 MiB keine lesbare Zahl sind. Aus dieser Angabe folgt
/// keine genaue Bytezahl, sondern ein Bereich: Die Datei ist ungefähr so groß wie
/// `groesse_mib` MiB, und kann ein Stück weniger oder mehr sein. Vorher wurde auf
/// Gleichheit geprüft, und das Ergebnis war, dass **jedes** Modell als „unvollständig"
/// verworfen wurde – 469 MiB erwartet, 468,64 MiB bekommen, Unterschied 0,36 MiB.
/// Der Download lief bis 100 % und wurde dann weggeräumt.
///
/// **Die Spanne nach unten ist das Kleinere aus einer MiB und einem Prozent.** Die
/// eine MiB folgt aus dem Aufrunden des Katalogwerts; das Prozent ist die Grenze,
/// unter der eine Datei als wirklich abgebrochen gilt. Das Kleinere zu nehmen ist
/// wichtig: Bei einem echten Modell sind 1 MiB ein halbes Promille, bei einer
/// Testdatei von zwei MiB wären sie die Hälfte – eine feste MiB-Grenze hielte dort
/// jede halbe Datei für vollständig.
fn vollstaendig(geladen: u64, erwartet: u64) -> bool {
    if geladen == 0 {
        return false;
    }

    let prozent = (erwartet / 100).min(MIB);
    let hoechstens = (erwartet as f64 * UEBERLAUF_FAKTOR) as u64;

    geladen >= erwartet.saturating_sub(prozent) && geladen <= hoechstens
}

/// Wie viel größer eine angefangene Datei sein darf als erwartet.
///
/// Der Vergleich ist die einzige Prüfung, die ein unvollständig geladenes Modell
/// vom fertigen unterscheidet – der Inhalt wird nicht nachgerechnet.
const UEBERLAUF_FAKTOR: f64 = 1.05;

/// Ein Modell aus dem Katalog.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eintrag {
    /// Kurzname, zugleich der Name der Datei ohne Endung. Er kommt in keine
    /// Befehle und Pfade außer dem eigenen, sonst müsste er geprüft werden.
    pub id: &'static str,
    /// Wie es in der Oberfläche heißt.
    pub name: &'static str,
    /// Wofür das Modell gedacht ist.
    pub beschreibung: &'static str,
    /// Größe der Parameter in Milliarden.
    pub parameter_milliarden: u32,
    /// Platzbedarf der Datei in MiB, aufgerundet.
    pub groesse_mib: u32,
    /// Die Quantisierung im Namen, damit der Benutzer sie nicht nachschlagen muss.
    pub quantisierung: &'static str,
    /// Lizenz des Modells, im Klartext. Sie gehört hierher, weil sie beim
    /// Herunterladen etwas bedeutet und später am Nachschlageort niemand mehr
    /// findet.
    pub lizenz: &'static str,
    /// Freier Arbeitsspeicher, ab dem das Modell sinnvoll läuft.
    pub ram_mindestens_gib: u32,
    /// Was das Modell nicht kann. Ohne diesen Satz wäre die Oberfläche eine
    /// Werbung.
    pub anmerkung: &'static str,
    /// Das Repository auf Hugging Face.
    pub repository: &'static str,
    /// Der Dateiname im Repository.
    pub datei: &'static str,
}

impl Eintrag {
    /// Der vollständige Name, unter dem das Modell abgelegt wird.
    pub fn ablage_name(&self) -> String {
        format!("{id}.{ENDUNG}", id = self.id)
    }

    /// Der Pfad, an dem das Modell liegt oder liegen wird.
    pub fn pfad_in(&self, ordner: &Path) -> PathBuf {
        ordner.join(self.ablage_name())
    }
}

/// Der Katalog. Feste Liste, siehe oben.
pub const KATALOG: &[Eintrag] = &[
    Eintrag {
        id: "qwen2.5-0.5b",
        name: "Qwen2.5 0,5B",
        beschreibung: "Kleinstes Angebot. Für den Chat mit einem Modell, das Termine anlegt.",
        parameter_milliarden: 1,
        groesse_mib: 469,
        quantisierung: "Q4_K_M",
        lizenz: "Apache-2.0",
        ram_mindestens_gib: 2,
        anmerkung: "Sehr schwach im Werkzeugaufruf: Es wählt das falsche Werkzeug oder vergisst \
                    ein Feld. Für Termine nicht zu gebrauchen, für kurze Fragen schon.",
        repository: "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
        datei: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
    },
    Eintrag {
        id: "qwen2.5-1.5b",
        name: "Qwen2.5 1,5B",
        beschreibung: "Der kleinste Eintrag, der Kalenderargumente brauchbar liefert.",
        parameter_milliarden: 2,
        groesse_mib: 1066,
        quantisierung: "Q4_K_M",
        lizenz: "Apache-2.0",
        ram_mindestens_gib: 3,
        anmerkung: "Verwechselt Tage und Uhrzeiten, braucht Rückfragen. Rechnet langsam, aber \
                    antwortet.",
        repository: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
        datei: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
    },
    Eintrag {
        id: "qwen2.5-3b",
        name: "Qwen2.5 3B",
        beschreibung: "Das brauchbare Modell: Die Empfehlung für die meisten Rechner.",
        parameter_milliarden: 3,
        groesse_mib: 2008,
        quantisierung: "Q4_K_M",
        lizenz: "Apache-2.0",
        ram_mindestens_gib: 4,
        anmerkung: "Braucht rund 4 GiB freien Speicher und mehrere Minuten für die erste Antwort \
                    auf einem Rechner ohne Grafik.",
        repository: "Qwen/Qwen2.5-3B-Instruct-GGUF",
        datei: "qwen2.5-3b-instruct-q4_k_m.gguf",
    },
];

/// Der Katalogeintrag mit dieser Kennung.
pub fn aus_id(id: &str) -> Option<&'static Eintrag> {
    KATALOG.iter().find(|eintrag| eintrag.id == id)
}

/// Ob das Modell auf diesen Rechner passt.
///
/// Zwei Bedingungen, und beide sind Hürden statt Wünsche: Der Rechner muss die
/// schnellen Kerne der Engine besitzen, und es muss Speicher für die Gewichte und
/// den Kontext da sein. Wird die erste nicht erfüllt, ist es keine Modellauswahl
/// mehr, sondern eine Begründung – die liefert [`Hardware::einordnung`], nicht
/// diese Funktion.
pub fn passt_zu(eintrag: &Eintrag, hardware: &Hardware) -> bool {
    hardware
        .befehlssaetze
        .is_some_and(|saetze| saetze.schnell())
        && hardware.ram_frei_gib.unwrap_or(0) >= eintrag.ram_mindestens_gib
}

/// Die Empfehlungen für diesen Rechner, vom kleinsten auf.
///
/// Aufsteigend sortiert, weil das kleinste passende Modell das ist, das am
/// schnellsten antwortet – und weil eine Liste, die mit dem größten beginnt, zum
/// Probieren einlädt. Die Oberzeile des Katalogs hat den Größzwang damit schon
/// erledigt: Wer schneller werden will, nimmt den nächsten Eintrag.
pub fn empfehlungen(hardware: &Hardware) -> Vec<&'static Eintrag> {
    let mut passend: Vec<&'static Eintrag> = KATALOG
        .iter()
        .filter(|eintrag| passt_zu(eintrag, hardware))
        .collect();

    passend.sort_by_key(|eintrag| eintrag.groesse_mib);
    passend
}

/// Ein Modell, das auf diesem Rechner liegt.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Geladen {
    pub id: String,
    pub name: String,
    /// Größe der Datei in MiB. Sie kommt von der Platte, nicht aus dem Katalog:
    /// Ein abgebrochener Download kann eine kleinere Datei hinterlassen.
    pub groesse_mib: u64,
    /// Ob der Katalog dieses Modell kennt. Eine unbekannte Datei im Ordner wird
    /// angezeigt und nicht angeboten – sie könnte ein halber Download sein.
    pub im_katalog: bool,
}

/// Die Modelle, die im Ordner liegen.
///
/// Nicht im Katalog bekannte Dateien tauchen mit `im_katalog: false` auf, damit
/// Mimir sie nicht als benutzbar anbietet. Sie zu verschweigen wäre schlechter:
/// Dann wüsste der Benutzer nicht, warum sein Ordner nicht leer ist.
pub fn geladene(ordner: &Path) -> Vec<Geladen> {
    let Ok(eintraege) = std::fs::read_dir(ordner) else {
        // Kein Ordner heißt: nichts geladen. Das ist der Normalzustand nach der
        // Installation und kein Fehler.
        return Vec::new();
    };

    let mut modelle: Vec<Geladen> = eintraege
        .filter_map(|eintrag| eintrag.ok())
        .filter(|eintrag| {
            eintrag
                .path()
                .extension()
                .and_then(|endung| endung.to_str())
                == Some(ENDUNG)
        })
        .filter_map(|eintrag| {
            let pfad = eintrag.path();
            let groesse = eintrag.metadata().ok()?.len();
            let id = pfad.file_stem()?.to_str()?.to_string();
            let bekannt = aus_id(&id);

            Some(Geladen {
                name: bekannt.map_or_else(|| id.clone(), |k| k.name.to_string()),
                id,
                groesse_mib: groesse.div_ceil(MIB),
                im_katalog: bekannt.is_some(),
            })
        })
        .collect();

    modelle.sort_by_key(|modell| modell.groesse_mib);
    modelle
}

/// Wie viel Platz auf dem Datenträger noch frei ist, auf dem `ziel` liegt.
///
/// Gesucht wird der Datenträger, in dessen Einhängepunkt `ziel` liegt. Ohne
/// Treffer wird `None` gemeldet und **nicht** abgebrochen: Die Platzprüfung ist
/// eine Bequemlichkeit, und ein unbekannter freier Platz ist kein Grund, einem
/// Benutzer den Download zu verbieten, der ihn sich sonst nicht kaputt macht.
pub fn freier_platz(ziel: &Path) -> Option<u64> {
    let datentraeger = Disks::new_with_refreshed_list();
    let pfad = ziel.to_string_lossy().to_string();

    datentraeger
        .iter()
        .filter(|datentraeger| {
            let einhaengepunkt = datentraeger.mount_point().to_string_lossy().to_string();
            !einhaengepunkt.is_empty() && pfad.starts_with(&einhaengepunkt)
        })
        // Der längste passende Einhängepunkt gewinnt: `/` und `/home` können beide
        // passen, gefragt ist aber nach dem Mount, der das Verzeichnis wirklich
        // enthält.
        .max_by_key(|datentraeger| datentraeger.mount_point().as_os_str().len())
        .map(|datentraeger| datentraeger.available_space())
}

/// Woher die Modelle kommen.
///
/// Steht hier und nicht in den Einträgen: Die Quelle ist eine Eigenschaft des
/// Katalogs, nicht des einzelnen Modells. Sie ist zugleich der einzige
/// überschreibbare Wert, damit die Prüfungen einen eigenen Server auf
/// `127.0.0.1` aufsetzen können, statt ins Netz zu müssen.
pub const QUELLE_BASIS: &str = "https://huggingface.co";

/// Die Adresse, von der das Modell kommt.
pub fn quelle(eintrag: &Eintrag) -> String {
    quelle_in(QUELLE_BASIS, eintrag)
}

/// Die Adresse eines Eintrags an einer bestimmten Quelle.
pub fn quelle_in(basis: &str, eintrag: &Eintrag) -> String {
    format!(
        "{basis}/{repository}/resolve/main/{datei}",
        basis = basis.trim_end_matches('/'),
        repository = eintrag.repository,
        datei = eintrag.datei
    )
}

/// Wie oft einer Weiterleitung gefolgt wird, bevor es als Fehler gilt.
const MAX_WEITERLEITUNGEN: usize = 5;

/// Wie lange der Aufbau der Verbindung zum Modelldienst dauern darf.
///
/// Ohne Grenze bliebe ein Download, dessen Gegenstelle gar nicht antwortet, für
/// immer stehen – bei zwei Gigabyte ist das ein Fehler, kein Warten.
const VERBINDUNG_ZUM_MODELLDIENST_SECS: u64 = 30;

/// Der HTTP-Client für den Modell-Download.
///
/// Ein eigener Client und **nicht** der für Ollama. Der folgt einer Weiterleitung
/// mit Absicht nicht – das gehört zu einer Adresse, die der Benutzer eintippt.
/// Hugging Face antwortet auf `resolve/main/…` aber immer mit einer Weiterleitung
/// auf seinen CDN, und genau daran ist der Ladeprozess gescheitert: Der Aufruf
/// bekam `302 Moved Permanently` und meldete eine Quelle, die das Modell nicht
/// hergibt.
///
/// Gefolgt wird deshalb – aber nur über HTTPS. Das Ziel nennt die Gegenstelle, und
/// ein HTTP-Ziel würde die Datei auf dem Weg dorthin unverschlüsselt machen.
pub fn ladeclient() -> Result<reqwest::Client, String> {
    let verbindung = std::time::Duration::from_secs(VERBINDUNG_ZUM_MODELLDIENST_SECS);

    reqwest::Client::builder()
        .redirect(weiterleitungsregel())
        .connect_timeout(verbindung)
        .build()
        .map_err(|fehler| format!("Client-Fehler: {fehler}"))
}

/// Die Regel für Weiterleitungen beim Laden: HTTPS, und nicht endlos.
///
/// Ohne die Grenze bei der Zahl wäre eine Kette von Weiterleitungen ein Weg, den
/// Ladevorgang endlos laufen zu lassen.
fn weiterleitungsregel() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|versuch| {
        if versuch.previous().len() >= MAX_WEITERLEITUNGEN {
            return versuch.error(format!(
                "Der Modelldienst hat mehr als {MAX_WEITERLEITUNGEN}-mal weitergeleitet."
            ));
        }

        match ziel_erlaubt(versuch.url()) {
            Ok(()) => versuch.follow(),
            Err(grund) => versuch.error(grund),
        }
    })
}

/// Ob an dieser Adresse weitergeladen werden darf.
///
/// Nur HTTPS: Das Ziel nennt die Gegenstelle, und ein HTTP-Ziel würde die Datei
/// auf dem Weg dorthin unverschlüsselt machen. Die Katalogeinträge sind alle über
/// HTTPS erreichbar, es gibt also keinen Fall, in dem das etwas kostet.
fn ziel_erlaubt(url: &reqwest::Url) -> Result<(), String> {
    match url.scheme() {
        "https" => Ok(()),
        _ => Err(format!(
            "Der Modelldienst hat auf {url} verwiesen. Mimir lädt Modelle nur über HTTPS."
        )),
    }
}

/// Der Fortschritt eines Ladevorgangs, für die Oberfläche.
#[derive(Serialize, Clone, Copy, Debug)]
pub struct Fortschritt {
    pub id: &'static str,
    pub geladen: u64,
    pub gesamt: u64,
}

/// Der teilweise geladene Zustand einer Datei, aus dem weitergeladen werden kann.
///
/// Kein `Serialize`: Der Zustand ist eine Angabe zwischen zwei Aufrufen und
/// verlässt das Backend nicht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Teil {
    /// Nichts geladen: Die Datei wird neu angelegt.
    Neu,
    /// So viele Bytes sind da. Der Server muss sie bestätigen, sonst wird
    /// neu angefangen.
    Ab(u64),
    /// Die Datei ist fertig, ohne dass es einer gemerkt hat.
    Fertig(u64),
}

/// Wie viel von einem Download schon da ist.
///
/// Geprüft wird die angefangene Datei gegen die erwartete Länge: Stimmt sie nicht,
/// ist das ein abgebrochener Download und nicht eine Unterbrechung – dann wird
/// neu angefangen, statt eine Datei zu vervollständigen, die nie ganz geladen war.
///
/// **Dieselbe Spanne wie beim Abschluss.** Wäre es hier eine Gleichheit, würde eine
/// fertige Datei, die 0,36 MiB unter dem aufgerundeten Katalogwert liegt, für „zu
/// groß" oder „unvollständig" gehalten und der Download begönne von vorn – obwohl
/// genau diese Datei die ist, die am Ende [`vollstaendig`] akzeptiert.
fn teilstand(pfad: &Path, erwartet: u64) -> Teil {
    let Ok(angefangen) = std::fs::metadata(pfad) else {
        return Teil::Neu;
    };

    if vollstaendig(angefangen.len(), erwartet) {
        return Teil::Fertig(angefangen.len());
    }

    match angefangen.len().cmp(&erwartet) {
        std::cmp::Ordering::Less => Teil::Ab(angefangen.len()),
        // Zu groß heißt: Die gekürzte Größe von vorhin passt nicht mehr dazu.
        _ => Teil::Neu,
    }
}

/// Schreibt die Antwort in die Datei und meldet den Fortschritt.
///
/// Das ist der Teil, der Weitermachen kann: Eine vorhandene Datei wird nicht
/// überschrieben, sondern von der Endposition an weiterbeschrieben. Das ist genau
/// der Fall, in dem ein unterbrochener Download von zwei GB nicht wieder bei null
/// beginnt.
async fn schreibe_antwort(
    antwort: reqwest::Response,
    datei: &mut tokio::fs::File,
    erwartet: u64,
    melde: &(dyn Fn(u64) + Send + Sync),
) -> Result<(), String> {
    use futures_util::StreamExt;

    let mut stream = antwort.bytes_stream();
    let mut geladen = 0u64;

    while let Some(stueck) = stream.next().await {
        let stueck = stueck.map_err(|fehler| {
            format!("Die Verbindung ist während des Ladens abgebrochen: {fehler}")
        })?;

        // Eine Prüfung gegen den Erwartungswert, bevor geschrieben wird: Eine
        // kaputte Antwort, die endlos weiterläuft, würde die Platte füllen, und
        // das ist der einzige Punkt, an dem ein Download wirklich Schaden
        // anrichtet.
        geladen += stueck.len() as u64;
        if geladen > (erwartet as f64 * UEBERLAUF_FAKTOR) as u64 {
            return Err(
                "Der Download liefert mehr Daten, als das Modell groß ist. Abgebrochen."
                    .to_string(),
            );
        }

        tokio::io::AsyncWriteExt::write_all(&mut *datei, &stueck)
            .await
            .map_err(|fehler| format!("Schreiben nicht möglich: {fehler}"))?;

        melde(geladen);
    }

    Ok(())
}

/// Wie viel von einem Modell geladen ist, und wie viel noch fehlt.
fn ladeplan(ordner: &Path, eintrag: &Eintrag) -> (PathBuf, PathBuf, Teil) {
    let fertig = eintrag.pfad_in(ordner);
    let teil = fertig.with_extension(ENDUNG_TEIL);
    let stand = teilstand(&teil, (eintrag.groesse_mib as u64) * MIB);

    (fertig, teil, stand)
}
/// Lädt ein Modell. Läuft ein Ladeprozess schon, gibt es dafür genau einen.
///
/// Der Abbruch ist nicht der Plan: Nach einem Verbindungsabriss wird derselbe
/// Aufruf erneut gemacht und macht dort weiter, wo die Datei endete. Genau dafür
/// gibt es die Teil-Datei – und deshalb wird sie auch nicht bei jedem Versuch
/// weggeräumt.
///
/// **Was hier nicht geprüft wird:** ob die heruntergeladene Datei wirklich das
/// angebotene Modell ist. Dafür gibt es keinen Vergleich ohne die Prüfsumme des
/// Anbieters, die es für diese Dateien nicht gibt. Geprüft wird die Länge, und die
/// schließt die üblichen Fälle aus – ein vollständig anderes Modell gleicher Länge
/// wäre möglich und kommt nicht vor.
pub async fn installiere(
    client: &reqwest::Client,
    ordner: &Path,
    eintrag: &'static Eintrag,
    basis: &str,
    melde: &(dyn Fn(u64) + Send + Sync),
) -> Result<(), String> {
    std::fs::create_dir_all(ordner)
        .map_err(|fehler| format!("Der Modellordner lässt sich nicht anlegen: {fehler}"))?;

    let erwartet = (eintrag.groesse_mib as u64) * MIB;
    let (fertig, teil, stand) = ladeplan(ordner, eintrag);

    if fertig.exists() {
        return Ok(());
    }

    // Vorher die Platzfrage stellen. Sie kostet nichts und beendet den Versuch
    // vor dem Download statt nach zwei GiB geschriebener Datei.
    if let Some(frei) = freier_platz(&fertig) {
        // 100 MiB bleiben für alles, was während des Ladens noch danebenliegt.
        let benoetigt = erwartet + 100 * MIB;

        if frei < benoetigt {
            return Err(format!(
                "Auf diesem Datenträger sind {} frei, gebraucht werden {}.",
                gib_text(frei),
                gib_text(benoetigt)
            ));
        }
    }

    let beginn = match stand {
        Teil::Fertig(_) => {
            // Die Datei ist da, nur unter dem falschen Namen. Sie umbenennen ist
            // billiger als sie neu zu holen.
            std::fs::rename(&teil, &fertig).map_err(|fehler| {
                format!("Die geladene Datei lässt sich nicht umbenennen: {fehler}")
            })?;
            melde(erwartet);
            return Ok(());
        }
        Teil::Ab(beginn) => beginn,
        Teil::Neu => {
            // Eine zu große Reste-Datei wird entfernt, statt sie weiterzufüllen.
            std::fs::remove_file(&teil).ok();
            0
        }
    };

    let mut anfrage = client.get(quelle_in(basis, eintrag));
    if beginn > 0 {
        anfrage = anfrage.header(reqwest::header::RANGE, format!("bytes={beginn}-"));
    }

    let antwort = tokio::time::timeout(
        std::time::Duration::from_secs(LADEN_LEISE_GRENZE_SECS),
        anfrage.send(),
    )
    .await
    .map_err(|_| {
        format!(
            "{quelle} hat nach {LADEN_LEISE_GRENZE_SECS} Sekunden nicht geantwortet.",
            quelle = quelle_in(basis, eintrag)
        )
    })
    .and_then(|ergebnis| {
        ergebnis.map_err(|fehler| format!("Der Download kam nicht zustande: {fehler}"))
    })?;

    let status = antwort.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Der Server sagt, dass hinter `beginn` nichts mehr kommt: Die Datei ist
        // vollständig, er weiß es nur nicht.
        std::fs::rename(&teil, &fertig).map_err(|fehler| {
            format!("Die geladene Datei lässt sich nicht umbenennen: {fehler}")
        })?;
        melde(erwartet);
        return Ok(());
    }

    if !status.is_success() {
        return Err(format!(
            "Die Quelle hat den Download mit {status} abgelehnt. Unter {adresse} lässt sich \
             prüfen, ob das Modell dort noch liegt.",
            adresse = quelle_in(basis, eintrag)
        ));
    }

    // 206 heißt: Der Server hat nur den Rest geschickt. Bei 200 schickt er alles
    // noch einmal, und die vorhandene Datei muss vorher weg, sonst entstünde eine
    // Datei aus Rest und Rest.
    let weitermachen = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !weitermachen {
        std::fs::remove_file(&teil).ok();
    }

    let mut datei = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(weitermachen)
        .truncate(!weitermachen)
        .open(&teil)
        .await
        .map_err(|fehler| format!("Die Teildatei lässt sich nicht öffnen: {fehler}"))?;

    let standort = beginn;
    melde(standort);
    schreibe_antwort(antwort, &mut datei, erwartet, &|geladen| {
        melde(standort + geladen);
    })
    .await?;

    tokio::io::AsyncWriteExt::flush(&mut datei)
        .await
        .map_err(|fehler| format!("Die Datei lässt sich nicht abschließen: {fehler}"))?;
    drop(datei);

    // Jetzt erst das Umbenennen: Vorher ist die Datei unvollständig, und ein
    // Absturz mitten im Laden soll kein halbes Modell hinterlassen, das wie ein
    // fertiges aussieht.
    let geladen = std::fs::metadata(&teil)
        .map(|angaben| angaben.len())
        .unwrap_or(0);

    if !vollstaendig(geladen, erwartet) {
        std::fs::remove_file(&teil).ok();

        return Err(format!(
            "Der Download ist unvollständig: {ist} statt etwa {soll}. Die Teildatei wurde \
             entfernt, ein erneuter Versuch macht dort weiter, wo aufgehört wurde.",
            ist = gib_text(geladen),
            soll = gib_text(erwartet)
        ));
    }

    std::fs::rename(&teil, &fertig)
        .map_err(|fehler| format!("Die geladene Datei lässt sich nicht umbenennen: {fehler}"))?;

    Ok(())
}

/// Entfernt ein Modell und die Reste eines Ladeprocesses.
///
/// Gibt den gewonnenen Platz in Bytes zurück, damit die Oberfläche sagen kann, wie
/// viel wieder frei ist.
pub fn loesche(ordner: &Path, eintrag: &Eintrag) -> Result<u64, String> {
    let mut gewonnen = 0u64;

    for pfad in [
        eintrag.pfad_in(ordner),
        eintrag.pfad_in(ordner).with_extension(ENDUNG_TEIL),
    ] {
        // Nur innerhalb des Modellordners wird gelöscht. Das ist hier Formsache –
        // die Pfade stammen aus dem Katalog und nicht aus der Oberfläche –, aber
        // es kostet eine Zeile und macht den Fall unverdachtig.
        if !pfad.starts_with(ordner) {
            continue;
        }

        match std::fs::metadata(&pfad) {
            Ok(angaben) => {
                gewonnen += angaben.len();
                std::fs::remove_file(&pfad).map_err(|fehler| {
                    format!("{} lässt sich nicht löschen: {fehler}", pfad.display())
                })?;
            }
            Err(_) => continue,
        }
    }

    Ok(gewonnen)
}

/// Bytes als lesbare Größe: eine Nachkommastelle, GiB, sobald es sich lohnt.
fn gib_text(bytes: u64) -> String {
    if bytes >= 1024 * MIB {
        format!("{:.1} GB", bytes as f64 / (1024.0 * MIB as f64))
    } else {
        format!("{:.0} MB", bytes as f64 / MIB as f64)
    }
}

#[cfg(test)]
mod tests;
