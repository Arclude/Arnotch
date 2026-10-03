#pragma once

#include <QApplication>
#include <QQmlApplicationEngine>
#include <memory>

// QApplication (not QGuiApplication): the system tray menu on KDE is a QMenu widget.
std::unique_ptr<QApplication> notchAppNew();
int notchAppExec(QApplication &app);
// False when the main QML file failed to load (the errors are already logged).
bool notchEngineLoaded(const QQmlApplicationEngine &engine);
