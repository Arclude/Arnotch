use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("dev.notch").qml_files([
        "qml/Main.qml",
        "qml/NotchState.qml",
        "qml/Island.qml",
        "qml/SettingsWindow.qml",
        "qml/Face.qml",
        "qml/SessionRow.qml",
        "qml/ActivityCard.qml",
    ]))
    .qt_module("Gui")
    .qt_module("Quick")
    .qt_module("Widgets")
    .include_dir("cpp")
    .cpp_file("cpp/app.cpp")
    .qrc_resources(["icons/notch.svg"])
    .files(["src/app.rs", "src/bridge.rs"])
    .build();
}
