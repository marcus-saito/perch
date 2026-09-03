// Windows would otherwise open a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    perch_app_lib::run()
}
