#include "app.h"

#include <QAnimationDriver>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QIcon>
#include <QLocale>
#include <QQuickWindow>
#include <QRegion>
#include <QScreen>
#include <QStandardPaths>
#include <QTimer>
#include <QTranslator>

namespace {

// Qt's software renderer advances animations from its own 16 ms timer, out of step with
// the display: on a 144 or 180 Hz screen frames came in bunches (two within a millisecond,
// then a 15 ms gap), which shows as stutter. This driver advances animations when the
// compositor is ready for a window's next frame (its update request, which Qt on Wayland
// holds back until the frame callback), just before that frame is drawn, and asks for
// another one after it is shown, so the compositor sets the pace. The animations' clock
// moves one refresh interval per frame rather than by the wall clock: when that request
// comes varies by a millisecond or two within a refresh, and the motion would vary with it.
// It catches up in whole intervals when frames are skipped. When nothing changes on screen
// no frame is shown; a short watchdog keeps the animations going then.
class FrameDriver : public QAnimationDriver
{
public:
    explicit FrameDriver(QObject *parent)
        : QAnimationDriver(parent)
    {
        watchdog.setSingleShot(true);
        watchdog.setTimerType(Qt::PreciseTimer);
        connect(&watchdog, &QTimer::timeout, this, &FrameDriver::requestFrames);
    }

    bool eventFilter(QObject *watched, QEvent *event) override
    {
        auto *window = qobject_cast<QQuickWindow *>(watched);
        if (!window)
            return false;
        if (event->type() == QEvent::UpdateRequest && isRunning()) {
            // Advanced before the window handles the request, so the frame it draws now
            // already has the animations' new state.
            step(window);
            advance();
            if (isRunning())
                watchdog.start(watchdogMs());
        } else if (event->type() == QEvent::Show && !window->property("notchFramesHooked").toBool()) {
            window->setProperty("notchFramesHooked", true);
            connect(window, &QQuickWindow::frameSwapped, this, [this, window] {
                // Throttled by Qt until the compositor wants the next frame.
                if (isRunning())
                    window->requestUpdate();
            });
        }
        return false;
    }

    qint64 elapsed() const override
    {
        return qRound64(qMax<qreal>(time, 0));
    }

protected:
    void start() override
    {
        clock.start();
        time = -1;
        QAnimationDriver::start();
        requestFrames();
    }

    void stop() override
    {
        watchdog.stop();
        QAnimationDriver::stop();
    }

private:
    void step(const QQuickWindow *window)
    {
        const qreal hz = window->screen() ? window->screen()->refreshRate() : 60;
        const qreal interval = 1000 / (hz > 0 ? hz : 60);
        const qreal now = clock.nsecsElapsed() / 1e6;
        if (time < 0) { // the first frame since start()
            time = now;
            return;
        }
        qreal next = time + interval;
        while (next < now - interval * 1.5)
            next += interval;
        // Ahead, as when windows on two screens both draw: follow the wall clock.
        if (next > now + interval)
            next = qMax(time, now);
        time = next;
    }

    void requestFrames()
    {
        for (QWindow *window : QGuiApplication::topLevelWindows()) {
            if (window->isVisible() && qobject_cast<QQuickWindow *>(window))
                window->requestUpdate();
        }
        watchdog.start(watchdogMs());
    }

    // A little over two refresh intervals of the fastest screen.
    static int watchdogMs()
    {
        static const int ms = [] {
            qreal hz = 60;
            for (const QScreen *screen : QGuiApplication::screens())
                hz = qMax(hz, screen->refreshRate());
            return qMax(4, int(2000 / hz) + 1);
        }();
        return ms;
    }

    QTimer watchdog;
    QElapsedTimer clock;
    qreal time = -1; // ms since start()
};

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
    // Each frame is drawn whole. With partial updates, what the island clips away as it
    // shrinks was sometimes left behind in the transparent part of the window, as grey
    // streaks under the closed island. The window is small; drawing it all costs little.
    if (!qEnvironmentVariableIsSet("QSG_SOFTWARE_RENDERER_FORCE_PARTIAL_UPDATES"))
        qputenv("QSG_SOFTWARE_RENDERER_FORCE_PARTIAL_UPDATES", "0");

    auto app = std::make_unique<QApplication>(argc, argv);
    app->setWindowIcon(QIcon(QStringLiteral(":/icons/arnotch.svg")));

    app->installEventFilter(new InputMaskFilter(app.get()));
    if (QQuickWindow::sceneGraphBackend() == QLatin1String("software")
        && qEnvironmentVariableIsEmpty("ARNOTCH_QT_ANIMATIONS")) {
        auto *driver = new FrameDriver(app.get());
        app->installEventFilter(driver);
        driver->install();
    }

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
