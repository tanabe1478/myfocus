// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--myfocus-tool") {
        if let Err(error) = myfocus_lib::run_tool_cli(&args[2..]) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    myfocus_lib::run()
}
