#include <QAccessible>
#include <QApplication>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QVBoxLayout>
#include <QWidget>

#include <iostream>

int main(int argc, char **argv) {
    QApplication app(argc, argv);
    QCoreApplication::setApplicationName("SemwrightQtFixture");
    QApplication::setApplicationDisplayName("Semwright Qt AT-SPI Fixture");

    QWidget window;
    window.setWindowTitle("Semwright Qt AT-SPI Fixture");
    window.setAccessibleName("Semwright Qt AT-SPI Fixture");

    QVBoxLayout layout(&window);
    QLabel label("Disposable accessibility fixture");
    QLineEdit entry;
    entry.setAccessibleName("Semwright editable entry");
    entry.setAccessibleDescription("Editable field used by the Semwright AT-SPI conformance test");
    entry.setPlaceholderText("Type here");
    QPushButton button("Close");

    layout.addWidget(&label);
    layout.addWidget(&entry);
    layout.addWidget(&button);
    QObject::connect(&button, &QPushButton::clicked, &window, &QWidget::close);

    window.resize(360, 160);
    window.show();

    QAccessible::setRootObject(&app);
    auto *root = QAccessible::queryAccessibleInterface(&app);
    std::cerr << "qt_accessibility_active=" << (QAccessible::isActive() ? "true" : "false")
              << " root_interface=" << (root != nullptr ? "present" : "missing") << std::endl;

    return app.exec();
}
