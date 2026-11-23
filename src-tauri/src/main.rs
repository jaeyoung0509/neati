// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Console actions (`--doctor`) run before Tauri starts, so they work in
    // headless CI with no window, no runtime, and no WebView. The default path
    // is unchanged: no arguments means the application starts normally.
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `neati <command>` (#498) runs the terminal interface the same way: only a
    // recognised command word is the CLI's; every other launch is the app's.
    if let Some(exit_code) = neati_lib::cli::run(&args) {
        std::process::exit(exit_code);
    }
    if let Some(exit_code) = neati_lib::diagnostics::doctor::run_cli(&args) {
        std::process::exit(exit_code);
    }
    neati_lib::run();
}
