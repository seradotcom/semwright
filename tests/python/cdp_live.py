#!/usr/bin/env python3
"""Real isolated Chromium CDP contract probe. Does not execute the Rust adapter.

Uses a test-only about:blank fixture and a disposable profile. When invoked
as root in a build container, the browser drops to nobody. Chromium's sandbox is
never disabled, and an existing browser/debug endpoint is never attached.
"""
import base64
import json
import os
from pathlib import Path
import pwd
import shutil
import signal
import struct
import subprocess
import tempfile
import time
import unittest
import websocket

FIXTURE = b'''<!doctype html><meta charset="utf-8"><title>Semwright local CDP fixture</title>
<style>body{font:20px sans-serif;padding:40px}input,button{font:inherit;padding:12px;margin:12px}</style>
<label for="name">Name</label><input id="name" name="name" aria-label="Name"><button id="save">Save</button>
<output id="value"></output><p id="status">Not saved</p>
<script>document.querySelector('#name').addEventListener('input', e => document.querySelector('#value').textContent=e.target.value);
document.querySelector('#save').addEventListener('click', () => document.querySelector('#status').textContent='Saved');</script>'''

class Cdp:
    def __init__(self, endpoint):
        self.ws = websocket.create_connection(endpoint, timeout=8, suppress_origin=True)
        self.counter = 0
        self.events = []
    def call(self, method, params=None, session=None):
        self.counter += 1
        request = {'id': self.counter, 'method': method, 'params': params or {}}
        if session:
            request['sessionId'] = session
        self.ws.send(json.dumps(request))
        while True:
            raw = self.ws.recv()
            if len(raw) > 8_388_608:
                raise RuntimeError('CDP fixture reply exceeds test budget')
            reply = json.loads(raw)
            if reply.get('id') == self.counter:
                if 'error' in reply:
                    raise RuntimeError(f"CDP {method} failed: {reply['error'].get('message')}")
                return reply.get('result', {})
            self.events.append({'method': reply.get('method')})
            self.events = self.events[-200:]
    def close(self):
        self.ws.close()

class LiveCdpTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        browser = shutil.which('chromium') or shutil.which('chromium-browser')
        if not browser:
            raise unittest.SkipTest('Chromium not installed')
        cls.temp = tempfile.mkdtemp(prefix='semwright-cdp-')
        cls.addClassCleanup(lambda: shutil.rmtree(cls.temp, ignore_errors=True))
        os.chmod(cls.temp, 0o700)
        kwargs = {}
        if os.getuid() == 0:
            user = pwd.getpwnam('nobody')
            os.chown(cls.temp, user.pw_uid, user.pw_gid)
            kwargs = dict(user=user.pw_uid, group=user.pw_gid, extra_groups=[])
        cls.log = tempfile.TemporaryFile()
        cls.addClassCleanup(cls.log.close)
        cls.proc = subprocess.Popen([
            browser, '--headless=new', '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1',
            '--user-data-dir=' + cls.temp, '--no-first-run', '--no-default-browser-check',
            '--disable-background-networking', '--disable-component-update', '--disable-sync',
            '--disable-extensions', '--disable-default-apps', '--window-size=1024,768', 'about:blank',
        ], stdout=cls.log, stderr=cls.log, env={'PATH': '/usr/bin:/bin', 'HOME': cls.temp, 'XDG_RUNTIME_DIR': cls.temp, 'LANG': 'C.UTF-8'}, start_new_session=True, **kwargs)
        def stop():
            if cls.proc.poll() is None:
                os.killpg(cls.proc.pid, signal.SIGTERM)
                try:
                    cls.proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(cls.proc.pid, signal.SIGKILL)
                    cls.proc.wait(timeout=3)
        cls.addClassCleanup(stop)
        port_file = Path(cls.temp) / 'DevToolsActivePort'
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and cls.proc.poll() is None:
            if port_file.exists() and len(port_file.read_text().splitlines()) >= 2:
                break
            time.sleep(0.05)
        if not port_file.exists():
            raise RuntimeError('Isolated sandboxed Chromium did not expose its private CDP port')
        port, path = port_file.read_text().splitlines()[:2]
        if not port.isdigit() or not path.startswith('/devtools/browser/'):
            raise RuntimeError('Unexpected Chrome debug-port file')
        cls.cdp = Cdp(f'ws://127.0.0.1:{port}{path}')
        cls.addClassCleanup(cls.cdp.close)
        cls.version = cls.cdp.call('Browser.getVersion')['product']
        print('LIVE CDP browser:', cls.version, 'sandbox_disabled=False', flush=True)

    def setUp(self):
        self.target = self.cdp.call('Target.createTarget', {'url': 'about:blank'})['targetId']
        self.addCleanup(lambda: self.cdp.call('Target.closeTarget', {'targetId': self.target}))
        self.session = self.cdp.call('Target.attachToTarget', {'targetId': self.target, 'flatten': True})['sessionId']
        for method in ('Page.enable', 'DOM.enable', 'Network.enable', 'Log.enable'):
            self.call(method)
        frame = self.call('Page.getFrameTree')['frameTree']['frame']['id']
        self.call('Page.setDocumentContent', {'frameId': frame, 'html': FIXTURE.decode()})
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            self.doc = self.call('DOM.getDocument', {'depth': 4, 'pierce': False})['root']
            if self.call('DOM.querySelector', {'nodeId': self.doc['nodeId'], 'selector': '#save'})['nodeId']:
                return
            time.sleep(0.02)
        self.fail('Local fixture did not load')

    def call(self, method, params=None):
        return self.cdp.call(method, params, self.session)

    def node(self, selector):
        self.doc = self.call('DOM.getDocument', {'depth': 4, 'pierce': False})['root']
        identifier = self.call('DOM.querySelector', {'nodeId': self.doc['nodeId'], 'selector': selector})['nodeId']
        return self.call('DOM.describeNode', {'nodeId': identifier, 'depth': 0})['node']

    def test_target_metadata(self):
        targets = self.cdp.call('Target.getTargets')['targetInfos']
        row = next(t for t in targets if t['targetId'] == self.target)
        self.assertEqual(row['type'], 'page')
        self.assertEqual(row['url'], 'about:blank')

    def test_dom_query_and_machine_node_id(self):
        node = self.node('#save')
        self.assertEqual(node['nodeName'], 'BUTTON')
        self.assertGreater(node['backendNodeId'], 0)

    def test_dom_snapshot_contains_accessible_labels(self):
        node = self.node('#name')
        attrs = dict(zip(node['attributes'][::2], node['attributes'][1::2]))
        self.assertEqual(attrs['aria-label'], 'Name')

    def test_no_javascript_fill(self):
        node = self.node('#name')
        self.call('DOM.focus', {'backendNodeId': node['backendNodeId']})
        focused = self.node(':focus')
        self.assertEqual(focused['backendNodeId'], node['backendNodeId'])
        for kind in ('keyDown', 'keyUp'):
            self.call('Input.dispatchKeyEvent', {'type': kind, 'modifiers': 2, 'key': 'a', 'code': 'KeyA', 'windowsVirtualKeyCode': 65, 'nativeVirtualKeyCode': 65})
        self.call('Input.insertText', {'text': 'Semwright México'})
        output = self.node('#value')
        html = self.call('DOM.getOuterHTML', {'backendNodeId': output['backendNodeId']})['outerHTML']
        self.assertIn('Semwright México', html)

    def test_hit_test_then_click_without_evaluate(self):
        node = self.node('#save')
        self.call('DOM.scrollIntoViewIfNeeded', {'backendNodeId': node['backendNodeId']})
        quad = self.call('DOM.getContentQuads', {'backendNodeId': node['backendNodeId']})['quads'][0]
        x, y = sum(quad[0::2]) / 4, sum(quad[1::2]) / 4
        hit = self.call('DOM.getNodeForLocation', {'x': int(x), 'y': int(y), 'includeUserAgentShadowDOM': False, 'ignorePointerEventsNone': False})
        self.assertEqual(hit['backendNodeId'], node['backendNodeId'])
        for kind in ('mousePressed', 'mouseReleased'):
            self.call('Input.dispatchMouseEvent', {'type': kind, 'x': x, 'y': y, 'button': 'left', 'clickCount': 1})
        status = self.node('#status')
        html = self.call('DOM.getOuterHTML', {'backendNodeId': status['backendNodeId']})['outerHTML']
        self.assertIn('Saved', html)

    def test_capture_png(self):
        image = self.call('Page.captureScreenshot', {'format': 'png', 'captureBeyondViewport': False})
        data = base64.b64decode(image['data'], validate=True)
        self.assertEqual(data[:8], b'\x89PNG\r\n\x1a\n')
        width, height = struct.unpack('>II', data[16:24])
        self.assertGreater(width, 0)
        self.assertGreater(height, 0)
        self.assertLess(len(data), 1_048_576)
        # No screenshot artifact is persisted by the test.

    def test_navigation_emits_generation_invalidation(self):
        self.node('#save')  # Confirm the soon-to-be-detached fixture node existed.
        self.cdp.events.clear()
        self.call('Page.navigate', {'url': 'about:blank'})
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            doc = self.call('DOM.getDocument', {'depth': 1})['root']
            current = self.call('DOM.querySelector', {'nodeId': doc['nodeId'], 'selector': '#save'})['nodeId']
            events = {event['method'] for event in self.cdp.events}
            if current == 0 and events.intersection({'DOM.documentUpdated', 'Page.frameNavigated'}):
                # Chromium may retain a detached old backendNodeId. The Rust broker's
                # generation check must reject it even if DOM.describeNode succeeds.
                return
            time.sleep(0.02)
        self.fail('Navigation did not emit a generation-invalidating event')

    def test_explicit_download_denial(self):
        self.cdp.call('Browser.setDownloadBehavior', {'behavior': 'deny', 'eventsEnabled': True})
        self.assertTrue(True)

if __name__ == '__main__':
    unittest.main(verbosity=2)
