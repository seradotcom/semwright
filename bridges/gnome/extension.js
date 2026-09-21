import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {validateOperation, exactWindow} from './contract.js';

const BROKER = 'org.semwright.Broker';
const NAME = 'org.semwright.GnomeBridge';
const PATH = '/org/semwright/GnomeBridge';
const XML = `<node><interface name="org.semwright.WindowBridge1">
  <method name="Hello"><arg type="s" direction="out"/></method>
  <method name="Snapshot"><arg type="s" direction="out"/></method>
  <method name="Execute"><arg type="s" direction="in"/><arg type="s" direction="out"/></method>
</interface></node>`;

export default class SemwrightExtension extends Extension {
    enable() {
        this._epoch = GLib.uuid_string_random();
        this._tracker = Shell.WindowTracker.get_default();
        this._exported = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._exported.export(Gio.DBus.session, PATH);
        this._owner = Gio.bus_own_name_on_connection(Gio.DBus.session, NAME,
            Gio.BusNameOwnerFlags.NONE, null, null);
    }
    disable() {
        if (this._exported) this._exported.unexport();
        if (this._owner) Gio.bus_unown_name(this._owner);
        this._exported = null;
        this._owner = null;
        this._tracker = null;
    }
    _describe(window) {
        const app = this._tracker.get_window_app(window)?.get_id() ?? window.get_wm_class() ?? '';
        const rect = window.get_frame_rect();
        return {id: String(window.get_stable_sequence()), app,
            fingerprint: `${this._epoch}:${window.get_pid()}:${app}`,
            title: (window.get_title() ?? '').slice(0, 4096),
            focused: global.display.focus_window === window,
            bounds: {x: rect.x, y: rect.y, width: rect.width, height: rect.height},
            coordinate_space: 'compositor_logical'};
    }
    _windows() { return global.get_window_actors().map(actor => actor.meta_window).filter(window => !window.override_redirect); }
    _authorized(invocation, operation) {
        const sender = invocation.get_sender();
        Gio.DBus.session.call('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'GetNameOwner', new GLib.Variant('(s)', [BROKER]),
            new GLib.VariantType('(s)'), Gio.DBusCallFlags.NONE, 2000, null,
            (connection, result) => {
                try {
                    const [owner] = connection.call_finish(result).deepUnpack();
                    if (owner !== sender || !this._exported) throw new Error('PermissionDenied');
                    const output = operation();
                    invocation.return_value(new GLib.Variant('(s)', [JSON.stringify(output)]));
                } catch (error) {
                    const safe = ['PermissionDenied', 'InvalidArgument', 'Unsupported', 'StaleReference', 'AmbiguousTarget'].includes(error.message)
                        ? error.message : 'BackendFailed';
                    invocation.return_dbus_error(`org.semwright.Error.${safe}`, safe);
                }
            });
    }
    HelloAsync(_parameters, invocation) {
        this._authorized(invocation, () => ({version: 1, epoch: this._epoch, operations: ['focus', 'move', 'resize', 'close']}));
    }
    SnapshotAsync(_parameters, invocation) {
        this._authorized(invocation, () => ({version: 1, windows: this._windows().slice(0, 2000).map(window => this._describe(window))}));
    }
    ExecuteAsync([raw], invocation) {
        this._authorized(invocation, () => {
            const command = validateOperation(raw);
            const window = exactWindow(this._windows(), command, value => this._describe(value));
            const time = global.get_current_time();
            switch (command.operation) {
            case 'focus': window.activate(time); break;
            case 'move': window.move_frame(true, command.x, command.y); break;
            case 'resize': {
                const rect = window.get_frame_rect();
                window.move_resize_frame(true, rect.x, rect.y, command.width, command.height);
                break;
            }
            case 'close': window.delete(time); break;
            default: throw new Error('Unsupported');
            }
            return {accepted: true, id: command.id};
        });
    }
}
