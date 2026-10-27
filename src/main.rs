use mcnp_input_parser::{
    analyze_diagnostics, analyze_symbols, format_inspect_report, format_symbols_report,
    format_validation_report, get_template, inspect_target, list_templates, parse_mcnp_input,
};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

fn print_usage(program: &str) {
    eprintln!(
        "Usage: {} [OPTIONS] [FILE]",
        program
    );
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  [FILE]                 Path to MCNP input file (reads from stdin if omitted)");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --validate             Run syntax and semantic validation with fix hints");
    eprintln!("  --symbols              List all defined symbols (Cells, Surfaces, Materials, Tallies) and next available IDs");
    eprintln!("  --inspect <TARGET>     Inspect definition and all cross-references of a target (e.g. c1, s10, m1, u1, f4, tr1, d1)");
    eprintln!("  --template [TOPIC]     Print MCNP card templates / reference snippets (omit TOPIC to list all topics)");
    eprintln!("  --summary              Print a brief card count summary instead of full AST JSON");
    eprintln!("  --json                 Format output of --validate, --symbols, or --inspect as JSON");
    eprintln!("  -h, --help             Print this help message");
}

fn read_input_content(file_path: &Option<String>) -> String {
    match file_path {
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
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args
        .first()
        .map(|s| s.as_str())
        .unwrap_or("mcnp-input-parser");

    let mut summary_only = false;
    let mut validate_mode = false;
    let mut symbols_mode = false;
    let mut json_mode = false;
    let mut inspect_query: Option<String> = None;
    let mut template_mode = false;
    let mut template_topic: Option<String> = None;
    let mut file_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
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
            "--symbols" => {
                symbols_mode = true;
            }
            "--json" => {
                json_mode = true;
            }
            "--inspect" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: `--inspect` requires a target argument (e.g. `c1`, `s10`, `m1`, `f4`).");
                    process::exit(1);
                }
                i += 1;
                inspect_query = Some(args[i].clone());
            }
            "--template" => {
                template_mode = true;
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    template_topic = Some(args[i].clone());
                }
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
        i += 1;
    }

    if template_mode {
        match template_topic {
            None => {
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&list_templates()).unwrap()
                    );
                } else {
                    println!("Available MCNP Template Topics (`--template <TOPIC>`):");
                    for t in list_templates() {
                        println!("  {:<20} - {}", t.name, t.summary);
                    }
                }
            }
            Some(ref topic) => match get_template(topic) {
                Some(t) => {
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&t).unwrap());
                    } else {
                        print!("{}", t.content);
                    }
                }
                None => {
                    eprintln!("Unknown template topic `{}`. Available topics:", topic);
                    for t in list_templates() {
                        eprintln!("  {:<20} - {}", t.name, t.summary);
                    }
                    process::exit(1);
                }
            },
        }
        return;
    }

    let content = read_input_content(&file_path);

    if validate_mode {
        let report = analyze_diagnostics(&content);
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            print!("{}", format_validation_report(&report));
        }
        if report.error_count > 0 {
            process::exit(1);
        }
        return;
    }

    if symbols_mode {
        let report = analyze_symbols(&content);
        if json_mode {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            print!("{}", format_symbols_report(&report));
        }
        return;
    }

    if let Some(ref target) = inspect_query {
        match inspect_target(&content, target) {
            Ok(report) => {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    print!("{}", format_inspect_report(&report));
                }
                if !report.found && report.references.is_empty() {
                    process::exit(1);
                }
            }
            Err(msg) => {
                eprintln!("Inspect error: {}", msg);
                process::exit(1);
            }
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
