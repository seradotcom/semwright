import assert from 'node:assert/strict';
import {test} from 'node:test';
import fs from 'node:fs';
import vm from 'node:vm';

const root = new URL('../../', import.meta.url);
const source = fs.readFileSync(new URL('bridges/kwin/contents/code/main.js', root), 'utf8');

function signal() {
    const handlers = [];
    return {
        handlers,
        connect(handler) {
            handlers.push(handler);
        },
    };
}

function makeWindow() {
    return {
        internalId: 'window-1',
        pid: 4242,
        resourceClass: 'fixture',
        caption: 'Fixture',
        normalWindow: true,
        dialog: false,
        frameGeometry: {x: 10, y: 20, width: 640, height: 480},
        frameGeometryChanged: signal(),
        captionChanged: signal(),
        closeWindow() {},
    };
}

function runBridge(workspace) {
    const calls = [];
    let nextCallback;
    const context = vm.createContext({
        workspace,
        callDBus(_service, _path, _interface, method, payloadOrCallback) {
            calls.push({method, payload: typeof payloadOrCallback === 'string' ? payloadOrCallback : null});
            if (method === 'Next') nextCallback = payloadOrCallback;
        },
    });
    vm.runInContext(source, context);
    const published = calls.find(call => call.method === 'Publish');
    assert.ok(published, 'bridge must publish an initial snapshot');
    const snapshot = JSON.parse(published.payload);
    assert.equal(snapshot.windows.length, 1);
    assert.equal(snapshot.windows[0].title, 'Fixture');
    assert.equal(typeof nextCallback, 'function');
    return {
        calls,
        snapshot,
        dispatch(command) {
            nextCallback(JSON.stringify(command));
        },
    };
}

test('KWin 5 workspace compatibility publishes and focuses via client API', () => {
    const window = makeWindow();
    const workspace = {
        clientList: () => [window],
        activeClient: null,
        clientAdded: signal(),
        clientRemoved: signal(),
        clientActivated: signal(),
    };
    const bridge = runBridge(workspace);
    assert.equal(bridge.snapshot.windows[0].focused, false);
    assert.equal(workspace.clientAdded.handlers.length, 2);
    assert.equal(workspace.clientRemoved.handlers.length, 1);
    assert.equal(workspace.clientActivated.handlers.length, 1);

    const target = bridge.snapshot.windows[0];
    bridge.dispatch({
        id: 'request-1',
        operation: 'focus',
        target: target.id,
        fingerprint: target.fingerprint,
    });
    assert.equal(workspace.activeClient, window);
    const complete = bridge.calls.find(call => call.method === 'Complete');
    assert.deepEqual(JSON.parse(complete.payload), {accepted: true, id: 'request-1'});
});

test('KWin 6 workspace compatibility retains window API behavior', () => {
    const window = makeWindow();
    const workspace = {
        stackingOrder: [window],
        activeWindow: null,
        windowAdded: signal(),
        windowRemoved: signal(),
        windowActivated: signal(),
    };
    const bridge = runBridge(workspace);
    assert.equal(bridge.snapshot.windows[0].focused, false);
    assert.equal(workspace.windowAdded.handlers.length, 2);
    assert.equal(workspace.windowRemoved.handlers.length, 1);
    assert.equal(workspace.windowActivated.handlers.length, 1);

    const target = bridge.snapshot.windows[0];
    bridge.dispatch({
        id: 'request-2',
        operation: 'focus',
        target: target.id,
        fingerprint: target.fingerprint,
    });
    assert.equal(workspace.activeWindow, window);
    const complete = bridge.calls.find(call => call.method === 'Complete');
    assert.deepEqual(JSON.parse(complete.payload), {accepted: true, id: 'request-2'});
});
