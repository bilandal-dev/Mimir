console.log("main.js wurde erfolgreich geladen!");

// Kein Import mehr nötig! Tauri stellt window.__TAURI__ bereit.
const { invoke } = window.__TAURI__.core;
const { Channel } = window.__TAURI__.core;
const eventApi = window.__TAURI__.event;

const chatContainer = document.getElementById('chat-container');
const promptInput = document.getElementById('prompt-input');
const commandHint = document.getElementById('command-hint');
const commandHintList = document.getElementById('command-hint-list');
const sendBtn = document.getElementById('send-btn');
const cancelChatBtn = document.getElementById('cancel-chat-btn');
const modelSelect = document.getElementById('model-select');
const providerSelect = document.getElementById('provider-select');
const serverStatus = document.getElementById('server-status');
const checkServerBtn = document.getElementById('check-server-btn');
const startServerBtn = document.getElementById('start-server-btn');
const serverUrlBtn = document.getElementById('server-url-btn');
const serverUrlDialog = document.getElementById('server-url-dialog');
const serverUrlForm = document.getElementById('server-url-form');
const serverUrlInput = document.getElementById('server-url-input');
const serverUrlStatus = document.getElementById('server-url-status');
const serverUrlCancel = document.getElementById('server-url-cancel');
const serverUrlSave = document.getElementById('server-url-save');
const sshPasswordDialog = document.getElementById('ssh-password-dialog');
const sshPasswordForm = document.getElementById('ssh-password-form');
const sshPasswordInput = document.getElementById('ssh-password-input');
const sshPasswordTarget = document.getElementById('ssh-password-target');
const sshPasswordCancel = document.getElementById('ssh-password-cancel');
const agentToggleBtn = document.getElementById('agent-toggle-btn');
const writeToggleBtn = document.getElementById('write-toggle-btn');
const toolDialog = document.getElementById('tool-dialog');
const toolForm = document.getElementById('tool-form');
const toolName = document.getElementById('tool-name');
const toolScope = document.getElementById('tool-scope');
const toolArguments = document.getElementById('tool-arguments');
const toolCancel = document.getElementById('tool-cancel');
const writeDialog = document.getElementById('write-dialog');
const writeForm = document.getElementById('write-form');
const writeTarget = document.getElementById('write-target');
const writeScope = document.getElementById('write-scope');
const writeSummary = document.getElementById('write-summary');
const writeDiff = document.getElementById('write-diff');
const writeCancel = document.getElementById('write-cancel');
const systemDialog = document.getElementById('system-dialog');
const systemForm = document.getElementById('system-form');
const systemInput = document.getElementById('system-input');
const systemCancel = document.getElementById('system-cancel');
const systemClear = document.getElementById('system-clear');
const contextUsage = document.getElementById('context-usage');
const eventDialog = document.getElementById('event-dialog');
const eventForm = document.getElementById('event-form');
const eventHeading = document.getElementById('event-heading');
const eventScope = document.getElementById('event-scope');
const eventBlocked = document.getElementById('event-blocked');
const eventSummary = document.getElementById('event-summary');
const eventStart = document.getElementById('event-start');
const eventEnd = document.getElementById('event-end');
const eventAllDay = document.getElementById('event-all-day');
const eventFloatingHint = document.getElementById('event-floating-hint');
const eventReminder = document.getElementById('event-reminder');
const eventLocation = document.getElementById('event-location');
const eventCategories = document.getElementById('event-categories');
const eventDescription = document.getElementById('event-description');
const eventDiffWrap = document.getElementById('event-diff-wrap');
const eventDiffHead = document.getElementById('event-diff-head');
const eventDiff = document.getElementById('event-diff');
const eventError = document.getElementById('event-error');
const eventCancel = document.getElementById('event-cancel');
const eventSave = document.getElementById('event-save');
const attachBtn = document.getElementById('attach-btn');
const attachmentInput = document.getElementById('attachment-input');
const calendarPanel = document.getElementById('calendar-panel');
const calendarList = document.getElementById('calendar-list');
const calendarState = document.getElementById('calendar-state');
const calendarFoot = document.getElementById('calendar-foot');
const calendarRefresh = document.getElementById('calendar-refresh');
const calendarDialog = document.getElementById('calendar-dialog');
const calendarForm = document.getElementById('calendar-form');
const calendarUrlInput = document.getElementById('calendar-url-input');
const calendarUserInput = document.getElementById('calendar-user-input');
const calendarPasswordInput = document.getElementById('calendar-password-input');
const calendarDialogError = document.getElementById('calendar-dialog-error');
const calendarCancel = document.getElementById('calendar-cancel');
const calendarToggleBtn = document.getElementById('calendar-toggle-btn');
const calendarLoginBtn = document.getElementById('calendar-login-btn');
const calendarToggleList = document.getElementById('calendar-toggle-list');
const calendarChoose = document.getElementById('calendar-choose');
const calendarPicker = document.getElementById('calendar-picker');
const calendarPickerList = document.getElementById('calendar-picker-list');
const calendarPickerSave = document.getElementById('calendar-picker-save');
const calendarRememberInput = document.getElementById('calendar-remember-input');
const certificateDialog = document.getElementById('certificate-dialog');
const certificateHost = document.getElementById('certificate-host');
const certificateFingerprint = document.getElementById('certificate-fingerprint');
const certificateHint = document.getElementById('certificate-hint');
const certificateTrust = document.getElementById('certificate-trust');
const certificateCancel = document.getElementById('certificate-cancel');

// Chat-Verlauf im Speicher halten, damit Ollama den Kontext kennt
let messageHistory = [];
let isGenerating = false;
let modelLoadGeneration = 0;
let retryNotice = null;

// Einstellungen des Chats und der Kontextfenster-Anzeige.
const chat = { systemPrompt: '', contextTokens: 0, saveHistory: false, verlaufGekuerzt: false };
// Angehängte Dateien warten als eigene Nachricht vor der eigentlichen Eingabe.
let pendingAttachments = [];

// Zustand der Kalenderleiste. Bewusst getrennt vom Chatverlauf: Termine sind
// Daten, keine Nachrichten, und landen nicht im Modellkontext.
const calendar = { status: null, events: [], error: null, loading: false, expanded: false };
let calendarTimer = null;
// Die Grenze für angehängte Dateien liegt im Backend (`MAX_ATTACHMENT_BYTES`).
// Bewusst keine zweite hier: Zwei Werte an zwei Stellen sind die wahrscheinlichste
// Quelle dafür, dass eine Datei als angehängt quittiert wird und die Anfrage dann
// scheitert – genau das war vorher der Fehler.

// Werkzeugschritte und -ergebnisse eines Zuges. Mehr als 60 Nachrichten im
// Systemfeld würden die Prompt-Validierung des Backends blockieren, deshalb
// faellt der aelteste Teil weg.
const AGENT_MESSAGE_BUDGET = 60;
// Wie viele Nachrichten in die Verlaufsdatei passen. Das Backend weist mehr
// ab, damit keine unbegrenzte Datei entsteht.
const MAX_GESPEICHERTE_NACHRICHTEN = 100;

const EVENT_TOOL = 'create_calendar_event';
const UPDATE_EVENT_TOOL = 'update_calendar_event';
const DELETE_EVENT_TOOL = 'delete_calendar_event';
// Die drei Kalenderwerkzeuge schreiben in den Kalender des Benutzers und
// brauchen dieselbe Bestätigung wie eine Datei.
const CALENDAR_TOOL_NAMES = [EVENT_TOOL, UPDATE_EVENT_TOOL, DELETE_EVENT_TOOL];
// Werkzeuge, die etwas beim Benutzer anlegen. Sie brauchen dieselbe
// Bestaetigung wie eine Datei und gehoeren deshalb in eine einzige Liste.
const WRITE_TOOL_NAMES = ['write_file', 'edit_file', ...CALENDAR_TOOL_NAMES];

// Die beiden Umfänge, in denen das Modell Werkzeuge bekommt. `agent` ist der
// bisherige Stand mit dem festen Arbeitsverzeichnis, `termine` nur der Kalender.
// Der Wert kommt aus der Konfiguration und gilt über einen Neustart hinweg, weil
// er eine Absicht beschreibt und nicht eine Sitzung.
// Das Backend entscheidet, was daraus folgt; diese Liste hier dient nur der
// Anzeige und der Prüfung von Schreibvorgängen.
const SCOPE_NAMES = ['agent', 'termine'];
const PROVIDER_NAMES = ['remote', 'local'];

// Woher die Modelle kommen. Das ist keine Sitzungseinstellung wie der
// Agentenmodus, sondern ein gespeicherter Zustand: Wer auf das lokale Ollama
// wechselt, will das nach einem Neustart wieder so vorfinden.
//
// `remote` nimmt die eingetragene Adresse, `local` immer den lokalen Rechner.
// Beim lokalen Provider gilt der Terminumfang, egal was gespeichert ist – das
// Backend setzt das durch, und dieser Wert hier folgt ihm nur für die Anzeige.
let provider = 'remote';

function lokalesOllama() {
    return provider === 'local';
}

// Was die Oberfläche zu einem Provider sagt. Die Wörter sind dieselben wie in der
// Auswahl im Kopf – „Server“ und „Lokal“ –, damit die Kopfzeile und der Chat
// nicht verschiedene Namen für dieselbe Sache verwenden. Für einen ganzen Satz
// wird das noch ergänzt: „das lokale Ollama“ liest sich, „das Lokal“ nicht.
function providername(art) {
    return art === 'local' ? 'lokales Ollama' : 'entferntes Ollama';
}
// Zustand des Agentenmodus. Der Modus gilt nur für diese Sitzung: Nach einem
// Neustart wird bewusst wieder normal gechatten, damit die Anwendung nie
// ungefragt Werkzeuge anbietet. Dasselbe gilt für den Schreibmodus. Im
// Terminumfang ist es andersherum: Dort ist der Umfang die Freischaltung, und
// die Werkzeugschleife läuft ohne diesen Sitzungsschalter.
const agent = { enabled: false, scope: 'agent', root: '', maxSteps: 8, toolset: null, writeEnabled: false, writeCount: 0, maxWrites: 0, maxWriteBytes: 0 };

// Läuft die Werkzeugschleife? Im Terminumfang ja, weil der Umfang die
// Kalenderwerkzeuge mitbringt und es dort nichts anderes gibt.
function werkzeugschleifeAktiv() {
    return agent.scope === 'termine' || agent.enabled;
}

function umfangsname(scope) {
    return scope === 'termine' ? 'Terminumfang' : 'Agentenmodus';
}

// Der Text für `/scope`. Er nennt immer beide Umfänge mit dem Weg zurück, weil
// der eingestellte sonst nur in der Konfiguration steht und im Chat nicht
// auffindbar ist.
function umfangstext(scope, config = agent) {
    if (scope === 'termine') {
        return 'Umfang: Terminumfang. Das Modell bekommt nur die vier Kalenderwerkzeuge – Termine '
            + 'auflisten, anlegen, ändern und löschen. Es kann keine Dateien lesen oder schreiben, und ein '
            + 'Arbeitsverzeichnis ist nicht nötig. Termine ändert es nur nach Vorschau und deiner Freigabe. '
            + (calendar.status?.logged_in_hint
                ? 'Der Kalender ist angemeldet.'
                : 'Achtung: Ohne Anmeldung über /calendar gibt es kein Werkzeug. ')
            + 'Umfang dauerhaft eingestellt. Mit /scope agent kommst du zu den Dateiwerkzeugen zurück.';
    }

    return `Umfang: Agentenmodus. Das Modell kann die Dateiwerkzeuge benutzen${
        config.root ? `, begrenzt auf ${config.root}` : ', sobald ein Arbeitsverzeichnis gesetzt ist'
    }${config.writeEnabled ? '. Schreibende Werkzeuge sind für diese Sitzung freigegeben' : ''}. `
        + 'Mit /scope termine beschränkst du es auf den Kalender.';
}

// Wie der laufende Zug im Chat heißt. Im Terminumfang gibt es kein
// Arbeitsverzeichnis, und ein Text, der davon spricht, wäre dort falsch.
function zugname(termine) {
    return termine ? 'Terminlauf' : 'Agentenlauf';
}
let cancelRequested = false;

// Ein einziger Systemeintrag für den ganzen Vorgang: Er wird bei Erfolg oder
// endgültigem Scheitern in performChat umgeschrieben, damit der Chat nicht mit
// zwei Meldungen endet.
function updateRetryNotice(text) {
    if (retryNotice) {
        retryNotice.textContent = text;
    } else {
        retryNotice = appendMessageToUI('system', text);
    }
    scrollToBottom();
}

if (eventApi?.listen) {
    eventApi.listen('chat-retry', (event) => {
        const attempt = Number(event.payload?.attempt ?? 0);
        const maxAttempts = Number(event.payload?.max_attempts ?? 0);
        updateRetryNotice(`Verbindung unterbrochen – neuer Versuch ${attempt}/${maxAttempts} ...`);
    }).catch((error) => console.error('Retry-Listener fehlgeschlagen:', error));
}

function setGenerating(active) {
    isGenerating = active;
    sendBtn.disabled = active;
    cancelChatBtn.hidden = !active;
    cancelChatBtn.disabled = !active;
    checkServerBtn.disabled = active;
}

function setServerStatus(state, label) {
    serverStatus.className = `server-status ${state}`;
    serverStatus.textContent = label;
    // Nur ein wirklich ausgefallener Server bekommt den Neustart angeboten. Bei
    // einer Funkstelle waere der Vorschlag falsch und ärgerlich. Im lokalen
    // Provider beides nicht: Hier gibt es keinen Server im Netz zu starten und
    // keine Adresse einzutragen – beides hätte keine Wirkung, also gar keine
    // Wirkung zu behaupten.
    const isOffline = state === 'offline';
    startServerBtn.hidden = !isOffline || lokalesOllama();
    startServerBtn.disabled = !isOffline || lokalesOllama();
    // Der Knopf erscheint, wenn etwas zu reparieren ist. Beim Start wäre er eine
    // Sekunde lang da, ohne dass jemand etwas ändern müsste; bei „instabel“ ist er
    // da, denn eine schwankende Verbindung ist manchmal genau das Symptom einer
    // falschen Adresse.
    serverUrlBtn.hidden = lokalesOllama() || (!isOffline && state !== 'unstable');
    serverUrlBtn.disabled = state === 'checking' || state === 'starting';
    checkServerBtn.disabled = state === 'checking' || state === 'starting';
}

// Wie lange eine kürzlich bestätigte Verbindung als „war da“ gilt. Danach ist
// „instabel“ gegenüber „weg“ keine brauchbare Aussage mehr.
const UNSTABLE_BISHER_SECONDS = 120;
let lastContact = null;

function setLastContact(seconds) {
    if (typeof seconds === 'number' && seconds > 0) lastContact = seconds;
}

const nowSeconds = () => Math.floor(Date.now() / 1000);

// „Instabel“ heißt: Der Server hat vor Kurzem geantwortet, dieser eine Test kam
// nicht durch. Das ist auf einer wackligen Strecke die Normalform und kein
// Grund, den Server neu zu starten.
function setServerStatusFromProbe(status) {
    if (status.online) {
        setLastContact(status.last_contact ?? nowSeconds());
        setServerStatus('online', 'Server: Online');
        return true;
    }

    setLastContact(status.last_contact);

    if (lastContact !== null && nowSeconds() - lastContact <= UNSTABLE_BISHER_SECONDS) {
        setServerStatus('unstable', 'Server: instabel');
    } else {
        setServerStatus('offline', 'Server: Offline');
    }

    return false;
}

async function loadModels() {
    const generation = ++modelLoadGeneration;
    // Die bisherige Wahl merken: das Neuladen der Liste darf das Modell, das der
    // Benutzer gewählt hat, nicht still auf das erste zurücksetzen.
    const previouslySelected = modelSelect.value;
    // Entscheidend: Die vorhandene Liste bleibt stehen, bis neue Modelle da
    // sind. Vorher wurde sie schon vor der Anfrage geleert, und ein kurzer
    // Aussetzer hat damit jede Auswahl zerstört und den Senden-Knopf gesperrt –
    // auf einer schwankenden Strecke also nach jedem zweiten Satz.
    const listeVorhanden = [...modelSelect.options].some((option) => option.value !== '');

    setServerStatus('checking', 'Server: Prüfe ...');

    if (!listeVorhanden) {
        // Ohne jedes Modell kann ohnehin nichts gesendet werden. Das ist der
        // einzige Fall, in dem die Liste ersetzt werden darf.
        modelSelect.replaceChildren(new Option('Modelle werden geladen ...', ''));
        modelSelect.disabled = true;
        sendBtn.disabled = true;
    }

    try {
        const models = await invoke('get_models');
        if (generation !== modelLoadGeneration) return null;
        modelSelect.replaceChildren();

        if (models.length === 0) {
            // Der Server hat geantwortet und sagt, er habe nichts. Das ist
            // etwas anderes als ein gescheiterter Abruf: Hier wäre jede alte
            // Liste irreführend, also wird die Auswahl gesperrt.
            modelSelect.add(new Option('Keine Modelle verfügbar', ''));
            modelSelect.disabled = true;
            sendBtn.disabled = true;
            setLastContact(nowSeconds());
            stoppeModelllistenWiederholung();
            setServerStatus('online', 'Server: Online');
            return [];
        }

        for (const model of models) {
            modelSelect.add(new Option(model, model));
        }

        // Weiterhin vorhandenes Modell bleibt ausgewählt, sonst das erste der Liste.
        modelSelect.value = models.includes(previouslySelected) ? previouslySelected : models[0];
        modelSelect.disabled = false;
        sendBtn.disabled = false;
        // Eine geladene Liste ist ein Lebenszeichen: Die Anzeige merkt sich das,
        // damit ein späterer Aussetzer als „instabel“ und nicht als „Offline“
        // erscheint.
        setLastContact(nowSeconds());
        stoppeModelllistenWiederholung();
        setServerStatus('online', 'Server: Online');
        return models;
    } catch (error) {
        if (generation !== modelLoadGeneration) return null;
        console.error('Fehler beim Laden der Modelle:', error);
        // Dieselbe Entscheidung wie bei der Statuspruefung: instabel nur, wenn
        // es wirklich einen neuen Kontakt gab. Sonst behauptet die Anzeige
        // etwas, das nicht stimmt, und der Neustart-Knopf bleibt weg.
        setServerStatusFromProbe({ online: false, last_contact: lastContact });

        // Die Liste und die Auswahl bleiben unangetastet. Der Rückgabewert
        // bleibt null: Aufrufer wie /server-status müssen weiterhin sagen
        // können, dass der Server in diesem Moment nicht antwortet – das ist
        // etwas anderes als „es gibt keine Modelle“.
        if (listeVorhanden) {
            modelSelect.disabled = false;
            sendBtn.disabled = false;
            return null;
        }

        modelSelect.replaceChildren(new Option('Modelle konnten nicht geladen werden', ''));
        modelSelect.disabled = true;
        sendBtn.disabled = true;
        starteModelllistenWiederholung();
        return null;
    }
}

// Wie oft und wie lange nach einem gescheiterten Laden erneut versucht wird.
//
// Ohne das bleibt die Anwendung nach einem einzigen Aussetzer kaputt, bis von
// Hand geprüft wurde – auf einer schwankenden Strecke der Normalfall. Die Grenze
// verhindert, dass ein wirklich ausgefallener Server endlos angesprochen wird.
//
// Die Wiederholung läuft nur, solange noch keine brauchbare Liste steht: Gibt es
// eine, bleibt sie bedienbar und es wird gar nicht erneut versucht.
const MODELLLISTE_VERSUCHE = 4;
const MODELLLISTE_PAUSE_MS = 4000;
let modelllisteVersuche = 0;
let modelllisteTimer = null;

function stoppeModelllistenWiederholung() {
    if (modelllisteTimer !== null) {
        clearTimeout(modelllisteTimer);
        modelllisteTimer = null;
    }

    modelllisteVersuche = 0;
}

function starteModelllistenWiederholung() {
    if (modelllisteTimer !== null) return;
    modelllisteVersuche += 1;

    if (modelllisteVersuche > MODELLLISTE_VERSUCHE) {
        // Genug versucht. Der Platzhalter bleibt stehen und erklaert den Zustand,
        // statt still zu einer weiteren Runde anzusetzen.
        modelllisteTimer = null;
        return;
    }

    modelllisteTimer = setTimeout(async () => {
        modelllisteTimer = null;
        await loadModels();
    }, MODELLLISTE_PAUSE_MS);
}

// ------------------------------------------------------------------- Provider
//
// Der Provider beantwortet die Frage „woher kommen die Modelle". Er ist die
// bewusste Alternative dazu, die Adresse im Chat umzuschreiben: Dort bleibt der
// Verlauf des anderen Servers stehen und die Kopfzeile zeigt zwischendurch den
// falschen Server. Hier bleibt die eingetragene Adresse unangetastet, und der
// Umfang wird mitgezogen, weil das lokale Modell nur den Terminumfang kennt.

/**
 * Stellt auf einen Provider um und lädt alles neu, was daran hängt.
 *
 * Derselbe Weg für Auswahl im Kopf und für `/provider`: Zwei Wege zur selben
 * Sache würden auseinanderlaufen, sobald einer von ihnen etwas vergisst.
 */
async function wechsleProvider(art, { fragen = true } = {}) {
    if (!PROVIDER_NAMES.includes(art)) {
        appendMessageToUI('system', usageHint('provider'));
        return false;
    }

    if (art === provider) {
        appendMessageToUI('system', `Es läuft bereits ${providername(art)}.`);
        return false;
    }

    if (fragen && !window.confirm(`Auf ${providername(art)} umstellen?`)) {
        // Die Auswahl im Kopf zurücknehmen: Sie zeigt den Provider, und ein
        // stehengebliebener Wert wäre eine Anzeige ohne Wirkung.
        providerSelect.value = provider;
        appendMessageToUI('system', 'Provider unverändert.');
        return false;
    }

    const vorher = provider;
    providerSelect.disabled = true;

    try {
        provider = await invoke('set_provider', { provider: art });
        providerSelect.value = provider;

        // Der Umfang kann mit dem Provider wechseln: Lokal gibt es nur den
        // Terminumfang. Das Backend entscheidet das, hier wird nur nachgezogen.
        const vorherigerUmfang = agent.scope;
        agent.toolset = null;
        await refreshAgentConfig();
        setAgentMode(agent.enabled);

        // Der alte Server und der neue haben nichts miteinander zu tun. Der
        // Verlauf des anderen bliebe sonst beim Start wieder da und liefe in
        // Antworten, die es nicht gab.
        if (vorher !== provider) {
            messageHistory = [];
            await invoke('delete_chat_history').catch(() => {});
            updateContextUsage();
        }

        // Die Modelle des anderen Servers dürfen keinen Moment lang auswählbar
        // sein: Eine Anfrage mit einem Namen, den der neue Server nicht kennt,
        // ergäbe nur einen Fehler.
        modelSelect.replaceChildren(new Option('Modelle werden geladen ...', ''));
        modelSelect.disabled = true;
        sendBtn.disabled = true;

        const modelle = await loadModels();

        const teile = [`Provider: ${providername(provider)}.`];
        if (vorherigerUmfang !== agent.scope) {
            teile.push(`Umfang: ${umfangsname(agent.scope)} – im lokalen Provider gibt es keine Dateiwerkzeuge.`);
        }
        if (vorher !== provider) {
            teile.push('Chatverlauf zurückgesetzt.');
        }
        teile.push(modelle === null
            ? 'Der Server antwortet nicht – die Modellliste konnte nicht geladen werden.'
            : (modelle.length === 0
                ? 'Der Server ist erreichbar, hat aber keine Modelle.'
                : `${modelle.length} Modell(e) verfügbar.`));

        appendMessageToUI('system', teile.join(' '));
        return true;
    } catch (error) {
        providerSelect.value = provider;
        provider = vorher;
        appendMessageToUI('system', `Fehler beim Wechsel des Providers: ${error}`);
        return false;
    } finally {
        providerSelect.disabled = false;
    }
}

// Liest den eingestellten Provider und stellt die Auswahl darauf ein. Läuft vor
// dem ersten Laden der Modelle, damit die Kopfzeile nicht kurz den falschen
// Server ankündigt.
async function ladeProvider() {
    try {
        provider = await invoke('get_provider');
    } catch (error) {
        console.error('Provider nicht lesbar:', error);
        provider = 'remote';
    }

    providerSelect.value = provider;
    return provider;
}

// Beim Start soll ohne Mausklick getippt werden können. WebKitGTK setzt den
// Fokus beim ersten Zeichnen des Fensters gern wieder auf das Dokument zurück,
// und beim Start ist das Fenster womöglich noch gar nicht fokussiert. Deshalb
// wird beim ersten Fokusieren des Fensters einmal nachgefasst - aber nur solange
// der Benutzer noch nicht selbst getippt hat.
let startupFocusSettled = false;

function focusPromptInput() {
    promptInput.focus();
}

window.addEventListener('focus', () => {
    if (startupFocusSettled || promptInput.value) {
        return;
    }

    startupFocusSettled = true;
    focusPromptInput();
});

// Die Anleitung beim ersten Start.
//
// Wer Mimir zum ersten Mal öffnet, sieht genau nichts: keinen Server, keine Modelle,
// einen leeren Chat. Ohne diese Nachricht ist nicht zu erkennen, ob die Anwendung
// kaputt ist oder nur noch nicht eingerichtet. Die Kopfzeile sagt „Server: Offline“
// – und die einzige sichtbare Reaktion darauf ist ein Knopf, dessen Zweck man raten
// muss.
//
// Deshalb steht hier, was zu tun ist, in der Reihenfolge, in der es zu tun ist. Der
// Aufbau folgt dem, was tatsächlich fehlt: Läuft Ollama auf diesem Rechner, ist es
// ein Schritt; läuft es woanders, kommt der mit der Adresse dazu.
//
// Bewusst ohne Zahlen und IP-Adressen im Text: Die Anleitung wird bei jedem ersten
// Start angezeigt, und eine fremde Adresse darin wäre die, die gerade vermieden
// wurde.

/**
 * Zeigt die Anleitung, wenn noch nichts eingestellt ist.
 *
 * Nur einmal je Sitzung: Wer die Adresse einträgt, sieht danach nur noch den Dialog
 * und nicht noch einmal die Liste. Und wer sie im Chat überschreibt, bekommt sie
 * beim nächsten Start wieder – das ist beabsichtigt, weil die Liste dann noch
 * sinnvoll ist.
 */
async function zeigeErsteinrichtung() {
    let stand;

    try {
        stand = await invoke('get_einrichtung');
    } catch (error) {
        // Das Backend antwortet nicht. Eine Anleitung, die auf keinem Befehl
        // aufbaut, würde selbst scheitern – und die Meldung bliebe stumm. Der
        // Serverzustand im Kopf sagt das Nötige.
        console.error('Einrichtungsstand nicht lesbar:', error);
        return;
    }

    if (stand.server_eingetragen) {
        return;
    }

    const zeilen = [
        'Willkommen bei Mimir.',
        '',
        'Mimir spricht mit Ollama. Ollama muss auf diesem Rechner laufen – dann',
        'genügt dieser eine Schritt. Läuft es auf einem anderen Rechner, steht die',
        'Adresse weiter unten.',
        '',
        '1. Prüfen, ob Ollama läuft:',
        '      im Terminal:  ollama list',
        '   Läuft das, sollten dort Modelle stehen. Läuft es nicht, starten mit:',
        '      ollama serve',
        '',
        '2. Adresse eintragen. Beim ersten Start steht hier die Vorgabe',
        `      ${stand.server_vorgabe}`,
        '   Läuft Ollama woanders, hier die Adresse eintragen – zum Beispiel',
        '      ollama.example.org:11434',
        '   Der Port ist nur nötig, wenn er nicht 11434 ist. „http://“ darf',
        '   davorstehen, muss es aber nicht.',
        '',
        '3. Fertig. Die Modelle erscheinen dann in der Liste oben rechts.',
    ];

    if (stand.kalender_eingetragen) {
        zeilen.push('', 'Der Kalender ist eingerichtet und in der Leiste rechts sichtbar.');
    } else {
        zeilen.push(
            '',
            'Der Kalender ist freiwillig. Er hängt an deinem Nextcloud:',
            '      /calendar',
            '   Ohne ihn bleibt Mimir beim reinen Chat.'
        );
    }

    zeilen.push(
        '',
        'Alle Befehle: /help',
        'Der Agentenmodus liest Dateien aus einem Verzeichnis und ist ebenfalls',
        'freiwillig. Ohne ihn bleibt Mimir beim Chat mit dem Modell.'
    );

    // Nicht `system`: Diese Nachricht ist zentriert und in Monospace formatiert,
    // was für eine Liste mit Einrückungen unlesbar ist. Sie ist ein Text, kein
    // Protokoll.
    const blase = document.createElement('div');
    blase.classList.add('message', 'setup');
    blase.textContent = zeilen.join('\n');
    chatContainer.appendChild(blase);

    // Ein Knopf direkt an der Nachricht. Ein Knopf irgendwo im Fenster wäre bei
    // einem leeren Bildschirm nicht zu finden.
    const knopf = document.createElement('button');
    knopf.type = 'button';
    knopf.className = 'setup-button';
    knopf.textContent = 'Server-Adresse jetzt eintragen';
    knopf.addEventListener('click', () => {
        blase.remove();
        knopf.remove();
        openServerUrlDialog();
    });

    blase.after(knopf);
    scrollToBottom();
    focusPromptInput();
}

// Reihenfolge: Erst der Provider, dann `loadModels`, dann die Anleitung. Ohne den
// Provider zuerst würde die Kopfzeile für einen Moment den falschen Server
// ankündigen. Und die beiden Meldungen überschreiben sich sonst, weil die
// Anleitung erscheint, während das Backend noch prüft, und der Serverzustand
// wechselt zurück auf „Prüfe ...“.
//
// `loadModels` fängt seine Fehler selbst ab und liefert bei einem Ausfall `null`,
// ein Ablehnen käme nur bei einem Fehler in Mimir selbst. Deshalb genügt ein
// `finally`: Ob der Server da ist oder nicht, entscheidet `get_einrichtung`, und
// das fragt die Anleitung selbst.
ladeProvider()
    .finally(loadModels)
    .finally(zeigeErsteinrichtung)
    .catch((error) => console.error('Modelle nicht ladbar:', error));
focusPromptInput();

// Arbeitsverzeichnis und Schrittzahl für die Beschriftung des Umschalters holen.
refreshAgentConfig().catch((error) => console.error('Agentenkonfiguration nicht lesbar:', error));

// Systemanweisung, Kontextfenster und Verlauf wiederherstellen. Ohne Freischaltung
// liefert das Backend keinen Verlauf, es kann also keiner nachgeladen werden.
(async () => {
    try {
        await refreshChatConfig();
        const stored = await invoke('load_chat_history');

        if (stored && stored.length > 0) {
            messageHistory = stored;
            updateContextUsage();
        }
    } catch (error) {
        console.error('Chat-Konfiguration nicht lesbar:', error);
    }
})();

attachBtn.addEventListener('click', () => attachmentInput.click());

// Kalenderleiste: erst den Zustand holen, damit die Anzeige ohne Netzabruf
// gefüllt ist, danach die Termine. Das Passwort wird dabei nicht gebraucht.
// Auf schmalen Fenstern startet die Leiste eingeklappt, damit die
// Unterhaltung nicht auf einen Rest reduziert wird.
//
// Nur beim Start: Ein späteres Verbreitern des Fensters holt die Leiste nicht
// automatisch zurück, dafür gibt es den Knopf im Kopf. Im eingeklappten Fall
// wird auch der Zustand nicht geholt – dort ist ja nichts zu sehen.
if (window.innerWidth < 720) {
    calendarPanel.classList.add('collapsed');
    calendarToggleBtn.setAttribute('aria-pressed', 'false');
    calendarToggleBtn.textContent = 'Kalender: aus';
    calendarToggleBtn.title = 'Kalenderleiste einblenden';
} else {
    aktualisiereKalender()
        .then(starteKalenderTimer)
        .catch((error) => console.error('Kalender nicht lesbar:', error));
}

calendarRefresh.addEventListener('click', () => {
    aktualisiereKalender().catch((error) => console.error('Kalender:', error));
});

calendarToggleBtn.addEventListener('click', toggleCalendarPanel);
calendarToggleList.addEventListener('click', toggleCalendarExpanded);
calendarChoose.addEventListener('click', () => {
    toggleCalendarPicker().catch((error) => console.error('Kalenderauswahl:', error));
});
calendarPickerSave.addEventListener('click', () => {
    speichereKalenderAuswahl().catch((error) => console.error('Kalenderauswahl:', error));
});

attachmentInput.addEventListener('change', async () => {
    const files = [...attachmentInput.files];
    attachmentInput.value = '';
    await readAttachments(files);
});

function requestSshPassword(target) {
    return new Promise((resolve) => {
        let submittedPassword = null;
        sshPasswordTarget.textContent = `Ziel: ${target}`;
        sshPasswordInput.value = '';
        sshPasswordDialog.returnValue = '';

        const handleSubmit = (event) => {
            event.preventDefault();
            submittedPassword = sshPasswordInput.value;
            sshPasswordDialog.close('confirmed');
        };
        const handleClose = () => {
            sshPasswordForm.removeEventListener('submit', handleSubmit);
            sshPasswordCancel.removeEventListener('click', handleCancel);
            sshPasswordInput.value = '';
            resolve(sshPasswordDialog.returnValue === 'confirmed' ? submittedPassword : null);
        };
        const handleCancel = () => sshPasswordDialog.close('cancelled');

        sshPasswordForm.addEventListener('submit', handleSubmit);
        sshPasswordCancel.addEventListener('click', handleCancel);
        sshPasswordDialog.addEventListener('close', handleClose, { once: true });
        sshPasswordDialog.showModal();
        sshPasswordInput.focus();
    });
}

async function startOllamaViaSsh() {
    // Ein Server auf diesem Rechner wird nicht über SSH gestartet. Ohne diese
    // Prüfung liefe der Knopf im lokalen Provider an einem Rechner im Netz
    // vorbei und meldete danach einen Erfolg, der nichts gebracht hat.
    if (lokalesOllama()) {
        appendMessageToUI(
            'system',
            'Im lokalen Provider wird kein Server über SSH gestartet – hier läuft das Modell auf '
            + 'diesem Rechner. Läuft hier kein Ollama, wird es mit `sudo pacman -S ollama` '
            + 'installiert und mit `ollama serve` gestartet. Mit /provider remote steht der '
            + 'SSH-Weg wieder zur Verfügung.'
        );
        return;
    }

    isGenerating = true;
    setServerStatus('starting', 'Server: SSH startet ...');
    appendMessageToUI('system', 'SSH-Startbefehl wird ausgeführt ...');

    try {
        // Ohne Passwort versucht das Backend genau eine Verbindung mit den
        // hinterlegten Schlüsseln bzw. dem ssh-agent.
        let result = await invoke('start_ollama_via_ssh', { password: null });

        if (result.status === 'password_required') {
            appendMessageToUI('system', 'SSH-Key oder Agent nicht verfügbar. Passwort erforderlich.');
            const ssh = await invoke('get_ssh_config');
            const password = await requestSshPassword(`${ssh.target}:${ssh.port}`);

            if (!password) {
                setServerStatus('offline', 'Server: Offline');
                appendMessageToUI('system', 'Passwortabfrage abgebrochen.');
                return;
            }

            // Das Backend springt bei gesetztem Passwort direkt in den
            // Passwortversuch, es gibt also keinen zweiten Schlüsselversuch.
            result = await invoke('start_ollama_via_ssh', { password });
        }

        let online = false;
        const deadline = Date.now() + 20000;

        while (Date.now() < deadline) {
            const remaining = deadline - Date.now();
            await new Promise((resolve) => setTimeout(resolve, Math.min(1000, remaining)));
            const status = await invoke('check_server');

            if (status.online) {
                online = true;
                break;
            }
        }

        if (online) {
            const models = await loadModels();
            appendMessageToUI(
                'system',
                models === null
                    ? 'Ollama ist online, aber die Modellliste konnte nicht geladen werden.'
                    : 'Ollama ist online. Die Modellliste wurde aktualisiert.'
            );
        } else {
            await loadModels();
            appendMessageToUI(
                'system',
                'Der SSH-Startbefehl wurde ausgeführt, aber der Server war nach 20 Sekunden noch nicht erreichbar. Prüfe ~/.local/state/mimir/ollama.log auf dem Server.'
            );
        }
    } catch (error) {
        setServerStatus('offline', 'Server: Offline');
        appendMessageToUI('system', `Fehler beim SSH-Start: ${error}`);
    } finally {
        isGenerating = false;
    }
}

function describeIdentityFile(identityFile) {
    return identityFile ? identityFile : 'Standardpfade und ssh-agent (kein fester Schlüssel)';
}

function resetPromptInput() {
    promptInput.value = '';
    // Entspricht der Feldhöhe im Stylesheet, damit nichts springt.
    promptInput.style.height = '48px';
    // Der Vorschlag bezog sich auf ein Wort, das es jetzt nicht mehr gibt.
    schliesseVorschlagsliste();
}

// Eine Quelle der Wahrheit für die Übersicht: Die Hilfeanzeige und die
// Verwendungshinweise bei falscher Eingabe lesen beide hier aus, damit Anzeige
// und Verhalten nicht auseinanderlaufen können.
const COMMAND_HELP = [
    {
        title: 'Kalender',
        entries: [
            {
                name: '/calendar',
                text: 'Nextcloud-Instanz eintragen oder anmelden, zeigt Adresse und Termine. '
                    + 'Ein Klick auf einen Termin in der Leiste öffnet ihn zum Ändern.',
                usage: '/calendar [aus|zertifikat]',
            },
            {
                name: '/calendar zertifikat',
                text: 'Fingerabdruck des Zertifikats ansehen und es bestätigen',
                usage: '/calendar zertifikat',
            },
            { name: '/termine', text: 'Die nächsten Termine als Agenda im Chat, nach Tagen gruppiert, mit Dauer, Erinnerung und Kategorien', usage: '/termine' },
        ],
    },
    {
        title: 'Kontext und Verlauf',
        entries: [
            {
                name: '/system',
                text: 'Dauerhafte Anweisung an das Modell, zum Beispiel Sprache oder Antwortlänge',
                usage: '/system [aus]',
            },
            {
                name: '/context',
                text: 'Größe des Kontextfensters, 0 übernimmt die Vorgabe des Modells',
                usage: '/context <token>',
            },
            {
                name: '/history',
                text: 'Verlauf auf der Platte halten oder löschen, nur nach Rückfrage',
                usage: '/history [an|aus|löschen]',
            },
            {
                name: 'Datei (Knopf neben der Eingabe)',
                text: 'Hängt eine Textdatei an die nächste Nachricht an. Bis 64 KiB kommt sie '
                    + 'vollständig mit, eine größere wird gekürzt und Mimir sagt dir das im Chat. Der '
                    + 'Anhang gilt nur für die eine Nachricht und wird danach verworfen. Für ein ganzes '
                    + 'Dokument siehe „Dokumente ablegen statt anhängen" unter Agentenmodus.',
            },
        ],
    },
    {
        title: 'Allgemein',
        entries: [{ name: '/help', text: 'Zeigt diese Übersicht', usage: '/help' }],
    },
    {
        title: 'Ollama-Server',
        entries: [
            {
                name: '/provider',
                text: 'Zeigt oder stellt ein, woher die Modelle kommen. Die Auswahl „Ollama“ im '
                    + 'Kopf steht auf „Server“ – dann nimmt Mimir die eingetragene Adresse aus dem '
                    + 'Netz – oder auf „Lokal“, dann das Ollama auf diesem Rechner. Beides entspricht '
                    + '/provider remote und /provider local. Die Wahl bleibt über einen Neustart stehen, '
                    + 'und die eingetragene Adresse wird dabei nicht verändert. Lokal gibt es nur den '
                    + 'Terminumfang: Dort läuft das Modell auf diesem Rechner und bekommt keine '
                    + 'Dateiwerkzeuge – mit /provider remote kommst du zu denen zurück.',
                usage: '/provider [remote|local]',
            },
            { name: '/provider remote', text: 'Entferntes Ollama aus dem Netz, mit Dateiwerkzeugen' },
            { name: '/provider local', text: 'Ollama auf diesem Rechner, nur Kalenderwerkzeuge' },
            { name: '/server-status', text: 'Prüft die Erreichbarkeit und aktualisiert die Modellliste' },
            { name: '/server-start', text: 'Startet Ollama über das konfigurierte SSH-Ziel, nur bei /provider remote' },
            {
                name: '/server-url',
                text: 'Zeigt die aktuell konfigurierte Server-URL – im lokalen Provider die Adresse '
                    + 'dieses Rechners. Ändern lässt sie sich nur beim entfernten.',
                usage: '/server-url <URL>',
            },
            { name: '/server-url <URL>', text: 'Ändert die Server-URL dauerhaft nach Bestätigung' },
        ],
    },
    {
        title: 'SSH',
        entries: [
            {
                name: '/ssh-target',
                text: 'Zeigt SSH-Ziel und verwendeten Schlüssel',
                usage: '/ssh-target <benutzer@host> [port]',
            },
            {
                name: '/ssh-target <benutzer@host> [port]',
                text: 'Setzt Benutzer, Host und optionalen Port; die Schlüsselvorgabe bleibt erhalten',
            },
            {
                name: '/ssh-key',
                text: 'Zeigt den eingestellten privaten Schlüssel',
                usage: '/ssh-key <pfad>  |  /ssh-key aus',
            },
            { name: '/ssh-key <pfad>', text: 'Verwendet genau diesen Schlüssel, ~ wird aufgelöst' },
            { name: '/ssh-key aus', text: 'Entfernt die Vorgabe, es gelten wieder Standardpfade und ssh-agent' },
        ],
    },
    {
        title: 'Agentenmodus',
        entries: [
            {
                name: '/agent',
                text: 'Schaltet den Agentenmodus ein oder aus, nur lesende Werkzeuge',
                usage: '/agent',
            },
            {
                name: '/agent-write',
                text: 'Gibt schreibende Werkzeuge frei oder sperrt sie wieder; jeder Vorgang wird als Unterschied gezeigt. Bei angemeldetem Kalender kommen create_calendar_event, update_calendar_event und delete_calendar_event dazu; Termine lassen sich damit anlegen, ändern und löschen – jeweils nach Vorschau und Freigabe. Bei Terminen lassen sich auch Erinnerung und Kategorie angeben, etwa „5 Minuten vorher“ oder „Arbeit“. Tageszeiten wie „früh“ und „mittags“ liest Mimir selbst: früh wird 8 Uhr, mittags 12 Uhr, abends 18 Uhr. Teilnehmer und Anlagen nicht.',
                usage: '/agent-write',
            },
{
                name: '/scope',
                text: 'Zeigt oder stellt ein, wie weit das Modell reichen darf. Im Agentenmodus sind es die '
                    + 'Dateiwerkzeuge im festen Arbeitsverzeichnis, im Terminumfang nur die vier Kalenderwerkzeuge – '
                    + 'dann ohne Dateizugriff und ohne Arbeitsverzeichnis. Der Terminumfang gilt über einen Neustart '
                    + 'hinweg und gibt die Kalenderwerkzeuge ohne /agent und ohne /agent-write frei; jeder Vorgang '
                    + 'wird trotzdem vorher als Unterschied gezeigt. Womit das Modell Termine anlegt, hängt an den '
                    + 'Kalendernamen, die es in der Anweisung sieht – ein erfundener Name wird abgelehnt.',
                usage: '/scope [agent|termine]',
            },
            { name: '/scope agent', text: 'Dateiwerkzeuge im Arbeitsverzeichnis wie bisher' },
            { name: '/scope termine', text: 'Nur die Kalenderwerkzeuge, ohne Dateizugriff' },
            {
                name: '/agent-dir',
                text: 'Zeigt das feste Arbeitsverzeichnis und die maximale Schrittzahl',
                usage: '/agent-dir <pfad>',
            },
            { name: '/agent-dir <pfad>', text: 'Legt fest, wo gelesen werden darf' },
            {
                name: 'Dokumente ablegen statt anhängen',
                text: 'Leg ein Handbuch, Protokoll oder Datenblatt in das Arbeitsverzeichnis von '
                    + '/agent-dir und schalte /agent ein. Mimir findet es über search_files und liest '
                    + 'es über read_file – immer ganz, höchstens 128 KiB, und wird es größer abgelehnt. '
                    + 'Der Inhalt bleibt für alle folgenden Fragen da, während ein Anhang nur für die '
                    + 'eine Nachricht gilt. Jeder Lesezugriff muss bestätigt werden.',
            },
            { name: '/tools', text: 'Listet die verfügbaren Werkzeuge mit Beschreibung, getrennt nach lesend und schreibend', usage: '/tools' },
        ],
    },
];

function usageHint(command) {
    for (const group of COMMAND_HELP) {
        for (const entry of group.entries) {
            if (entry.usage && entry.name.split(' ')[0] === `/${command}`) {
                return `Verwendung: ${entry.usage}`;
            }
        }
    }

    return `Verwendung: /${command}`;
}

// Die Übersicht als eigener Block: Befehl und Beschreibung in Spalten, nach
// Themen gruppiert. Ein Fließtext aus Newlines wäre bei der Zahl der Befehle
// kaum lesbar.
function renderHelp() {
    const help = document.createElement('div');
    help.className = 'help';

    const intro = document.createElement('p');
    intro.className = 'help-intro';
    intro.textContent = 'Eingaben, die mit / beginnen, werden lokal ausgeführt und nicht an Ollama gesendet. '
        + 'TAB ergänzt den Befehl und, nach einem Leerzeichen, sein Argument; ein zweites TAB geht '
        + 'durch die Möglichkeiten, ENTER übernimmt sie.';
    help.appendChild(intro);

    for (const group of COMMAND_HELP) {
        const section = document.createElement('div');
        section.className = 'help-group';

        const title = document.createElement('div');
        title.className = 'help-group-title';
        title.textContent = group.title;

        const list = document.createElement('dl');
        list.className = 'help-list';

        for (const entry of group.entries) {
            const term = document.createElement('dt');
            term.textContent = entry.name;
            const description = document.createElement('dd');
            description.textContent = entry.text;
            list.append(term, description);
        }

        section.append(title, list);
        help.appendChild(section);
    }

    const note = document.createElement('p');
    note.className = 'help-note';
    // `ollama.example.org` statt einer echten Adresse: Der Text landet im
    // ausgelieferten Binary, und eine konkrete IP dort sähe wie ein eingebauter
    // Vorgabewert aus – auch wenn es nur ein Beispiel ist.
    note.textContent = 'Angaben in spitzen Klammern ersetzen, zum Beispiel: /server-url ollama.example.org:11434. '
        + 'Jede Änderung an Server-URL, SSH-Ziel oder Arbeitsverzeichnis wird vorher bestätigt und lädt die Modellliste neu.';
    help.appendChild(note);

    return help;
}

// Die Werkzeuge als Übersicht, nicht als Textbrei.
//
// Nach Gruppen getrennt, weil es die einzige Frage ist, die der Benutzer
// wirklich hat: Was darf das Modell gerade? Eine flache Liste aus sechs langen
// Zeilen beantwortet sie nicht – man muss jede Zeile lesen, um zu sehen, dass
// `write_file` gar nicht dabei ist, solange der Schreibmodus gesperrt ist.
//
// Die Beschreibungen kommen unverändert aus dem Angebot an das Modell. Sie hier
// zu kürzen hieße eine zweite Fassung pflegen, die irgendwann nicht mehr stimmt.
function renderTools(toolset) {
    // Ohne Namen kommt nichts in die Liste: Eine Zeile „unbekannt" wäre eine
    // Anzeige ohne Aussage, und sie würde als gezähltes Werkzeug mitzählen.
    const alle = toolset.tools
        .map((tool) => ({
            name: String(tool.function?.name ?? ''),
            description: String(tool.function?.description ?? '').trim(),
            schreibt: WRITE_TOOL_NAMES.includes(String(tool.function?.name ?? '')),
        }))
        .filter((tool) => tool.name !== '');

    const lesend = alle.filter((tool) => !tool.schreibt);
    const schreibend = alle.filter((tool) => tool.schreibt);
    const angemeldet = Boolean(calendar.status?.logged_in_hint);
    const termine = agent.scope === 'termine';
    const box = document.createElement('div');
    box.className = 'help tools';

    const kopf = document.createElement('p');
    kopf.className = 'help-intro';
    // Das Arbeitsverzeichnis steht in der Kopfzeile: Es ist die Grenze, an der
    // das Modell scheitert, und man sucht es sonst in den Einstellungen. Im
    // Terminumfang gibt es diese Grenze nicht, und eine Zeile „nicht gesetzt"
    // täte dort so, als fehle etwas.
    kopf.textContent = termine
        ? 'Terminumfang: Das Modell darf ausschließlich den Kalender lesen und verändern. '
            + 'Es kann keine Dateien erreichen, und jeder Aufruf wird bestätigt, jeder Schreibvorgang als Unterschied gezeigt.'
        : (agent.writeEnabled
            ? 'Das Modell darf lesen und schreiben. Jeder Aufruf wird bestätigt, jeder Schreibvorgang als Unterschied gezeigt.'
            : 'Das Modell darf nur lesen. Jeder Aufruf wird bestätigt.');
    box.appendChild(kopf);

    if (!termine) {
        const pfad = document.createElement('p');
        pfad.className = 'tools-root';
        pfad.textContent = agent.root || 'nicht gesetzt';
        box.appendChild(pfad);
    }

    const gruppe = (titel, eintraege, hinweis) => {
        if (eintraege.length === 0) {
            return;
        }

        const abschnitt = document.createElement('div');
        abschnitt.className = 'help-group';

        const ueberschrift = document.createElement('div');
        ueberschrift.className = 'help-group-title';
        ueberschrift.textContent = titel;
        abschnitt.appendChild(ueberschrift);

        const liste = document.createElement('dl');
        liste.className = 'help-list';

        for (const werkzeug of eintraege) {
            const name = document.createElement('dt');
            name.textContent = werkzeug.name;
            const text = document.createElement('dd');
            text.textContent = werkzeug.description;
            liste.append(name, text);
        }

        abschnitt.appendChild(liste);

        if (hinweis) {
            const fuss = document.createElement('p');
            fuss.className = 'help-note';
            fuss.textContent = hinweis;
            abschnitt.appendChild(fuss);
        }

        box.appendChild(abschnitt);
    };

    gruppe(
        `Lesend (${lesend.length})`,
        lesend,
        // Der Kalenderzugang steht nicht in den Werkzeugnamen, und ohne den
        // Hinweis sucht man ihn vergeblich in der Liste.
        angemeldet
            ? 'Der Kalender ist angemeldet; list_calendar_events liest daraus.'
            : 'Für list_calendar_events muss Mimir mit /calendar angemeldet sein.',
    );

    if (termine) {
        // Im Terminumfang sind die drei schreibenden Kalenderwerkzeuge da und
        // die zwei Dateiwerkzeuge nicht. Beide Fälle als „an" oder „gesperrt"
        // zu beschriften wäre falsch: Es gibt nichts zu freischalten.
        gruppe(
            `Schreibend (${schreibend.length})`,
            schreibend,
            'Jeder Vorgang wird vor dem Schreiben als Unterschied gezeigt und wartet auf deine '
            + 'Freigabe. Teilnehmer, Anlagen und Serientermine bleiben Nextcloud vorbehalten. '
            + 'Termine lassen sich über Mimir nicht zurücknehmen.',
        );
    } else if (agent.writeEnabled) {
        gruppe(
            `Schreibend (${schreibend.length})`,
            schreibend,
            'Jeder Vorgang wird vor dem Schreiben als Unterschied gezeigt und lässt sich bei '
            + 'Dateien im Chat zurücknehmen. Teilnehmer, Anlagen und Serientermine bleiben '
            + 'Nextcloud vorbehalten.',
        );
    } else {
        // Die Zahl steht nicht fest, sondern richtet sich danach, was sonst noch
        // fehlt: Ohne Anmeldung kämen die drei Kalenderwerkzeuge auch mit
        // /agent-write nicht dazu. Eine Zahl zu behaupten, die nicht stimmt, wäre
        // genau die Sorte Fehler, die man hier vermeiden will.
        const kommen = ['write_file', 'edit_file'];
        if (angemeldet) {
            kommen.push(...CALENDAR_TOOL_NAMES);
        }

        const gesperrt = document.createElement('p');
        gesperrt.className = 'help-note tools-locked';
        gesperrt.textContent = `Schreibend (${kommen.length}): gesperrt. `
            + `Mit /agent-write kommen ${kommen.join(', ')} dazu.`;
        box.appendChild(gesperrt);
    }

    return box;
}

async function handleChatCommand(text) {
    // An mehreren Leerzeichen getrennt: Deshalb funktioniert ein Pfad mit
    // Leerzeichen nicht (/agent-dir /home/me/Meine Docs endet als
    // Verwendungshinweis). Der Rest der Zeile müsste als ein Argument gelten.
    const [rawCommand, ...args] = text.slice(1).trim().split(/\s+/);
    const command = rawCommand.toLowerCase();
    appendMessageToUI('system', text);

    if (command === 'help') {
        const bubble = appendMessageToUI('system', '');
        bubble.classList.add('help-message');
        bubble.replaceChildren(renderHelp());
        return;
    }

    if (command === 'calendar') {
        const bekannt = calendar.status?.configured
            ? `${calendar.status.server_url} als ${calendar.status.username}`
            : 'noch keine';

        if (args.includes('aus')) {
            const ok = window.confirm(
                `Meldet sich ab und vergisst das App-Passwort. Adresse und Benutzername (${bekannt}) bleiben stehen, `
                + 'damit ein erneutes Anmelden nur ein Feld braucht.',
            );

            if (!ok) {
                return;
            }

            try {
                await invoke('calendar_logout');
                await aktualisiereKalender();
                appendMessageToUI('system', 'Abgemeldet. Das App-Passwort ist aus dem Arbeitsspeicher entfernt.');
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }

            return;
        }

        if (args.includes('zertifikat')) {
            const ergebnis = await bereiteZertifikatVor();
            await aktualisiereKalenderZustand();

            if (ergebnis === 'bestätigt') {
                appendMessageToUI('system', 'Zertifikat bestätigt und gemerkt.');
            } else if (ergebnis === 'abgebrochen') {
                appendMessageToUI('system', 'Zertifikat nicht bestätigt. Ohne Bestätigung bleibt der Zugang gesperrt.');
            } else {
                appendMessageToUI('system', 'Die Adresse läuft über HTTP, es wird kein Zertifikat benötigt.');
            }

            return;
        }

        if (args.length > 0) {
            appendMessageToUI('system', usageHint('calendar'));
            return;
        }

        // Ohne Angabe: Ist schon alles eingetragen und angemeldet, genügt die
        // Auskunft. Sonst öffnet das Fenster und fragt nach.
        if (calendar.status?.logged_in) {
            const stand = calendar.status.last_success
                ? `, letzter Stand ${uhrzeitFormatter.format(new Date(calendar.status.last_success * 1000))}`
                : '';
            appendMessageToUI(
                'system',
                `Angemeldet als ${calendar.status.username} an ${calendar.status.server_url}${stand}. `
                + `${calendar.events.length} Termin(e) in der Leiste.`,
            );
            return;
        }

        if (calendar.status?.configured) {
            const bestaetigt = window.confirm(
                `Möchtest du ${calendar.status.server_url} erneut anmelden? `
                + 'Die Adresse bleibt stehen, das Fenster fragt nur nach dem App-Passwort.',
            );

            if (!bestaetigt) {
                return;
            }
        }

        const ok = await oeffneKalenderDialog();

        if (!ok) {
            appendMessageToUI('system', 'Anmeldung abgebrochen.');
            return;
        }

        await aktualisiereKalender();

        if (calendar.error) {
            appendMessageToUI('system', `Anmeldung möglich, aber die Termine fehlen: ${calendar.error}`);
            return;
        }

        const namen = (calendar.status.known_calendars || []).map((item) => item.display_name);
        appendMessageToUI(
            'system',
            `Angemeldet als ${calendar.status.username}. Kalender: ${namen.join(', ') || 'keine'}. `
            + `${calendar.events.length} Termin(e) in den nächsten Wochen.`,
        );
        return;
    }

    if (command === 'termine') {
        await aktualisiereKalenderTermine();

        if (calendar.error) {
            appendMessageToUI('system', calendar.error);
            return;
        }

        // Ohne Anmeldung ist die Ursache wichtig, sonst liest sich die
        // Meldung wie ein leerer Kalender.
        if (!calendar.status?.logged_in) {
            appendMessageToUI('system', 'Nicht angemeldet. Mit /calendar anmelden.');
            return;
        }

        if (calendar.events.length === 0) {
            appendMessageToUI('system', 'Keine Termine in den nächsten Wochen.');
            return;
        }

        // Alle Termine, nach Tagen gruppiert. Das ist die Form, in der man
        // Kalender sonst auch liest, und die Zuordnung zum Kalender steht am
        // Balken statt in Klammern am Zeilenende.
        const blase = document.createElement('div');
        blase.className = 'message system agenda-message';
        blase.appendChild(bauAgenda(calendar.events));

        const anzahl = document.createElement('div');
        anzahl.className = 'agenda-count';
        anzahl.textContent = `${calendar.events.length} Termin${calendar.events.length === 1 ? '' : 'e'}`;
        blase.appendChild(anzahl);

        chatContainer.appendChild(blase);
        scrollToBottom();
        return;
    }

    if (command === 'system') {
        if (args.includes('aus')) {
            try {
                await invoke('set_chat_config', { systemPrompt: '' });
                chat.systemPrompt = '';
                updateContextUsage();
                appendMessageToUI('system', 'Systemanweisung entfernt.');
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args.length > 0) {
            appendMessageToUI('system', usageHint('system'));
            return;
        }

        const value = await requestSystemPrompt();

        if (value === null) {
            appendMessageToUI('system', 'Systemanweisung unverändert.');
            return;
        }

        try {
            await invoke('set_chat_config', { systemPrompt: value });
            chat.systemPrompt = value.trim();
            updateContextUsage();
            appendMessageToUI(
                'system',
                chat.systemPrompt
                    ? `Systemanweisung gesetzt (${chat.systemPrompt.length} Zeichen).`
                    : 'Systemanweisung entfernt.',
            );
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'context') {
        if (args.length === 0) {
            try {
                const config = await refreshChatConfig();
                appendMessageToUI(
                    'system',
                    config.context_tokens > 0
                        ? `Kontextfenster: ${config.context_tokens} Token. Belegung und Schätzung stehen oben rechts.`
                        : 'Kontextfenster: Vorgabe des Modells. Mit /context <token> lässt sich eine Größe festlegen, /context 0 stellt wieder auf die Vorgabe um.',
                );
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args.length > 1) {
            appendMessageToUI('system', usageHint('context'));
            return;
        }

        const tokens = Number(args[0]);

        if (!Number.isInteger(tokens) || tokens < 0) {
            appendMessageToUI('system', 'Die Angabe muss eine ganze Zahl Token sein, 0 für die Modellvorgabe.');
            return;
        }

        try {
            const config = await invoke('set_chat_config', { contextTokens: tokens });
            chat.contextTokens = Number(config.context_tokens) || 0;
            updateContextUsage();
            appendMessageToUI(
                'system',
                chat.contextTokens > 0
                    ? `Kontextfenster auf ${chat.contextTokens} Token gesetzt. Ollama lädt das Modell beim ersten Mal neu.`
                    : 'Kontextfenster wieder auf die Vorgabe des Modells gesetzt.',
            );
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'history') {
        try {
            await refreshChatConfig();
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
            return;
        }

        if (args.length === 0) {
            appendMessageToUI(
                'system',
                chat.saveHistory
                    ? `Der Verlauf wird auf der Platte gehalten. Gespeichert sind ${Math.min(messageHistory.length, MAX_GESPEICHERTE_NACHRICHTEN)} Nachrichten in ${historyFileName()}.`
                    + (chat.verlaufGekuerzt ? ` Die ersten ${messageHistory.length - MAX_GESPEICHERTE_NACHRICHTEN} Nachrichten passen nicht mehr in die Datei und wurden nicht mitgeschrieben.` : '')
                    : 'Der Verlauf wird nicht gespeichert und beim Beenden verworfen. Mit /history an wird er auf der Platte gehalten, immer nach Rückfrage.',
            );
            return;
        }

        if (args[0] === 'löschen' || args[0] === 'loeschen') {
            try {
                await invoke('delete_chat_history');
                appendMessageToUI('system', 'Gespeicherter Verlauf gelöscht.');
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args[0] !== 'an' && args[0] !== 'aus') {
            appendMessageToUI('system', usageHint('history'));
            return;
        }

        const wanted = args[0] === 'an';

        if (wanted && !window.confirm(
            'Verlauf auf der Platte halten? Mimir schreibt dann jede Unterhaltung als Klartextdatei neben die Konfiguration '
            + 'und lädt sie beim nächsten Start wieder. Das betrifft auch alles, was du eingetippt hast.',
        )) {
            appendMessageToUI('system', 'Speichern des Verlaufs nicht freigegeben.');
            return;
        }

        try {
            const config = await invoke('set_chat_config', { saveHistory: wanted });
            chat.saveHistory = Boolean(config.save_history);

            if (chat.saveHistory) {
                await persistHistory();
            } else {
                await invoke('delete_chat_history');
                appendMessageToUI('system', 'Verlauf wird nicht mehr gespeichert, die gespeicherte Datei ist gelöscht.');
            }
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'agent') {
        if (args.length > 0) {
            appendMessageToUI('system', usageHint('agent'));
            return;
        }

        agentToggleBtn.click();
        return;
    }

    if (command === 'provider') {
        if (args.length > 1) {
            appendMessageToUI('system', usageHint('provider'));
            return;
        }

        // Ohne Argument wird nur gezeigt, was eingestellt ist. Der Provider
        // entscheidet, wo die Modelle herkommen, und der steht sonst nirgends.
        if (args.length === 0) {
            try {
                const url = await invoke('get_server_url');
                const umfang = (await refreshAgentConfig()).scope;
                appendMessageToUI(
                    'system',
                    `Provider: ${providername(provider)} – ${url}. Umfang: ${umfangsname(umfang)}.`
                    + (lokalesOllama()
                        ? ' Lokal gibt es nur den Terminumfang; mit /provider remote kommst du zu den Dateiwerkzeugen.'
                        : '')
                );
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        await wechsleProvider(args[0].toLowerCase());
        return;
    }

    if (command === 'scope') {
        if (args.length > 1) {
            appendMessageToUI('system', usageHint('scope'));
            return;
        }

        // Der lokale Provider gibt keine Dateiwerkzeuge heraus. Das Backend
        // lehnt den Wechsel ab; hier steht der Grund vorher im Chat, statt ihn
        // erst nach dem Bestätigungsfenster zu erfahren.
        if (args.length === 1 && args[0].toLowerCase() === 'agent' && lokalesOllama()) {
            appendMessageToUI(
                'system',
                'Im lokalen Provider gibt es nur den Terminumfang: Das Modell läuft auf diesem '
                + 'Rechner und bekommt dort keine Dateiwerkzeuge. Mit /provider remote kommst du '
                + 'zu den Dateiwerkzeugen zurück.'
            );
            return;
        }

        // Ohne Argument wird nur gezeigt, was eingestellt ist. Das ist keine
        // Nebenbemerkung: Der Umfang entscheidet, welche Werkzeuge das Modell
        // sieht, und der steht in keinem Fenster.
        if (args.length === 0) {
            try {
                const config = await refreshAgentConfig();
                appendMessageToUI('system', umfangstext(config.scope, config));
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        const gewaehlt = args[0].toLowerCase();

        if (!SCOPE_NAMES.includes(gewaehlt)) {
            appendMessageToUI('system', usageHint('scope'));
            return;
        }

        try {
            const config = await refreshAgentConfig();

            if (config.scope === gewaehlt) {
                appendMessageToUI('system', `Der Umfang ist bereits ${umfangsname(config.scope)}.`);
                return;
            }

            const frage = gewaehlt === 'termine'
                ? 'Terminumfang einstellen? Das Modell bekommt dann nur die vier Kalenderwerkzeuge. '
                    + 'Es kann keine Dateien lesen oder schreiben, und es braucht dafür kein Arbeitsverzeichnis. '
                    + 'Termine legt es an, ändert und löscht es, jeweils nach Vorschau und deiner Freigabe. '
                    + 'Das gilt über einen Neustart hinweg.'
                : 'Agentenmodus einstellen? Das Modell bekommt wieder die Dateiwerkzeuge im festen '
                    + `Arbeitsverzeichnis${config.root ? ` (${config.root})` : ''}. Ohne Arbeitsverzeichnis `
                    + 'gibt es dort nichts zu lesen. Termine bleiben möglich.';

            if (!window.confirm(frage)) {
                appendMessageToUI('system', 'Umfang unverändert.');
                return;
            }

            // Ohne `root` würde das Arbeitsverzeichnis mitgeschickt und damit
            // beim bloßen Umfangwechsel überschrieben. Das Backend behält die
            // eingestellte Größe, wenn nichts angegeben wird, und das gilt hier
            // genauso für den Umfang.
            const neu = await invoke('set_agent_config', {
                root: config.root,
                maxSteps: null,
                scope: gewaehlt,
            });

            // Das Werkzeugangebot hängt am Umfang, also muss der Cache weg.
            agent.toolset = null;
            agent.scope = gewaehlt;
            setAgentMode(agent.enabled);
            appendMessageToUI('system', umfangstext(neu.scope, neu));
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'agent-dir') {
        if (args.length === 0) {
            try {
                const config = await refreshAgentConfig();
                appendMessageToUI(
                    'system',
                    config.root
                        ? `Arbeitsverzeichnis des Agentenmodus: ${config.root} (höchstens ${config.max_steps} Schritte je Nachricht)`
                        : 'Es ist kein Arbeitsverzeichnis gesetzt. Mit /agent-dir <pfad> eines festlegen.'
                );
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args.length > 1) {
            appendMessageToUI('system', usageHint('agent-dir'));
            return;
        }

        const root = args[0];

        if (!window.confirm(`Arbeitsverzeichnis für den Agentenmodus auf ${root} setzen? Dort darf das Modell ausschließlich lesen.`)) {
            appendMessageToUI('system', 'Änderung des Arbeitsverzeichnisses abgebrochen.');
            return;
        }

        try {
            const config = await invoke('set_agent_config', { root, maxSteps: null });
            // Der Systemprompt nennt den Pfad, also muss der Cache neu geladen
            // werden.
            agent.toolset = null;
            agent.root = config.root;
            agent.maxSteps = Number(config.max_steps) || agent.maxSteps;
            setAgentMode(agent.enabled);
            appendMessageToUI('system', `Arbeitsverzeichnis gesetzt: ${config.root}`);
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'agent-write') {
        if (args.length > 0) {
            appendMessageToUI('system', usageHint('agent-write'));
            return;
        }

        if (agent.scope === 'termine') {
            appendMessageToUI('system', 'Im Terminumfang gibt es keine Dateiwerkzeuge, die freizugeben wären. Termine legt das Modell an, ändert und löscht sie nach Vorschau und deiner Freigabe. Mit /scope agent wird der Schreibmodus wieder nötig.');
            return;
        }

        if (!agent.enabled) {
            appendMessageToUI('system', 'Der Schreibmodus gehört zum Agentenmodus. Erst /agent benutzen.');
            return;
        }

        try {
            const enabled = await setWriteMode(!agent.writeEnabled);
            appendMessageToUI(
                'system',
                enabled
                    ? `Schreibende Werkzeuge freigegeben. Jeder Vorgang erscheint als Unterschied und wartet auf deine Genehmigung.${calendar.status?.logged_in ? ' Im Kalender kann das Modell Termine anlegen, ändern und löschen – es nennt dafür Titel und Uhrzeit, wie du sie gesagt hast.' : ''}`
                    : 'Schreibende Werkzeuge gesperrt. Es wird nichts mehr verändert.',
            );
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'tools') {
        if (args.length > 0) {
            appendMessageToUI('system', usageHint('tools'));
            return;
        }

        try {
            const toolset = await loadAgentToolset();
            const bubble = appendMessageToUI('system', '');
            bubble.classList.add('help-message');
            bubble.replaceChildren(renderTools(toolset));
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'server-status') {
        const models = await loadModels();
        let message;

        if (models === null) {
            // Ein einzelner Aussetzer und ein ausgefallener Server werden nicht
            // gleich gemeldet: Beim ersten hilft Abwarten, beim zweiten der
            // Neustart.
            const gesehen = lastContact;
            const frisch = gesehen !== null && nowSeconds() - gesehen <= UNSTABLE_BISHER_SECONDS;
            message = frisch
                ? `Ollama antwortet gerade nicht, war aber vor ${nowSeconds() - gesehen} Sekunden noch da. Das sieht nach einer Funkstelle aus; die vorhandene Modellliste bleibt in Benutzung.`
                : `Ollama ist unter ${await invoke('get_server_url')} nicht erreichbar. `
                    + 'Läuft der Server auf einem anderen Rechner, ändere die Adresse über „Adresse“ im Kopf. '
                    + 'Mit /server-start wird der Server über SSH gestartet.';
        } else if (models.length > 0) {
            message = `Ollama ist online. ${models.length} Modell(e) verfügbar.`;
        } else {
            message = 'Ollama ist online, aber es wurden keine Modelle gefunden.';
        }

        appendMessageToUI('system', message);
        return;
    }

    if (command === 'server-start') {
        await startOllamaViaSsh();
        return;
    }

    if (command === 'ssh-target') {
        if (args.length === 0) {
            try {
                const ssh = await invoke('get_ssh_config');
                const target = ssh.target || 'nicht konfiguriert';
                appendMessageToUI('system', `SSH-Ziel: ${target}:${ssh.port}`);
                appendMessageToUI('system', `SSH-Schlüssel: ${describeIdentityFile(ssh.identity_file)}`);
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args.length > 2) {
            appendMessageToUI('system', usageHint('ssh-target'));
            return;
        }

        const port = Number(args[1] ?? 22);

        if (!Number.isInteger(port) || port < 1 || port > 65535) {
            appendMessageToUI('system', 'Der SSH-Port muss eine Zahl zwischen 1 und 65535 sein.');
            return;
        }

        if (!window.confirm(`SSH-Ziel auf ${args[0]}:${port} setzen?`)) {
            appendMessageToUI('system', 'SSH-Zieländerung abgebrochen.');
            return;
        }

        try {
            // Die Schlüsselvorgabe bleibt beim Wechsel des Ziels erhalten.
            const current = await invoke('get_ssh_config');
            const ssh = await invoke('set_ssh_config', {
                target: args[0],
                port,
                identityFile: current.identity_file || null,
            });
            appendMessageToUI('system', `SSH-Ziel gespeichert: ${ssh.target}:${ssh.port}`);
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'ssh-key') {
        if (args.length === 0) {
            try {
                const ssh = await invoke('get_ssh_config');
                appendMessageToUI('system', `SSH-Schlüssel: ${describeIdentityFile(ssh.identity_file)}`);
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args.length > 1) {
            appendMessageToUI('system', usageHint('ssh-key'));
            return;
        }

        const clearing = args[0].toLowerCase() === 'aus';
        const identityFile = clearing ? null : args[0];

        if (!clearing && !window.confirm(`SSH-Schlüssel auf ${args[0]} setzen?`)) {
            appendMessageToUI('system', 'SSH-Schlüsseländerung abgebrochen.');
            return;
        }

        try {
            const current = await invoke('get_ssh_config');
            if (!current.target) {
                appendMessageToUI('system', 'Kein SSH-Ziel konfiguriert. Erst /ssh-target <benutzer@host> setzen.');
                return;
            }
            const ssh = await invoke('set_ssh_config', {
                target: current.target,
                port: current.port,
                identityFile,
            });
            appendMessageToUI('system', `SSH-Schlüssel gespeichert: ${describeIdentityFile(ssh.identity_file)}`);
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    if (command === 'server-url') {
        if (args.length === 0) {
            try {
                const serverUrl = await invoke('get_server_url');
                appendMessageToUI('system', `Aktive Ollama-URL: ${serverUrl}`);
            } catch (error) {
                appendMessageToUI('system', `Fehler: ${error}`);
            }
            return;
        }

        if (args.length > 1) {
            appendMessageToUI('system', usageHint('server-url'));
            return;
        }

        try {
            const previousUrl = await invoke('get_server_url');

            if (!window.confirm(`Ollama-URL auf ${args[0]} setzen und Modellliste neu laden?`)) {
                appendMessageToUI('system', 'Server-URL-Änderung abgebrochen.');
                return;
            }

            const serverUrl = await invoke('set_server_url', {
                serverUrl: args[0],
            });
            const models = await loadModels();

            if (models === null) {
                await invoke('set_server_url', { serverUrl: previousUrl });
                await loadModels();
                appendMessageToUI(
                    'system',
                    `Server-URL nicht erreichbar. Die vorherige URL wurde wiederhergestellt: ${previousUrl}`
                );
                return;
            }

            if (previousUrl !== serverUrl) {
                // Der alte Verlauf gehört zum alten Server und wird auch von der
                // Platte entfernt, sonst taucht er beim nächsten Start wieder auf.
                messageHistory = [];
                await invoke('delete_chat_history').catch(() => {});
                updateContextUsage();
            }

            const modelStatus = models.length > 0
                ? `${models.length} Modell(e) verfügbar.`
                : 'Der Server ist erreichbar, es wurden aber keine Modelle gefunden.';
            appendMessageToUI(
                'system',
                `Ollama-URL gespeichert: ${serverUrl}\n${previousUrl !== serverUrl ? 'Chatverlauf zurückgesetzt. ' : ''}${modelStatus}`
            );
        } catch (error) {
            appendMessageToUI('system', `Fehler: ${error}`);
        }
        return;
    }

    appendMessageToUI('system', `Unbekannter Befehl: /${command}. Nutze /help.`);
}

// Zustand der Denktext-Blöcke: Umschalter, Textbereich und ob der Benutzer den
// Block selbst bedient hat. WeakMap, damit die Blasen beim Leeren mit sterben.
const reasoningBlocks = new WeakMap();

function setReasoningExpanded(reasoning, expanded) {
    const state = reasoningBlocks.get(reasoning);

    if (!state) {
        return;
    }

    state.toggle.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    state.toggle.textContent = `${expanded ? '▾' : '▸'} Denkprozess`;
    state.text.hidden = !expanded;
}

// Sobald das Modell antwortet, ist das Denken vorbei: Der Block klappt sich
// selbst ein. Hat der Benutzer ihn zwischendurch selbst bedient, bleibt seine
// Wahl maßgeblich.
function collapseReasoningWhenAnswerStarts(bubble) {
    const reasoning = bubble.querySelector('.reasoning');
    const state = reasoning ? reasoningBlocks.get(reasoning) : null;

    if (!state || state.touched) {
        return;
    }

    setReasoningExpanded(reasoning, false);
}

// Hilfsfunktion: Denktext-Block in der Antwortblase anlegen oder zurückgeben.
// Während der Denkphase ist er aufgeklappt, damit man dem Modell zusehen kann.
function ensureReasoningBlock(bubble) {
    let reasoning = bubble.querySelector('.reasoning');

    if (reasoning) {
        return reasoning;
    }

    const reasoningText = document.createElement('div');
    reasoningText.className = 'reasoning-text markdown';

    const toggle = document.createElement('button');
    toggle.type = 'button';
    toggle.className = 'reasoning-toggle';
    toggle.addEventListener('click', () => {
        const state = reasoningBlocks.get(reasoning);
        state.touched = true;
        setReasoningExpanded(reasoning, toggle.getAttribute('aria-expanded') !== 'true');
    });

    reasoning = document.createElement('div');
    reasoning.className = 'reasoning';
    reasoning.append(toggle, reasoningText);
    reasoningBlocks.set(reasoning, { toggle, text: reasoningText, touched: false });
    setReasoningExpanded(reasoning, true);
    bubble.prepend(reasoning);
    return reasoning;
}

// ------------------------------------------------------ Kontext, Anweisung, Verlauf
// Ollamas Standardfenster liegt bei 4096 Token; Mimir kennt die Vorgabe des
// Modells nicht. Die Anzeige schätzt deshalb aus der Textlänge und warnt, bevor
// das Fenster zum eigentlichen Problem wird.
//
// Achtung: hier werden Zeichen gezählt (Länge eines JavaScript-Strings), keine
// Bytes – deshalb ist der Wert „Zeichen je Token“. Mehrsprachiger Text braucht
// mehr Zeichen je Token, die Schätzung fällt also eher zu niedrig aus.
const ZEICHEN_PRO_TOKEN = 3.2;
const CONTEXT_WARNING_RATIO = 0.9;

function estimateTokens(messages) {
    let zeichen = 0;

    // Die acht Zeichen je Nachricht stehen für den Rahmen, den Ollama um jede
    // Nachricht herumschreibt (Rolle, Trennzeichen, Nachrichtentrenner).
    for (const message of messages) {
        zeichen += (message.content || '').length + (message.role || '').length + 8;
    }

    return Math.ceil((zeichen + (chat.systemPrompt || '').length) / ZEICHEN_PRO_TOKEN);
}

function formatTokenCount(value) {
    if (value >= 10000) {
        return `${Math.round(value / 1000)}k`;
    }

    if (value >= 1000) {
        return `${(value / 1000).toFixed(1)}k`;
    }

    return String(value);
}

function updateContextUsage(extraMessages = []) {
    const tokens = estimateTokens([...messageHistory, ...extraMessages]);
    const limit = chat.contextTokens;
    const ratio = limit > 0 ? tokens / limit : 0;

    contextUsage.textContent = limit > 0
        ? `Kontext: ~${formatTokenCount(tokens)} / ${formatTokenCount(limit)} Token`
        : `Kontext: ~${formatTokenCount(tokens)} Token (Modellvorgabe)`;
    contextUsage.classList.toggle('context-warning', ratio >= CONTEXT_WARNING_RATIO);
    contextUsage.title = limit > 0
        ? `Grobe Schätzung aus der Textlänge, etwa ${ZEICHEN_PRO_TOKEN} Zeichen je Token. `
            + `Ab ${Math.round(CONTEXT_WARNING_RATIO * 100)} Prozent kann das Modell den Anfang der Unterhaltung verlieren.`
        : 'Grobe Schätzung aus der Textlänge. Das Fenster des Modells ist nicht bekannt; '
            + 'mit /context lässt es sich festlegen.';
}

async function refreshChatConfig() {
    const config = await invoke('get_chat_config');
    chat.systemPrompt = config.system_prompt || '';
    chat.contextTokens = Number(config.context_tokens) || 0;
    chat.saveHistory = Boolean(config.save_history);
    updateContextUsage();
    return config;
}

// Die Systemanweisung geht als eigenes Feld an Ollama und landet nicht als
// Nachricht im Verlauf. Im Agentenmodus steht sie gemeinsam mit der
// Werkzeuganweisung in diesem Feld.
function systemPromptForRequest(agentPrompt = null) {
    const parts = [];

    if (agentPrompt) {
        parts.push(agentPrompt);
    }

    if (chat.systemPrompt) {
        parts.push(chat.systemPrompt);
    }

    // Ganz hinten angehängt: Das Datum ist keine Anweisung des Benutzers und
    // gehört nicht in die Agenten-Anweisung. Es steht trotzdem in **jeder**
    // Anfrage, auch ohne eigene Systemanweisung – sonst hinge die Antwort davon
    // ab, ob eine gesetzt ist.
    parts.push(datumsangabeFuerModell());

    return parts.join('\n\n');
}

function historyFileName() {
    return 'chat-history.json im Konfigurationsverzeichnis';
}

async function persistHistory() {
    if (!chat.saveHistory) {
        return;
    }

    // Gespeichert werden nur die jüngsten Nachrichten. Übernähme das Backend
    // unverändert, würde das Sichern nach hundert Nachrichten ganz aufhören
    // und der Benutzer bekäme davon nichts zu sehen. Der Kürzungsfall wird
    // bei /history benannt, damit nichts lautlos verloren geht.
    const zuSpeichern = messageHistory.slice(-MAX_GESPEICHERTE_NACHRICHTEN);
    chat.verlaufGekuerzt = zuSpeichern.length < messageHistory.length;

    try {
        await invoke('save_chat_history', { messages: zuSpeichern });
    } catch (error) {
        // Ein Fehler beim Sichern darf das Gespräch nicht unterbrechen.
        console.error('Verlauf nicht gespeichert:', error);
    }
}

function requestSystemPrompt() {
    return new Promise((resolve) => {
        let value = null;
        systemInput.value = chat.systemPrompt;
        systemDialog.returnValue = '';

        const handleSubmit = (event) => {
            event.preventDefault();
            value = systemInput.value;
            systemDialog.close('confirmed');
        };
        const handleClear = () => {
            value = '';
            systemDialog.close('cleared');
        };
        const handleClose = () => {
            systemForm.removeEventListener('submit', handleSubmit);
            systemClear.removeEventListener('click', handleClear);
            systemCancel.removeEventListener('click', handleCancel);
            resolve(value);
        };
        const handleCancel = () => systemDialog.close('cancelled');

        systemForm.addEventListener('submit', handleSubmit);
        systemClear.addEventListener('click', handleClear);
        systemCancel.addEventListener('click', handleCancel);
        systemDialog.addEventListener('close', handleClose, { once: true });
        systemDialog.showModal();
        systemInput.focus();
    });
}

// ------------------------------------------------------------------ Kalender
// Die Leiste zeigt die nächsten Termine der eingetragenen Nextcloud-Instanz.
// Sie liest selbst und lässt das Modell unangetastet: Termine sind Daten und
// gehören nicht in den Prompt. Die Leiste selbst bleibt rein lesend – Anlegen,
// Ändern und Löschen gehen über dieselbe Bestätigung wie eine Datei
// (CALENDAR_TOOL_NAMES), nicht über die Leiste.
const CALENDAR_MAX_ENTRIES = 8;
// Fünf Stunden, weil es so gewollt ist: Die Leiste ist ein Anzeigegerät und
// kein Meldedienst. Nach einem angelegten Termin wird ohnehin sofort neu
// geholt, und ein Klick auf den Erfrischungsknopf holt auf Zuruf.
const CALENDAR_REFRESH_STUNDEN = 5;
const CALENDAR_REFRESH_MS = CALENDAR_REFRESH_STUNDEN * 60 * 60 * 1000;

const tagesFormatter = new Intl.DateTimeFormat('de-DE', { weekday: 'short', day: '2-digit', month: '2-digit' });
const uhrzeitFormatter = new Intl.DateTimeFormat('de-DE', { hour: '2-digit', minute: '2-digit' });

// Für die Agenda im Chat: Der Wochentag steht über dem Tag, damit ein Kopf wie
// „Donnerstag, 5. November“ ohne Umstellen lesbar ist.
const WOCHENTAG_LANG = new Intl.DateTimeFormat('de-DE', { weekday: 'long' });
const TAG_MONAT_LANG = new Intl.DateTimeFormat('de-DE', { day: 'numeric', month: 'long' });

// Das heutige Datum für die Anfrage an das Modell.
//
// Ohne das rechnet das Modell bei „morgen“ oder „nächste Woche“ aus dem
// Gedächtnis und landet daneben – gefragt wurde nach den Terminen von morgen und
// genannt wurde der Vortag. Die Uhrzeit steht mit drin, weil Termine auch
// „heute um 15 Uhr“ heißen können und „morgen“ am späten Abend schon der
// nächste Tag ist.
const DATUM_LANG = new Intl.DateTimeFormat('de-DE', {
    weekday: 'long',
    day: 'numeric',
    month: 'long',
    year: 'numeric',
});
const ZEIT_LANG = new Intl.DateTimeFormat('de-DE', { hour: '2-digit', minute: '2-digit' });

const ZEITZONE = Intl.DateTimeFormat().resolvedOptions().timeZone || 'lokale Zeit';

// Das Datum wird bei **jeder** Anfrage neu bestimmt, nicht beim Laden der Seite:
// Eine offene App läuft über Mitternacht, und das Modell soll beim zweiten Wochentag
// trotzdem im richtigen Monat stehen.
//
// Die ausgerechneten Folgetage stehen mit drin. Eine Regel allein hat nicht
// genügt: Mit „ rechne aus dem heutigen Datum “ hat das Modell den Vortag genannt.
// Erst der ausgeschriebene Wert für MORGEN lässt sich mit dem Kalender abgleichen.
// In welcher Woche ein Tag liegt, in den Worten des Benutzers.
//
// Montag bis Sonntag, wie in Deutschland üblich. Die Woche des heutigen Tages
// heißt „laufenden“, die danach „nächsten“ und die dritte „übernächsten“.
function wochenname(tag, heute) {
    // Auf den Montag der Woche rechnen, nicht auf den Abstand in Tagen: Der
    // Sonntag derselben Woche ist sieben Tage vom Donnerstag entfernt und läge
    // bei einem Vergleich der Tage in der nächsten Woche.
    const montag = new Date(tag.getTime());
    montag.setDate(montag.getDate() - ((montag.getDay() + 6) % 7));
    const dieserMontag = new Date(heute.getTime());
    dieserMontag.setDate(dieserMontag.getDate() - ((dieserMontag.getDay() + 6) % 7));
    const wochen = Math.round(
        (montag.getTime() - dieserMontag.getTime()) / 604_800_000,
    );

    if (wochen === 0) return 'laufenden';
    if (wochen === 1) return 'nächsten';
    if (wochen === 2) return 'übernächsten';
    if (wochen === -1) return 'letzten';
    return wochen < 0 ? 'früheren' : 'späteren';
}

function datumsangabeFuerModell() {
    const jetzt = new Date();
    const iso = (tag) => `${tag.getFullYear()}-${String(tag.getMonth() + 1).padStart(2, '0')}-${String(tag.getDate()).padStart(2, '0')}`;
    const verschoben = (tage) => {
        const d = new Date(jetzt.getTime());
        d.setDate(d.getDate() + tage);
        return d;
    };

    // Montag als Wochenanfang, wie es in Deutschland üblich ist.
    const wochenStart = verschoben(-((jetzt.getDay() + 6) % 7));
    const wochenEnde = verschoben(6 - ((jetzt.getDay() + 6) % 7));
    const morgen = verschoben(1);
    const uebermorgen = verschoben(2);

    return [
        `HEUTE IST: ${iso(jetzt)}`,
        `Das ist ${DATUM_LANG.format(jetzt)}, ${ZEIT_LANG.format(jetzt)} Uhr ${ZEITZONE}.`,
        '',
        'Grundregel für jede Datumsangabe: Relative Wörter – „heute“, „morgen“,',
        '„übermorgen“, „gestern“, „in drei Tagen“, „nächste Woche“, „letzten Montag“,',
        '„am Wochenende“ – werden IMMER aus HEUTE ausgerechnet. Nicht aus deinem',
        'Trainingsstand, nicht aus einem Datum, das im Gesprächsverlauf steht, und',
        'nicht aus der Uhrzeit, zu der dieser Verlauf angefangen hat. Rechne selbst',
        'und nenne in deiner Antwort das ausgerechnete Datum.',
        '',
        'Dieselbe Rechnung, jeweils aus HEUTE – zur Kontrolle:',
        `- GESTERN war der ${iso(verschoben(-1))}.`,
        `- MORGEN ist der ${iso(morgen)} (${WOCHENTAG_LANG.format(morgen)}).`,
        `- ÜBERMORGEN ist der ${iso(uebermorgen)} (${WOCHENTAG_LANG.format(uebermorgen)}).`,
        `- DIESE WOCHE ist ${iso(wochenStart)} bis ${iso(wochenEnde)} (${WOCHENTAG_LANG.format(wochenStart)} bis ${WOCHENTAG_LANG.format(wochenEnde)}).`,
        `- NÄCHSTE WOCHE ist ${iso(verschoben(7 - ((jetzt.getDay() + 6) % 7)))} bis ${iso(verschoben(13 - ((jetzt.getDay() + 6) % 7)))}.`,
        `- In ${Math.max(0, 7 - ((jetzt.getDay() + 6) % 7))} Tagen beginnt die nächste Woche.`,
        // Die nächsten sieben Tage einzeln, mit Wochentag. Ein Wochentag allein
        // lässt offen, welches Jahr gemeint ist – „Sonntag“ passt auf jedes Jahr,
        // und die Antwort sah daraufhin aus wie aus einem alten Trainingsstand.
        // Jeder Tag nennt **seine** Woche. „2026-10-04 ist ein Sonntag“ allein
        // genügt nicht: Ein Sonntag ohne Jahr passt auf jede Woche, und das
        // Modell nannte ihn daraufhin die „übermächste Woche“ – er liegt in der
        // laufenden, die genau an ihm endet.
        '- Die nächsten sieben Tage, jeweils mit Wochentag und Woche:',
        ...Array.from({ length: 7 }, (_, i) => {
            const tag = verschoben(i + 1);
            return `  ${iso(tag)} ist ein ${WOCHENTAG_LANG.format(tag)} in der ${wochenname(tag, jetzt)}.`;
        }),
        '',
        'Wenn du ein Werkzeug für Termine aufrufst, übergib ihm den Zeitraum als',
        'Datumsgrenze im Format JJJJ-MM-TT oder als das Wort des Benutzers – beides',
        'wird gelesen. Ein Wort ist sogar sicherer: Es kann nicht aus dem',
        'Trainingsstand stammen, sondern nur aus diesem Systemfeld.',
        '',
        'Nenne im Chat ein Datum **mit Wochentag und Woche**, wie sie oben bei den',
        'einzelnen Tagen steht. Ein Wochentag allein lässt offen, welche Woche',
        'gemeint ist: „Sonntag“ passt auf jede Woche, und du hattest einmal den',
        'Sonntag, mit dem diese Woche endet, die übernächste Woche genannt. Wenn',
        'nach einem Termin gefragt wird, nenne zusätzlich den Tag, den das',
        'Werkzeug zurückgibt – dort steht die Woche ausgeschrieben dabei.',
        '',
        'Wochentagswörter im Zeitraum bedeuten: „montag“ ist der NÄCHSTE Montag nach',
        'HEUTE, nicht der kommende volle Montag – am Donnerstag ist der nächste',
        'Freitag der morgige Tag. „übernächster montag“ und „montag nächste woche“',
        'sind beide der Montag zwei Wochen nach heute. „montag diese woche“ ist der',
        'Montag der laufenden Woche, auch wenn er schon vorbei war.',
    ].join('\n');
}

// Die Farbe eines Kalenders, wie der Server sie meldet.
//
// Ohne Farbe greift ein eigener Ton, damit die Zuordnung auch dann lesbar
// bleibt, wenn kein Kalender eine Farbe mitbringt. Die Farben stehen in der
// Reihenfolge, in der die Kalender in der Leiste erscheinen – ein Kalender
// behält damit über die Termine hinweg dieselbe Farbe.
const KALENDER_FARBEN = [
    '#00e5c0',
    '#ffb454',
    '#7aa2ff',
    '#ff8ab4',
    '#a8e05f',
    '#c792ea',
    '#ffd866',
];

function kalenderFarbe(name) {
    const bekannt = calendar.status?.known_calendars ?? [];
    const gefunden = bekannt.find((entry) => entry.display_name === name || entry.href === name);

    if (gefunden?.color) {
        return gefunden.color;
    }

    // Ohne gemeldete Farbe: Der Platz in der Liste entscheidet. Sortiert wird
    // nach Namen, damit die Zuordnung zwischen zwei Aufrufen nicht springt.
    //
    // Die Namen der Termine selbst gehören mit in die Liste: Kommt die
    // Kalenderliste nicht an, ist sie sonst leer und jeder Kalender landete auf
    // demselben Platz – also auf derselben Farbe.
    const ausTerminen = calendar.events.map((event) => event.calendar);
    const namen = [...new Set([...bekannt.map((entry) => entry.display_name), ...ausTerminen, name])].sort();
    const platz = namen.indexOf(name);

    return KALENDER_FARBEN[(platz < 0 ? 0 : platz) % KALENDER_FARBEN.length];
}

// Baut die Agenda für den Chat: nach Tagen gruppiert, mit der Farbe des
// Kalenders als Balken.
//
// Ohne diese Form bleiben nur durchlaufende Zeilen, in denen man die Kalender
// an den Klammern am Zeilenende erkennt – bei einer Zeile mit zwei Terminen an
// einem Tag ist nicht mehr zu sehen, welcher zu welchem gehört.
function bauAgenda(events) {
    const tage = new Map();

    for (const event of events) {
        // Die Kalenderdaten stehen auf volle Minuten. Die 60 Sekunden sind ein
        // Puffer, damit ein Termin um 14:00:00 nicht wegen Sekundenrundung wie
        // 13:59 in den Vortag rutscht.
        const start = new Date((event.start + 60) * 1000);
        const schluessel = `${start.getFullYear()}-${start.getMonth()}-${start.getDate()}`;

        if (!tage.has(schluessel)) {
            tage.set(schluessel, { datum: start, termine: [] });
        }

        tage.get(schluessel).termine.push(event);
    }

    const liste = document.createElement('div');
    liste.className = 'agenda';

    for (const { datum, termine } of tage.values()) {
        const tagesBlock = document.createElement('section');
        tagesBlock.className = 'agenda-day';

        const kopf = document.createElement('div');
        kopf.className = 'agenda-day-head';

        const wochentag = document.createElement('span');
        wochentag.className = 'agenda-weekday';
        wochentag.textContent = WOCHENTAG_LANG.format(datum);
        kopf.appendChild(wochentag);

        const datumText = document.createElement('span');
        datumText.className = 'agenda-date';
        datumText.textContent = TAG_MONAT_LANG.format(datum);
        kopf.appendChild(datumText);

        // Das relative Wort steht rechts, weil es die Dringlichkeit sagt und
        // nicht zum Datum gehört.
        const relativ = document.createElement('span');
        relativ.className = 'agenda-relative';
        relativ.textContent = tagLabel(datum);
        kopf.appendChild(relativ);

        tagesBlock.appendChild(kopf);

        for (const event of termine) {
            tagesBlock.appendChild(bauAgendaEintrag(event));
        }

        liste.appendChild(tagesBlock);
    }

    return liste;
}

// Ein Termin in der Agenda.
function bauAgendaEintrag(event) {
    const punkt = document.createElement('article');
    punkt.className = 'agenda-entry';
    punkt.classList.toggle('running', laeuftJetzt(event));

    const balken = document.createElement('span');
    balken.className = 'agenda-bar';
    balken.style.background = kalenderFarbe(event.calendar);
    punkt.appendChild(balken);

    // Ein Ganztagestermin hat keine Uhrzeit. „Ganztägig“ in der Uhrzeitspalte
    // würde wie eine Uhrzeit gelesen, deshalb steht es als Wort im Text.
    const ganztag = event.all_day && !event.floating;

    if (ganztag) {
        // Ohne Zeitspalte rückt der Inhalt sonst in deren Feld, und der
        // Kalendername stünde mitten im Text. Die Klasse sagt dem Raster, dass
        // eine Spalte fehlt.
        punkt.classList.add('agenda-no-time');
    }

    if (!ganztag) {
        const zeit = document.createElement('span');
        zeit.className = 'agenda-time';
        zeit.textContent = zeitraum(event);
        punkt.appendChild(zeit);
    }

    const inhalt = document.createElement('span');
    inhalt.className = 'agenda-body';

    if (ganztag) {
        const hinweis = document.createElement('span');
        hinweis.className = 'agenda-allday';
        hinweis.textContent = 'ganztägig';
        inhalt.appendChild(hinweis);
    }

    const titel = document.createElement('span');
    titel.className = 'agenda-title';
    titel.textContent = event.summary || '(ohne Titel)';
    inhalt.appendChild(titel);

    if (event.location) {
        const ort = document.createElement('span');
        ort.className = 'agenda-meta';
        ort.textContent = event.location;
        inhalt.appendChild(ort);
    }

    for (const merkmal of merkmaleDesTermins(event)) {
        const spanne = document.createElement('span');
        spanne.className = 'agenda-meta';
        spanne.textContent = merkmal;
        inhalt.appendChild(spanne);
    }

    // Die Dauer ist keine Eigenschaft des Termins, sondern eine Angabe über ihn
    // – deshalb steht sie in derselben gedämpften Zeile wie Ort und Kategorien
    // und nicht in der schmalen Zeitspalte.
    const dauer = dauerText(event);

    if (dauer) {
        const spanne = document.createElement('span');
        spanne.className = 'agenda-meta';
        spanne.textContent = dauer;
        inhalt.appendChild(spanne);
    }

    punkt.appendChild(inhalt);

    const quelle = document.createElement('span');
    quelle.className = 'agenda-calendar';
    quelle.textContent = event.calendar;
    punkt.appendChild(quelle);

    return punkt;
}

// Erinnerung und Kategorien als kurze Zeilen unter dem Termin.
//
// Die Formulierung kommt aus dem Backend (`reminder_text`), damit Leiste,
// Agenda und Bestätigungsfenster dasselbe sagen. Hier wird nur angezeigt –
// eine zweite Übersetzung der Minuten in Worte wäre eine Stelle, an der die
// drei Ansichten auseinanderlaufen könnten.
function merkmaleDesTermins(event) {
    const merkmale = [];

    if (event.reminder_text) {
        merkmale.push(event.reminder_text);
    }

    for (const kategorie of event.categories || []) {
        merkmale.push(kategorie);
    }

    return merkmale;
}

// Der Folgetag über `setDate`, nicht über 86 400 000 Millisekunden.
//
// Ein Tag ist nicht immer 24 Stunden: In der Nacht der Sommerzeitumstellung
// sind es 23 oder 25. Über Millisekunden verschoben liegt „morgen“ dann um eine
// Stunde daneben, der Vergleich schlägt fehl, und der Tag erscheint als
// gewöhnliches Datum – zwei Tage im Jahr, an denen die Leiste „Heute“, „Morgen“
// und „Gestern“ nicht erkennt.
function einenTagWeiter(wann, tage) {
    const tag = new Date(wann);

    tag.setDate(tag.getDate() + tage);
    return tag;
}

function tagLabel(date) {
    const heute = new Date();
    heute.setHours(0, 0, 0, 0);
    const morgen = einenTagWeiter(heute, 1);
    const gestern = einenTagWeiter(heute, -1);
    const tag = new Date(date);
    tag.setHours(0, 0, 0, 0);

    if (tag.getTime() === heute.getTime()) {
        return 'Heute';
    }

    if (tag.getTime() === morgen.getTime()) {
        return 'Morgen';
    }

    if (tag.getTime() === gestern.getTime()) {
        return 'Gestern';
    }

    return tagesFormatter.format(tag);
}

// Die Dauer des Termins, aus Ende minus Beginn.
//
// Leer bleibt sie bei einem Punkttermin (Beginn = Ende) und bei einem
// eintägigen Ganztagestermin: „0 Minuten“ oder „1 Tag“ stünde bei fast jedem
// Termin da und wäre nur Rauschen. Bei einem Ganztagestermin zählt sie die Tage,
// sonst die Minuten – gerundet, weil ein Ganztag über eine Zeitumstellung hinweg
// 23 oder 25 Stunden hat.
function dauerText(event) {
    const sekunden = event.end - event.start;

    if (!Number.isFinite(sekunden) || sekunden <= 0) {
        return '';
    }

    if (event.all_day) {
        const tage = Math.round(sekunden / 86_400);
        return tage > 1 ? zahl(tage, 'Tag', 'Tage') : '';
    }

    // Unter einer Minute ist nichts zu sagen. Gerundet würde daraus „1 Min.“
    // werden, obwohl der Termin kürzer war – das wäre eine Angabe, die es nicht gibt.
    if (sekunden < 60) {
        return '';
    }

    const minuten = Math.round(sekunden / 60);
    const stunden = Math.floor(minuten / 60);
    const rest = minuten % 60;

    if (stunden === 0) {
        return zahl(minuten, 'Min.');
    }

    return rest === 0 ? zahl(stunden, 'Std.') : `${zahl(stunden, 'Std.')} ${zahl(rest, 'Min.')}`;
}

// „1 Std.“ statt „1 Std.n“.
function zahl(anzahl, einzahl, mehrzahl = einzahl) {
    return anzahl === 1 ? `1 ${einzahl}` : `${anzahl} ${mehrzahl}`;
}

function zeitraum(event) {
    if (event.all_day) {
        return 'Ganztägig';
    }

    // Ohne Zeitzone im Termin bleibt die Uhrzeit so stehen, wie sie im Kalender
    // steht; sie zu interpretieren wäre geraten. Der Minutenpuffer wie in
    // bauAgenda verhindert, dass 14:00:00 als 13:59 angezeigt wird.
    if (event.floating) {
        return new Date((event.start + 60) * 1000).toISOString().slice(11, 16);
    }

    return uhrzeitFormatter.format(new Date(event.start * 1000));
}

// Die Zeile der Leiste: Beginn, und wenn es etwas zu sagen gibt, die Dauer.
//
// Hier steht beides in einer Zeile, weil 260 px keinen Platz für eine weitere
// lassen – die Leiste ist mit Erinnerung und Kategorien schon gewachsen. In der
// Agenda ist es umgekehrt, dort hat jede Angabe ihre eigene Spalte.
function zeitUndDauer(event) {
    if (event.all_day) {
        const dauer = dauerText(event);
        return dauer ? `${dauer} ganztägig` : 'Ganztägig';
    }

    const dauer = dauerText(event);
    return dauer ? `${zeitraum(event)} · ${dauer}` : zeitraum(event);
}

function laeuftJetzt(event) {
    if (event.floating) {
        return false;
    }

    const jetzt = Math.floor(Date.now() / 1000);

    return event.start <= jetzt && event.end > jetzt;
}

function zeichneKalender() {
    calendarList.replaceChildren();

    if (calendar.loading) {
        calendarState.textContent = 'Termine werden geladen ...';
        calendarState.classList.remove('error');
        return;
    }

    if (calendar.error) {
        calendarState.textContent = calendar.error;
        calendarState.classList.add('error');
        return;
    }

    if (!calendar.status || !calendar.status.configured) {
        calendarState.textContent = 'Keine Instanz eingetragen. Mit /calendar anmelden.';
        calendarState.classList.remove('error');
        return;
    }

    if (!calendar.status.logged_in) {
        calendarState.textContent = 'Nicht angemeldet. Mit /calendar anmelden.';
        calendarState.classList.remove('error');
        return;
    }

    if (calendar.events.length === 0) {
        calendarState.textContent = 'Keine Termine in den nächsten Wochen.';
        calendarState.classList.remove('error');
        return;
    }

    calendarState.textContent = '';
    calendarState.classList.remove('error');

    let letzterTag = null;

    for (const event of calendar.events.slice(0, calendarMaxAnzeigen())) {
        const punkt = document.createElement('li');
        punkt.className = 'calendar-entry';
        punkt.classList.toggle('running', laeuftJetzt(event));

        // Siehe bauAgenda: Der Minutenpuffer verhindert das Abrunden in den
        // Vortag.
        const start = new Date((event.start + 60) * 1000);
        const tag = tagLabel(start);

        if (tag !== letzterTag) {
            const tagesZeile = document.createElement('div');
            tagesZeile.className = 'calendar-day';
            tagesZeile.textContent = tag;
            punkt.appendChild(tagesZeile);
            letzterTag = tag;
        }

        const zeit = document.createElement('div');
        zeit.className = 'calendar-time';
        zeit.textContent = zeitUndDauer(event);
        punkt.appendChild(zeit);

        const titel = document.createElement('div');
        titel.className = 'calendar-title';
        // Ohne Summary bleibt die Zeile nicht leer: Es steht "(ohne Titel)". Der
        // Kalendername steht ohnehin in der eigenen Zeile darunter.
        titel.textContent = event.summary || '(ohne Titel)';
        punkt.appendChild(titel);

        if (event.location) {
            const ort = document.createElement('div');
            ort.className = 'calendar-source';
            ort.textContent = event.location;
            punkt.appendChild(ort);
        }

    for (const merkmal of merkmaleDesTermins(event)) {
        const zeile = document.createElement('div');
        zeile.className = event.reminder_text === merkmal ? 'calendar-tag reminder' : 'calendar-tag';
        zeile.textContent = merkmal;
        punkt.appendChild(zeile);
    }

        const quelle = document.createElement('div');
        quelle.className = 'calendar-source';
        quelle.textContent = event.calendar;
        punkt.appendChild(quelle);

        // Der ganze Eintrag öffnet den Termin. `tabindex` und die Tastatur-
        // Ereignisse gehören dazu: Eine klickbare Zeile ohne Tastaturzugang wäre
        // für niemanden ohne Maus erreichbar.
        punkt.classList.add('calendar-entry-clickable');
        punkt.tabIndex = 0;
        punkt.setAttribute('role', 'button');
        punkt.title = 'Termin öffnen';
        punkt.addEventListener('click', () => {
            oeffneTerminFenster(event).catch((error) => console.error('Termin:', error));
        });
        punkt.addEventListener('keydown', (ereignis) => {
            if (ereignis.key !== 'Enter' && ereignis.key !== ' ') {
                return;
            }

            ereignis.preventDefault();
            oeffneTerminFenster(event).catch((error) => console.error('Termin:', error));
        });

        calendarList.appendChild(punkt);
    }

    const rest = calendar.events.length - calendarMaxAnzeigen();

    if (rest > 0) {
        const hinweis = document.createElement('li');
        hinweis.className = 'calendar-state';
        hinweis.textContent = `und ${rest} weitere`;
        calendarList.appendChild(hinweis);
    }
}

function calendarMaxAnzeigen() {
    return calendar.expanded ? calendar.events.length : CALENDAR_MAX_ENTRIES;
}

// --- Termin im Fenster ------------------------------------------------------
// Ein Klick auf einen Termin in der Leiste öffnet ihn. Zwei Schritte bis zum
// Speichern: Der erste zeigt den Unterschied, der zweite schreibt. Grund ist
// dieselbe wie beim Werkzeugaufruf – der Benutzer soll sehen, was sich ändert,
// und nicht nur ein Formular absegnen.
//
// Der Zustand des Fensters liegt hier und nicht im Fenster selbst, weil das
// Backend beim Speichern den Termin neu holt: Was hier steht, ist die Absicht des
// Benutzers, nicht der Stand im Kalender.

// Der Termin, den das Fenster gerade zeigt.
let eventState = null;

// Wechselt der Termin zwischen Ganztag und Uhrzeit, braucht `datetime-local`
// ein anderes Format. Das Umstellen ist deshalb Sache des Fensters.
function setAllDay(ganztag) {
    eventAllDay.checked = ganztag;

    for (const feld of [eventStart, eventEnd]) {
        feld.type = ganztag ? 'date' : 'datetime-local';
    }
}

async function oeffneTerminFenster(termin) {
    eventError.hidden = true;
    eventError.textContent = '';
    eventDiffWrap.hidden = true;
    eventSave.textContent = 'Speichern';
    eventSave.disabled = true;
    eventState = null;

    let details;

    try {
        details = await invoke('calendar_event_open', {
            uid: termin.uid,
            calendarHref: termin.calendar_href,
        });
    } catch (error) {
        eventError.textContent = String(error);
        eventError.hidden = false;
        eventDialog.showModal();
        return;
    }

    eventState = details;
    eventHeading.textContent = details.summary || 'Termin ohne Titel';
    eventScope.textContent = `Kalender ${details.calendar}`;

    eventSummary.value = details.summary;
    setAllDay(details.all_day);
    eventStart.value = details.start;
    eventEnd.value = details.end;
    eventReminder.value = details.reminder;
    eventLocation.value = details.location;
    eventCategories.value = details.categories;
    eventDescription.value = details.description;

    // Eine Uhrzeit ohne Zone im Kalender ist etwas anderes als eine in der Zone
    // des Rechners. Das wird gesagt, statt es stillschweigend umzudeuten – und
    // beim Speichern als Ortszeit geschrieben, weil das die ist, was der
    // Benutzer in das Feld getippt hat.
    eventFloatingHint.textContent = 'Dieser Termin steht ohne Zeitzone im Kalender. Die Uhrzeit wird so übernommen, wie sie hier steht.';
    eventFloatingHint.hidden = !details.floating;

    // Ein Serientermin und ein Termin mit Teilnehmern lassen sich nicht ändern.
    // Die Felder werden abgeschaltet und der Grund genannt, statt den Klick
    // ins Leere laufen zu lassen.
    const gesperrt = Boolean(details.gesperrt);
    eventBlocked.textContent = details.gesperrt;
    eventBlocked.hidden = !gesperrt;

    for (const feld of [eventSummary, eventStart, eventEnd, eventAllDay, eventReminder, eventLocation, eventCategories, eventDescription]) {
        feld.disabled = gesperrt;
    }

    eventSave.disabled = gesperrt;
    eventSave.textContent = 'Speichern';
    eventDialog.showModal();
    eventSummary.focus();
}

// Was das Formular als Absicht meldet.
//
// Der Termin wird vollständig geschickt, auch das Unveränderte. Das Backend
// vergleicht mit dem, was im Kalender steht, und schreibt nur die Unterschiede –
// so kann hier kein Feld fehlen.
function leseTerminAenderung() {
    return {
        uid: eventState.uid,
        calendar_href: eventState.calendar_href,
        summary: eventSummary.value,
        start: eventStart.value,
        end: eventEnd.value,
        all_day: eventAllDay.checked,
        location: eventLocation.value,
        description: eventDescription.value,
        categories: eventCategories.value,
        reminder: eventReminder.value,
    };
}

async function speichereTermin(nurAnsicht) {
    eventError.hidden = true;
    eventError.textContent = '';
    eventSave.disabled = true;

    try {
        const ergebnis = await invoke('calendar_event_save', {
            aenderung: leseTerminAenderung(),
            nurAnsicht: nurAnsicht,
        });

        eventDiffHead.textContent = nurAnsicht
            ? 'Das würde im Kalender geändert:'
            : ergebnis.zusammenfassung;
        eventDiffWrap.hidden = false;
        renderDiff(eventDiff, ergebnis.vorher, ergebnis.nachher);
        eventDiff.scrollIntoView({ block: 'nearest' });

        if (nurAnsicht) {
            // Der Unterschied steht da. Jetzt erst der zweite Klick schreibt.
            eventSave.textContent = 'Wirklich speichern';
            eventSave.disabled = false;
            eventBestätigt = true;
            return;
        }

        eventDialog.close('gespeichert');
        // Die Leiste sofort neu holen: Sonst stünde dort einen Moment lang der
        // alte Stand, und der Benutzer sähe seine Änderung nicht an.
        await aktualisiereKalender();
    } catch (error) {
        eventError.textContent = String(error);
        eventError.hidden = false;
        eventSave.textContent = 'Speichern';
        eventSave.disabled = false;
        eventBestätigt = false;
    }
}

// `true`, wenn der Benutzer den Unterschied zum aktuellen Stand schon gesehen
// hat. Dann schreibt der nächste Klick, statt wieder eine Vorschau zu holen.
let eventBestätigt = false;

eventForm.addEventListener('submit', (ereignis) => {
    ereignis.preventDefault();

    if (!eventState) {
        return;
    }

    speichereTermin(!eventBestätigt).catch((error) => {
        eventError.textContent = String(error);
        eventError.hidden = false;
        eventSave.disabled = false;
        eventBestätigt = false;
    });
});

// Eine Änderung an einem Feld macht die gezeigte Vorschau ungültig: Sonst
// schriebe der zweite Klick einen Unterschied, den der Benutzer gar nicht mehr
// sieht. Der erste Klick muss also wieder eine Vorschau holen.
for (const feld of [eventSummary, eventStart, eventEnd, eventAllDay, eventReminder, eventLocation, eventCategories, eventDescription]) {
    feld.addEventListener('input', () => {
        eventBestätigt = false;
        eventDiffWrap.hidden = true;
        eventSave.textContent = 'Speichern';
    });
}

// Ganztag und Uhrzeit brauchen verschiedene Felder. Ohne dieses Umschalten stünde
// in einem Datumsfeld eine Uhrzeit, die der Browser als ungültig verwirft – und
// der Termin ginge beim Speichern verloren.
eventAllDay.addEventListener('change', () => {
    const von = eventStart.value;
    setAllDay(eventAllDay.checked);

    // Die Uhrzeit abschneiden bzw. auf null Uhr setzen: `datetime-local` lehnt
    // einen Wert mit Uhrzeit ab, ein `date`-Feld einen mit.
    if (eventAllDay.checked) {
        eventStart.value = von.slice(0, 10);
        eventEnd.value = eventEnd.value.slice(0, 10);
    } else {
        if (eventStart.value.length === 10) eventStart.value = `${eventStart.value}T09:00`;
        if (eventEnd.value.length === 10) eventEnd.value = `${eventEnd.value}T10:00`;
    }
});

eventCancel.addEventListener('click', () => {
    eventDialog.close('abgebrochen');
});

// Beim Schließen ist der Zustand weg: Ein wieder geöffneter Termin darf nicht die
// Werte des letzten zeigen.
eventDialog.addEventListener('close', () => {
    eventState = null;
    eventBestätigt = false;
    eventDiffWrap.hidden = true;
    eventDiff.replaceChildren();
});

function zeichneKalenderFuß() {
    if (!calendar.status || !calendar.status.logged_in) {
        calendarFoot.textContent = '';
        return;
    }

    const stand = calendar.status.last_success
        ? new Date(calendar.status.last_success * 1000)
        : null;
    const teile = [];

    if (stand) {
        teile.push(`Stand ${uhrzeitFormatter.format(stand)}`);
    }

    if (calendar.status.version) {
        teile.push(`Nextcloud ${calendar.status.version}`);
    }

    if (calendar.status.remembered) {
        teile.push('Passwort gemerkt');
    }

    if (calendar.status.server_url) {
        teile.push(calendar.status.server_url.replace(/^https?:\/\//, ''));
    }

    calendarFoot.textContent = teile.join(' \u00b7 ');
}

function zeichneKalenderAlles() {
    zeichneKalender();
    zeichneKalenderFuß();
}

// Holt den Zustand ohne Netzabruf. Die Leiste soll immer etwas anzeigen, auch
// wenn der Server gerade nicht antwortet.
async function aktualisiereKalenderZustand() {
    const vorherAngemeldet = Boolean(calendar.status?.logged_in);

    try {
        calendar.status = await invoke('calendar_status');
    } catch (error) {
        calendar.status = null;
        calendar.error = `Zustand nicht lesbar: ${error}`;
    }

    // Mit der Anmeldung ändert sich auch das Werkzeugangebot: Ohne Anmeldung
    // gibt es create_calendar_event nicht. Der zwischengespeicherte Stand
    // würde das Modell sonst noch einen Zug lang ein Werkzeug anbieten lassen,
    // das es nicht benutzen kann.
    if (Boolean(calendar.status?.logged_in) !== vorherAngemeldet) {
        agent.toolset = null;
    }
}

// Holt die Termine. Fehler landen in der Leiste und nicht als Nachricht im
// Chat, sonst mischt sich ein Serverproblem in die Unterhaltung.
async function aktualisiereKalenderTermine() {
    if (!calendar.status || !calendar.status.logged_in) {
        // Nach dem Abmelden dürfen keine Termine mehr stehen bleiben, sonst zeigt
        // die Leiste weiterhin den Stand des letzten angemeldeten Abrufs.
        calendar.events = [];
        calendar.error = null;
        zeichneKalenderAlles();
        return;
    }

    calendar.loading = true;
    calendar.error = null;
    zeichneKalenderAlles();

    try {
        calendar.events = await invoke('calendar_events');
        await aktualisiereKalenderZustand();
    } catch (error) {
        // Der letzte Stand wird verworfen, nicht behalten: Ein liegen gebliebener
        // Termin wäre nicht als veraltet erkennbar und damit schlimmer als eine
        // leere Leiste mit Fehlermeldung. Der Chat läuft weiter.
        calendar.events = [];
        calendar.error = `Termine nicht abrufbar: ${error}`;
    }

    calendar.loading = false;
    zeichneKalenderAlles();
}

async function aktualisiereKalender() {
    await aktualisiereKalenderZustand();
    await aktualisiereKalenderTermine();
}

function stoppeKalenderTimer() {
    if (calendarTimer === null) {
        return;
    }

    clearInterval(calendarTimer);
    calendarTimer = null;
}

function starteKalenderTimer() {
    if (calendarTimer !== null) {
        return;
    }

    // Nur abholen, wenn das Fenster sichtbar ist. Im Hintergrund ginge der
    // ohnehin langsame Takt verloren, ohne dass es jemanden kümmert.
    calendarTimer = setInterval(() => {
        if (document.visibilityState === 'visible') {
            aktualisiereKalenderTermine().catch((error) => console.error('Kalender:', error));
        }
    }, CALENDAR_REFRESH_MS);
}

// Fragt das Zertifikat der Instanz ab und führt es im Fenster vor. Erst ein
// ausdrückliches Bestätigen macht es zum Vergleichsmaßstab; ohne diesen Schritt
// bleibt der Zugang bei https gesperrt.
async function frageZertifikat(addresse = null) {
    const status = await invoke('calendar_certificate', { serverUrl: addresse });

    if (status.state === 'ohne') {
        return 'ohne';
    }

    if (status.state === 'bestätigt') {
        return 'bestätigt';
    }

    const bestaetigt = await new Promise((resolve) => {
        certificateHost.textContent = status.host;
        certificateFingerprint.textContent = status.fingerprint || 'unbekannt';
        certificateHint.textContent = status.detail || '';
        certificateHint.hidden = !status.detail;
        certificateTrust.textContent = status.state === 'geändert' ? 'Neues Zertifikat bestätigen' : 'Zertifikat bestätigen';
        certificateDialog.returnValue = '';

        const handleTrust = async () => {
            certificateTrust.disabled = true;

            try {
                await invoke('calendar_trust_certificate', { serverUrl: addresse });
                certificateDialog.close('bestätigt');
            } catch (error) {
                certificateHint.textContent = error;
                certificateHint.hidden = false;
            } finally {
                certificateTrust.disabled = false;
            }
        };
        const handleCancel = () => certificateDialog.close('abgebrochen');
        const handleClose = () => {
            certificateTrust.removeEventListener('click', handleTrust);
            certificateCancel.removeEventListener('click', handleCancel);
            resolve(certificateDialog.returnValue === 'bestätigt');
        };

        certificateTrust.addEventListener('click', handleTrust);
        certificateCancel.addEventListener('click', handleCancel);
        certificateDialog.addEventListener('close', handleClose, { once: true });
        certificateDialog.showModal();
    });

    return bestaetigt ? 'bestätigt' : 'abgebrochen';
}

// Blendet die Leiste ein oder aus.
//
// Eingeklappt pausiert nur die automatische Aktualisierung: Der Server im LAN
// soll nicht umsonst angesprochen werden, solange niemand die Termine sieht.
// Ein ausdrücklicher Befehl wie `/termine` holt weiterhin Termine, denn er
// wird ja gerade angefragt.
function toggleCalendarPanel() {
    const eingeklappt = calendarPanel.classList.toggle('collapsed');
    calendarToggleBtn.setAttribute('aria-pressed', String(!eingeklappt));
    calendarToggleBtn.textContent = eingeklappt ? 'Kalender: aus' : 'Kalender: an';
    calendarToggleBtn.title = eingeklappt
        ? 'Kalenderleiste einblenden'
        : 'Kalenderleiste ausblenden';

    if (eingeklappt) {
        stoppeKalenderTimer();
    } else {
        starteKalenderTimer();
        aktualisiereKalenderTermine().catch((error) => console.error('Kalender:', error));
    }
}

// Prüft das Zertifikat vor dem Anmelden und holt es gegebenenfalls ein.
async function bereiteZertifikatVor() {
    try {
        return await frageZertifikat();
    } catch (error) {
        calendar.error = `Zertifikat nicht prüfbar: ${error}`;
        zeichneKalenderAlles();
        return 'abgebrochen';
    }
}

// Zeigt die Kalender der Instanz zum Abwählen. Ohne Auswahl kommen alle
// lesbaren Kalender in die Leiste, was bei vielen Kalendern schnell zu viel wird.
async function toggleCalendarPicker() {
    if (!calendarPicker.hidden) {
        calendarPicker.hidden = true;
        return;
    }

    calendarPicker.hidden = false;
    calendarPickerList.replaceChildren();
    const verfuegbar = calendar.status?.known_calendars ?? [];

    if (verfuegbar.length === 0) {
        const hinweis = document.createElement('p');
        hinweis.className = 'calendar-state';
        hinweis.textContent = 'Keine Kalender bekannt. Erst anmelden.';
        calendarPickerList.appendChild(hinweis);
        return;
    }

    // Ohne ausdrückliche Auswahl ist alles drin.
    const gewaehlt = new Set(calendar.status.calendars);

    for (const entry of verfuegbar) {
        const zeile = document.createElement('label');
        zeile.className = 'calendar-picker-entry';

        const kasten = document.createElement('input');
        kasten.type = 'checkbox';
        kasten.value = entry.href;
        kasten.checked = gewaehlt.size === 0 || gewaehlt.has(entry.href);

        const text = document.createElement('span');
        text.textContent = entry.display_name;

        if (entry.color) {
            const punkt = document.createElement('span');
            punkt.className = 'calendar-color';
            punkt.style.background = entry.color;
            zeile.appendChild(punkt);
        }

        zeile.appendChild(kasten);
        zeile.appendChild(text);
        calendarPickerList.appendChild(zeile);
    }
}

async function speichereKalenderAuswahl() {
    const gewaehlt = [...calendarPickerList.querySelectorAll('input:checked')].map((kasten) => kasten.value);

    try {
        await invoke('set_calendar_config', { calendars: gewaehlt });
        await aktualisiereKalenderZustand();
        await aktualisiereKalenderTermine();
        calendarPicker.hidden = true;
        appendMessageToUI(
            'system',
            gewaehlt.length === 0
                ? 'Alle lesbaren Kalender werden in der Leiste gezeigt.'
                : `Die Leiste zeigt: ${gewaehlt.join(', ')}.`,
        );
    } catch (error) {
        appendMessageToUI('system', `Auswahl nicht gespeichert: ${error}`);
    }
}

function toggleCalendarExpanded() {
    calendar.expanded = !calendar.expanded;
    calendarToggleList.textContent = calendar.expanded ? 'Weniger' : 'Alle';
    zeichneKalender();
}

async function oeffneKalenderDialog() {
    // Was schon eingetragen ist, steht schon drin: die Adresse muss nicht bei
    // jeder Sitzung neu getippt werden.
    const config = await invoke('get_calendar_config');

    return new Promise((resolve) => {
        calendarPasswordInput.value = '';
        // Bewusst immer leer: ein unbedacht angeklicktes Kästchen darf kein
        // altes Passwort erneut speichern oder überschreiben.
        calendarRememberInput.checked = false;
        calendarDialogError.hidden = true;
        calendarDialogError.textContent = '';
        calendarDialog.returnValue = '';
        calendarUrlInput.value = config.server_url;
        calendarUserInput.value = config.username;

        // Ein Versuch, danach noch einer nach der Zertifikatsbestätigung. Mehr
        // nicht: Zwei Fehlversuche mit falschen Daten sollen nicht wiederholt
        // werden, sonst wird der Server gesperrt. Der Parameter unterscheidet nur
        // die beiden Aufrufe beim Lesen und entscheidet nichts, weil die
        // Zertifikatsprüfung ohnehin vor jedem Versuch läuft; er bleibt, damit
        // die Begründung am Aufruf stehen bleibt.
        const versuche = async (zertifikatGeprueft) => {
            await invoke('calendar_login', {
                serverUrl: calendarUrlInput.value,
                username: calendarUserInput.value,
                appPassword: calendarPasswordInput.value,
                remember: calendarRememberInput.checked,
            });
            calendarPasswordInput.value = '';
            calendarDialog.close('ok');
            return true;
        };

        // Die aus den Nextcloud-Einstellungen kopierte WebDAV-Adresse zeigt auf
        // den eigenen Principal und trägt den Benutzernamen im Pfad. Steht im
        // Feld nichts, wird er von dort übernommen und sichtbar gemacht, statt
        // ihn zu verlangen.
        const nameAusAdresse = (adresse) => {
            const teil = adresse.split('/remote.php/dav/principals/users/')[1];

            if (!teil) {
                return '';
            }

            const name = teil.split(/[/?#]/)[0];

            try {
                return name ? decodeURIComponent(name) : '';
            } catch {
                return name;
            }
        };

        const handleSubmit = async (event) => {
            event.preventDefault();
            calendarLoginBtn.disabled = true;
            calendarDialogError.hidden = true;

            if (!calendarUserInput.value.trim()) {
                calendarUserInput.value = nameAusAdresse(calendarUrlInput.value.trim());
            }

            // Bei https kommt das Zertifikat vor der Anmeldung: Ohne
            // bestätigtes Zertifikat sendet das Backend das Passwort gar nicht
            // erst, und der Fingerabdruck soll nicht erst nach einer
            // Fehlermeldung erscheinen.
            if (calendarUrlInput.value.trim().startsWith('https://')) {
                try {
                    const vorher = await frageZertifikat(calendarUrlInput.value);

                    if (vorher === 'abgebrochen') {
                        calendarDialogError.hidden = false;
                        calendarDialogError.textContent =
                            'Anmeldung abgebrochen, solange das Zertifikat nicht bestätigt ist.';
                        return;
                    }
                } catch (fehler) {
                    calendarDialogError.hidden = false;
                    calendarDialogError.textContent = `Zertifikat nicht prüfbar: ${fehler}`;
                    return;
                }
            }

            try {
                await versuche(false);
            } catch (error) {
                // Das kann nur noch ein geändertes Zertifikat sein, weil es
                // vorher geprüft wurde. Einmal nachfassen, mehr nicht.
                if (!String(error).includes('CERTIFICATE:')) {
                    calendarDialogError.hidden = false;
                    calendarDialogError.textContent = error;
                    return;
                }

                const zertifikat = await frageZertifikat(calendarUrlInput.value);

                if (zertifikat !== 'bestätigt') {
                    calendarDialogError.hidden = false;
                    calendarDialogError.textContent =
                        'Anmeldung abgebrochen, solange das Zertifikat nicht bestätigt ist.';
                    return;
                }

                try {
                    await versuche(true);
                } catch (wiederholung) {
                    calendarDialogError.hidden = false;
                    calendarDialogError.textContent = wiederholung;
                }
            } finally {
                calendarLoginBtn.disabled = false;
            }
        };
        const handleCancel = () => calendarDialog.close('abgebrochen');
        const handleClose = () => {
            calendarForm.removeEventListener('submit', handleSubmit);
            calendarCancel.removeEventListener('click', handleCancel);
            resolve(calendarDialog.returnValue === 'ok');
        };

        calendarForm.addEventListener('submit', handleSubmit);
        calendarCancel.addEventListener('click', handleCancel);
        calendarDialog.addEventListener('close', handleClose, { once: true });
        calendarDialog.showModal();
        calendarUrlInput.focus();
    });
}

// Angehängte Dateien werden zu eigenen Nachrichten vor der Eingabe, damit der
// eigene Text unangetastet bleibt.
function attachmentMessages() {
    return pendingAttachments.map((entry) => ({
        role: 'user',
        content: entry.message,
    }));
}

async function readAttachments(files) {
    const accepted = [];
    const gekuerzt = [];

    for (const file of files) {
        // Das Backend entscheidet über die Länge, nicht diese Oberfläche: Nur
        // dort gilt die Grenze, die ein Anhang wirklich passieren muss. Vorher
        // prüfte das Frontend eine eigene, größere Grenze, worauf eine
        // mittelgroße Datei als angehängt quittiert wurde und die Anfrage
        // anschließend scheiterte. Der Preis dafür: Eine riesige Datei wird
        // hier vollständig gelesen und erst danach abgelehnt.
        const text = await file.text();

        // Ein Nullbyte ist das einfachste Kennzeichen einer Binärdatei; als Text
        // geschickt wäre sie für das Modell sinnlos.
        if (text.includes('\0')) {
            appendMessageToUI('system', `${file.name} ist keine Textdatei und wurde nicht angehängt.`);
            continue;
        }

        let prepared;

        try {
            prepared = await invoke('prepare_attachment', { name: file.name, content: text });
        } catch (error) {
            appendMessageToUI('system', String(error));
            continue;
        }

        accepted.push({ name: file.name, message: prepared.message, original_bytes: prepared.original_bytes });
        if (prepared.truncated) {
            gekuerzt.push(file.name);
        }
    }

    if (accepted.length > 0) {
        pendingAttachments = [...pendingAttachments, ...accepted];
        appendMessageToUI('system', `Angehängt: ${accepted.map((entry) => entry.name).join(', ')}`);

        // Eine gekürzte Datei muss man sehen können, sonst fragt man das Modell
        // nach einem Absatz, den es gar nicht bekommen hat.
        for (const name of gekuerzt) {
            const groesse = Math.round(accepted.find((a) => a.name === name).original_bytes / 1024);
            appendMessageToUI(
                'system',
                `${name} war ${groesse} KiB groß. Für eine Nachricht wurde der Text gekürzt – `
                + 'das Modell sieht nur den Anfang. Für ein ganzes Dokument: Datei ins '
                + 'Arbeitsverzeichnis legen und den Agentenmodus nutzen.',
            );
        }

        updateContextUsage();
    }
}

// ------------------------------------------------------------------ Agentenmodus
// Der Modus nutzt solange ausschließlich lesende Werkzeuge, wie der Schreibmodus
// gesperrt ist; mit /agent-write kommen write_file, edit_file und die drei
// Kalenderwerkzeuge dazu. Das Backend führt nichts aus, was nicht in seiner
// Allowlist steht; hier wird zusätzlich jeder einzelne Aufruf bestätigt, bevor er
// abgeschickt wird.

function setAgentMode(enabled) {
    agent.enabled = enabled;
    const termine = agent.scope === 'termine';
    agentToggleBtn.setAttribute('aria-pressed', termine || enabled ? 'true' : 'false');
    agentToggleBtn.textContent = termine ? 'Termine: an' : (enabled ? 'Agent: an' : 'Agent: aus');
    agentToggleBtn.title = termine
        ? 'Terminumfang: nur Kalenderwerkzeuge, kein Dateizugriff'
        : (enabled
            ? `Agentenmodus: nur lesende Werkzeuge in ${agent.root}`
            : 'Agentenmodus mit lesenden Werkzeugen einschalten');
    // Der Schalter ist im Terminumfang gegenstandslos: Der Umfang selbst gibt
    // die Kalenderwerkzeuge frei, und ein Schalter, der hier nichts änderte,
    // wäre eine Anzeige ohne Wirkung.
    agentToggleBtn.disabled = termine;
}

async function refreshAgentConfig() {
    const config = await invoke('get_agent_config');
    agent.root = config.root || '';
    agent.maxSteps = Number(config.max_steps) || 0;
    // Das Backend entscheidet, welche Werte es kennt; hier wird nichts geraten.
    // Ein unbekannter Wert fällt auf den Agentenmodus zurück, weil der die
    // Werkzeuge nur nach ausdrücklicher Freischaltung anbietet.
    agent.scope = SCOPE_NAMES.includes(config.scope) ? config.scope : 'agent';
    setAgentMode(agent.enabled);
    return config;
}

// Schemata und Systemprompt kommen aus dem Backend, damit Anweisung und
// Werkzeugimplementierung nicht auseinanderlaufen können. Nach einem Wechsel des
// Arbeitsverzeichnisses oder des Umfangs wird der Cache bewusst verworfen, weil
// der Prompt den Pfad nennt und das Angebot vom Umfang abhängt.
async function loadAgentToolset() {
    if (agent.toolset) {
        return agent.toolset;
    }

    agent.toolset = await invoke('list_tools');
    return agent.toolset;
}

function formatToolArguments(argumentsValue) {
    if (argumentsValue === null || typeof argumentsValue !== 'object') {
        return String(argumentsValue ?? '');
    }

    const entries = Object.entries(argumentsValue);

    if (entries.length === 1) {
        // Bei genau einem Argument reicht "name: wert"; ab zwei kommt das JSON
        // hübsch formatiert, sonst wäre eine Zeile unlesbar.
        const [name, value] = entries[0];
        return `${name}: ${typeof value === 'string' ? value : JSON.stringify(value)}`;
    }

    return JSON.stringify(argumentsValue, null, 2);
}

function toolCallTarget(call) {
    const args = call?.function?.arguments ?? {};

    if (CALENDAR_TOOL_NAMES.includes(call?.function?.name)) {
        // Beim Anlegen und Ändern steht der neue Titel in summary, beim Löschen
        // der jetzige in title – dort gäbe es kein summary und die Leiste zeigte
        // nur „Termin“ ohne sagen, worum es geht.
        const titel = args.summary ?? args.title;

        if (typeof titel === 'string' && titel.trim()) {
            const wann = typeof args.on_date === 'string' && args.on_date.trim() ? args.on_date.trim() : '';
            return wann ? `${titel.trim()}, ${wann}` : titel.trim();
        }

        return 'Termin';
    }

    const path = args.path;
    return typeof path === 'string' && path ? path : '';
}

function requestToolApproval(call) {
    return new Promise((resolve) => {
        let approved = false;
        const name = call?.function?.name ?? 'unbekannt';
        const target = toolCallTarget(call);

        toolName.textContent = target ? `${name} (${target})` : name;
        toolScope.textContent = `Arbeitsverzeichnis: ${agent.root}`;
        toolArguments.textContent = formatToolArguments(call?.function?.arguments);
        toolDialog.returnValue = '';

        const handleSubmit = (event) => {
            event.preventDefault();
            approved = true;
            toolDialog.close('confirmed');
        };
        const handleClose = () => {
            toolForm.removeEventListener('submit', handleSubmit);
            toolCancel.removeEventListener('click', handleCancel);
            resolve(approved && toolDialog.returnValue === 'confirmed');
        };
        const handleCancel = () => toolDialog.close('cancelled');

        toolForm.addEventListener('submit', handleSubmit);
        toolCancel.addEventListener('click', handleCancel);
        toolDialog.addEventListener('close', handleClose, { once: true });
        toolDialog.showModal();
    });
}

// Zeilenweiser Vergleich. Gemeinsames Ende und Anfang werden abgeschnitten, der
// Rest über eine klassische längste gemeinsame Teilfolge verglichen - für die
// Fallhöhe, die ein Schreibvorgang haben kann, ist das schnell genug.
function buildDiff(before, after) {
    // Ein abschließender Zeilenumbruch ist implizit vorhanden. Ohne dieses
    // Abstreichen erschiene am Dateiende jedes Mal eine zusätzliche leere Zeile.
    const normalize = (text) => (text.endsWith('\n') ? text.slice(0, -1) : text);
    const oldLines = before === null ? [] : normalize(before).split('\n');
    const newLines = normalize(after).split('\n');

    let start = 0;

    while (start < oldLines.length && start < newLines.length && oldLines[start] === newLines[start]) {
        start += 1;
    }

    let end = 0;

    while (
        end < oldLines.length - start
        && end < newLines.length - start
        && oldLines[oldLines.length - 1 - end] === newLines[newLines.length - 1 - end]
    ) {
        end += 1;
    }

    const middleOld = oldLines.slice(start, oldLines.length - end);
    const middleNew = newLines.slice(start, newLines.length - end);
    // Kleinste-Gemeinsame-Teilfolge über die Zeilen: der übliche Zeilenvergleich,
    // der gegenüber einem Zeichenvergleich falsche Verschiebungen vermeidet.
    const table = Array.from({ length: middleNew.length + 1 }, () => new Uint32Array(middleOld.length + 1));

    for (let i = 1; i <= middleNew.length; i += 1) {
        for (let j = 1; j <= middleOld.length; j += 1) {
            table[i][j] = middleNew[i - 1] === middleOld[j - 1]
                ? table[i - 1][j - 1] + 1
                : Math.max(table[i - 1][j], table[i][j - 1]);
        }
    }

    const rows = [];
    let i = middleNew.length;
    let j = middleOld.length;

    while (i > 0 || j > 0) {
        if (i > 0 && j > 0 && middleNew[i - 1] === middleOld[j - 1]) {
            rows.unshift({ kind: 'equal', text: middleNew[i - 1] });
            i -= 1;
            j -= 1;
        } else if (j > 0 && (i === 0 || table[i][j - 1] >= table[i - 1][j])) {
            rows.unshift({ kind: 'remove', text: middleOld[j - 1] });
            j -= 1;
        } else {
            rows.unshift({ kind: 'add', text: middleNew[i - 1] });
            i -= 1;
        }
    }

    // Gemeinsamer Anfang und Ende gehören in die Ausgabe, sonst verliert man
    // genau den Kontext, der die Änderung verständlich macht.
    const context = [];

    for (let index = 0; index < start; index += 1) {
        context.push({ kind: 'equal', text: oldLines[index] });
    }

    context.push(...rows);

    for (let index = oldLines.length - end; index < oldLines.length; index += 1) {
        context.push({ kind: 'equal', text: oldLines[index] });
    }

    return context;
}

const DIFF_CONTEXT_LINES = 3;

function renderDiff(container, before, after) {
    container.replaceChildren();
    const rows = buildDiff(before, after);
    const interesting = rows
        .map((row, index) => (row.kind === 'equal' ? -1 : index))
        .filter((index) => index >= 0);

    if (interesting.length === 0) {
        const note = document.createElement('div');
        note.className = 'diff-note';
        note.textContent = 'Kein Unterschied im Inhalt.';
        container.appendChild(note);
        return { added: 0, removed: 0 };
    }

    const first = Math.max(0, interesting[0] - DIFF_CONTEXT_LINES);
    const last = Math.min(rows.length - 1, interesting[interesting.length - 1] + DIFF_CONTEXT_LINES);
    let added = 0;
    let removed = 0;
    let skipped = 0;

    rows.forEach((row, index) => {
        if (row.kind === 'equal' && (index < first || index > last)) {
            skipped += 1;
            return;
        }

        if (skipped > 0) {
            const note = document.createElement('div');
            note.className = 'diff-note';
            note.textContent = `… ${skipped} unveränderte Zeilen …`;
            container.appendChild(note);
            skipped = 0;
        }

        const line = document.createElement('div');
        line.className = `diff-line diff-${row.kind}`;
        line.textContent = `${row.kind === 'add' ? '+' : row.kind === 'remove' ? '−' : ' '} ${row.text}`;
        container.appendChild(line);

        if (row.kind === 'add') added += 1;
        if (row.kind === 'remove') removed += 1;
    });

    if (skipped > 0) {
        const note = document.createElement('div');
        note.className = 'diff-note';
        note.textContent = `… ${skipped} unveränderte Zeilen …`;
        container.appendChild(note);
    }

    return { added, removed };
}

// Schreibvorgänge werden nie mit den Argumenten freigegeben, sondern mit dem
// daraus folgenden Unterschied. Die Vorschau entsteht im Backend aus derselben
// Funktion wie das Schreiben selbst, sie kann also nicht von der wirklichen
// Änderung abweichen.
// Die Worte des Benutzers gehören zum Aufruf, weil Mimir daran prüft, was das
// Modell erfunden hat: Ein Ort oder ein Kalender, den der Benutzer nicht genannt
// hat, kommt nicht in den Termin. Ohne diese Angabe kann die Prüfung nichts
// entscheiden und ließe jedes Feld stehen.
//
// Gesucht ist die letzte Nachricht des Benutzers aus dieser Unterhaltung – nicht
// die ganze Unterhaltung. Sonst würde ein Ort, den der Benutzer vor drei Fragen
// genannt hat, heute als genannt gelten.
function letzteBenutzernachricht(working) {
    for (let i = working.length - 1; i >= 0; i -= 1) {
        if (working[i].role === 'user') {
            return working[i].content || '';
        }
    }

    return '';
}

async function requestWriteApproval(call, working) {
    const name = call?.function?.name ?? 'unbekannt';

    let preview;

    try {
        preview = await invoke('preview_tool_call', {
            name,
            arguments: call.function.arguments,
            benutzertext: letzteBenutzernachricht(working),
        });
    } catch (error) {
        return { approved: false, previewError: `Der Schreibvorgang ist nicht durchführbar: ${error}` };
    }

    const counts = renderDiff(writeDiff, preview.current, preview.next);
    const ist_termin = CALENDAR_TOOL_NAMES.includes(name);
    const ist_loeschung = name === DELETE_EVENT_TOOL;
    writeTarget.textContent = `${name}: ${preview.relative_path}`;

    if (ist_loeschung) {
        // Beim Löschen gibt es keinen neuen Inhalt; das Fenster zeigt deshalb
        // genau das, was verschwindet.
        writeScope.textContent = 'Der Termin wird endgültig gelöscht. In Nextcloud lässt er sich nur von Hand wiederherstellen.';
        writeSummary.textContent = `${preview.summary} – ${counts.removed} Zeilen werden entfernt`;
    } else if (ist_termin) {
        // Ein Termin hat kein Arbeitsverzeichnis; dort zu stehen wäre irreführend.
        writeScope.textContent =
            'Wird im Kalender angelegt und lässt sich über Mimir nicht zurücknehmen.';
        writeSummary.textContent = preview.current === null
            ? `${preview.summary} – neuer Termin, ${counts.added} Zeilen`
            : `${preview.summary} – ${counts.added} Zeilen geändert, ${counts.removed} entfernt`;
    } else {
        writeScope.textContent = `Arbeitsverzeichnis: ${agent.root}`;
        writeSummary.textContent = preview.current === null
            ? `${preview.summary} – neue Datei, ${counts.added} Zeilen`
            : `${preview.summary} – ${counts.added} Zeilen hinzugefügt, ${counts.removed} entfernt`;
    }

    return new Promise((resolve) => {
        let approved = false;
        writeDialog.returnValue = '';

        const handleSubmit = (event) => {
            event.preventDefault();
            approved = true;
            writeDialog.close('confirmed');
        };
        const handleClose = () => {
            writeForm.removeEventListener('submit', handleSubmit);
            writeCancel.removeEventListener('click', handleCancel);
            resolve({ approved: approved && writeDialog.returnValue === 'confirmed', preview });
        };
        const handleCancel = () => writeDialog.close('cancelled');

        writeForm.addEventListener('submit', handleSubmit);
        writeCancel.addEventListener('click', handleCancel);
        writeDialog.addEventListener('close', handleClose, { once: true });
        writeDialog.showModal();
    });
}

// Ein Schreibaufruf: erst die Vorschau im Dialog, dann die Ausführung. Die
// Vorschau entsteht im Backend aus derselben Funktion wie das Schreiben, zeigt
// also genau die Wirkung, die freigegeben wird.
async function runWriteCall(call, steps, working, label) {
    const name = call?.function?.name ?? 'unbekannt';
    const path = call?.function?.arguments?.path ?? '';
    const isEvent = CALENDAR_TOOL_NAMES.includes(name);
    const decision = await requestWriteApproval(call, working);

    if (decision.previewError) {
        const entry = addAgentStep(steps, call, decision.previewError);
        entry.step.classList.add('agent-step-failed');
        working.push({ role: 'tool', tool_name: name, content: decision.previewError });
        return;
    }

    const entry = addAgentStep(steps, call, 'wird geschrieben ...');
    entry.step.classList.add('agent-step-write');

    if (!decision.approved) {
        setAgentStepResult(entry, 'Vom Benutzer abgelehnt. Es wurde nichts verändert.');
        entry.step.classList.remove('agent-step-write');
        entry.step.classList.add('agent-step-rejected');
        working.push({
            role: 'tool',
            tool_name: name,
            content: 'Der Benutzer hat diesen Schreibvorgang abgelehnt. Es wurde nichts verändert. Arbeite mit dem, was du hast, oder erkläre, was du bräuchtest.',
        });
        return;
    }

    agent.writeCount += 1;
    updateAgentStepsHead();

    try {
        const output = await invoke('execute_tool', {
            name,
            arguments: call.function.arguments,
            benutzertext: letzteBenutzernachricht(working),
        });
        setAgentStepResult(entry, `${output.summary}${output.truncated ? ' (gekürzt)' : ''}`);

        if (isEvent) {
            // Die Leiste zeigt den neuen Termin gleich an, statt auf den
            // nächsten Abruf zu warten.
            aktualisiereKalender().catch((error) => console.error('Kalender:', error));
        } else {
            addUndoAction(entry, path, label);
        }

        working.push({ role: 'tool', tool_name: name, content: output.content });
    } catch (error) {
        setAgentStepResult(entry, `Fehler: ${error}`);
        entry.step.classList.add('agent-step-failed');
        working.push({ role: 'tool', tool_name: name, content: `Fehler: ${error}` });
    }
}

// Rückgängig-Knopf an einem abgeschlossenen Werkzeugschritt.
function addUndoAction(entry, path, label) {
    if (!path) {
        return;
    }

    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'step-undo';
    button.textContent = 'Rückgängig';
    button.addEventListener('click', async () => {
        button.disabled = true;

        try {
            const output = await invoke('undo_write', { path });
            setAgentStepResult(entry, `${output.summary}\n\n${output.content}`);
            entry.step.classList.add('agent-step-undone');
        } catch (error) {
            button.disabled = false;
            setAgentStepResult(entry, `Rückgängig nicht möglich: ${error}`);
        }
    });

    entry.step.appendChild(button);
}

function updateAgentStepsHead() {
    const head = chatContainer.querySelector('.agent-steps-head');

    if (!head) {
        return;
    }

    const base = 'Nur lesende Werkzeuge';
    // Beide Grenzen kommen aus dem Backend, damit Anzeige und Durchsetzung nicht
    // auseinanderlaufen: die Anzahl der Schreibvorgänge und das Byte-Budget je Zug.
    const kib = (bytes) => `${Math.round(bytes / 1024)} KiB`;
    head.textContent = agent.writeEnabled
        ? `${base}, Schreiben freigegeben (${agent.writeCount}/${agent.maxWrites || '?'} in diesem Zug, ${kib(agent.maxWriteBytes)} Budget) in ${agent.root}`
        : `${base} in ${agent.root}`;
}

function createAgentSteps(bubble) {
    // Ein erneuter Versuch nutzt dieselbe Blase. Die Schritte des gescheiterten
    // Versuchs werden entfernt, sonst stünden am Ende zwei Kästen mit
    // unterschiedlichem Inhalt übereinander.
    for (const stale of [...bubble.querySelectorAll('.agent-steps')]) {
        stale.remove();
    }

    const steps = document.createElement('div');
    steps.className = 'agent-steps';
    const head = document.createElement('div');
    head.className = 'agent-steps-head';
    steps.appendChild(head);
    bubble.prepend(steps);
    updateAgentStepsHead();
    return steps;
}

function addAgentStep(steps, call, status) {
    const step = document.createElement('div');
    step.className = 'agent-step';
    const name = call?.function?.name ?? 'unbekannt';
    const target = toolCallTarget(call);
    const label = target ? `${name} (${target})` : name;

    const output = document.createElement('pre');
    output.className = 'step-output';
    output.hidden = true;
    output.textContent = status;

    const toggle = document.createElement('button');
    toggle.type = 'button';
    toggle.className = 'step-toggle';
    toggle.setAttribute('aria-expanded', 'false');
    toggle.textContent = `▸ ${label}`;
    toggle.addEventListener('click', () => {
        const expanded = toggle.getAttribute('aria-expanded') === 'true';
        toggle.setAttribute('aria-expanded', expanded ? 'false' : 'true');
        toggle.textContent = `${expanded ? '▸' : '▾'} ${label}`;
        output.hidden = expanded;
    });

    step.append(toggle, output);
    steps.appendChild(step);
    scrollToBottom();
    return { step, toggle, output };
}

function setAgentStepResult(entry, text) {
    entry.output.textContent = text;
    entry.step.classList.add('agent-step-done');
}

// Werkzeugschritte machen den Verlauf schnell länger als die Obergrenze der
// Backend-Prüfung. Die Systemanweisung bleibt, der älteste Kontext fällt weg.
function trimAgentMessages(working) {
    if (working.length <= AGENT_MESSAGE_BUDGET) {
        return working;
    }

    return working.slice(-AGENT_MESSAGE_BUDGET);
}

// Für den Chatverlauf bleiben nur eigene und Antworttexte. Werkzeugaufrufe und
// ihre Ergebnisse gehören zu einem Agentenzug: Ohne den auslösenden
// Assistenten-eintrag sind sie in einem späteren Chat sinnlos.
function toChatHistory(working) {
    return working
        .filter((entry) => entry.role === 'user' || entry.role === 'assistant')
        .map((entry) => ({ role: entry.role, content: entry.content }));
}

// Namen im Chat: Der umlaufende Zug heißt „Terminlauf", wenn nur der Kalender
// beteiligt ist. Eine Fehlermeldung, die vom Agentenmodus spricht, wäre in einem
// Zug richtig falsch, in dem es gar kein Arbeitsverzeichnis gibt.
async function runAgentTurn(model, messages, bubble) {
    setGenerating(true);
    cancelRequested = false;
    agent.writeCount = 0;

    try {
        await refreshAgentConfig();

        const termine = agent.scope === 'termine';

        // Das Arbeitsverzeichnis wird nur dort gebraucht, wo gelesen wird.
        if (!termine && !agent.root) {
            await renderResult(bubble, 'Für den Agentenmodus fehlt das Arbeitsverzeichnis. Setze es mit /agent-dir <pfad>.');
            return;
        }

        // Jeder Zug bekommt das Schreibbudget des Backends zurück; die Grenzen
        // holen wir von dort, damit Anzeige und Durchsetzung nicht auseinanderlaufen.
        await invoke('reset_write_budget');
        const writeState = await invoke('get_write_state');
        agent.maxWrites = Number(writeState.max_writes) || 0;
        agent.maxWriteBytes = Number(writeState.max_bytes) || 0;

        const toolset = await loadAgentToolset();
        const steps = createAgentSteps(bubble);
        // Die Werkzeuganweisung und eine eigene Systemanweisung gehen gemeinsam
        // in das Systemfeld; im Verlauf steht keine Systemnachricht.
        //
        // Das Systemfeld entsteht bewusst **nicht** hier, sondern erst beim
        // Senden unten: Es enthält das aktuelle Datum. Ein Agentenlauf kann über
        // eine Mitternacht dauern, und ein einmal gebautes Datum wäre für alle
        // folgenden Schritte dann der Vortag – genau der Fehler, den wir
        // vermeiden wollen.
        const working = [...messages];
        let lastText = '';

        for (let step = 0; step < agent.maxSteps; step += 1) {
            if (cancelRequested) {
                appendMessageToUI('system', `${zugname(termine)} abgebrochen.`);
                return;
            }

            const result = await runChatAttempt(model, trimAgentMessages(working), bubble, {
                tools: toolset.tools,
                system: systemPromptForRequest(toolset.system_prompt),
            });

            if (!result.ok) {
                const separator = result.text ? '\n\n' : '';
                await renderResult(bubble, `${result.text}${separator}[Fehler: ${result.error}]`, result.thinking);
                // Anders als im normalen Chat bleibt hier das Modell, das beim
                // Senden gewählt war: Der Agentenlauf hat bereits Werkzeugschritte
                // ausgeführt, ein Wechsel des Modells mitten darin ergäbe keinen
                // sinnvollen Zug.
                attachRetryAction(bubble, () => runAgentTurn(model, messages, bubble));
                return;
            }

            lastText = result.text;
            const assistant = { role: 'assistant', content: result.text };

            if (result.toolCalls.length > 0) {
                assistant.tool_calls = result.toolCalls;
            }

            working.push(assistant);
            await renderResult(bubble, result.text, result.thinking);

            if (result.toolCalls.length === 0) {
                messageHistory = toChatHistory(working);
                updateContextUsage();
                await persistHistory();
                return;
            }

            for (const call of result.toolCalls) {
                if (cancelRequested) {
                    appendMessageToUI('system', `${zugname(termine)} abgebrochen.`);
                    return;
                }

                const name = call?.function?.name ?? 'unbekannt';
                const isWrite = WRITE_TOOL_NAMES.includes(name);
                const label = `${isWrite ? 'Schreiben: ' : ''}${toolCallTarget(call) || name}`;

                if (isWrite) {
                    await runWriteCall(call, steps, working, label);
                    continue;
                }

                const approved = await requestToolApproval(call);

                if (!approved) {
                    const entry = addAgentStep(steps, call, 'Vom Benutzer abgelehnt. Das Modell wurde darüber informiert.');
                    entry.step.classList.add('agent-step-rejected');
                    working.push({
                        role: 'tool',
                        tool_name: name,
                        content: 'Der Benutzer hat die Ausführung dieses Werkzeugs abgelehnt. Arbeite mit dem, was du hast, oder erkläre, was du bräuchtest.',
                    });
                    continue;
                }

                const entry = addAgentStep(steps, call, 'wird ausgeführt ...');

                try {
                    const output = await invoke('execute_tool', {
                        name: name,
                        arguments: call.function.arguments,
                        benutzertext: letzteBenutzernachricht(working),
                    });
                    setAgentStepResult(entry, `${output.summary}${output.truncated ? ' (gekürzt)' : ''}\n\n${output.content}`);
                    working.push({ role: 'tool', tool_name: name, content: output.content });
                } catch (error) {
                    // Ein Werkzeugfehler ist kein Abbruch: Das Modell soll ihn
                    // sehen und damit weiterarbeiten können.
                    setAgentStepResult(entry, `Fehler: ${error}`);
                    entry.step.classList.add('agent-step-failed');
                    working.push({ role: 'tool', tool_name: name, content: `Fehler: ${error}` });
                }
            }
        }

        messageHistory = toChatHistory(working);
        await persistHistory();
        await renderResult(bubble, `${lastText}\n\n[${zugname(termine)} beendet: maximale Schrittzahl ${agent.maxSteps} erreicht. ${termine ? 'Mit /scope agent kommst du zurück zu den Dateiwerkzeugen.' : 'Mit /agent-dir oder /agent lässt sich das Arbeitsverzeichnis bzw. der Modus anpassen.'}]`);
    } catch (error) {
        await renderResult(bubble, `Der ${zugname(termine)} ist fehlgeschlagen: ${error}`);
    } finally {
        setGenerating(false);
        // Der Agentenlauf holt den Fokus auch nach einem Fehler oder Abbruch
        // zurück: Die Eingabe ist die einzige verbleibende Möglichkeit, den Zug
        // zu wiederholen oder etwas anderes zu fragen.
        promptInput.focus();
    }
}

// ------------------------------------------- Antwortstrom (Chat und Agentenmodus)
// Hilfsfunktion: Antwortstream in eine Blase schreiben. Wird aus beiden Wegen
// benutzt: aus performChat und aus dem Agentenlauf.
async function runChatAttempt(model, messages, bubble, options = {}) {
    let fullResponse = '';
    let fullThinking = '';
    const toolCalls = [];
    bubble.classList.add('markdown');

    // Vorherige Inhalte entfernen. Die Werkzeugschritte bleiben erhalten: Ein
    // Agentenzug besteht aus mehreren Modellanfragen, und die Schritte sollen
    // alle in derselben Blase stehen.
    //
    // Nebeneffekt im Agentenmodus: Ein Denktextblock wird dabei mit entfernt und
    // je Modellanfrage neu aufgebaut. Sichtbar bleibt am Ende nur der Denktext
    // des letzten Schritts, und er steht wieder aufgeklappt da.
    for (const child of [...bubble.children]) {
        if (!child.classList.contains('agent-steps')) {
            child.remove();
        }
    }

    // Denk- und Antworttext stehen in getrennten Bereichen, damit der Denktext
    // zurücktreten kann und die Antwort als Markdown gerendert wird.
    const answer = document.createElement('div');
    answer.className = 'answer';
    bubble.appendChild(answer);
    bubble.setAttribute('aria-busy', 'true');
    chatContainer.classList.add('loading');

    const onChunk = new Channel();
    onChunk.onmessage = (chunk) => {
        if (chunk.thinking) {
            fullThinking += chunk.thinking;
            const reasoning = ensureReasoningBlock(bubble);
            reasoningBlocks.get(reasoning).text.textContent = fullThinking;
        }

        if (chunk.content) {
            fullResponse += chunk.content;
            answer.textContent = fullResponse;
            collapseReasoningWhenAnswerStarts(bubble);
        }

        // Werkzeugaufrufe kommen mit dem letzten Chunk einer Runde.
        if (chunk.tool_calls) {
            toolCalls.push(...chunk.tool_calls);
        }

        scrollToBottom();
    };

    try {
        await invoke('send_chat_message', {
            model: model,
            messages: messages,
            onChunk: onChunk,
            tools: options.tools ?? null,
            system: options.system ?? null,
        });
        return { ok: true, text: fullResponse, thinking: fullThinking, toolCalls };
    } catch (error) {
        console.error('Fehler bei der Kommunikation:', error);
        return { ok: false, text: fullResponse, thinking: fullThinking, toolCalls, error: String(error).slice(0, 1000) };
    } finally {
        chatContainer.classList.remove('loading');
        bubble.setAttribute('aria-busy', 'false');
    }
}

// Hilfsfunktion: fertige Antwort (oder Fehlermeldung) als Markdown darstellen
async function renderResult(bubble, text, thinking = '') {
    const answer = bubble.querySelector('.answer') ?? bubble;

    try {
        answer.innerHTML = await invoke('render_markdown', { markdown: text });

        for (const link of answer.querySelectorAll('a[href]')) {
            link.target = '_blank';
            link.rel = 'noopener noreferrer';
        }
    } catch (error) {
        console.error('Fehler beim Rendern von Markdown:', error);
        answer.textContent = text;
    }

    if (thinking) {
        const reasoningText = ensureReasoningBlock(bubble).querySelector('.reasoning-text');

        try {
            reasoningText.innerHTML = await invoke('render_markdown', { markdown: thinking });
        } catch (error) {
            console.error('Fehler beim Rendern des Denktexts:', error);
            reasoningText.textContent = thinking;
        }
    }

    scrollToBottom();
}

// Hilfsfunktion: Retry-Knopf unter eine fehlgeschlagene Antwort setzen
function attachRetryAction(bubble, onRetry) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'retry-button';
    button.textContent = 'Erneut versuchen';
    button.addEventListener('click', async () => {
        if (isGenerating) return;
        button.remove();
        await onRetry();
    });
    bubble.appendChild(button);
    scrollToBottom();
}

// Ein Chat-Versuch inklusive Abschluss-UI; erneut aufrufbar für Retries
async function performChat(model, messages, bubble) {
    // Mit Werkzeugen übernimmt die Schleife; im einfachen Chat bleibt es bei
    // genau einer Anfrage.
    if (werkzeugschleifeAktiv()) {
        await runAgentTurn(model, messages, bubble);
        return;
    }

    setGenerating(true);
    cancelRequested = false;

    const result = await runChatAttempt(model, messages, bubble, {
        system: systemPromptForRequest(),
    });

    if (result.ok) {
        messageHistory = [...messages, { role: 'assistant', content: result.text }];
        await renderResult(bubble, result.text, result.thinking);
        if (retryNotice) {
            updateRetryNotice('Antwort erhalten – die Verbindung war kurzzeitig unterbrochen.');
        }
        setGenerating(false);
        updateContextUsage();
        await persistHistory();
        // Nur bei Erfolg: Nach einem Fehler soll der Retry-Knopf in der Blase
        // bedienbar bleiben, und ein hier gesetzter Fokus würde den Blick darauf
        // wegnehmen.
        promptInput.focus();
        return;
    }

    const separator = result.text ? '\n\n' : '';
    await renderResult(bubble, `${result.text}${separator}[Fehler: ${result.error}]`, result.thinking);
    if (retryNotice) {
        updateRetryNotice('Verbindung bleibt instabil – alle automatischen Versuche sind fehlgeschlagen.');
    }
    // Ein erneuter Versuch verwendet die aktuelle Auswahl oben rechts. Nur solange
    // keine möglich ist, bleibt das Modell des ersten Versuchs erhalten.
    attachRetryAction(bubble, () => performChat(modelSelect.value || model, messages, bubble));
    setGenerating(false);

    try {
        setServerStatusFromProbe(await invoke('check_server'));
    } catch {
        setServerStatus('unstable', 'Server: Status unbekannt');
    }
}

async function handleSendMessage() {
    const text = promptInput.value.trim();

    if (!text || isGenerating) return;

    if (text.startsWith('/')) {
        resetPromptInput();
        isGenerating = true;

        try {
            await handleChatCommand(text);
        } finally {
            isGenerating = false;
        }

        return;
    }

    const model = modelSelect.value;

    // Beim Start ist die Modellliste noch leer, aber das Eingabefeld bereits
    // fokussiert. Ohne diesen Hinweis bliebe ein Tastendruck stumm.
    if (!model) {
        appendMessageToUI('system', 'Es ist kein Modell ausgewählt. Prüfe, ob der Server erreichbar ist.');
        return;
    }

    // Eingabe zurücksetzen & UI sperren
    resetPromptInput();
    setGenerating(true);

    // Angehängte Dateien stehen als eigene Nachrichten vor der Eingabe.
    const attachments = attachmentMessages();
    const userMessage = { role: 'user', content: text };
    // Dieselbe Grenze wie in der Verlaufsdatei (MAX_GESPEICHERTE_NACHRICHTEN): auch
    // die Anfrage selbst bleibt bei den letzten 100 Nachrichten.
    const requestMessages = [...messageHistory, ...attachments, userMessage].slice(-100);
    appendMessageToUI('user', text);

    // Leeres AI-Nachrichten-Element für das Live-Streaming vorbereiten
    const aiBubble = appendMessageToUI('ai', '');
    updateContextUsage(requestMessages);

    await performChat(model, requestMessages, aiBubble);

    pendingAttachments = [];
    updateContextUsage();
}

// Hilfsfunktion: Nachricht im Chat-Fenster anzeigen
function appendMessageToUI(role, content) {
    const messageDiv = document.createElement('div');
    messageDiv.classList.add('message', role);
    messageDiv.textContent = content;
    chatContainer.appendChild(messageDiv);
    scrollToBottom();
    return messageDiv;
}

// Hilfsfunktion: Automatisch zum Ende scrollen
function scrollToBottom() {
    chatContainer.scrollTop = chatContainer.scrollHeight;
}

cancelChatBtn.addEventListener('click', async () => {
    cancelChatBtn.disabled = true;
    // Merken, damit auch die Werkzeugschleife des Agentenmodus abbricht, nicht
    // nur der laufende Request.
    cancelRequested = true;
    try {
        await invoke('cancel_chat');
    } catch (error) {
        console.error('Fehler beim Abbrechen des Chats:', error);
    }
});

async function setWriteMode(enabled) {
    if (enabled && !window.confirm(
        'Schreibende Werkzeuge freigeben? Das Modell darf dann neue Dateien anlegen und vorhandene Dateien an genau einer Stelle ändern. '
        + 'Jeder Vorgang wird dir als Unterschied gezeigt und muss von dir genehmigt werden. Es gibt weiterhin keine Lösch- oder Ausführungsbefehle.',
    )) {
        return false;
    }

    await invoke('set_write_enabled', { enabled });

    // Der maßgebliche Stand kommt aus dem Backend. Würde hier der Wunsch
    // übernommen, könnte die Oberfläche einen Schreibmodus anzeigen, den es gar
    // nicht gibt - das Modell hätte die Werkzeuge dann nicht.
    const writeState = await invoke('get_write_state');
    agent.writeEnabled = Boolean(writeState.enabled);
    agent.maxWrites = Number(writeState.max_writes) || agent.maxWrites;
    agent.maxWriteBytes = Number(writeState.max_bytes) || agent.maxWriteBytes;

    if (agent.writeEnabled !== enabled) {
        return false;
    }

    // Die Werkzeugliste ändert sich mit dem Schreibmodus, also neu laden.
    agent.toolset = null;
    writeToggleBtn.setAttribute('aria-pressed', agent.writeEnabled ? 'true' : 'false');
    writeToggleBtn.textContent = agent.writeEnabled ? 'Schreiben: an' : 'Schreiben: aus';
    writeToggleBtn.title = agent.writeEnabled
        ? 'Schreibende Werkzeuge sind freigegeben'
        : 'Schreibende Werkzeuge freigeben (nur mit Agentenmodus)';
    updateAgentStepsHead();
    return true;
}

agentToggleBtn.addEventListener('click', async () => {
    if (agent.enabled) {
        setAgentMode(false);
        appendMessageToUI('system', 'Agentenmodus ausgeschaltet. Es werden keine Werkzeuge mehr benutzt.');
        return;
    }

    try {
        await refreshAgentConfig();

        if (agent.scope === 'termine') {
            appendMessageToUI('system', 'Im Terminumfang sind die Kalenderwerkzeuge ohnehin da. Der Schalter gilt nur für das Arbeitsverzeichnis; mit /scope agent kommst du zurück.');
            return;
        }

        if (!agent.root) {
            appendMessageToUI('system', 'Es ist kein Arbeitsverzeichnis gesetzt. Erst /agent-dir <pfad> benutzen.');
            return;
        }

        agent.toolset = null;
        setAgentMode(true);
        appendMessageToUI(
            'system',
            `Agentenmodus an. Das Modell darf nur lesende Werkzeuge in ${agent.root} benutzen, jeder Aufruf wird bestätigt. Höchstens ${agent.maxSteps} Schritte je Nachricht.`
            + (agent.writeEnabled ? ' Schreibende Werkzeuge sind freigegeben.' : ' Schreibende Werkzeuge sind gesperrt, /agent-write schaltet sie frei.'),
        );
    } catch (error) {
        appendMessageToUI('system', `Fehler beim Einschalten: ${error}`);
    }
});

writeToggleBtn.addEventListener('click', async () => {
    if (agent.scope === 'termine') {
        appendMessageToUI('system', 'Im Terminumfang gibt es keine Dateiwerkzeuge. Termine ändert das Modell ohnehin nach Vorschau; mit /scope agent wird der Schalter wieder nötig.');
        return;
    }

    if (!agent.enabled) {
        appendMessageToUI('system', 'Zuerst den Agentenmodus einschalten.');
        return;
    }

    try {
        const enabled = await setWriteMode(!agent.writeEnabled);
        appendMessageToUI(
            'system',
            enabled
                ? 'Schreibende Werkzeuge freigegeben. Jeder Vorgang erscheint als Unterschied und wartet auf deine Genehmigung.'
                : 'Schreibende Werkzeuge gesperrt. Es wird nichts mehr verändert.',
        );
    } catch (error) {
        appendMessageToUI('system', `Fehler: ${error}`);
    }
});

// Dialog für die Adresse des Ollama-Servers.
//
// Warum ein eigener Dialog und nicht der Chat-Befehl `/server-url`: Der Befehl
// funktioniert, aber er muss im Chat getippt werden. Wer den Server zum ersten
// Mal startet und eine falsche Adresse hat, sieht einen ausgefallenen Server –
// und genau dann ist der Chat der einzige Ort mit einer Eingabemöglichkeit. Das
// ist eine Zirkulärheit: Die Adresse lässt sich nur ändern, wenn die Verbindung
// schon steht.
//
// Zwei Dinge unterscheidet die Oberfläche vom Befehl:
//
// 1. Die Adresse wird **geprüft, bevor sie gespeichert wird**. Ein Tippfehler wird
//    nicht still angenommen und danach als „Server offline" gemeldet.
// 2. Bei einem nicht erreichbaren Server wird die Eingabe **nicht** verworfen. Der
//    Server kann aus mehreren Gründen weg sein – Rechner aus, Firewall, falscher
//    Port –, und die getippte Adresse ist in dem Fall richtig.

checkServerBtn.addEventListener('click', loadModels);
startServerBtn.addEventListener('click', startOllamaViaSsh);

// Die Auswahl im Kopf geht denselben Weg wie `/provider`: ein Wechsel ändert
// Server, Umfang, Modelle und Verlauf, und das an zwei Stellen zu pflegen hieße,
// dass eines von beidem irgendwann stehen bleibt.
providerSelect.addEventListener('change', async () => {
    // Der Wert steht schon auf der neuen Wahl, sobald dieses Ereignis kommt.
    // `wechsleProvider` stellt ihn zurück, wenn der Benutzer abbricht oder das
    // Speichern scheitert – deshalb wird hier nichts selbst zurückgesetzt.
    await wechsleProvider(providerSelect.value, { fragen: true }).catch((error) =>
        console.error('Providerwechsel fehlgeschlagen:', error)
    );
});

// --- Die Adresse des Ollama-Servers ----------------------------------------

/** Setzt die Statuszeile im Dialog und sperrt den Knopf beim Prüfen. */
function setServerUrlStatus(text, laeuft = false) {
    serverUrlStatus.textContent = text;
    serverUrlStatus.className = `field-status${laeuft ? ' ok' : ''}`;
    serverUrlSave.disabled = laeuft;
}

async function openServerUrlDialog() {
    // Die aktuell gesetzte Adresse vorzeigen, nicht eine leere Zeile: Wer sie
    // ändern will, muss sie nicht erst abtippen.
    try {
        serverUrlInput.value = await invoke('get_server_url');
    } catch (error) {
        serverUrlInput.value = '';
        setServerUrlStatus(`Aktuelle Adresse nicht lesbar: ${error}`);
    }

    serverUrlDialog.showModal();
    serverUrlInput.focus();
    serverUrlInput.select();
}

serverUrlBtn.addEventListener('click', openServerUrlDialog);
serverUrlCancel.addEventListener('click', () => serverUrlDialog.close());

serverUrlForm.addEventListener('submit', async (ereignis) => {
    ereignis.preventDefault();

    const eintrag = serverUrlInput.value.trim();
    // Leer heißt ausdrücklich zurück auf den Vorgabewert. Sonst wäre nicht mehr
    // zu erkennen, ob der Benutzer die Adresse entfernen oder den Vorgabewert
    // eintippen wollte.
    const roh = eintrag === '' ? 'localhost:11434' : eintrag;
    const vorher = await invoke('get_server_url').catch(() => null);

    setServerUrlStatus('Prüfe ...', true);

    try {
        const neue = await invoke('set_server_url', { serverUrl: roh });
        const modelle = await loadModels();

        if (modelle === null) {
            // Nicht erreichbar. Die Adresse wird trotzdem behalten: Der Server
            // kann aus anderen Gründen weg sein, und ein Zurückrollen auf eine
            // nachweislich falsche Adresse hilft niemandem. „Abbrechen“ bleibt
            // möglich und stellt den Ausgangszustand wieder her.
            setServerUrlStatus(
                `${neue} nicht erreichbar. Die Adresse ist gespeichert, der Server antwortet nicht. `
                + 'Prüfe Netzwerk, Port und ob der Server läuft.'
            );
            return;
        }

        // Erreichbar. Der alte Verlauf gehört zum alten Server und würde sonst
        // beim nächsten Start wieder auftauchen.
        if (vorher && vorher !== neue) {
            messageHistory = [];
            await invoke('delete_chat_history').catch(() => {});
            updateContextUsage();
        }

        setServerUrlStatus(`${neue} – ${modelle.length} Modell(e) gefunden.`, true);
        // Kurz stehen lassen, damit der Erfolg zu sehen ist, dann schließen.
        setTimeout(() => {
            if (serverUrlDialog.open) serverUrlDialog.close();
        }, 1200);
    } catch (error) {
        setServerUrlStatus(`Adresse nicht gültig: ${error}`);
    }
});

// Event Listener für den Senden-Button
sendBtn.addEventListener('click', handleSendMessage);

// --- Vervollständigung -------------------------------------------------------
// Die Befehlsliste wird aus `COMMAND_HELP` gelesen, nicht ein zweites Mal
// hingeschrieben. Sonst stünde hier eine Liste, die irgendwann nicht mehr
// stimmt – und eine falsche Befehlsliste ist schlimmer als gar keine, weil der
// Benutzer dem Vorschlag vertraut.
//
// Die Grammatik ist absichtlich winzig und steht in den `usage`-Zeilen:
//   `/calendar [aus|zertifikat]`  – zwei Alternativen in Klammern
//   `/history [an|aus|löschen]`    – auch mehr als zwei
//   `/ssh-key <pfad> | /ssh-key aus` – Alternativen mit `|` getrennt
//   `/context <token>`             – ein Platzhalter, kein Befehlswort
// Alles in spitzen Klammern ist ein Platzhalter und wird nicht vorgeschlagen; was
// in eckigen Klammern steht, sind echte Alternativen.

// Die Befehlsnamen, ohne Argumente, aus der Übersicht gelesen.
function befehlsnamen() {
    const namen = new Set();

    for (const gruppe of COMMAND_HELP) {
        for (const eintrag of gruppe.entries) {
            if (eintrag.name.startsWith('/')) {
                namen.add(eintrag.name.split(' ')[0].toLowerCase());
            }
        }
    }

    return [...namen].sort();
}

// Die Argumente, die der Benutzer tippen kann – ohne Platzhalter.
//
// Ein Platzhalter ist etwas, das der Benutzer erfinden muss: ein Pfad, eine Zahl,
// eine Adresse. Ein Vorschlag wäre dort eine Lüge, und ein eingefügter
// Platzhalter (`/context <token>`) als fertiger Befehl wäre schlimmer.
function befehlsargumente(befehl) {
    const argumente = new Set();

    for (const gruppe of COMMAND_HELP) {
        for (const eintrag of gruppe.entries) {
            if (!eintrag.usage) {
                continue;
            }

            // `|` trennt Alternativen – aber nur außerhalb von Klammern. Sonst
            // zerlegte es `[aus|zertifikat]` in zwei kaputte Hälften und `aus`
            // fehlte in der Liste.
            for (const teil of teileAnPipe(eintrag.usage)) {
                const worte = teil.trim().split(/\s+/);

                if (worte[0]?.toLowerCase() !== befehl) {
                    continue;
                }

                const rest = teil.trim().slice(worte[0].length).trim();

                if (!rest) {
                    continue;
                }

                if (rest.startsWith('[') && rest.endsWith(']')) {
                    // In eckigen Klammern stehen die Alternativen selbst.
                    for (const wahl of rest.slice(1, -1).split('|')) {
                        if (wahl) {
                            argumente.add(wahl.trim().toLowerCase());
                        }
                    }

                    continue;
                }

                // Alles andere: nur wenn es wirklich ein Wort ist. `<pfad>` und
                // `<benutzer@host> [port]` sind Platzhalter und werden
                // übersprungen.
                if (!/^[^\s<>\[\]]+$/.test(rest)) {
                    continue;
                }

                argumente.add(rest.toLowerCase());
            }
        }
    }

    return [...argumente].sort();
}

// Teilt an `|`, aber nur in Tiefe null. `[aus|zertifikat]` bleibt ganz.
function teileAnPipe(text) {
    const teile = [];
    let tiefe = 0;
    let anfang = 0;

    for (let index = 0; index < text.length; index += 1) {
        const zeichen = text[index];

        if (zeichen === '[' || zeichen === '<') {
            tiefe += 1;
        } else if (zeichen === ']' || zeichen === '>') {
            tiefe -= 1;
        } else if (zeichen === '|' && tiefe === 0) {
            teile.push(text.slice(anfang, index));
            anfang = index + 1;
        }
    }

    teile.push(text.slice(anfang));
    return teile.map((teil) => teil.trim()).filter(Boolean);
}

// Der längste Anfang, den alle Kandidaten gemeinsam haben.
//
// Wird ein Kandidat eingesetzt, an dem alle anderen hängen, wäre es irreführend:
// Der Benutzer sähe einen vollständigen Befehl, den es so nicht gibt.
function gemeinsamerAnfang(kandidaten) {
    if (kandidaten.length === 0) {
        return '';
    }

    let laenge = kandidaten[0].length;

    for (const kandidat of kandidaten) {
        while (laenge > 0 && !kandidat.startsWith(kandidaten[0].slice(0, laenge))) {
            laenge -= 1;
        }
    }

    return kandidaten[0].slice(0, laenge);
}

// Was der Benutzer gerade tippt: das Wort vor dem Cursor und wo es anfängt.
//
// `start` ist das Leerzeichen vor dem Wort, `ende` das nächste danach. Der
// Bereich wird beim Ergänzen ersetzt, damit Text hinter dem Cursor stehen
// bleibt – sonst würde ein TAB mitten im Text alles danach löschen.
function letztesWort(text, cursor) {
    const bis = Math.max(0, Math.min(cursor ?? text.length, text.length));
    let start = bis;

    while (start > 0 && !/\s/.test(text[start - 1])) {
        start -= 1;
    }

    let ende = start;

    while (ende < text.length && !/\s/.test(text[ende])) {
        ende += 1;
    }

    return { start, ende, wort: text.slice(start, bis) };
}

// Der Zustand der Vorschlagsliste. Sie gehört an die Eingabe, nicht an ein
// Fenster: Was der Benutzer tippt, entscheidet, was vorgeschlagen wird.
const vorschlag = {
    offen: false,
    kandidaten: [],
    auswahl: 0,
    // Wo das zu ersetzende Wort stand, was genau eingesetzt wurde und was danach
    // im Feld steht. Beides wird gemerkt, damit jede Auswahl **genau dieselbe**
    // Stelle ersetzt: Aus dem Textinhalt neu erraten ginge schief, sobald sich
    // das Feld zwischen zwei Tastendrücken geändert hat – dann landete der
    // Kandidat an einer völlig anderen Stelle.
    start: 0,
    ersetzung: '',
    nach: '',
};

// Die Kandidaten, die zu einem Feld passen – oder `null`, wenn es keine gibt.
//
// Eine reine Funktion: Sie rechnet aus Eingabefeld und Cursor die Treffer, ohne
// etwas zu ändern. Damit ist das Verhalten ohne WebView prüfbar.
function kandidatenFuer(text, cursor) {
    if (!text.startsWith('/')) {
        return null;
    }

    const { start, ende, wort } = letztesWort(text, cursor);
    const istBefehl = start === 0;
    const befehl = istBefehl ? '' : text.trim().split(/\s+/)[0].toLowerCase();
    const klein = wort.toLowerCase();
    const namen = befehlsnamen();

    // Beim Befehlsnamen wird nur vervollständigt, was schon ein `/` ist: Nach
    // einem Leerzeichen ist ein Wort wie „aus" kein Befehl.
    const treffer = istBefehl
        ? namen.filter((name) => name.startsWith('/') && name.startsWith(klein))
        : befehlsargumente(befehl).filter((argument) => argument.startsWith(klein));

    if (treffer.length === 0) {
        return null;
    }

    // Ist das Wort schon vollständig getippt und passt trotzdem mehr als ein
    // Kandidat, wird der Treffer selbst herausgefiltert. Sonst wäre `/agent`
    // eindeutig, weil es exakt so in der Liste steht, und es gäbe nichts zu
    // durchblättern.
    const kandidaten = treffer.filter((name) => name !== klein || treffer.length === 1);
    const alle = istBefehl ? namen : befehlsargumente(befehl);

    return { kandidaten, start, ende, alle, nach: text.slice(ende) };
}

// Setzt ein Wort an eine feste Stelle und gibt zurück, was eingesetzt wurde.
function setzeWort(start, einsetzung, nach) {
    promptInput.value = promptInput.value.slice(0, start) + einsetzung + nach;
    // Der Cursor landet hinter dem Einsetzten, nicht am Ende des ganzen Feldes.
    const cursor = start + einsetzung.length;
    promptInput.setSelectionRange(cursor, cursor);
    return einsetzung;
}

// Schreibt einen Kandidaten an die Stelle des Wortes, das getippt wurde.
function uebernehmeKandidat(kandidat, alle, start, nach) {
    // Nur ein Leerzeichen anhängen, wenn das Wort ein **fertiger** Befehl ist und
    // danach nichts im Feld steht. Sonst stünde nach einem angefangenen Wort ein
    // Leerzeichen im Feld, das wieder weg muss.
    return setzeWort(start, einsetzungFuer(kandidat, alle, nach), nach);
}

// Zeichnet die Liste über der Eingabe.
//
// `aria-selected` und `role="listbox"` sorgen dafür, dass ein Screenreader den
// markierten Eintrag mitbekommt – die Auswahl ist nur visuell, und ohne diese
// Angaben wäre sie blind.
function zeichneVorschlagsliste() {
    commandHintList.replaceChildren();

    vorschlag.kandidaten.forEach((kandidat, index) => {
        const eintrag = document.createElement('li');
        eintrag.className = 'command-hint-item';
        eintrag.setAttribute('role', 'option');
        eintrag.textContent = kandidat;

        if (index === vorschlag.auswahl) {
            eintrag.classList.add('command-hint-selected');
            eintrag.setAttribute('aria-selected', 'true');
        }

        commandHintList.appendChild(eintrag);
    });

    commandHint.hidden = false;
}

function oeffneVorschlagsliste(treffer, vomEnde = false) {
    vorschlag.offen = true;
    vorschlag.kandidaten = treffer.kandidaten;
    vorschlag.alle = treffer.alle;
    // Umschalt+TAB als erste Taste markiert das **letzte** der Liste: Zurück von
    // Anfang heißt nicht gar nichts.
    vorschlag.auswahl = vomEnde ? treffer.kandidaten.length - 1 : 0;
    vorschlag.start = treffer.start;
    vorschlag.nach = treffer.nach;
    vorschlag.ersetzung = setzeWort(
        treffer.start,
        einsetzungFuer(treffer.kandidaten[vorschlag.auswahl], treffer.alle, treffer.nach),
        treffer.nach,
    );
    zeichneVorschlagsliste();
}

function schliesseVorschlagsliste() {
    vorschlag.offen = false;
    vorschlag.kandidaten = [];
    vorschlag.auswahl = 0;
    commandHint.hidden = true;
    commandHintList.replaceChildren();
}

// Was für einen Kandidaten im Feld stehen soll – mit oder ohne Leerzeichen.
function einsetzungFuer(kandidat, alle, nach) {
    const istFertig = !alle.some((name) => name !== kandidat && name.startsWith(kandidat));
    const passt = nach.length === 0 || !/\s/.test(nach[0]);
    return istFertig && passt ? `${kandidat} ` : kandidat;
}

// Der nächste markierte Eintrag, mit Umlauf am Ende.
//
// Reine Rechnung, damit sie ohne Bildschirm prüfbar ist: Von 3 Kandidaten auf
// dem letzten weitergeblättert landet wieder auf dem ersten.
function naechsteAuswahl(aktuelle, anzahl, schritt) {
    if (anzahl === 0) {
        return 0;
    }

    return (aktuelle + schritt + anzahl) % anzahl;
}

// Einen Schritt weiter oder zurück.
function verschiebeAuswahl(schritt) {
    if (vorschlag.kandidaten.length === 0) {
        return;
    }

    vorschlag.auswahl = naechsteAuswahl(vorschlag.auswahl, vorschlag.kandidaten.length, schritt);

    // Genau die Zeichen ersetzen, die zuletzt eingesetzt wurden. Die alte Stelle
    // mit dem eingesetzten Text zu überschreiben ist der ganze Widerspruch.
    vorschlag.ersetzung = setzeWort(
        vorschlag.start,
        einsetzungFuer(vorschlag.kandidaten[vorschlag.auswahl], vorschlag.alle, vorschlag.nach),
        vorschlag.nach,
    );
    zeichneVorschlagsliste();
}

// TAB für die Befehlseingabe.
//
// Der Hörer hängt an `window` und in der **Fangphase**, nicht am Feld selbst. Der
// Grund ist nicht Geschmack: Der Fokus wanderte beim Umschalt+TAB zum Knopf davor,
// obwohl `preventDefault` im Hörer am Feld stand. Dass der Fokus wandert, heißt,
// dass der Tastendruck zugestellt wurde – nur eben nicht dort, wo man ihn
// abfängt. In der Fangphase kommt er garantiert an, und `preventDefault` dort
// wirkt genauso. Zusätzlich `stopPropagation`, damit der Hörer am Feld nicht
// ein zweites Mal läuft.
//
// Solange die Eingabe mit `/` beginnt, wird TAB abgefangen. Sonst bleibt die
// Taste, was sie sonst ist – in einem Eingabefeld wechselt sie sonst den Fokus
// weiter, und das ist die einzige Möglichkeit, das Feld ohne Maus zu verlassen.
window.addEventListener('keydown', (ereignis) => {
    if (ereignis.key !== 'Tab' || ereignis.target !== promptInput) {
        return;
    }

    if (ereignis.ctrlKey || ereignis.metaKey || ereignis.altKey) {
        return;
    }

    ereignis.stopPropagation();

    if (!vorschlag.offen) {
        const treffer = kandidatenFuer(promptInput.value, promptInput.selectionStart);

        if (!treffer) {
            return;
        }

        // Auch beim ersten Umschalt+TAB wird der Tastendruck verbraucht, sonst
        // springt der Fokus aus dem Feld heraus und die Liste ist weg, ohne
        // dass etwas ergänzt wurde. Zurück von Anfang heißt: das letzte der
        // Liste.
        ereignis.preventDefault();

        if (treffer.kandidaten.length === 1 && !ereignis.shiftKey) {
            // Bei genau einem Kandidaten gibt es nichts zu durchblättern: Er
            // wird eingesetzt, und die Liste bleibt zu. Das ist der übliche
            // Weg und soll kein Fenster aufmachen.
            uebernehmeKandidat(treffer.kandidaten[0], treffer.alle, treffer.start, treffer.nach);
            return;
        }

        oeffneVorschlagsliste(treffer, ereignis.shiftKey);
        return;
    }

    ereignis.preventDefault();
    verschiebeAuswahl(ereignis.shiftKey ? -1 : 1);
}, true);

// Solange die Liste offen ist, gehört der Fokus dem Eingabefeld.
//
// Zweites Netz hinter dem Hörer in der Fangphase: Sollte ein WebView den
// Tastendruck doch an den Knopf durchreichen, holt dieses `focusin` den Fokus
// zurück, statt ihn beim Bediener zu lassen. Ohne offene Liste greift es nicht
// und verhindert damit nicht, das Feld mit TAB zu verlassen.
window.addEventListener('focusin', (ereignis) => {
    if (vorschlag.offen && ereignis.target !== promptInput) {
        promptInput.focus();
    }
});

// ENTER und Esc in der Befehlseingabe.
promptInput.addEventListener('keydown', (ereignis) => {
    if (ereignis.ctrlKey || ereignis.metaKey || ereignis.altKey) {
        return;
    }

    if (ereignis.key === 'Escape' && vorschlag.offen) {
        ereignis.preventDefault();
        schliesseVorschlagsliste();
        return;
    }

    // Beide Fälle von ENTER stehen hier und nicht in einem zweiten Hörer:
    // `preventDefault` hält den nächsten Hörer nicht auf, ein zweiter würde
    // trotzdem senden.
    if (ereignis.key === 'Enter' && !ereignis.shiftKey) {
        ereignis.preventDefault();

        // Mit offener Liste übernimmt ENTER den markierten Befehl, statt ihn zu
        // senden. Sonst wäre ein halb fertiger Befehl abgeschickt, nur weil der
        // Benutzer ihn gerade ansehen wollte. Ein zweites ENTER sendet dann.
        if (vorschlag.offen) {
            schliesseVorschlagsliste();
            return;
        }

        handleSendMessage();
    }
});

// Jede weitere Eingabe macht die Liste ungültig: Sie bezog sich auf ein Wort, das
// es jetzt nicht mehr gibt.
promptInput.addEventListener('input', schliesseVorschlagsliste);
// Beim Verlassen des Feldes ebenso – sonst bliebe die Liste über dem Fenster
// stehen, während sie zu nichts mehr gehört.
promptInput.addEventListener('blur', schliesseVorschlagsliste);

// Automatisches Vergrößern des Textarea-Felds bei viel Text
promptInput.addEventListener('input', function() {
    this.style.height = 'auto';
    this.style.height = `${this.scrollHeight}px`;
});
