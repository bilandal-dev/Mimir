//! Prüfungen für die Hardware-Erkennung.
//!
//! Geprüft wird das, was entschieden wird: die Umrechnung in GiB, die Stufen aus
//! dem Arbeitsspeicher und der Text, der dem Benutzer angezeigt wird. Das
//! *Abfragen* ist hier nicht prüfbar und wird es auch nicht sein – CPUID sagt auf
//! dem Rechner, auf dem sie läuft, immer dasselbe, und eine Prüfung darauf würde
//! nur den Testrechner beschreiben.
//!
//! Zwei Entscheidungen sind hier festgehalten, weil sie später leicht versehentlich
//! geändert werden: dass eine **nicht nachweisbare** Grafik nicht als „keine
//! Grafik“ gemeldet wird, und dass die Grafik die Stufe **nicht** anhebt.

use super::*;

#[test]
fn bytes_werden_abgerundet_umgerechnet() {
    assert_eq!(gib(0), Some(0));
    assert_eq!(gib(GIB - 1), Some(0), "unter einem GiB bleibt null");
    assert_eq!(gib(GIB), Some(1));
    // 7,6 GiB: 7, nicht 8. Sonst sähe ein Rechner mit knappem Speicher hier wie
    // einer mit genug aus und die Empfehlung fiele eine Stufe zu hoch.
    assert_eq!(gib(8_193_408_700), Some(7));
    assert_eq!(gib(64 * GIB), Some(64));
}

#[test]
fn die_einordnung_fragt_den_freien_speicher() {
    // Der gesamte Speicher sagt hier 32 GiB, der freie nur 3. Gefragt wird der
    // freie: Ein Modell, das 32 GiB braucht, um zu passen, passt nicht.
    let ergebnis = einordnung(Some(32), Some(3), None);

    assert_eq!(ergebnis.stufe, Stufe::Klein);
    assert_eq!(ergebnis.max_milliarden, 2);
}

#[test]
fn ohne_freien_speicher_zaehlt_der_gesamte() {
    // Wenn der freie Speicher nicht auslesbar ist, bleibt der gesamte die
    // schwächere Zahl – aber eine Antwort ist besser als keine.
    let ergebnis = einordnung(Some(12), None, None);

    assert_eq!(ergebnis.stufe, Stufe::Gross);
    assert_eq!(ergebnis.max_milliarden, 7);
}

#[test]
fn die_stufen_gehen_nach_oben() {
    let schnell = Some(Befehlssaetze {
        avx2: true,
        ..Befehlssaetze::default()
    });

    let faelle = [
        (0, Stufe::ZuKlein, 0),
        (1, Stufe::ZuKlein, 0),
        (2, Stufe::Klein, 2),
        (3, Stufe::Klein, 2),
        (4, Stufe::Mittel, 3),
        (7, Stufe::Mittel, 3),
        (8, Stufe::Gross, 7),
        (15, Stufe::Gross, 7),
        (16, Stufe::SehrGross, 13),
        (64, Stufe::SehrGross, 13),
    ];

    for (frei, stufe, max) in faelle {
        let ergebnis = einordnung(Some(64), Some(frei), schnell);

        assert_eq!(ergebnis.stufe, stufe, "{frei} GiB frei");
        assert_eq!(ergebnis.max_milliarden, max, "{frei} GiB frei");
    }
}

#[test]
fn ohne_speicherangabe_wird_nichts_versprochen() {
    let ergebnis = einordnung(None, None, None);

    assert_eq!(ergebnis.stufe, Stufe::ZuKlein);
    assert_eq!(ergebnis.max_milliarden, 0);
    assert!(
        ergebnis.hinweis.contains("Arbeitsspeicher"),
        "der Benutzer muss erfahren, dass es an der Auskunft lag: {}",
        ergebnis.hinweis
    );
}

#[test]
fn zu_wenig_speicher_bekommt_einen_hinweis() {
    let ergebnis = einordnung(Some(64), Some(1), None);

    assert_eq!(ergebnis.stufe, Stufe::ZuKlein);
    assert!(
        ergebnis.hinweis.contains("Arbeitsspeicher"),
        "{}",
        ergebnis.hinweis
    );
}

#[test]
fn ein_rechner_ohne_schnelle_kerne_bekommt_es_zu_lesen() {
    // Das ist der wichtigste Hinweis überhaupt: Er betrifft nicht die
    // Modellwahl, sondern die Tauglichkeit überhaupt, und darf nicht im
    // Kleingedruckten verschwinden.
    let langsam = Befehlssaetze::default();
    assert!(!langsam.schnell());

    let ergebnis = einordnung(Some(64), Some(32), Some(langsam));

    assert_eq!(ergebnis.stufe, Stufe::SehrGross);
    assert!(
        ergebnis.hinweis.contains("schnellen Kerne"),
        "{}",
        ergebnis.hinweis
    );
    assert!(
        ergebnis.hinweis.contains("Minuten"),
        "es soll benannt werden, wie lange das dauert: {}",
        ergebnis.hinweis
    );
}

#[test]
fn ein_tauglicher_rechner_bekommt_keinen_hinweis() {
    let schnell = Befehlssaetze {
        avx2: true,
        ..Befehlssaetze::default()
    };

    assert_eq!(einordnung(Some(32), Some(16), Some(schnell)).hinweis, "");
    assert_eq!(einordnung(Some(32), Some(16), None).hinweis, "");
}

#[test]
fn die_grafik_kennt_kein_nein() {
    // Dass es keinen Fall „keine Grafik“ gibt, ist eine Eigenschaft des Typs und
    // damit beim Übersetzen nicht zu verlieren: Wer nichts nachweisen kann, meldet
    // `unbekannt` und nicht `keine`. Eine als leer gemeldete Karte, die nur nicht
    // gefunden wurde, wäre eine Lüge – und würde die Empfehlung verfälschen.
    //
    // Geprüft wird deshalb die Verdrahtung nach außen, weil die Oberfläche an
    // diesen Zeichenketten hängt.
    let namen: Vec<String> = [Grafik::Nvidia, Grafik::Apple, Grafik::Unbekannt]
        .iter()
        .map(|grafik| serde_json::to_string(grafik).expect("Grafik lässt sich ausgeben"))
        .collect();

    assert_eq!(namen, [r#""nvidia""#, r#""apple""#, r#""unbekannt""#]);
    assert!(!namen.iter().any(|name| name.contains("keine")));
}

#[test]
fn die_grafikaussage_followgt_dem_nachweis() {
    // Auf diesem Rechner: Findet sich `nvidia-smi` nicht, ist das Ergebnis
    // `unbekannt` – nicht `Nvidia`.
    let gefunden = liegt_im_pfad("nvidia-smi") || std::path::Path::new("/dev/nvidiactl").exists();

    assert_eq!(bestimme_grafik() == Grafik::Nvidia, gefunden);
}

#[test]
fn die_grafik_hebt_die_stufe_nicht_an() {
    // Absicht, nicht Versehen: Eine GPU rettet dem lokalen Modell nichts, solange
    // die Engine ohne Grafikunterstützung übersetzt wurde. Würde die Stufe hier
    // steigen, bekäme ein Rechner mit NVIDIA-Karte ein 7B-Modell angeboten, das
    // danach auf der CPU landet und unbrauchbar langsam ist.
    let ohne = einordnung(
        Some(64),
        Some(16),
        Some(Befehlssaetze {
            avx2: true,
            ..Default::default()
        }),
    );

    assert_eq!(ohne.stufe, Stufe::SehrGross);
    assert!(!Grafik::Nvidia.nutzbar());
}

#[test]
fn der_befehlssatz_wird_als_einer_gelesen() {
    // Hier wird nichts erfunden: Die Abfrage läuft auf diesem Rechner, und das
    // Ergebnis muss zu ihm passen. Auf x86 muss `avx2` gelten, sobald `avx512`
    // gilt – es gibt keine CPU mit 512-Bit-Befehlen ohne 256-Bit-Ganzzahlen.
    let saetze = befehlssaetze();

    if saetze.avx512 {
        assert!(saetze.avx2, "avx512 ohne avx2 gibt es nicht");
    }

    if saetze.avx2 {
        assert!(saetze.avx, "avx2 ohne avx gibt es nicht");
    }

    // Und ARM ist hier keine Ausnahme, sondern normal: ohne NEON wäre jeder
    // ARM-Rechner für zu langsam gehalten worden.
    if cfg!(target_arch = "aarch64") {
        assert!(saetze.neon, "auf aarch64 gibt es kein ohne NEON");
        assert!(saetze.schnell());
    }
}

#[test]
fn die_erkennung_liefert_immer_eine_auskunft() {
    // Keine Zusicherung über den Inhalt: Mimir soll starten, auch wenn die
    // Auskunft unvollständig bleibt. Gar nichts zu liefern wäre ein Absturz beim
    // Start, und das ist der schlechteste Ausgang.
    let antwort = erkenne();

    assert!(antwort.threads >= 1);
    assert!(antwort.ram_frei_gib.unwrap_or(0) <= antwort.ram_gib.unwrap_or(0));
    assert!(antwort.befehlssaetze.is_some());
    assert!(
        antwort.einordnung.max_milliarden <= 13,
        "{}",
        antwort.einordnung.hinweis
    );
}

/// Die Abfrage holt den Arbeitsspeicher.
///
/// Ohne `with_ram()` bleibt `total_memory()` bei null, und die Empfehlung schlägt
/// für jedes Modell fehl: Das sah auf jedem Rechner aus wie ein zu schwacher
/// Rechner. Geprüft wird deshalb die Abfrage und nicht das Ergebnis – ein Rechner
/// ohne auslesbaren Speicher ist ein Ausnahmefall, die Abfrage nicht.
#[test]
fn die_abfrage_holt_den_arbeitsspeicher() {
    let art = abfrage_art();
    let speicher = art.memory().expect("ohne Speicherabfrage");

    assert!(speicher.ram(), "der Arbeitsspeicher wird nicht geholt");
    assert!(
        !speicher.swap(),
        "der Tausch wird nirgends gelesen und soll nicht geholt werden"
    );
}
