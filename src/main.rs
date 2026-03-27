use chip_as_text::{
    build_runtime_ir, canonical_hash, canonical_text, checkpoint_state, diff_states, elaborate,
    initialize_state, load_state, parse_file, save_state, state_hash, validate, Diagnostic,
    SourceSpan, ValidationReport,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return;
    }

    let command = args[1].as_str();
    let json = args.iter().any(|flag| flag == "--json");

    match command {
        "parse" | "hash" | "validate" | "explain" | "ir" => {
            if args.len() < 3 {
                print_usage();
                return;
            }
            let path = &args[2];
            let flags = &args[3..];
            run_definition_command(command, path, flags, json);
        }
        "state-init" => {
            if args.len() < 4 {
                print_usage();
                return;
            }
            let chip_path = &args[2];
            let out_path = &args[3];
            let checkpoint_label = args
                .iter()
                .position(|v| v == "--label")
                .and_then(|idx| args.get(idx + 1))
                .cloned();

            match parse_file(chip_path) {
                Ok(def) => match build_runtime_ir(&def) {
                    Ok(ir) => {
                        let mut state = initialize_state(&ir);
                        if let Some(label) = checkpoint_label {
                            state.lineage.checkpoint_label = Some(label);
                        }
                        match save_state(out_path, &state) {
                            Ok(()) => {
                                if json {
                                    print_json(&state);
                                } else {
                                    println!("Initialized chipstate: {}", out_path);
                                    println!("Design: {}", state.design_name);
                                    println!("Regions: {}", state.regions.len());
                                    println!("State Hash: {}", state_hash(&state));
                                }
                            }
                            Err(err) => eprintln!("State write error: {}", err),
                        }
                    }
                    Err(diagnostics) => print_diagnostic_list(&diagnostics, json),
                },
                Err(e) => eprintln!("Parse error: {}", e),
            }
        }
        "state-inspect" => {
            if args.len() < 3 {
                print_usage();
                return;
            }
            let state_path = &args[2];
            match load_state(state_path) {
                Ok(state) => {
                    if json {
                        print_json(&state);
                    } else {
                        println!("ChipState summary");
                        println!("Design: {}", state.design_name);
                        println!("Design Hash: {}", state.design_hash);
                        println!("Regions: {}", state.regions.len());
                        println!("Page Size: {}", state.page_size);
                        println!("Created: {}", state.created_unix_seconds);
                        println!("State Hash: {}", state_hash(&state));
                    }
                }
                Err(err) => eprintln!("State read error: {}", err),
            }
        }
        "state-hash" => {
            if args.len() < 3 {
                print_usage();
                return;
            }
            match load_state(&args[2]) {
                Ok(state) => println!("{}", state_hash(&state)),
                Err(err) => eprintln!("State read error: {}", err),
            }
        }
        "state-checkpoint" => {
            if args.len() < 5 {
                print_usage();
                return;
            }
            let input = &args[2];
            let output = &args[3];
            let label = &args[4];

            match load_state(input) {
                Ok(state) => {
                    let checkpoint = checkpoint_state(&state, label.clone());
                    match save_state(output, &checkpoint) {
                        Ok(()) => {
                            if json {
                                print_json(&checkpoint);
                            } else {
                                println!("Checkpoint written: {}", output);
                                println!("Label: {}", label);
                                println!("Parent: {}", state_hash(&state));
                                println!("Checkpoint Hash: {}", state_hash(&checkpoint));
                            }
                        }
                        Err(err) => eprintln!("State write error: {}", err),
                    }
                }
                Err(err) => eprintln!("State read error: {}", err),
            }
        }
        "state-diff" => {
            if args.len() < 4 {
                print_usage();
                return;
            }
            let a_path = &args[2];
            let b_path = &args[3];

            match (load_state(a_path), load_state(b_path)) {
                (Ok(a), Ok(b)) => {
                    let diff = diff_states(&a, &b);
                    if json {
                        print_json(&diff);
                    } else {
                        println!("State diff");
                        println!("Same Design Hash: {}", diff.same_design_hash);
                        println!("Added Regions: {}", diff.added_regions.len());
                        println!("Removed Regions: {}", diff.removed_regions.len());
                        println!("Resized Regions: {}", diff.resized_regions.len());
                        println!("Changed Page Sets: {}", diff.changed_pages.len());
                    }
                }
                (Err(err), _) | (_, Err(err)) => eprintln!("State read error: {}", err),
            }
        }
        _ => print_usage(),
    }
}

fn run_definition_command(command: &str, path: &str, flags: &[String], json: bool) {
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
                Err(diagnostics) => print_diagnostic_list(&diagnostics, json),
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
                Err(diagnostics) => print_diagnostic_list(&diagnostics, json),
            },
            Err(e) => eprintln!("Parse error: {}", e),
        },
        _ => print_usage(),
    }
}

fn print_diagnostic_list(diagnostics: &[Diagnostic], json: bool) {
    if json {
        print_json(diagnostics);
    } else {
        for diagnostic in diagnostics {
            print_diagnostic(diagnostic);
        }
    }
}

fn print_json<T: serde::Serialize + ?Sized>(value: &T) {
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
    println!("  chip state-init <chip-file> <state-file> [--label <checkpoint>] [--json]");
    println!("  chip state-inspect <state-file> [--json]");
    println!("  chip state-hash <state-file>");
    println!("  chip state-checkpoint <state-file> <new-state-file> <label> [--json]");
    println!("  chip state-diff <state-a> <state-b> [--json]");
}
