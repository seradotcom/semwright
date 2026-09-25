#!/usr/bin/env python3
"""Disposable Qt accessibility fixture for hosted AT-SPI conformance."""
import sys

from PyQt6.QtCore import QLibraryInfo, QTimer, QT_VERSION_STR, Qt
from PyQt6.QtWidgets import (
    QApplication,
    QLabel,
    QLineEdit,
    QPushButton,
    QSlider,
    QVBoxLayout,
    QWidget,
)


app = QApplication(sys.argv)
app.setApplicationName("SemwrightQtFixture")
app.setApplicationDisplayName("Semwright Qt AT-SPI Fixture")

window = QWidget()
window.setWindowTitle("Semwright Qt AT-SPI Fixture")
window.setAccessibleName("Semwright Qt AT-SPI Fixture")

layout = QVBoxLayout(window)
label = QLabel("Disposable accessibility fixture")
entry = QLineEdit()
entry.setAccessibleName("Semwright editable entry")
entry.setAccessibleDescription(
    "Editable field used by the Semwright AT-SPI conformance test"
)
entry.setPlaceholderText("Type here")

password = QLineEdit()
password.setEchoMode(QLineEdit.EchoMode.Password)
password.setAccessibleName("Secret fixture input")
password.setAccessibleDescription("Protected field for redaction conformance")

slider = QSlider(Qt.Orientation.Horizontal)
slider.setRange(0, 100)
slider.setValue(25)
slider.setAccessibleName("Level")

export = QPushButton("Export")
export.setAccessibleName("Export")

layout.addWidget(label)
layout.addWidget(entry)
layout.addWidget(password)
layout.addWidget(slider)
layout.addWidget(export)
window.resize(360, 260)
window.show()


def report_ready() -> None:
    print(
        "qt_fixture_ready"
        f" qt={QT_VERSION_STR}"
        f" plugins={QLibraryInfo.path(QLibraryInfo.LibraryPath.PluginsPath)}",
        flush=True,
    )


QTimer.singleShot(250, report_ready)
raise SystemExit(app.exec())
