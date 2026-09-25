#!/usr/bin/env python3
"""Disposable Qt accessibility fixture for hosted AT-SPI conformance."""
import sys

from PyQt6.QtCore import QLibraryInfo, QTimer, QT_VERSION_STR, Qt
from PyQt6.QtWidgets import (
    QApplication,
    QLabel,
    QLineEdit,
    QListWidget,
    QPushButton,
    QSlider,
    QTableWidget,
    QTableWidgetItem,
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
label = QLabel("Filename")
entry = QLineEdit()
entry.setAccessibleName("Semwright editable entry")
entry.setAccessibleDescription(
    "Editable field used by the Semwright AT-SPI conformance test"
)
entry.setPlaceholderText("Type here")
label.setBuddy(entry)

password = QLineEdit()
password.setEchoMode(QLineEdit.EchoMode.Password)
password.setAccessibleName("Secret fixture input")
password.setAccessibleDescription("Protected field for redaction conformance")

slider = QSlider(Qt.Orientation.Horizontal)
slider.setRange(0, 100)
slider.setValue(25)
slider.setAccessibleName("Level")

choices = QListWidget()
choices.setAccessibleName("Semwright choices")
choices.addItems(["Alpha", "Beta", "Gamma"])
choices.setCurrentRow(1)

table = QTableWidget(2, 2)
table.setAccessibleName("Semwright data table")
table.setHorizontalHeaderLabels(["Name", "Value"])
table.setVerticalHeaderLabels(["Row 1", "Row 2"])
table.setItem(0, 0, QTableWidgetItem("Alpha"))
table.setItem(0, 1, QTableWidgetItem("10"))
table.setItem(1, 0, QTableWidgetItem("Beta"))
table.setItem(1, 1, QTableWidgetItem("20"))
table.setCurrentCell(1, 1)

export = QPushButton("Export")
export.setAccessibleName("Export")

layout.addWidget(label)
layout.addWidget(entry)
layout.addWidget(password)
layout.addWidget(slider)
layout.addWidget(choices)
layout.addWidget(table)
layout.addWidget(export)
window.resize(520, 520)
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
