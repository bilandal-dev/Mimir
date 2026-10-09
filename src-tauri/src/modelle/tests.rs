//! Prüfungen für den Modellkatalog und die Ablage.
//!
//! Geprüft wird, was ohne Netz und ohne Platte entschieden wird: welcher Eintrag auf
//! welchen Rechner passt, was im Ordner als geladen gilt, und wie ein unterbrochener
//! Download wieder aufgenommen wird. Der eigentliche Ladevorgang braucht beides und
//! ist deshalb nicht hier.

use super::*;

use std::sync::Arc;

use crate::hardware::Befehlssaetze;

/// Ein Rechner, auf dem die Engine schnell rechnen kann.
fn schneller_rechner(ram_frei: Option<u32>) -> Hardware {
    Hardware {
        prozessor: "Test".to_string(),
        threads: 8,
        ram_gib: Some(32),
        ram_frei_gib: ram_frei,
        befehlssaetze: Some(Befehlssaetze {
            avx2: true,
            ..Befehlssaetze::default()
        }),
        grafik: crate::hardware::Grafik::Unbekannt,
        einordnung: crate::hardware::Einordnung {
            stufe: crate::hardware::Stufe::SehrGross,
            max_milliarden: 13,
            hinweis: String::new(),
        },
    }
}

/// Ein Rechner ohne die schnellen Kerne – der Fall, in dem nichts empfohlen wird.
fn langsamer_rechner(ram_frei: Option<u32>) -> Hardware {
    Hardware {
        befehlssaetze: Some(Befehlssaetze::default()),
        ..schneller_rechner(ram_frei)
    }
}

#[test]
fn der_katalog_braucht_kennung_und_lizenz() {
    for eintrag in KATALOG {
        assert!(!eintrag.id.is_empty(), "ein Eintrag ohne Kennung");
        assert!(!eintrag.name.is_empty(), "{} ohne Namen", eintrag.id);
        assert!(eintrag.groesse_mib > 0, "{} ohne Größe", eintrag.id);
        assert!(
            !eintrag.lizenz.is_empty(),
            "{} ohne Lizenz: Die gehört beim Laden sichtbar hin",
            eintrag.id
        );
        assert!(
            !eintrag.repository.is_empty() && !eintrag.datei.is_empty(),
            "{} ohne Quellangabe",
            eintrag.id
        );
        assert!(
            !eintrag.anmerkung.is_empty(),
            "{} ohne Anmerkung: Die kleinsten Modelle sind im Werkzeugaufruf die schwächsten",
            eintrag.id
        );
    }
}

#[test]
fn die_kennungen_sind_eindeutig() {
    for (i, eins) in KATALOG.iter().enumerate() {
        for zwei in &KATALOG[i + 1..] {
            assert_ne!(eins.id, zwei.id, "{} kommt zweimal vor", eins.id);
        }
    }
}

#[test]
fn ein_eintrag_wird_ueber_seine_kennung_gefunden() {
    for eintrag in KATALOG {
        assert_eq!(aus_id(eintrag.id), Some(eintrag));
    }
    assert_eq!(aus_id("gibtesnicht"), None);
}

#[test]
fn ein_model_passt_nur_mit_schnellen_kernen_und_speicher() {
    let klein = &KATALOG[0];

    // Der Fall dieses Rechners: genug Speicher, aber kein AVX2.
    assert!(!passt_zu(klein, &langsamer_rechner(Some(32))));

    // Und der umgekehrte: schnelle Kerne, aber zu wenig Speicher.
    assert!(!passt_zu(klein, &schneller_rechner(Some(1))));

    // Beides zusammen: es passt.
    assert!(passt_zu(klein, &schneller_rechner(Some(4))));
}

#[test]
fn die_empfehlungen_fangen_beim_kleinsten_an() {
    let empfehlung = empfehlungen(&schneller_rechner(Some(32)));

    assert!(
        !empfehlung.is_empty(),
        "ein schneller Rechner bekommt Modelle"
    );
    let groessen: Vec<u32> = empfehlung.iter().map(|e| e.groesse_mib).collect();
    let mut sortiert = groessen.clone();
    sortiert.sort_unstable();
    assert_eq!(
        groessen, sortiert,
        "die Liste ist nicht aufsteigend: {groessen:?}"
    );
}

#[test]
fn die_empfehlungen_schrumpfen_mit_dem_speicher() {
    let viel = empfehlungen(&schneller_rechner(Some(64))).len();
    let wenig = empfehlungen(&schneller_rechner(Some(2))).len();

    assert!(
        wenig < viel,
        "weniger Speicher, weniger Modelle: {wenig} gegen {viel}"
    );
}

#[test]
fn ein_leerer_ordner_heisst_nichts_geladen() {
    let ordner = test_ordner("leer");
    assert!(geladene(&ordner).is_empty());
    aufraeumen(&ordner);
}

#[test]
fn nur_vollstaendige_gguf_dateien_gelten_als_geladen() {
    let ordner = test_ordner("vollstaendig");
    let eintrag = &KATALOG[0];

    // Die fertige Datei.
    schreibe(&eintrag.pfad_in(&ordner), MIB as usize);

    // Und daneben eine Teildatei, die kein Modell ist, sondern ein Abbruchbild.
    schreibe(
        &eintrag.pfad_in(&ordner).with_extension(ENDUNG_TEIL),
        MIB as usize,
    );

    // Und eine Datei ganz anderen Namens, die Mimir nicht kennt.
    schreibe(&ordner.join("fremd.gguf"), MIB as usize);

    let modelle = geladene(&ordner);
    let bekannt: Vec<&str> = modelle
        .iter()
        .filter(|m| m.im_katalog)
        .map(|m| m.id.as_str())
        .collect();
    let unbekannt: Vec<&str> = modelle
        .iter()
        .filter(|m| !m.im_katalog)
        .map(|m| m.id.as_str())
        .collect();

    assert_eq!(bekannt, vec![eintrag.id], "das fertige Modell fehlt");
    assert_eq!(unbekannt, vec!["fremd"], "die fremde Datei fehlt");

    aufraeumen(&ordner);
}

#[test]
fn die_groesse_kommt_von_der_platte_und_nicht_aus_dem_katalog() {
    let ordner = test_ordner("groesse");
    let eintrag = &KATALOG[0];
    schreibe(&eintrag.pfad_in(&ordner), 3 * MIB as usize);

    let geladen = geladene(&ordner);
    let modell = geladen
        .iter()
        .find(|m| m.id == eintrag.id)
        .expect("geladen");

    assert_eq!(modell.groesse_mib, 3, "die Größe kam aus dem Katalog");

    aufraeumen(&ordner);
}

#[test]
fn eine_angefangene_datei_wird_weitergefuehrt() {
    // Eine Reste-Datei, die kleiner ist als das Modell, ist genau das, wofür die
    // Teil-Datei da ist: ein abgebrochener Download.
    //
    // **Das war früher die andere Richtung.** Dieser Test stand auf „wird
    // vorgeschnitten“ und legte eine Datei an, die **größer** war als das Modell.
    // Seit die Größenprüfung eine Spanne hat statt einer Gleichheit, gilt eine
    // Datei bis 1 % über dem Katalogwert noch als dasselbe Modell – eine Datei von
    // plus 1 Byte ist also gerade noch gültig, eine von plus 20 % nicht.
    let ordner = test_ordner("vorschnitt");
    let eintrag = &KATALOG[0];
    let teil = eintrag.pfad_in(&ordner).with_extension(ENDUNG_TEIL);
    schreibe(&teil, 4 * MIB as usize);

    let (_, teil_ausgabe, stand) = ladeplan(&ordner, eintrag);

    assert!(
        matches!(stand, Teil::Ab(n) if n == 4 * MIB),
        "eine kleinere Reste-Datei wird nicht fortgesetzt: {stand:?}"
    );
    assert_eq!(teil_ausgabe, teil);

    aufraeumen(&ordner);
}

#[test]
fn der_ladeplan_nennt_ziel_und_rest() {
    let ordner = test_ordner("plan");
    let eintrag = &KATALOG[0];
    let teil = eintrag.pfad_in(&ordner).with_extension(ENDUNG_TEIL);
    schreibe(&teil, MIB as usize);

    let (fertig, teil_ausgabe, stand) = ladeplan(&ordner, eintrag);

    assert_eq!(fertig, eintrag.pfad_in(&ordner));
    assert_eq!(teil_ausgabe, teil);
    assert!(matches!(stand, Teil::Ab(n) if n == MIB));

    aufraeumen(&ordner);
}

#[test]
fn das_loeschen_gibt_den_platz_zurueck_und_kenn_einen_fehler() {
    let ordner = test_ordner("loeschen");
    let eintrag = &KATALOG[0];
    schreibe(&eintrag.pfad_in(&ordner), MIB as usize);
    schreibe(
        &eintrag.pfad_in(&ordner).with_extension(ENDUNG_TEIL),
        2 * MIB as usize,
    );

    let gewonnen = loesche(&ordner, eintrag).expect("das Löschen gelingt");

    assert_eq!(gewonnen, 3 * MIB, "beide Dateien müssen Platz freigeben");
    assert!(!eintrag.pfad_in(&ordner).exists());

    // Und ein zweites Mal: Es gibt nichts mehr zu löschen, und das ist kein Fehler.
    assert_eq!(loesche(&ordner, eintrag).expect("leer ist kein Fehler"), 0);

    aufraeumen(&ordner);
}

#[test]
fn die_adresse_zeigt_auf_das_repository() {
    let eintrag = &KATALOG[0];
    let adresse = quelle(eintrag);

    assert!(adresse.starts_with(QUELLE_BASIS), "{adresse}");
    assert!(adresse.contains(eintrag.repository), "{adresse}");
    assert!(adresse.ends_with(eintrag.datei), "{adresse}");
    assert!(adresse.contains("/resolve/main/"), "{adresse}");

    // Und die Basis ohne doppelten Schrägstrich.
    let ohne = quelle_in("https://example.org/", eintrag);
    assert!(!ohne.contains("//resolve"), "{ohne}");
}

#[test]
fn die_groesse_wird_lesbar_ausgesprochen() {
    // Unter einem GiB in MB, darüber mit einer Nachkommastelle.
    //
    // **Der Dezimalpunkt ist Absicht und wird nicht hier ersetzt.** Diese Meldung
    // landet in `installiere`, und die Übersetzung ins Deutsche steht in der
    // Oberfläche (`groessentext`), weil Rusts `format!` keine localized Ausgabe
    // kennt. Wer beides an einer Stelle erwartet, sucht den Punkt zweimal.
    assert_eq!(gib_text(512 * MIB), "512 MB");
    assert_eq!(gib_text(1023 * MIB), "1023 MB");
    assert_eq!(gib_text(1024 * MIB), "1.0 GB");
    assert_eq!(gib_text(1536 * MIB), "1.5 GB");
    assert_eq!(gib_text(2 * 1024 * MIB), "2.0 GB");
}

/// Ein Eintrag, der nicht im Katalog steht.
///
/// Für die Ladevorgänge: Er braucht eine eigene Größe, weil die des Katalogs auf
/// andere Dateien zielt.
static NETZTEST: Eintrag = Eintrag {
    id: "netztest",
    name: "Netztest",
    beschreibung: "Nur für die Prüfungen.",
    parameter_milliarden: 1,
    groesse_mib: 2,
    quantisierung: "Q4_K_M",
    lizenz: "Apache-2.0",
    ram_mindestens_gib: 1,
    anmerkung: "Nur für die Prüfungen.",
    repository: "test/modell",
    datei: "modell.gguf",
};

fn test_ordner(label: &str) -> PathBuf {
    let ordner = test_ordner_basis().join(format!(
        "{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&ordner).expect("Ordner anlegen");
    ordner
}

fn test_ordner_basis() -> PathBuf {
    std::env::temp_dir()
}

fn schreibe(pfad: &Path, groesse: usize) {
    std::fs::create_dir_all(pfad.parent().expect("mit Elternordner")).expect("Ordner anlegen");
    std::fs::write(pfad, vec![0u8; groesse]).expect("Datei anlegen");
}

/// Nimmt den Ordner einer Prüfung wieder weg.
fn aufraeumen(ordner: &Path) {
    std::fs::remove_dir_all(ordner).ok();
}

// --- Die Größenprüfung beim Laden ---------------------------------------------

/// **Die echte Datei ist kleiner als der Katalogwert.** Das ist kein Fehler im
/// Testaufbau, sondern der Fall, der jedes Laden verhindert hat: `groesse_mib` ist
/// aufgerundet, weil 468,64 MiB keine lesbare Zahl sind, und aus 469 MiB folgt keine
/// Bytezahl, sondern eine Spanne. Vorher stand hier `== erwartet`, und der Download
/// lief bis 100 %, wurde dann als „unvollständig" verworfen – der Knopf kam zurück.
#[test]
fn eine_gerundete_angabe_gilt_als_vollstaendig() {
    let erwartet = 469 * MIB;

    // Die wirkliche Datei aus dem Katalog: 491400032 Bytes, gerundet 469 MiB.
    let wirklich = 491_400_032u64;
    assert!(
        vollstaendig(wirklich, erwartet),
        "die echte Datei wurde abgelehnt"
    );
    assert!(
        !vollstaendig(erwartet - 2 * MIB, erwartet),
        "eine deutlich zu kurze Datei gehört abgelehnt"
    );
    assert!(
        !vollstaendig(0, erwartet),
        "eine leere Datei ist kein Modell"
    );
    // Und mehr als das Anderthalbfache: Das wäre ein anderes Modell oder ein Fehler.
    assert!(
        !vollstaendig((erwartet as f64 * 1.2) as u64, erwartet),
        "eine viel zu große Datei gehört abgelehnt"
    );
}

/// Und die gleiche Spanne beim Fortsetzen.
///
/// Wäre `teilstand` strenger als `vollstaendig`, würde eine fertige Datei beim
/// nächsten Versuch wieder als „zu groß" gelten und der Download begönne von vorn –
/// obwohl genau diese Datei am Ende akzeptiert wird.
#[test]
fn eine_fertige_datei_wird_beim_fortsetzen_akzeptiert() {
    let ordner = test_ordner("gerundet");
    let eintrag = aus_id("qwen2.5-0.5b").expect("im Katalog");
    let teil = eintrag.pfad_in(&ordner).with_extension(ENDUNG_TEIL);
    schreibe(&teil, 491_400_032);

    let stand = teilstand(&teil, (eintrag.groesse_mib as u64) * MIB);
    assert!(
        matches!(stand, Teil::Fertig(_)),
        "eine fertige Datei wurde neu geladen: {stand:?}"
    );

    aufraeumen(&ordner);
}

#[test]
fn eine_zu_grosse_teildatei_wird_verworfen() {
    // Deutlich mehr Bytes als erwartet können nur von einem anderen Modell stammen.
    // Sie weiterzufüllen hieße, zwei Dateien zu einer zu machen.
    //
    // **Der Abstand ist hier 20 % und nicht ein Byte.** Vorher stand hier `+ 1`, und
    // das war nur nebenbei eine Folge der Rundung: Eine Datei, die 0,36 MiB unter dem
    // Katalogwert liegt, ist die richtige und wurde weggeworfen. Die Prüfung braucht
    // einen Abstand, der groß genug für die Rundung und klein genug für das Erkennen
    // ist – dafür steht `vollstaendig`, und 20 % liegen weit außerhalb beider.
    let ordner = test_ordner("zGross");
    let eintrag = aus_id("qwen2.5-0.5b").expect("im Katalog");
    let teil = eintrag.pfad_in(&ordner).with_extension(ENDUNG_TEIL);
    let erwartet = (eintrag.groesse_mib as u64) * MIB;
    schreibe(&teil, (erwartet * 6 / 5) as usize);

    assert_eq!(teilstand(&teil, erwartet), Teil::Neu);

    aufraeumen(&ordner);
}

// --- Der Ladevorgang -----------------------------------------------------------

/// Ein Server, der `[RANGE]` versteht und mitzählt, wie oft er gefragt wurde.
///
/// Der Umfang ist bewusst klein: Er beantwortet genau das, woran der Ladevorgang
/// sich festbeißen kann – ein Range über das Dateiende, ein Range mitten hinein
/// und einen Wunsch nach mehr Bytes, als es gibt.
fn ladeserver(gesendet: usize) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};

    let zuhaenge = Arc::new(std::sync::Mutex::new(Vec::new()));
    let protokoll = Arc::clone(&zuhaenge);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Port belegen");
    let adresse = format!("http://{}", listener.local_addr().expect("Adresse"));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut leser = BufReader::new(match stream.try_clone() {
                Ok(kopie) => kopie,
                Err(_) => continue,
            });

            let mut kopf = String::new();
            if leser.read_line(&mut kopf).is_err() {
                continue;
            }

            let mut range: Option<usize> = None;
            loop {
                let mut zeile = String::new();
                if leser.read_line(&mut zeile).unwrap_or(0) == 0 || zeile.trim().is_empty() {
                    break;
                }

                if let Some(rest) = zeile.to_ascii_lowercase().strip_prefix("range:") {
                    range = rest
                        .trim()
                        .trim_start_matches("bytes=")
                        .split('-')
                        .next()
                        .and_then(|zahl| zahl.trim().parse().ok());
                }
            }

            // Protokolliert wird die Zeile **mit** dem Bereich: Aus der
            // Anfragezeile allein sieht man nicht, ob fortgesetzt wurde – und
            // genau darum geht es bei den Prüfungen.
            let vermerk = match range {
                Some(beginn) => format!("{} von {beginn}", kopf.trim_end()),
                None => kopf.trim_end().to_string(),
            };

            protokoll.lock().expect("Protokoll").push(vermerk);

            let gesamt = gesendet;
            let beginn = range.unwrap_or(0).min(gesamt);
            let rest = gesamt - beginn;

            let (status, laenge) = match range {
                // Angefragt wurde hinter dem Ende: Es gibt nichts mehr.
                Some(gefragt) if gefragt >= gesamt && gesendet > 0 => {
                    ("416 Range Not Satisfiable", 0)
                }
                Some(_) if beginn > 0 => ("206 Partial Content", rest),
                _ => ("200 OK", gesendet),
            };

            let kopf_teil = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {laenge}\r\nConnection: close\r\n\r\n"
            );

            if stream.write_all(kopf_teil.as_bytes()).is_err() {
                continue;
            }

            if laenge > 0 {
                let mut puffer = vec![0u8; laenge];

                if stream.write_all(&puffer).is_err() {
                    continue;
                }

                puffer.clear();
            }

            let _ = stream.flush();
        }
    });

    (adresse, zuhaenge)
}

/// Ein Server, der die erste Anfrage mit einer Weiterleitung beantwortet.
///
/// So arbeitet Hugging Face: Die eigentliche Datei liegt beim CDN, und die
/// Quelladresse sagt das mit einem `302`. Wer dem nicht folgt, bekommt eine
/// Quellmeldung, die sich liest, als läge das Modell dort nicht.
fn weiterleitungsserver(groesse: usize) -> String {
    use std::io::{BufRead, BufReader, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Port belegen");
    let adresse = format!("http://{}", listener.local_addr().expect("Adresse"));
    let ziel = format!("{adresse}/eigentlich");

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut leser = BufReader::new(match stream.try_clone() {
                Ok(kopie) => kopie,
                Err(_) => continue,
            });

            let mut kopf = String::new();
            if leser.read_line(&mut kopf).is_err() {
                continue;
            }

            loop {
                let mut zeile = String::new();
                if leser.read_line(&mut zeile).unwrap_or(0) == 0 || zeile.trim().is_empty() {
                    break;
                }
            }

            // Die erste Anfrage ist die ohne „/eigentlich“ im Pfad: Sie wird
            // umgeleitet. Die zweite ist die Wiederholung auf dem Weg zur Datei.
            if kopf.contains("/eigentlich") {
                let kopfteil = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {groesse}\r\nConnection: close\r\n\r\n"
                );

                if stream.write_all(kopfteil.as_bytes()).is_err() {
                    continue;
                }

                let puffer = vec![0u8; groesse];
                let _ = stream.write_all(&puffer);
                let _ = stream.flush();
                continue;
            }

            let kopfteil = format!(
                "HTTP/1.1 302 Found\r\nLocation: {ziel}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );

            let _ = stream.write_all(kopfteil.as_bytes());
            let _ = stream.flush();
        }
    });

    adresse
}

/// Ein HTTP-Client für die Prüfungen.
///
/// `reqwest` braucht den Kryptografie-Anbieter von rustls schon beim Bauen des
/// Clients, auch für Klartext. Im Betrieb setzt ihn `run()` ganz vorn; in der
/// Prüfung gibt es kein `run()`, also steht es hier. Eigener Client statt
/// `ollama_client()`, weil der für Ollama gebaut ist und hier nichts zu tun hat.
fn testclient() -> reqwest::Client {
    crate::install_crypto_provider();
    reqwest::Client::new()
}

#[tokio::test]
async fn ein_ladevorgang_holt_die_ganze_datei() {
    let ordner = test_ordner("laden");
    let (adresse, protokoll) = ladeserver(2 * MIB as usize);

    let gemeldet = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let protokoll_fortschritt = std::sync::Arc::clone(&gemeldet);

    installiere(&testclient(), &ordner, &NETZTEST, &adresse, &|geladen| {
        protokoll_fortschritt
            .lock()
            .expect("Fortschritt")
            .push(geladen)
    })
    .await
    .expect("der Ladevorgang gelingt");

    let ziel = NETZTEST.pfad_in(&ordner);
    assert!(ziel.exists(), "die Datei steht nicht da");
    assert_eq!(std::fs::metadata(&ziel).expect("Angaben").len(), 2 * MIB);
    assert!(
        !ziel.with_extension(ENDUNG_TEIL).exists(),
        "die Teildatei wurde nicht weggeräumt"
    );

    // Der Fortschritt muss gelaufen sein – eine Oberfläche ohne Fortschritt sieht
    // bei zwei GiB wie ein Hänger aus.
    assert!(!gemeldet.lock().expect("Fortschritt").is_empty());

    // Und genau einmal gefragt, ohne Bereich: Es gab nichts fortzusetzen.
    let fragen = protokoll.lock().expect("Protokoll").clone();
    assert_eq!(fragen.len(), 1, "{fragen:?}");
    assert!(
        !fragen[0].contains("von "),
        "es wurde ein Bereich angefragt, obwohl die Datei fehlte: {:?}",
        fragen[0]
    );

    aufraeumen(&ordner);
}

#[tokio::test]
async fn ein_abgebrochener_ladevorgang_macht_weiter() {
    let ordner = test_ordner("weiter");
    let teil = NETZTEST.pfad_in(&ordner).with_extension(ENDUNG_TEIL);

    // Die erste Hälfte ist da, so wie nach einem Abbruch.
    schreibe(&teil, MIB as usize);

    let (adresse, protokoll) = ladeserver(2 * MIB as usize);

    installiere(&testclient(), &ordner, &NETZTEST, &adresse, &|_| {})
        .await
        .expect("der Ladevorgang setzt fort");

    // Der Server hat einen Bereich bekommen, und die Datei ist vollständig –
    // ohne doppelt so lang wie sie sein dürfte.
    let fragen = protokoll.lock().expect("Protokoll").clone();
    assert!(
        fragen.iter().any(|frage| frage.contains("von ")),
        "es wurde ohne Bereich gefragt, also wurde neu angefangen: {fragen:?}"
    );
    assert_eq!(
        fragen.len(),
        1,
        "es wurde mehr als einmal gefragt: {fragen:?}"
    );
    assert_eq!(
        std::fs::metadata(NETZTEST.pfad_in(&ordner))
            .expect("Angaben")
            .len(),
        2 * MIB,
        "die vorhandene Hälfte wurde überschrieben statt ergänzt"
    );

    aufraeumen(&ordner);
}

#[tokio::test]
async fn eine_zu_kurze_datei_wird_abgewiesen() {
    let ordner = test_ordner("zukurz");

    // Der Katalog sagt zwei MiB, der Server liefert eine. Genau das ist ein
    // abgebrochener Download, und genau so darf er nicht aussehen.
    let (adresse, _) = ladeserver(MIB as usize);

    let fehler = installiere(&testclient(), &ordner, &NETZTEST, &adresse, &|_| {})
        .await
        .expect_err("eine zu kurze Datei darf nicht durchgehen");

    assert!(fehler.contains("unvollständig"), "{fehler}");
    assert!(
        !NETZTEST.pfad_in(&ordner).exists(),
        "es steht eine Datei da"
    );
    assert!(
        !NETZTEST
            .pfad_in(&ordner)
            .with_extension(ENDUNG_TEIL)
            .exists(),
        "die Reste-Datei wurde liegen gelassen und blockiert den nächsten Versuch"
    );

    aufraeumen(&ordner);
}

#[tokio::test]
async fn eine_fertige_restedatei_wird_nicht_erneut_geholt() {
    let ordner = test_ordner("fertig");
    let teil = NETZTEST.pfad_in(&ordner).with_extension(ENDUNG_TEIL);
    schreibe(&teil, 2 * MIB as usize);

    // Der Server darf gar nicht gefragt werden – das wäre bei zwei GiB ein
    // Download, den niemand gebraucht hat.
    let (adresse, protokoll) = ladeserver(0);

    installiere(&testclient(), &ordner, &NETZTEST, &adresse, &|_| {})
        .await
        .expect("die fertige Datei wird übernommen");

    assert!(
        protokoll.lock().expect("Protokoll").is_empty(),
        "es wurde gefragt"
    );
    assert!(NETZTEST.pfad_in(&ordner).exists());

    aufraeumen(&ordner);
}

/// Weiterleitung wird verfolgt – auch über eine Kette.
///
/// Das ist der Grund für den eigenen Ladeclient: Hugging Face antwortet auf
/// `resolve/main/…` mit einem `302` auf seinen CDN, und ein Client, der
/// Weiterleitungen nicht folgt, meldet daraufhin eine Quelle, die das Modell gar
/// nicht hergibt. Genau damit ist jeder Ladevorgang gescheitert.
#[tokio::test]
async fn eine_weitergeleitete_quelle_wird_verfolgt() {
    let ordner = test_ordner("weiterleitung");
    let adresse = weiterleitungsserver(2 * MIB as usize);

    installiere(&testclient(), &ordner, &NETZTEST, &adresse, &|_| {})
        .await
        .expect("der Ladevorgang folgt der Weiterleitung");

    assert_eq!(
        std::fs::metadata(NETZTEST.pfad_in(&ordner))
            .expect("Angaben")
            .len(),
        2 * MIB,
        "die Datei ist nicht vollständig"
    );

    aufraeumen(&ordner);
}

/// Nur HTTPS wird weitergeladen.
///
/// Das Ziel nennt die Gegenstelle, nicht Mimir. Ein HTTP-Ziel hieße, die Datei
/// auf dem Weg dorthin unverschlüsselt zu holen.
#[test]
fn eine_weitergeleitete_quelle_muss_https_bleiben() {
    for ziel in [
        "https://cdn.example.org/datei.gguf",
        "https://cdn.example.org:8443/datei.gguf",
    ] {
        assert!(
            ziel_erlaubt(&ziel.parse().expect("Adresse")).is_ok(),
            "eine HTTPS-Adresse wird abgelehnt: {ziel}"
        );
    }

    for ziel in [
        "http://cdn.example.org/datei.gguf",
        "file:///etc/passwd",
        "ftp://beispiel.org/datei.gguf",
    ] {
        assert!(
            ziel_erlaubt(&ziel.parse().expect("Adresse")).is_err(),
            "eine Adresse ohne HTTPS wird zugelassen: {ziel}"
        );
    }
}
