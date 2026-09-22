#!/usr/bin/env python3
from PyQt6.QtWidgets import QApplication, QLabel, QLineEdit, QPushButton, QVBoxLayout, QWidget

app = QApplication([])
app.setApplicationName("SemwrightQtFixture")
app.setApplicationDisplayName("Semwright Qt AT-SPI Fixture")

window = QWidget()
window.setWindowTitle("Semwright Qt AT-SPI Fixture")
window.setAccessibleName("Semwright Qt AT-SPI Fixture")
layout = QVBoxLayout(window)

label = QLabel("Disposable accessibility fixture")
entry = QLineEdit()
entry.setAccessibleName("Semwright editable entry")
entry.setAccessibleDescription("Editable field used by the Semwright AT-SPI conformance test")
entry.setPlaceholderText("Type here")
button = QPushButton("Close")

layout.addWidget(label)
layout.addWidget(entry)
layout.addWidget(button)
button.clicked.connect(window.close)

window.resize(360, 160)
window.show()
app.exec()
