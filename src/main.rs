use mcnp_input_parser::{parse_mcnp_input, parse_mcnp_input_lossy, validate_mcnp_input, Severity};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

fn print_usage(program: &str) {
    eprintln!("Usage: {} [--summary] [--validate] [FILE]", program);
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  [FILE]       Path to MCNP input file (reads from stdin if omitted)");
    eprintln!("Options:");
    eprintln!("  --summary    Print a human-readable summary instead of full JSON");
    eprintln!("  --validate   Run syntax and semantic validation and report all errors/warnings");
    eprintln!("  -h, --help   Print this help message");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args.first().map(|s| s.as_str()).unwrap_or("mcnp-input-parser");

    let mut summary_only = false;
    let mut validate_mode = false;
    let mut file_path: Option<String> = None;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage(program);
                return;
            }
            "--summary" => {
                summary_only = true;
            }
            "--validate" => {
                validate_mode = true;
            }
            other if other.starts_with('-') => {
                eprintln!("Unknown option: {}", other);
                print_usage(program);
                process::exit(1);
            }
            path => {
                file_path = Some(path.to_string());
            }
        }
    }

    let content = match file_path {
        Some(ref path) => match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read file `{}`: {}", path, e);
                process::exit(1);
            }
        },
        None => {
            let mut buf = String::new();
            if let Err(e) = io::stdin().read_to_string(&mut buf) {
                eprintln!("Failed to read from stdin: {}", e);
                process::exit(1);
            }
            buf
        }
    };

    if validate_mode {
        let lossy = parse_mcnp_input_lossy(&content);
        let diagnostics = validate_mcnp_input(&lossy.input, &lossy.spans);
        for err in &lossy.errors {
            println!("[SYNTAX] {}", err);
        }
        for diag in &diagnostics {
            println!("{}", diag);
        }
        let error_count = lossy.errors.len()
            + diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count();
        let warning_count = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();
        println!(
            "Validation complete: {} error(s), {} warning(s).",
            error_count, warning_count
        );
        if error_count > 0 {
            process::exit(1);
        }
        return;
    }

    let ast = match parse_mcnp_input(&content) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };

    if summary_only {
        println!("Title: {}", ast.title);
        if let Some(ref msg) = ast.message {
            println!("Message: {}", msg);
        }
        println!("Cells: {} card(s)", ast.cells.len());
        println!("Surfaces: {} card(s)", ast.surfaces.len());
        println!("Data Cards: {} card(s)", ast.data_cards.len());
    } else {
        match serde_json::to_string_pretty(&ast) {
            Ok(json) => println!("{}", json),
            Err(e) => {
                eprintln!("Failed to serialize AST to JSON: {}", e);
                process::exit(1);
            }
        }
    }
}
