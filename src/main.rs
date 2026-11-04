use std::env;
use std::fs;
use std::path::Path;
use std::process;

use pain::codegen::CodeGenerator;
use pain::codegen::csharp::CSharpGenerator;
use pain::codegen::python::PythonGenerator;
use pain::codegen::rust::RustGenerator;
use pain::codegen::zig::ZigGenerator;
use pain::ir::SchemaIr;
use pain::lower::lower_schema;
use pain::parser::parse_schema;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "check" => run_check(&args[2]),
        "generate" => run_generate(&args[2], &args[3..]),
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}

fn load_ir(path: &str) -> SchemaIr {
    let src = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error reading {path}: {e}");
        process::exit(1);
    });
    let ast = parse_schema(&src).unwrap_or_else(|e| {
        eprintln!("{path}:{e}");
        process::exit(1);
    });
    lower_schema(&ast).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        process::exit(1);
    })
}

fn run_check(path: &str) {
    let ir = load_ir(path);
    println!(
        "OK: {} ({} enums, {} structs, {} choices, {} messages)",
        ir.package,
        ir.enums.len(),
        ir.structs.len(),
        ir.choices.len(),
        ir.messages.len()
    );
}

fn run_generate(schema_path: &str, flags: &[String]) {
    let ir = load_ir(schema_path);
    if flags.is_empty() || !flags.len().is_multiple_of(2) {
        print_usage();
        process::exit(1);
    }

    for pair in flags.chunks(2) {
        let flag = pair[0].as_str();
        let out_path = &pair[1];
        let content = match flag {
            "--rust" => RustGenerator.generate(&ir),
            "--python" => PythonGenerator.generate(&ir),
            "--csharp" => CSharpGenerator.generate(&ir),
            "--zig" => ZigGenerator.generate(&ir),
            _ => {
                eprintln!("unknown target flag '{flag}'");
                process::exit(1);
            }
        };
        if let Some(parent) = Path::new(out_path).parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("failed to create directory {}: {e}", parent.display());
                process::exit(1);
            });
        }
        fs::write(out_path, content).unwrap_or_else(|e| {
            eprintln!("failed to write {out_path}: {e}");
            process::exit(1);
        });
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  pain check <schema.pain>");
    eprintln!(
        "  pain generate <schema.pain> [--rust <file>] [--python <file>] [--csharp <file>] [--zig <file>]"
    );
}
