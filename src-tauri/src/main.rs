#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--execution-worker") {
        if args.len() != 3 {
            std::process::exit(2);
        }
        if let Err(error) = cloudmcp_lib::run_execution_worker(&args[1], &args[2]) {
            eprintln!("{error}");
            std::process::exit(1);
        }
    } else if args.first().is_some_and(|arg| arg == "--headless") {
        if args.len() != 2 {
            eprintln!("Usage: cloudmcp --headless /path/to/config.json");
            std::process::exit(2);
        }
        if let Err(error) = cloudmcp_lib::run_headless(&args[1]) {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    } else {
        cloudmcp_lib::run();
    }
}
