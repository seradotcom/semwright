#!/usr/bin/env python3
"""Disposable GTK4 accessibility fixture. No files, network or credentials are used."""
import gi
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk, GLib

GLib.set_application_name('SemwrightGtkFixture')
GLib.set_prgname('semwright-gtk-fixture')

class Fixture(Gtk.Application):
    def __init__(self):
        super().__init__(application_id='org.semwright.Fixture')
        self.connect('activate', self.activate_fixture)
        self.invocations = 0

    def activate_fixture(self, _app):
        window = Gtk.ApplicationWindow(application=self, title='Semwright GTK fixture')
        window.set_default_size(640, 600)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        for name in ('top', 'bottom', 'start', 'end'):
            getattr(box, 'set_margin_' + name)(16)
        window.set_child(box)
        box.append(Gtk.Label(label='Disposable fixture — buttons do not save real files.'))
        entry = Gtk.Entry()
        entry.update_property([Gtk.AccessibleProperty.LABEL], ['Filename'])
        box.append(entry)
        password = Gtk.PasswordEntry()
        password.update_property([Gtk.AccessibleProperty.LABEL], ['Secret fixture input'])
        box.append(password)
        self.status = Gtk.Label(label='Ready')
        box.append(self.status)
        export = Gtk.Button(label='Export')
        export.connect('clicked', lambda _button: self.mark('Export'))
        box.append(export)
        for name in ('Document A', 'Document B'):
            frame = Gtk.Frame(label=name)
            save = Gtk.Button(label='Save')
            save.connect('clicked', lambda _button, context=name: self.mark(context))
            frame.set_child(save)
            box.append(frame)
        enabled = Gtk.CheckButton(label='Enabled')
        enabled.set_active(True)
        box.append(enabled)
        disabled = Gtk.Button(label='Unavailable action')
        disabled.set_sensitive(False)
        box.append(disabled)
        scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        scale.set_value(25)
        scale.update_property([Gtk.AccessibleProperty.LABEL], ['Level'])
        box.append(scale)
        options = Gtk.DropDown.new_from_strings(['First', 'Second', 'Third'])
        options.update_property([Gtk.AccessibleProperty.LABEL], ['Choice'])
        box.append(options)
        self.dynamic = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.dynamic.append(Gtk.Button(label='Replaceable'))
        box.append(self.dynamic)
        replace = Gtk.Button(label='Replace control')
        replace.connect('clicked', self.replace_control)
        box.append(replace)
        slow = Gtk.Button(label='Delayed status')
        slow.connect('clicked', self.delayed)
        box.append(slow)
        window.present()

    def mark(self, name):
        self.invocations += 1
        self.status.set_label(f'{name}: invocation {self.invocations}')

    def replace_control(self, _button):
        self.dynamic.remove(self.dynamic.get_first_child())
        self.dynamic.append(Gtk.Button(label='Replaceable'))
        self.status.set_label('Control object replaced, same visible label')

    def delayed(self, _button):
        self.status.set_label('Working')
        def done():
            self.status.set_label('Delayed result ready')
            return False
        GLib.timeout_add(500, done)

if __name__ == '__main__':
    raise SystemExit(Fixture().run(None))
