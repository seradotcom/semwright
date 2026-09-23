#include <QAccessible>
#include <QApplication>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>
#include <QWidget>

#include <iostream>

int main(int argc, char **argv) {
    // Set the test-only accessibility policy before Qt creates its platform
    // integration. The real backend never mutates another application's state.
    qputenv("QT_ACCESSIBILITY", "1");
    qputenv("QT_LINUX_ACCESSIBILITY_ALWAYS_ON", "1");
    QCoreApplication::setApplicationName("SemwrightQtFixture");
    QApplication app(argc, argv);
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

    // Force creation of QXcbIntegration's accessibility bridge only in this
    // conformance fixture. Qt exposes setActive/setRootObject as static accessibility
    // hooks; production applications remain completely untouched by Semwright.
    QAccessible::setActive(true);
    QAccessible::setRootObject(&app);
    QAccessibleEvent shown(&window, QAccessible::ObjectShow);
    QAccessible::updateAccessibility(&shown);

    // Keep the fixture on the same event-loop path as a real Qt desktop application.
    QTimer::singleShot(250, [&app, &window]() {
        auto *app_root = QAccessible::queryAccessibleInterface(&app);
        auto *window_root = QAccessible::queryAccessibleInterface(&window);
        std::cerr << "qt_accessibility_active="
                  << (QAccessible::isActive() ? "true" : "false")
                  << " app_root=" << (app_root != nullptr ? "present" : "missing")
                  << " window_root=" << (window_root != nullptr ? "present" : "missing")
                  << " platform=" << QGuiApplication::platformName().toStdString()
                  << " atspi_bus="
                  << (!qEnvironmentVariableIsEmpty("AT_SPI_BUS_ADDRESS") ? "set" : "missing")
                  << std::endl;
    });

    return app.exec();
}
