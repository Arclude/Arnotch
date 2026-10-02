//! Minimal bindings for QApplication, which cxx-qt-lib does not provide.

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("app.h");

        type QApplication;

        #[cxx_name = "notchAppNew"]
        fn new_app() -> UniquePtr<QApplication>;

        #[cxx_name = "notchAppExec"]
        fn exec(app: Pin<&mut QApplication>) -> i32;
    }
}
