/* Pure shared contract. No shell, eval, dynamic property assignment or script source. */
function validateOperation(raw) {
    if (typeof raw !== 'string' || raw.length > 8192) throw new Error('InvalidArgument');
    const value = JSON.parse(raw);
    if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error('InvalidArgument');
    const allowed = ['id', 'operation', 'target', 'fingerprint', 'x', 'y', 'width', 'height'];
    if (Object.keys(value).some(key => !allowed.includes(key))) throw new Error('InvalidArgument');
    if (!['focus', 'move', 'resize', 'close'].includes(value.operation)) throw new Error('Unsupported');
    for (const key of ['id', 'target', 'fingerprint']) {
        if (typeof value[key] !== 'string' || value[key].length < 1 || value[key].length > 256)
            throw new Error('InvalidArgument');
    }
    for (const key of value.operation === 'move' ? ['x', 'y'] : value.operation === 'resize' ? ['width', 'height'] : []) {
        if (!Number.isInteger(value[key])) throw new Error('InvalidArgument');
        const minimum = value.operation === 'resize' ? 1 : -32768;
        const maximum = value.operation === 'resize' ? 16384 : 32767;
        if (value[key] < minimum || value[key] > maximum) throw new Error('InvalidArgument');
    }
    return value;
}
function exactWindow(windows, command, describe) {
    const found = windows.filter(window => String(describe(window).id) === command.target);
    if (found.length !== 1) throw new Error(found.length ? 'AmbiguousTarget' : 'StaleReference');
    if (describe(found[0]).fingerprint !== command.fingerprint) throw new Error('StaleReference');
    return found[0];
}

/* KWin 6 script. All calls are async; Next is a bounded long poll in the Rust broker. */
(function () {
    'use strict';
    const SERVICE = 'org.semwright.Broker';
    const PATH = '/org/semwright/KWinBridge';
    const INTERFACE = 'org.semwright.KWinMailbox1';
    const epoch = String(Date.now()) + '-' + String(Math.random());
    let polling = false;
    let enabled = true;
    function windows() { return workspace.stackingOrder.filter(window => window.normalWindow || window.dialog); }
    function describe(window) {
        const rect = window.frameGeometry;
        const app = String(window.resourceClass);
        return {id: String(window.internalId), app: app,
            fingerprint: epoch + ':' + String(window.pid) + ':' + app,
            title: String(window.caption).slice(0, 4096), focused: workspace.activeWindow === window,
            bounds: {x: rect.x, y: rect.y, width: rect.width, height: rect.height},
            coordinate_space: 'compositor_logical'};
    }
    function publish() {
        if (!enabled) return;
        callDBus(SERVICE, PATH, INTERFACE, 'Publish', JSON.stringify({version: 1, windows: windows().slice(0, 2000).map(describe)}));
    }
    function apply(raw) {
        const command = validateOperation(raw);
        const window = exactWindow(windows(), command, describe);
        const rect = window.frameGeometry;
        switch (command.operation) {
        case 'focus': workspace.activeWindow = window; break;
        case 'move': window.frameGeometry = {x: command.x, y: command.y, width: rect.width, height: rect.height}; break;
        case 'resize': window.frameGeometry = {x: rect.x, y: rect.y, width: command.width, height: command.height}; break;
        case 'close': window.closeWindow(); break;
        default: throw new Error('Unsupported');
        }
        return {accepted: true, id: command.id};
    }
    function poll() {
        if (polling || !enabled) return;
        polling = true;
        callDBus(SERVICE, PATH, INTERFACE, 'Next', function (raw) {
            polling = false;
            if (typeof raw !== 'string') { enabled = false; return; }
            const command = JSON.parse(raw);
            if (command.id) {
                let result;
                try { result = apply(raw); }
                catch (_error) { result = {accepted: false, id: command.id, code: 'BackendFailed'}; }
                callDBus(SERVICE, PATH, INTERFACE, 'Complete', JSON.stringify(result));
                publish();
            }
            poll();
        });
    }
    workspace.windowAdded.connect(publish);
    workspace.windowRemoved.connect(publish);
    workspace.windowActivated.connect(publish);
    for (const window of windows()) {
        window.frameGeometryChanged.connect(publish);
        window.captionChanged.connect(publish);
    }
    workspace.windowAdded.connect(function (window) {
        window.frameGeometryChanged.connect(publish);
        window.captionChanged.connect(publish);
    });
    publish();
    poll();
})();
