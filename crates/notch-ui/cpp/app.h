#pragma once

#include <QApplication>
#include <memory>

// QApplication (not QGuiApplication): the system tray menu on KDE is a QMenu widget.
std::unique_ptr<QApplication> notchAppNew();
int notchAppExec(QApplication &app);
