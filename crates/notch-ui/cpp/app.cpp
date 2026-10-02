#include "app.h"

#include <QQuickWindow>

std::unique_ptr<QApplication> notchAppNew()
{
    // QApplication keeps references to argc/argv, so they must outlive it.
    static int argc = 1;
    static char name[] = "notch";
    static char *argv[] = {name, nullptr};

    // Settings are stored in ~/.config/notch/notch.conf
    QCoreApplication::setOrganizationName(QStringLiteral("notch"));
    QCoreApplication::setApplicationName(QStringLiteral("notch"));

    // The island is a small surface: Qt's software renderer needs about half the memory
    // of a GPU context and less CPU. NOTCH_RENDERER=gpu (or QT_QUICK_BACKEND) opts out.
    if (qEnvironmentVariableIsEmpty("QT_QUICK_BACKEND") && qgetenv("NOTCH_RENDERER") != "gpu")
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);

    auto app = std::make_unique<QApplication>(argc, argv);
    // The islands live on even when the settings window is closed.
    app->setQuitOnLastWindowClosed(false);
    return app;
}

int notchAppExec(QApplication &app)
{
    return app.exec();
}
