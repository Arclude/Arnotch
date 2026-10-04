use std::path::PathBuf;
use std::process::Command;

use cxx_qt_build::{CxxQtBuilder, QResource, QResources, QmlModule};
use qt_build_utils::QResourceFile;

/// Languages with a translation in i18n/arnotch_<lang>.ts.
const LANGUAGES: &[&str] = &["tr"];

fn main() {
    let mut builder = CxxQtBuilder::new_qml_module(QmlModule::new("dev.arnotch").qml_files([
        "qml/Main.qml",
        "qml/NotchState.qml",
        "qml/Island.qml",
        "qml/SettingsWindow.qml",
        "qml/Face.qml",
        "qml/SessionRow.qml",
        "qml/ActivityCard.qml",
        "qml/TerminalsView.qml",
        "qml/IconButton.qml",
        "qml/PillButton.qml",
        "qml/SendField.qml",
        "qml/DropTargets.qml",
        "qml/ShelfView.qml",
        "qml/ChatView.qml",
        "qml/MediaCard.qml",
        "qml/LevelIndicator.qml",
        "qml/MeetingRow.qml",
        "qml/PromptPanel.qml",
        "qml/LimitsRow.qml",
        "qml/AttachDialog.qml",
        "qml/TimerRow.qml",
        "qml/Spring.qml",
        "qml/PhoneView.qml",
    ]))
    .qt_module("Gui")
    .qt_module("Quick")
    .qt_module("Widgets")
    .include_dir("cpp")
    .cpp_file("cpp/app.cpp")
    // Explicit "/" prefix: resources of a QML module default to /qt/qml/<module>.
    .qrc_resources(
        QResources::new().resource(QResource::new().prefix("/").file("icons/arnotch.svg")),
    )
    .files(["src/app.rs", "src/bridge.rs"]);

    if let Some(translations) = compile_translations() {
        builder = builder.qrc_resources(translations);
    }
    builder.build();
}

/// Compiles the .ts files with lrelease into :/i18n/arnotch_<lang>.qm.
/// Without lrelease (Qt's linguist tools) the notch builds in English only.
fn compile_translations() -> Option<QResources> {
    let lrelease = lrelease()?;
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("i18n");
    std::fs::create_dir_all(&out_dir).unwrap();

    let mut resource = QResource::new().prefix("/");
    for lang in LANGUAGES {
        let ts = format!("i18n/arnotch_{lang}.ts");
        let qm = out_dir.join(format!("arnotch_{lang}.qm"));
        println!("cargo::rerun-if-changed={ts}");
        let status = Command::new(&lrelease)
            .arg("-silent")
            .arg(&ts)
            .arg("-qm")
            .arg(&qm)
            .status();
        if !status.is_ok_and(|s| s.success()) {
            println!("cargo::warning=lrelease failed for {ts}; skipping translations");
            return None;
        }
        resource = resource.file(QResourceFile::new(&qm).alias(format!("i18n/arnotch_{lang}.qm")));
    }
    Some(QResources::new().resource(resource))
}

fn lrelease() -> Option<PathBuf> {
    // Qt keeps its tools in QT_HOST_BINS, which is often not on PATH.
    let from_qmake = ["qmake6", "qmake"].iter().find_map(|qmake| {
        let output = Command::new(qmake)
            .args(["-query", "QT_HOST_BINS"])
            .output()
            .ok()?;
        let bins = String::from_utf8(output.stdout).ok()?;
        let path = PathBuf::from(bins.trim()).join("lrelease");
        path.exists().then_some(path)
    });
    let found = from_qmake.or_else(|| {
        Command::new("lrelease")
            .arg("-version")
            .output()
            .ok()
            .map(|_| PathBuf::from("lrelease"))
    });
    if found.is_none() {
        println!(
            "cargo::warning=lrelease not found (install Qt's linguist tools); building without translations"
        );
    }
    found
}
