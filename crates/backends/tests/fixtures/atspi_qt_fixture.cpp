#include <QAccessible>
#include <QApplication>
#include <QDBusConnection>
#include <QDBusError>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTimer>
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

    // QApplication publishes its accessibility root when entering exec(). Keep the
    // fixture on the same initialization path as a real Qt desktop application.
    QTimer::singleShot(250, [&app, &window]() {
        const QByteArray address = qgetenv("AT_SPI_BUS_ADDRESS");
        auto probe = QDBusConnection::connectToBus(
            QString::fromLocal8Bit(address), "semwright_atspi_fixture_probe");
        const auto probe_error = probe.lastError();
        std::cerr << "qt_dbus_probe_connected=" << (probe.isConnected() ? "true" : "false")
                  << " qt_dbus_probe_error="
                  << probe_error.name().toStdString() << ":"
                  << probe_error.message().toStdString() << std::endl;
        QDBusConnection::disconnectFromBus("semwright_atspi_fixture_probe");
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
