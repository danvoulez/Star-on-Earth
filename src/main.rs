use chip_as_text::{
    build_runtime_ir, canonical_hash, canonical_text, elaborate, parse_file, validate, Diagnostic,
    SourceSpan, ValidationReport,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        print_usage();
        return;
    }

    let command = args[1].as_str();
    let path = &args[2];
    let flags = &args[3..];
    let json = flags.iter().any(|flag| flag == "--json");

    match command {
        "parse" => match parse_file(path) {
            Ok(def) => {
                if json {
                    print_json(&def);
                    return;
                }

                if flags.iter().any(|flag| flag == "--canonical") {
                    println!("{}", canonical_text(&def));
                    return;
                }

                println!("Parsed successfully!");
                println!("Kind: {}", def.kind);
                println!("Name: {}", def.name);
                if let Some(full) = &def.full_name {
                    println!("Full Name: {}", full);
                }
                println!("Canonical Hash: {}", canonical_hash(&def));
                println!("Modules: {}", def.modules.len());
                println!("Instances: {}", def.instantiate.len());
                println!("Connections: {}", def.connect.len());
                if !def.memory_blocks.is_empty() {
                    println!("Memory Blocks: {}", def.memory_blocks.len());
                }
            }
            Err(e) => eprintln!("Parse error: {}", e),
        },
        "hash" => match parse_file(path) {
            Ok(def) => println!("{}", canonical_hash(&def)),
            Err(e) => eprintln!("Parse error: {}", e),
        },
        "validate" => match parse_file(path) {
            Ok(def) => {
                let report = validate(&def);
                if json {
                    print_json(&report);
                } else {
                    print_validation_report(&report);
                }
            }
            Err(e) => eprintln!("Parse error: {}", e),
        },
        "explain" => match parse_file(path) {
            Ok(def) => match elaborate(&def) {
                Ok(elaborated) => {
                    if json {
                        print_json(&elaborated);
                    } else {
                        print_elaborated_summary(&elaborated);
                    }
                }
                Err(diagnostics) => {
                    if json {
                        print_json(&diagnostics);
                    } else {
                        for diagnostic in diagnostics {
                            print_diagnostic(&diagnostic);
                        }
                    }
                }
            },
            Err(e) => eprintln!("Parse error: {}", e),
        },
        "ir" => match parse_file(path) {
            Ok(def) => match build_runtime_ir(&def) {
                Ok(ir) => {
                    if json {
                        print_json(&ir);
                    } else {
                        println!("Runtime IR summary");
                        println!("Kind: {}", ir.kind);
                        println!("Name: {}", ir.name);
                        println!("Modules: {}", ir.modules.len());
                        println!("Instances: {}", ir.instances.len());
                        println!("Memory Blocks: {}", ir.memory_blocks.len());
                        println!("Connections: {}", ir.connections.len());
                        println!("Source Hash: {}", ir.source_hash);
                        println!("IR Hash: {}", ir.ir_hash);
                    }
                }
                Err(diagnostics) => {
                    if json {
                        print_json(&diagnostics);
                    } else {
                        for diagnostic in diagnostics {
                            print_diagnostic(&diagnostic);
                        }
                    }
                }
            },
            Err(e) => eprintln!("Parse error: {}", e),
        },
        _ => print_usage(),
    }
}

fn print_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{}", json),
        Err(err) => eprintln!("JSON serialization error: {}", err),
    }
}

fn print_validation_report(report: &ValidationReport) {
    if report.is_valid {
        println!("Valid definition.");
    } else {
        println!("Invalid definition.");
    }

    println!("Diagnostics: {}", report.diagnostics.len());
    for diagnostic in &report.diagnostics {
        print_diagnostic(diagnostic);
    }

    if let Some(elaborated) = &report.elaborated {
        println!("Resolved modules: {}", elaborated.modules.len());
        println!("Total instances: {}", elaborated.total_instances);
        println!("Resolved connections: {}", elaborated.connections.len());
        println!("Canonical Hash: {}", elaborated.canonical_hash);
    }
}

fn print_elaborated_summary(elaborated: &chip_as_text::ElaboratedDesign) {
    println!("Semantic design summary");
    println!("Kind: {}", elaborated.kind);
    println!("Name: {}", elaborated.name);
    println!("Modules: {}", elaborated.modules.len());
    println!("Total instances: {}", elaborated.total_instances);
    println!("Connections: {}", elaborated.connections.len());
    println!("Canonical Hash: {}", elaborated.canonical_hash);
    println!();
    println!("Resolved modules:");
    for module in &elaborated.modules {
        println!(
            "- {} (instances: {}, inbound: {}, outbound: {})",
            module.name,
            module.instance_count,
            module.inbound_connections,
            module.outbound_connections
        );
    }
    if !elaborated.connections.is_empty() {
        println!();
        println!("Resolved connections:");
        for connection in &elaborated.connections {
            println!(
                "- {} -> {}",
                format_endpoint(&connection.from),
                format_endpoint(&connection.to)
            );
        }
    }
}

fn format_endpoint(endpoint: &chip_as_text::ConnectionEndpoint) -> String {
    match &endpoint.port {
        Some(port) => format!("{}.{} [{:?}]", endpoint.component, port, endpoint.kind),
        None => format!("{} [{:?}]", endpoint.component, endpoint.kind),
    }
}

fn print_diagnostic(diagnostic: &Diagnostic) {
    let section = diagnostic.section.as_deref().unwrap_or("unknown");
    let subject = diagnostic.subject.as_deref().unwrap_or("-");
    let span = format_span(diagnostic.span.as_ref());
    println!(
        "[{severity:?}] {code} {span} section={section} subject={subject} :: {message}",
        severity = diagnostic.severity,
        code = diagnostic.code,
        message = diagnostic.message,
    );
}

fn format_span(span: Option<&SourceSpan>) -> String {
    match span {
        Some(span) if span.line_start == span.line_end => {
            format!(
                "line {}:{}-{}",
                span.line_start, span.column_start, span.column_end
            )
        }
        Some(span) => format!(
            "lines {}:{} -> {}:{}",
            span.line_start, span.column_start, span.line_end, span.column_end
        ),
        None => "line ?".to_string(),
    }
}

fn print_usage() {
    println!("Usage:");
    println!("  chip parse <file>");
    println!("  chip parse <file> --json");
    println!("  chip parse <file> --canonical");
    println!("  chip hash <file>");
    println!("  chip validate <file>");
    println!("  chip validate <file> --json");
    println!("  chip explain <file>");
    println!("  chip explain <file> --json");
    println!("  chip ir <file>");
    println!("  chip ir <file> --json");
}
