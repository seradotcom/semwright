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
    const QByteArray atspi_address = qgetenv("AT_SPI_BUS_ADDRESS");
    auto atspi_connection = QDBusConnection::connectToBus(
        QString::fromLocal8Bit(atspi_address), "a11y");
    if (!atspi_connection.isConnected()) {
        const auto error = atspi_connection.lastError();
        std::cerr << "qt_atspi_preconnect=false error="
                  << error.name().toStdString() << ":"
                  << error.message().toStdString() << std::endl;
        return 3;
    }
    std::cerr << "qt_atspi_preconnect=true" << std::endl;
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
