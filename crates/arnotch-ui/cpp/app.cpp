#include "app.h"

#include <QDir>
#include <QFile>
#include <QIcon>
#include <QLocale>
#include <QQuickWindow>
#include <QRegion>
#include <QStandardPaths>
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
    static char name[] = "arnotch";
    static char *argv[] = {name, nullptr};

    // Qt sends warnings to the journal when stderr isn't a terminal; keep them
    // next to notch's own log so QML errors are never silent.
    if (qEnvironmentVariableIsEmpty("QT_FORCE_STDERR_LOGGING"))
        qputenv("QT_FORCE_STDERR_LOGGING", "1");

    // Settings are stored in ~/.config/arnotch/arnotch.conf
    QCoreApplication::setOrganizationName(QStringLiteral("arnotch"));
    QCoreApplication::setApplicationName(QStringLiteral("arnotch"));

    // arnotch used to be called "notch": its settings and shelf come along.
    const QString config = QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation);
    const QString settings = config + QStringLiteral("/arnotch/arnotch.conf");
    const QString old = config + QStringLiteral("/notch/notch.conf");
    if (!QFile::exists(settings) && QFile::exists(old) && QDir().mkpath(config + QStringLiteral("/arnotch")))
        QFile::copy(old, settings);

    // The island is a small surface: Qt's software renderer needs about half the memory
    // of a GPU context and less CPU. ARNOTCH_RENDERER=gpu (or QT_QUICK_BACKEND) opts out.
    if (qEnvironmentVariableIsEmpty("QT_QUICK_BACKEND") && qgetenv("ARNOTCH_RENDERER") != "gpu")
        QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);

    auto app = std::make_unique<QApplication>(argc, argv);
    app->setWindowIcon(QIcon(QStringLiteral(":/icons/arnotch.svg")));

    app->installEventFilter(new InputMaskFilter(app.get()));

    // The islands live on even when the settings window is closed.
    app->setQuitOnLastWindowClosed(false);

    // The interface in the user's language, when notch has a translation for it.
    static QTranslator translator;
    if (translator.load(QLocale(), QStringLiteral("arnotch"), QStringLiteral("_"), QStringLiteral(":/i18n")))
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
