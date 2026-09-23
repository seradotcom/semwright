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

    // Let Qt initialize and publish its accessibility bridge through its normal
    // platform path. The harness toggles org.a11y.Status after startup so older Qt
    // releases cannot lose the initial enabledChanged notification during construction.
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
