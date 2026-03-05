#![expect(clippy::multiple_crate_versions)]
use thrum::{CliAction, parse_args, print_help, run};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        CliAction::Help => {
            print_help();
            Ok(())
        }
        CliAction::Version => {
            println!("thrum {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliAction::Error(msg) => {
            eprintln!("error: {msg}");
            std::process::exit(1);
        }
        CliAction::Config(cfg) => run(cfg),
    }
}
