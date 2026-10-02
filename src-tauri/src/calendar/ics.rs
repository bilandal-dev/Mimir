//! Ein Termin als Text, an dem genau eine Sache geändert wird.
//!
//! Beim Anlegen baut Mimir die Datei selbst und kann sie deshalb klein halten.
//! Beim Ändern ist das falsch: Die Datei im Kalender enthält, was andere
//! Programme hineingeschrieben haben – Erinnerungen, Kategorien, Anlagen,
//! Teilnehmer, eine Serie. Wer sie neu zusammensetzt, wirft all das weg, ohne
//! es zu sagen. Deshalb wird hier nichts aufgebaut, sondern nur die **genannten**
//! Eigenschaften gesetzt und der Rest unangetastet zurückgeschrieben. Ein Aufruf
//! kann mehrere Zeilen ändern (`SUMMARY`, `LOCATION`, `DESCRIPTION`, `DTSTART`,
//! `DTEND`, `CATEGORIES`, die Erinnerung) und dazu immer `DTSTAMP` und
//! `SEQUENCE` – entscheidend ist, dass nichts geändert wird, was nicht genannt
//! wurde.
//!
//! Zwei Dinge gehören dabei zwingend dazu:
//!
//! - **Falten und Entfalten.** Zeilen über 75 Oktett werden mit einem
//!   Leerzeichen am Zeilenanfang fortgesetzt. Wer sie nicht entfaltet, schreibt
//!   beim Ändern eine kaputte Datei.
//! - **Nur im VEVENT.** Eine Datei kann mehrere Ereignisse und Zeitzonenblöcke
//!   enthalten. Geändert wird ausschließlich das erste `VEVENT`; ein `TZID` im
//!   Kopf bleibt, wie er ist. Die Erinnerung ist ein Block **innerhalb** des
//!   `VEVENT` und wird deshalb über eigene Zugriffe angesprochen – ein `set` auf
//!   `TRIGGER` oder `DESCRIPTION` würde sonst die gleichnamigen Zeilen des
//!   Termins treffen.

/// Der Name einer Eigenschaft ohne Parameter und ohne Doppelpunkt.
///
/// `DTSTART;TZID=Europe/Berlin:20260914T090000` heißt `DTSTART`.
fn eigenschaft_name(zeile: &str) -> &str {
    zeile.split([';', ':']).next().unwrap_or_default().trim()
}

/// Der Wert einer Eigenschaft, ohne Parameter und ohne Doppelpunkt.
fn eigenschaft_wert(zeile: &str) -> Option<&str> {
    zeile.split_once(':').map(|(_, wert)| wert)
}

/// Eine ICS-Datei, in der gearbeitet werden kann.
pub struct Ics {
    zeilen: Vec<String>,
    /// Index der ersten Zeile von `BEGIN:VEVENT` bis `END:VEVENT`.
    ereignis: Option<(usize, usize)>,
}

/// Wo eine Erinnerung im Termin steht.
///
/// `von` ist `BEGIN:VALARM`, `bis` das `END:VALARM`. Fehlt das Ende, reicht der
/// Block bis zum Ende des Termins und `geschlossen` ist `false` – eine halbe
/// Erinnerung soll nicht dazu führen, dass Mimir eine zweite danebenlegt.
struct Alarm {
    von: usize,
    bis: usize,
    geschlossen: bool,
}

impl Ics {
    /// Liest eine Datei. Zeilen werden entfaltet, END:VEVENT gesucht.
    ///
    /// Gesucht wird jeweils das **erste** `BEGIN:` und das erste `END:` – bei
    /// mehreren `VEVENT` in einer Datei ist das der erste Block, wie im Modulkopf
    /// beschrieben. Nach einem `remove` wird die Spanne neu gesucht, weil sich
    /// die Zeilenzahl geändert hat.
    ///
    /// Eine Datei ohne `VEVENT` wird abgelehnt: Dann gäbe es nichts zu ändern,
    /// und ein blindes Einfügen würde eine kaputte Datei verschlimmbessern.
    pub fn parse(text: &str) -> Result<Self, String> {
        let zeilen = entfalten(text);
        let beginn = zeilen
            .iter()
            .position(|zeile| zeile.eq_ignore_ascii_case("BEGIN:VEVENT"))
            .ok_or_else(|| {
                "Die Datei enthält keinen Termin (BEGIN:VEVENT). In Nextcloud ist das \
                 ungewöhnlich; bitte dort nachsehen."
                    .to_string()
            })?;
        let ende = zeilen
            .iter()
            .position(|zeile| zeile.eq_ignore_ascii_case("END:VEVENT"))
            .ok_or_else(|| {
                "Der Termin in der Datei ist nicht abgeschlossen (END:VEVENT).".to_string()
            })?;

        if ende < beginn {
            return Err("Der Termin in der Datei ist kaputt.".to_string());
        }

        Ok(Self {
            zeilen,
            ereignis: Some((beginn, ende)),
        })
    }

    /// Die ganze Zeile einer Eigenschaft, mit Parametern.
    ///
    /// Nur hier sind `TZID` und `VALUE=DATE` sichtbar; für das Ändern von Zeiten
    /// sind beide entscheidend.
    pub fn zeile(&self, name: &str) -> Option<String> {
        self.eigenschaft_position(name)
            .map(|index| self.zeilen[index].clone())
    }

    /// Trägt die Eigenschaft ein reines Datum statt einer Uhrzeit?
    pub fn ist_datum(&self, name: &str) -> bool {
        self.zeile(name)
            .map(|zeile| zeile.to_ascii_uppercase().contains("VALUE=DATE"))
            .unwrap_or(false)
    }

    /// Der Wert einer Eigenschaft im Termin, etwa `SUMMARY` oder `DTSTART`.
    pub fn get(&self, name: &str) -> Option<String> {
        self.zeile(name)
            .as_deref()
            .and_then(eigenschaft_wert)
            .map(|wert| wert.to_string())
    }

    /// Steht die Eigenschaft im Termin? Für `RRULE` oder `ATTENDEE`.
    pub fn has(&self, name: &str) -> bool {
        self.eigenschaft_position(name).is_some()
    }

    /// Die Position der ersten passenden Zeile **im Termin**, ohne die Zeilen der
    /// Erinnerung.
    ///
    /// Der Erinnerungsblock gehört zum Termin, trägt aber eigene Zeilen mit
    /// denselben Namen wie der Termin selbst – `DESCRIPTION` vor allem. Ohne
    /// diese Trennung würde ein `set("DESCRIPTION", …)` die Erinnerung treffen
    /// und der Termin bekäme keine Beschreibung. Für die Erinnerung gibt es
    /// deshalb eigene Zugriffe (`setze_erinnerung`).
    fn eigenschaft_position(&self, name: &str) -> Option<usize> {
        let (von, bis) = self.ereignis?;
        let alarm = self.alarm().map(|alarm| (alarm.von, alarm.bis));

        (von..=bis).find(|index| {
            let im_alarm = alarm.is_some_and(|(von, bis)| *index > von && *index <= bis);

            !im_alarm && eigenschaft_name(&self.zeilen[*index]).eq_ignore_ascii_case(name)
        })
    }

    /// Steht ein Bestandteil im Termin, etwa eine Erinnerung?
    ///
    /// Eine Erinnerung steht als Block `BEGIN:VALARM … END:VALARM` im Termin,
    /// nicht als Zeile `VALARM:…`. Für die Prüfung „hat dieser Termin eine
    /// Erinnerung“ zählen deshalb beide Formen.
    pub fn has_component(&self, name: &str) -> bool {
        let Some((von, bis)) = self.ereignis else {
            return false;
        };

        let beginn = format!("BEGIN:{name}");
        self.has(name)
            || self.zeilen[von..=bis]
                .iter()
                .any(|zeile| zeile.eq_ignore_ascii_case(&beginn))
    }

    /// Setzt eine Eigenschaft und ersetzt einen vorhandenen Wert.
    ///
    /// Gibt es die Eigenschaft noch nicht, wird sie vor `END:VEVENT` eingesetzt –
    /// an dieser Stelle stehen die Felder eines Termins in der Regel, und danach
    /// folgt nichts mehr als `END:VEVENT`.
    pub fn set(&mut self, name: &str, wert: &str) {
        self.setze(name, wert, false)
    }

    /// Setzt eine Eigenschaft auf ein reines Datum, mit `VALUE=DATE`.
    ///
    /// `VALUE=DATE` ist ein Parameter und gehört vor den Doppelpunkt. Wer es in
    /// den Wert schreibt, erzeugt `DTSTART:VALUE=DATE:20261005` – eine Zeile, die
    /// kein Kalender liest.
    pub fn set_datum(&mut self, name: &str, wert: &str) {
        let Some((_, bis)) = self.ereignis else {
            return;
        };

        if let Some(index) = self.eigenschaft_position(name) {
            self.zeilen[index] = format!("{name};VALUE=DATE:{wert}");
            return;
        }

        self.zeilen.insert(bis, format!("{name};VALUE=DATE:{wert}"));
        self.grenze_suchen();
    }

    /// Setzt eine Eigenschaft und lässt die Parameter weg.
    ///
    /// Nötig beim Ändern von Zeiten: `DTSTART;TZID=Europe/Berlin:20260914T070000Z`
    /// bedeutet 07:00 Ortszeit in Berlin und nicht 09:00 wie beabsichtigt. Der
    /// Zeitzonenbezug muss mit dem Wert verschwinden, sonst verschiebt das
    /// Ändern den Termin stillschweigend um Stunden.
    pub fn set_ohne_parameter(&mut self, name: &str, wert: &str) {
        self.setze(name, wert, true)
    }

    /// `ohne_parameter` lässt `TZID` und Ähnliches weg; siehe `set_ohne_parameter`.
    fn setze(&mut self, name: &str, wert: &str, ohne_parameter: bool) {
        let Some((_, ende)) = self.ereignis else {
            return;
        };

        if let Some(index) = self.eigenschaft_position(name) {
            // Parameter bleiben erhalten: Bei DTSTART trägt der Zeitzonenbezug
            // in Parametern, und ein Weglassen würde den Termin verschieben –
            // außer es wird ausdrücklich ohne Parameter geschrieben.
            let parameter = if ohne_parameter {
                name.to_string()
            } else {
                self.zeilen[index]
                    .split_once(':')
                    .map(|(links, _)| links.to_string())
                    .unwrap_or_else(|| name.to_string())
            };

            self.zeilen[index] = format!("{parameter}:{wert}");
            return;
        }

        // Vor `END:VEVENT`, aber hinter einer vorhandenen Erinnerung: Sonst
        // landete die neue Zeile zwischen `BEGIN:VALARM` und `END:VALARM` und
        // gehörte fortan zur Erinnerung.
        self.zeilen.insert(ende, format!("{name}:{wert}"));
        self.grenze_suchen();
    }

    /// Entfernt alle Vorkommen einer Eigenschaft aus dem Termin.
    ///
    /// Die Zeilen der Erinnerung bleiben stehen: Sie gehören einem eigenen Block
    /// mit eigenen Regeln. Wäre `DESCRIPTION` die Eigenschaft, würde die
    /// Erinnerung sonst beim Leeren der Beschreibung mit verschwinden.
    pub fn remove(&mut self, name: &str) {
        let Some((von, bis)) = self.ereignis else {
            return;
        };
        let alarm = self.alarm().map(|alarm| (alarm.von - von, alarm.bis - von));

        // Nicht `retain`, weil `grenze_suchen` die Grenzen des `VEVENT` neu
        // bestimmen muss und sich die Zeilenzahl dabei ändert; die Schleife
        // schreibt deshalb wieder ab der alten Startposition zurück.
        let block: Vec<String> = self
            .zeilen
            .drain(von..=bis)
            .enumerate()
            .filter(|(versatz, zeile)| {
                let im_alarm = alarm.is_some_and(|(a, b)| *versatz > a && *versatz <= b);

                // Behalten wird, was zur Erinnerung gehört **oder** was nicht die
                // gesuchte Eigenschaft ist.
                im_alarm || !eigenschaft_name(zeile).eq_ignore_ascii_case(name)
            })
            .map(|(_, zeile)| zeile)
            .collect();

        for (index, zeile) in block.into_iter().enumerate() {
            self.zeilen.insert(von + index, zeile);
        }

        self.grenze_suchen();
    }

    /// Die erste Erinnerung des Termins, falls er eine hat.
    fn alarm(&self) -> Option<Alarm> {
        let (von_event, bis_event) = self.ereignis?;
        let von = self.zeilen[von_event..=bis_event]
            .iter()
            .position(|zeile| zeile.eq_ignore_ascii_case("BEGIN:VALARM"))?
            + von_event;

        let geschlossen = self.zeilen[von + 1..=bis_event]
            .iter()
            .position(|zeile| zeile.eq_ignore_ascii_case("END:VALARM"))
            .map(|versatz| von + 1 + versatz);

        Some(Alarm {
            von,
            bis: geschlossen.unwrap_or(bis_event),
            geschlossen: geschlossen.is_some(),
        })
    }

    /// Wie weit vor dem Beginn die Erinnerung klingelt, in Minuten. `None`, wenn
    /// der Termin keine hat oder sie einen Wert trägt, den Mimir nicht liest.
    pub fn trigger_minuten(&self) -> Option<i64> {
        let alarm = self.alarm()?;
        self.zeilen[alarm.von..=alarm.bis]
            .iter()
            .find(|zeile| eigenschaft_name(zeile).eq_ignore_ascii_case("TRIGGER"))
            .and_then(|zeile| eigenschaft_wert(zeile))
            .and_then(trigger_zu_minuten)
    }

    /// Setzt die Erinnerung auf einen neuen Auslösezeitpunkt und legt sie an,
    /// wenn der Termin noch keine hat.
    ///
    /// `trigger` ist der Wert der `TRIGGER`-Zeile, `text` der Text, der in der
    /// Benachrichtigung steht. Bei einer bereits vorhandenen Erinnerung bleiben
    /// alle übrigen Zeilen des Blocks stehen – auch `REPEAT` oder `DURATION`,
    /// die Mimir nicht schreibt, aber nicht verlieren will. Nur eine **fehlende**
    /// `DESCRIPTION` wird ergänzt: Ohne sie wäre die Meldung leer, und ein
    /// vorhandener Text gehört dem Kalender, nicht Mimir.
    pub fn setze_erinnerung(&mut self, trigger: &str, text: &str) {
        match self.alarm() {
            Some(alarm) => {
                let ohne_text = !self.zeilen[alarm.von..=alarm.bis]
                    .iter()
                    .any(|zeile| eigenschaft_name(zeile).eq_ignore_ascii_case("DESCRIPTION"));
                let mut bis = alarm.bis;

                if ohne_text {
                    // Der Text kommt vor den Auslösezeitpunkt, so wie es die
                    // Vorlagen der Kalender auch schreiben.
                    self.zeilen.insert(bis, format!("DESCRIPTION:{text}"));
                    bis += 1;
                    self.grenze_suchen();
                }

                self.setze_im_block(alarm.von, bis, "TRIGGER", trigger, false);

                // Eine Erinnerung ohne `END:VALARM` ist kaputt; beim Anfassen wird
                // sie hier gleich geschlossen, statt die Datei weiter kaputt zu
                // lassen.
                if !self.alarm().is_some_and(|alarm| alarm.geschlossen) {
                    if let Some(bis) = self.ereignis.map(|(_, ende)| ende) {
                        self.zeilen.insert(bis, "END:VALARM".to_string());
                        self.grenze_suchen();
                    }
                }
            }
            None => {
                let Some((_, ende)) = self.ereignis else {
                    return;
                };

                // Vor `END:VEVENT`: Eine Erinnerung gehört in den Termin, nicht
                // in die Datei daneben.
                let block = [
                    "BEGIN:VALARM".to_string(),
                    "ACTION:DISPLAY".to_string(),
                    format!("DESCRIPTION:{text}"),
                    format!("TRIGGER:{trigger}"),
                    "END:VALARM".to_string(),
                ];

                for (versatz, zeile) in block.into_iter().enumerate() {
                    self.zeilen.insert(ende + versatz, zeile);
                }

                self.grenze_suchen();
            }
        }
    }

    /// Nimmt die Erinnerung ganz aus dem Termin. `true`, wenn wirklich eine da
    /// war – sonst gilt der Aufruf als wirkungslos.
    pub fn entferne_erinnerung(&mut self) -> bool {
        let Some(alarm) = self.alarm() else {
            return false;
        };

        self.zeilen.drain(alarm.von..=alarm.bis);
        self.grenze_suchen();
        true
    }

    /// Setzt eine Eigenschaft **innerhalb** eines Blocks, etwa `TRIGGER` in der
    /// Erinnerung. Fehlt sie, wird sie vor dem Blockende eingesetzt.
    fn setze_im_block(
        &mut self,
        von: usize,
        bis: usize,
        name: &str,
        wert: &str,
        ohne_parameter: bool,
    ) {
        if let Some(versatz) = self.zeilen[von..=bis]
            .iter()
            .position(|zeile| eigenschaft_name(zeile).eq_ignore_ascii_case(name))
        {
            let index = von + versatz;
            let parameter = if ohne_parameter {
                name.to_string()
            } else {
                self.zeilen[index]
                    .split_once(':')
                    .map(|(links, _)| links.to_string())
                    .unwrap_or_else(|| name.to_string())
            };

            self.zeilen[index] = format!("{parameter}:{wert}");
            return;
        }

        self.zeilen.insert(bis, format!("{name}:{wert}"));
        self.grenze_suchen();
    }

    /// Sucht den Ereignisblock neu. Nach jeder Änderung an der Länge ist das
    /// einfacher und sicherer als Mitzählen.
    fn grenze_suchen(&mut self) {
        self.ereignis = self
            .zeilen
            .iter()
            .position(|zeile| zeile.eq_ignore_ascii_case("BEGIN:VEVENT"))
            .and_then(|von| {
                self.zeilen
                    .iter()
                    .position(|zeile| zeile.eq_ignore_ascii_case("END:VEVENT"))
                    .map(|bis| (von, bis))
            });
    }

    /// Die Datei, gefaltet und mit CRLF, wie der Kalender sie erwartet.
    pub fn to_text(&self) -> String {
        falten(&self.zeilen)
    }
}

/// Liest einen `TRIGGER`-Wert als Minuten vor dem Beginn.
///
/// Gelesen wird nur die Dauerform der Norm (`-PT15M`, `-P1D`, `-P1W`), nicht
/// `VALUE=DATE-TIME`: Einen absoluten Auslösezeitpunkt schreibt Mimir nicht, und ein
/// unbekannter Wert bedeutet hier „lieber nichts behaupten“ – sonst hielte Mimir
/// eine fremde Erinnerung für eine eigene und meldete eine wirkungslose Änderung.
///
/// Öffentlich, weil `events.rs` denselben Wert beim Lesen braucht: Zwei
/// Auslegungen derselben Schreibweise wären ein Widerspruch in der Datei.
pub(crate) fn trigger_zu_minuten(wert: &str) -> Option<i64> {
    let wert = wert.trim();
    let rest = wert.strip_prefix('-').or_else(|| wert.strip_prefix('+'))?;
    // Das „P" kommt vor dem „T" und muss deshalb vorher weg, sonst zerfällt
    // „PT15M" an genau diesem Buchstaben.
    let rest = rest.strip_prefix('P')?;
    let (datum, uhrzeit) = match rest.split_once('T') {
        Some((datum, uhrzeit)) => (datum, Some(uhrzeit)),
        None => (rest, None),
    };

    let mut minuten = 0;
    let mut zahl = String::new();

    for zeichen in datum.chars() {
        if zeichen.is_ascii_digit() {
            zahl.push(zeichen);
            continue;
        }

        let betrag: i64 = zahl.parse().ok()?;
        // Monate gibt es nicht: Mimir schreibt keine, und sie ließen sich nicht
        // in eine feste Zahl von Minuten umrechnen.
        minuten += match zeichen {
            'W' => betrag * 7 * 1440,
            'D' => betrag * 1440,
            _ => return None,
        };
        zahl.clear();
    }

    if !zahl.is_empty() {
        return None;
    }

    if let Some(uhrzeit) = uhrzeit {
        for zeichen in uhrzeit.chars() {
            if zeichen.is_ascii_digit() {
                zahl.push(zeichen);
                continue;
            }

            let betrag: i64 = zahl.parse().ok()?;
            minuten += match zeichen {
                'H' => betrag * 60,
                'M' => betrag,
                // Sekunden sind ungerade, aber sie kommen vor: Auf eine Minute
                // gerundet liegt die Erinnerung näher an der gemeinten Zeit.
                'S' => betrag.div_euclid(60),
                _ => return None,
            };
            zahl.clear();
        }

        if !zahl.is_empty() {
            return None;
        }
    }

    Some(minuten)
}

/// Macht aus einer Datei die logischen Zeilen.
///
/// Fortsetzungszeilen beginnen mit einem Leerzeichen oder Tabulator; das
/// Zeichen gehört zum Wert und wird entfernt.
pub fn entfalten(text: &str) -> Vec<String> {
    let mut zeilen: Vec<String> = Vec::new();

    for roh in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        if let Some(rest) = roh.strip_prefix(' ').or_else(|| roh.strip_prefix('\t')) {
            if let Some(letzte) = zeilen.last_mut() {
                letzte.push_str(rest);
                continue;
            }
        }

        if !roh.is_empty() {
            zeilen.push(roh.to_string());
        }
    }

    zeilen
}

/// Schreibt die Zeilen zurück, gefaltet und mit CRLF.
///
/// RFC 5545 erlaubt 75 Oktett ohne den Zeilenumbruch, das wird hier ausgenutzt.
/// `write::fold` faltet konservativer bei 74 – die eine Oktett-Differenz ist
/// gewollt und ändert nichts an der Lesbarkeit, sollte aber bewusst bleiben.
/// Umlaute zählen als zwei Oktett, deshalb wird nie in der Mitte eines Zeichens
/// getrennt.
pub fn falten(zeilen: &[String]) -> String {
    const MAX_OCTETTS: usize = 75;

    let mut raus = String::new();

    for (nummer, zeile) in zeilen.iter().enumerate() {
        if nummer > 0 {
            raus.push_str("\r\n");
        }

        if zeile.len() <= MAX_OCTETTS {
            raus.push_str(zeile);
            continue;
        }

        let ende = grenze(zeile, MAX_OCTETTS);
        raus.push_str(&zeile[..ende]);
        let mut rest = &zeile[ende..];

        while !rest.is_empty() {
            raus.push_str("\r\n ");
            let ende = grenze(rest, MAX_OCTETTS - 1);
            raus.push_str(&rest[..ende]);
            rest = &rest[ende..];
        }
    }

    raus
}

/// Die größte Länge, bei der noch kein Zeichen zerrissen wird.
fn grenze(text: &str, max: usize) -> usize {
    let mut ende = text.len().min(max);

    while ende > 0 && !text.is_char_boundary(ende) {
        ende -= 1;
    }

    ende
}

#[cfg(test)]
mod tests;
