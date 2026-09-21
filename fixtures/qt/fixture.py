#!/usr/bin/env python3
"""Disposable Qt6 accessibility fixture; does not open files or use the network."""
import sys
from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout, QLabel, QLineEdit, QPushButton, QCheckBox, QSlider, QComboBox, QGroupBox

class Fixture(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle('Semwright Qt fixture')
        self.resize(640, 620)
        self.count = 0
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel('Disposable fixture — no real files are saved.'))
        entry = QLineEdit('fixture.txt')
        entry.setAccessibleName('Filename')
        layout.addWidget(entry)
        secret = QLineEdit()
        secret.setEchoMode(QLineEdit.EchoMode.Password)
        secret.setAccessibleName('Secret fixture input')
        layout.addWidget(secret)
        self.status = QLabel('Ready')
        layout.addWidget(self.status)
        export = QPushButton('Export')
        export.clicked.connect(lambda: self.mark('Export'))
        layout.addWidget(export)
        for name in ('Document A', 'Document B'):
            group = QGroupBox(name)
            inside = QVBoxLayout(group)
            save = QPushButton('Save')
            save.clicked.connect(lambda _checked=False, context=name: self.mark(context))
            inside.addWidget(save)
            layout.addWidget(group)
        checkbox = QCheckBox('Enabled')
        checkbox.setChecked(True)
        layout.addWidget(checkbox)
        disabled = QPushButton('Unavailable action')
        disabled.setEnabled(False)
        layout.addWidget(disabled)
        slider = QSlider(Qt.Orientation.Horizontal)
        slider.setRange(0, 100)
        slider.setValue(25)
        slider.setAccessibleName('Level')
        layout.addWidget(slider)
        options = QComboBox()
        options.addItems(['First', 'Second', 'Third'])
        options.setAccessibleName('Choice')
        layout.addWidget(options)
        self.dynamic = QVBoxLayout()
        self.replaceable = QPushButton('Replaceable')
        self.dynamic.addWidget(self.replaceable)
        layout.addLayout(self.dynamic)
        replace = QPushButton('Replace control')
        replace.clicked.connect(self.replace_control)
        layout.addWidget(replace)
        delayed = QPushButton('Delayed status')
        delayed.clicked.connect(self.delayed)
        layout.addWidget(delayed)

    def mark(self, name):
        self.count += 1
        self.status.setText(f'{name}: invocation {self.count}')

    def replace_control(self):
        self.dynamic.removeWidget(self.replaceable)
        self.replaceable.deleteLater()
        self.replaceable = QPushButton('Replaceable')
        self.dynamic.addWidget(self.replaceable)
        self.status.setText('Control object replaced, same visible label')

    def delayed(self):
        self.status.setText('Working')
        QTimer.singleShot(500, lambda: self.status.setText('Delayed result ready'))

if __name__ == '__main__':
    app = QApplication(sys.argv)
    app.setApplicationName('SemwrightFixture')
    fixture = Fixture()
    fixture.show()
    raise SystemExit(app.exec())
