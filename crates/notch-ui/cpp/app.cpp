#include "app.h"

#include <QIcon>
#include <QLocale>
#include <QQuickWindow>
#include <QRegion>
#include <QTranslator>

namespace {

// Limits a window's input to the rect in its QML "inputRect" property, so the
// transparent rest of the window lets clicks through to the windows below.
// QML can't set a window mask itself; this applies it after every frame.
class InputMaskFilter : public QObject
{
public:
    using QObject::QObject;

    bool eventFilter(QObject *watched, QEvent *event) override
    {
        auto *window = qobject_cast<QQuickWindow *>(watched);
        if (event->type() != QEvent::Show || !window || window->property("inputRect").isNull()
            || window->property("notchMaskHooked").toBool())
            return false;
        window->setProperty("notchMaskHooked", true);
        connect(window, &QQuickWindow::frameSwapped, window, [window] {
            const QRect rect = window->property("inputRect").toRectF().toAlignedRect();
            if (window->mask() != QRegion(rect))
                window->setMask(rect);
        });
        return false;
    }
};

} // namespace

std::unique_ptr<QApplication> notchAppNew()
{
    // QApplication keeps references to argc/argv, so they must outlive it.
    static int argc = 1;
    static char name[] = "notch";
    static char *argv[] = {name, nullptr};

    // Qt sends warnings to the journal when stderr isn't a terminal; keep them
    // next to notch's own log so QML errors are never silent.
    if (qEnvironmentVariableIsEmpty("QT_FORCE_STDERR_LOGGING"))
        qputenv("QT_FORCE_STDERR_LOGGING", "1");

    // Settings are stored in ~/.config/notch/notch.conf
    QCoreApplication::setOrganizationName(QStringLiteral("notch"));
    QCoreApplication::setApplicationName(QStringLiteral("notch"));

    // The island is a small surface: Qt's software renderer needs about half the memory
    // of a GPU context and less CPU. NOTCH_RENDERER=gpu (or QT_QUICK_BACKEND) opts out.
    if (qEnvironmentVariableIsEmpty("QT_QUICK_BACKEND") && qgetenv("NOTCH_RENDERER") != "gpu")
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);

    auto app = std::make_unique<QApplication>(argc, argv);
    app->setWindowIcon(QIcon(QStringLiteral(":/icons/notch.svg")));

    app->installEventFilter(new InputMaskFilter(app.get()));

    // The islands live on even when the settings window is closed.
    app->setQuitOnLastWindowClosed(false);

    // The interface in the user's language, when notch has a translation for it.
    static QTranslator translator;
    if (translator.load(QLocale(), QStringLiteral("notch"), QStringLiteral("_"), QStringLiteral(":/i18n")))
        QCoreApplication::installTranslator(&translator);
    return app;
}

bool notchEngineLoaded(const QQmlApplicationEngine &engine)
{
    return !engine.rootObjects().isEmpty();
}

int notchAppExec(QApplication &app)
{
    return app.exec();
}
