//! Minimal bindings for QApplication, which cxx-qt-lib does not provide.

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("app.h");

        type QApplication;

        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;

        #[cxx_name = "notchAppNew"]
        fn new_app() -> UniquePtr<QApplication>;

        #[cxx_name = "notchAppExec"]
        fn exec(app: Pin<&mut QApplication>) -> i32;

        #[cxx_name = "notchEngineLoaded"]
        fn engine_loaded(engine: &QQmlApplicationEngine) -> bool;
    }
}
