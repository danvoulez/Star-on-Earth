use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;

use crate::parser::{
    canonical_hash, ConnectionSpec, Definition, Instance, MemoryBlock, Module, SourceSpan,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl DiagnosticSeverity {
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub section: Option<String>,
    pub subject: Option<String>,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub is_valid: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub elaborated: Option<ElaboratedDesign>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElaboratedDesign {
    pub kind: String,
    pub name: String,
    pub full_name: Option<String>,
    pub description: String,
    pub goals: Vec<String>,
    pub modules: Vec<ElaboratedModule>,
    pub connections: Vec<ElaboratedConnection>,
    pub memory_blocks: Vec<MemoryBlock>,
    pub total_instances: u32,
    pub canonical_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElaboratedModule {
    pub name: String,
    pub summary: Option<String>,
    pub operations: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub instance_count: u32,
    pub inbound_connections: usize,
    pub outbound_connections: usize,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElaboratedConnection {
    pub from: ConnectionEndpoint,
    pub to: ConnectionEndpoint,
    pub raw: String,
    pub span: Option<SourceSpan>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnectionTargetKind {
    Module,
    MemoryBlock,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionEndpoint {
    pub component: String,
    pub port: Option<String>,
    pub kind: ConnectionTargetKind,
}

#[derive(Debug, Clone)]
struct ParsedConnection {
    from: ParsedEndpoint,
    to: ParsedEndpoint,
}

#[derive(Debug, Clone)]
struct ParsedEndpoint {
    component: String,
    port: Option<String>,
}

pub fn validate(def: &Definition) -> ValidationReport {
    let diagnostics = collect_diagnostics(def);
    let has_errors = diagnostics.iter().any(|d| d.severity.is_error());

    let elaborated = if has_errors {
        None
    } else {
        Some(build_elaborated(def))
    };

    ValidationReport {
        is_valid: !has_errors,
        diagnostics,
        elaborated,
    }
}

pub fn elaborate(def: &Definition) -> Result<ElaboratedDesign, Vec<Diagnostic>> {
    let report = validate(def);
    if report.is_valid {
        Ok(report
            .elaborated
            .expect("valid report should always carry elaborated design"))
    } else {
        Err(report.diagnostics)
    }
}

fn collect_diagnostics(def: &Definition) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let module_map = build_module_map(&def.modules);
    let memory_map = build_memory_map(&def.memory_blocks);

    if def.description.trim().is_empty() {
        diagnostics.push(warning(
            "W001",
            "description",
            None,
            None,
            "Definition has no description.",
        ));
    }

    if def.output.trim().is_empty() {
        diagnostics.push(warning(
            "W002",
            "output",
            None,
            def.span.clone(),
            "Definition has no output summary.",
        ));
    }

    if def.kind == "CHIP" && def.modules.is_empty() {
        diagnostics.push(error(
            "E001",
            "modules",
            None,
            def.span.clone(),
            "CHIP definitions must declare at least one module.",
        ));
    }

    if def.kind == "MEMORY" && def.memory_blocks.is_empty() {
        diagnostics.push(warning(
            "W003",
            "memory",
            None,
            def.span.clone(),
            "MEMORY definitions usually declare at least one memory block.",
        ));
    }

    let duplicate_modules = find_duplicate_module_names(&def.modules);
    for name in duplicate_modules {
        let span = def
            .modules
            .iter()
            .find(|module| module.name == name)
            .and_then(|module| module.span.clone());
        diagnostics.push(error(
            "E002",
            "modules",
            Some(name),
            span,
            "Duplicate module definition.",
        ));
    }

    for module in &def.modules {
        if module.operations.is_empty() {
            diagnostics.push(warning(
                "W004",
                "modules",
                Some(module.name.clone()),
                module.span.clone(),
                "Module declares no operations.",
            ));
        }

        if module.inputs.is_empty() {
            diagnostics.push(warning(
                "W005",
                "modules",
                Some(module.name.clone()),
                module.span.clone(),
                "Module declares no inputs.",
            ));
        }

        if module.outputs.is_empty() {
            diagnostics.push(warning(
                "W006",
                "modules",
                Some(module.name.clone()),
                module.span.clone(),
                "Module declares no outputs.",
            ));
        }
    }

    let mut memory_names = BTreeSet::new();
    for block in &def.memory_blocks {
        if !memory_names.insert(block.name.clone()) {
            diagnostics.push(error(
                "E005",
                "memory",
                Some(block.name.clone()),
                block.span.clone(),
                "Duplicate memory block name.",
            ));
        }

        if block.size.trim().is_empty() {
            diagnostics.push(error(
                "E006",
                "memory",
                Some(block.name.clone()),
                block.span.clone(),
                "Memory block size cannot be empty.",
            ));
        }

        if module_map.contains_key(&block.name) {
            diagnostics.push(error(
                "E010",
                "memory",
                Some(block.name.clone()),
                block.span.clone(),
                "Memory block name conflicts with a module name.",
            ));
        }
    }

    let instance_totals = aggregate_instances(&def.instantiate);
    for instance in &def.instantiate {
        if instance.count == 0 {
            diagnostics.push(error(
                "E003",
                "instantiate",
                Some(instance.module_name.clone()),
                instance.span.clone(),
                "Instance count must be greater than zero.",
            ));
        }

        if !module_map.contains_key(&instance.module_name) {
            diagnostics.push(error(
                "E004",
                "instantiate",
                Some(instance.module_name.clone()),
                instance.span.clone(),
                "Instance references an unknown module.",
            ));
        }
    }

    let mut valid_edges = Vec::<ElaboratedConnection>::new();
    for connection in &def.connect {
        match validate_connection(connection, &module_map, &memory_map) {
            Ok(Some(valid_edge)) => valid_edges.push(valid_edge),
            Ok(None) => {}
            Err(mut errs) => diagnostics.append(&mut errs),
        }
    }

    for module in &def.modules {
        if !instance_totals.contains_key(&module.name) {
            diagnostics.push(warning(
                "W007",
                "instantiate",
                Some(module.name.clone()),
                module.span.clone(),
                "Module is defined but never instantiated.",
            ));
        }
    }

    let (inbound, outbound) = compute_module_connection_counts(&valid_edges);

    for module in &def.modules {
        let instance_count = instance_totals.get(&module.name).copied().unwrap_or(0);
        if instance_count == 0 {
            continue;
        }

        let in_count = inbound.get(&module.name).copied().unwrap_or(0);
        let out_count = outbound.get(&module.name).copied().unwrap_or(0);

        if in_count == 0 && out_count == 0 {
            diagnostics.push(warning(
                "W008",
                "connect",
                Some(module.name.clone()),
                module.span.clone(),
                "Instantiated module is never connected.",
            ));
            continue;
        }

        if in_count == 0 {
            diagnostics.push(warning(
                "W009",
                "connect",
                Some(module.name.clone()),
                module.span.clone(),
                "Instantiated module has no inbound connections.",
            ));
        }

        if out_count == 0 {
            diagnostics.push(warning(
                "W010",
                "connect",
                Some(module.name.clone()),
                module.span.clone(),
                "Instantiated module has no outbound connections.",
            ));
        }
    }

    for block in &def.memory_blocks {
        let connected = valid_edges.iter().any(|edge| {
            edge.from.kind == ConnectionTargetKind::MemoryBlock && edge.from.component == block.name
                || edge.to.kind == ConnectionTargetKind::MemoryBlock
                    && edge.to.component == block.name
        });

        if !connected {
            diagnostics.push(warning(
                "W011",
                "memory",
                Some(block.name.clone()),
                block.span.clone(),
                "Memory block is defined but never connected.",
            ));
        }
    }

    if has_multiple_subgraphs(&valid_edges) {
        diagnostics.push(warning(
            "W012",
            "connect",
            None,
            def.span.clone(),
            "Design contains multiple disconnected connectivity subgraphs.",
        ));
    }

    diagnostics
}

fn build_module_map<'a>(modules: &'a [Module]) -> BTreeMap<String, &'a Module> {
    let mut map = BTreeMap::new();
    for module in modules {
        map.entry(module.name.clone()).or_insert(module);
    }
    map
}

fn build_memory_map<'a>(blocks: &'a [MemoryBlock]) -> BTreeMap<String, &'a MemoryBlock> {
    let mut map = BTreeMap::new();
    for block in blocks {
        map.entry(block.name.clone()).or_insert(block);
    }
    map
}

fn find_duplicate_module_names(modules: &[Module]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for module in modules {
        if !seen.insert(module.name.clone()) {
            duplicates.insert(module.name.clone());
        }
    }
    duplicates
}

fn aggregate_instances(instances: &[Instance]) -> BTreeMap<String, u32> {
    let mut totals = BTreeMap::new();
    for instance in instances {
        *totals.entry(instance.module_name.clone()).or_insert(0) += instance.count;
    }
    totals
}

fn validate_connection(
    connection: &ConnectionSpec,
    module_map: &BTreeMap<String, &Module>,
    memory_map: &BTreeMap<String, &MemoryBlock>,
) -> Result<Option<ElaboratedConnection>, Vec<Diagnostic>> {
    let parsed = match parse_connection(&connection.raw) {
        Some(parsed) => parsed,
        None => {
            return Err(vec![error(
                "E009",
                "connect",
                Some(connection.raw.clone()),
                connection.span.clone(),
                "Connection must use the form '<component>[.<port>] -> <component>[.<port>]'.",
            )]);
        }
    };

    let mut diagnostics = Vec::new();
    let from = resolve_endpoint(
        &parsed.from,
        EndpointRole::Source,
        &connection.raw,
        connection.span.clone(),
        module_map,
        memory_map,
        &mut diagnostics,
    );
    let to = resolve_endpoint(
        &parsed.to,
        EndpointRole::Destination,
        &connection.raw,
        connection.span.clone(),
        module_map,
        memory_map,
        &mut diagnostics,
    );

    if diagnostics.is_empty() {
        Ok(Some(ElaboratedConnection {
            from: from.expect("validated source endpoint should exist"),
            to: to.expect("validated destination endpoint should exist"),
            raw: connection.raw.clone(),
            span: connection.span.clone(),
        }))
    } else {
        Err(diagnostics)
    }
}

#[derive(Copy, Clone)]
enum EndpointRole {
    Source,
    Destination,
}

fn resolve_endpoint(
    endpoint: &ParsedEndpoint,
    role: EndpointRole,
    raw_connection: &str,
    span: Option<SourceSpan>,
    module_map: &BTreeMap<String, &Module>,
    memory_map: &BTreeMap<String, &MemoryBlock>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ConnectionEndpoint> {
    if let Some(module) = module_map.get(&endpoint.component) {
        if let Some(port) = &endpoint.port {
            let valid_ports = match role {
                EndpointRole::Source => &module.outputs,
                EndpointRole::Destination => &module.inputs,
            };
            if !valid_ports.iter().any(|candidate| candidate == port) {
                let (code, message) = match role {
                    EndpointRole::Source => (
                        "E011",
                        "Connection source port does not match any declared module output.",
                    ),
                    EndpointRole::Destination => (
                        "E012",
                        "Connection destination port does not match any declared module input.",
                    ),
                };
                diagnostics.push(error(
                    code,
                    "connect",
                    Some(raw_connection.to_string()),
                    span,
                    message,
                ));
                return None;
            }
        }

        return Some(ConnectionEndpoint {
            component: module.name.clone(),
            port: endpoint.port.clone(),
            kind: ConnectionTargetKind::Module,
        });
    }

    if let Some(block) = memory_map.get(&endpoint.component) {
        return Some(ConnectionEndpoint {
            component: block.name.clone(),
            port: endpoint.port.clone(),
            kind: ConnectionTargetKind::MemoryBlock,
        });
    }

    let (code, message) = match role {
        EndpointRole::Source => (
            "E007",
            "Connection source references an unknown module or memory block.",
        ),
        EndpointRole::Destination => (
            "E008",
            "Connection destination references an unknown module or memory block.",
        ),
    };
    diagnostics.push(error(
        code,
        "connect",
        Some(raw_connection.to_string()),
        span,
        message,
    ));
    None
}

fn compute_module_connection_counts(
    edges: &[ElaboratedConnection],
) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
    let mut inbound = BTreeMap::new();
    let mut outbound = BTreeMap::new();

    for edge in edges {
        if edge.from.kind == ConnectionTargetKind::Module {
            *outbound.entry(edge.from.component.clone()).or_insert(0) += 1;
        }
        if edge.to.kind == ConnectionTargetKind::Module {
            *inbound.entry(edge.to.component.clone()).or_insert(0) += 1;
        }
    }

    (inbound, outbound)
}

fn has_multiple_subgraphs(edges: &[ElaboratedConnection]) -> bool {
    let mut adjacency: BTreeMap<
        (ConnectionTargetKind, String),
        BTreeSet<(ConnectionTargetKind, String)>,
    > = BTreeMap::new();

    for edge in edges {
        let from = (edge.from.kind.clone(), edge.from.component.clone());
        let to = (edge.to.kind.clone(), edge.to.component.clone());
        adjacency
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        adjacency.entry(to).or_default().insert(from);
    }

    if adjacency.len() <= 1 {
        return false;
    }

    let mut visited = BTreeSet::new();
    let mut components = 0_usize;

    for node in adjacency.keys() {
        if visited.contains(node) {
            continue;
        }

        components += 1;
        let mut queue = VecDeque::from([node.clone()]);
        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            if let Some(neighbors) = adjacency.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
    }

    components > 1
}

fn build_elaborated(def: &Definition) -> ElaboratedDesign {
    let instance_totals = aggregate_instances(&def.instantiate);
    let module_map = build_module_map(&def.modules);
    let memory_map = build_memory_map(&def.memory_blocks);

    let valid_connections = def
        .connect
        .iter()
        .filter_map(|connection| {
            validate_connection(connection, &module_map, &memory_map)
                .ok()
                .flatten()
        })
        .collect::<Vec<_>>();

    let (inbound, outbound) = compute_module_connection_counts(&valid_connections);

    let modules = def
        .modules
        .iter()
        .map(|module| ElaboratedModule {
            name: module.name.clone(),
            summary: module.summary.clone(),
            operations: module.operations.clone(),
            inputs: module.inputs.clone(),
            outputs: module.outputs.clone(),
            instance_count: instance_totals.get(&module.name).copied().unwrap_or(0),
            inbound_connections: inbound.get(&module.name).copied().unwrap_or(0),
            outbound_connections: outbound.get(&module.name).copied().unwrap_or(0),
            span: module.span.clone(),
        })
        .collect::<Vec<_>>();

    let total_instances = instance_totals.values().copied().sum::<u32>();

    ElaboratedDesign {
        kind: def.kind.clone(),
        name: def.name.clone(),
        full_name: def.full_name.clone(),
        description: def.description.clone(),
        goals: def.goals.clone(),
        modules,
        connections: valid_connections,
        memory_blocks: def.memory_blocks.clone(),
        total_instances,
        canonical_hash: canonical_hash(def),
    }
}

fn parse_connection(raw: &str) -> Option<ParsedConnection> {
    let (from, to) = raw.split_once("->")?;
    let from = parse_endpoint(from.trim())?;
    let to = parse_endpoint(to.trim())?;
    Some(ParsedConnection { from, to })
}

fn parse_endpoint(raw: &str) -> Option<ParsedEndpoint> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if let Some((component, port)) = raw.rsplit_once('.') {
        let component = component.trim();
        let port = port.trim();
        if !component.is_empty() && is_valid_port_token(port) {
            return Some(ParsedEndpoint {
                component: component.to_string(),
                port: Some(port.to_string()),
            });
        }
    }

    Some(ParsedEndpoint {
        component: raw.to_string(),
        port: None,
    })
}

fn is_valid_port_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':'))
}

fn error(
    code: &str,
    section: &str,
    subject: Option<String>,
    span: Option<SourceSpan>,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        section: Some(section.to_string()),
        subject,
        span,
    }
}

fn warning(
    code: &str,
    section: &str,
    subject: Option<String>,
    span: Option<SourceSpan>,
    message: &str,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        severity: DiagnosticSeverity::Warning,
        message: message.to_string(),
        section: Some(section.to_string()),
        subject,
        span,
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::{parse, parse_file};

    use super::{elaborate, validate};

    #[test]
    fn elaborates_valid_chip() {
        let def = parse_file("examples/blackwell-sm-imported.chip").unwrap();
        let elaborated = elaborate(&def).unwrap();

        assert_eq!(elaborated.modules.len(), 3);
        assert_eq!(elaborated.total_instances, 13);
        assert_eq!(elaborated.modules[0].instance_count, 4);
        assert_eq!(elaborated.connections.len(), 2);
    }

    #[test]
    fn validates_invalid_references() {
        let def = parse(
            "# CHIP v2 invalid\nDescription:\nInvalid references.\nModules:\nDefine module ALU:\nSummary: Arithmetic\nOperations: add\nInputs: a\nOutputs: b\nInstantiate:\nCreate 2 instances of Missing Module\nConnect:\nALU => Missing Module\nOutput: bad\n",
        )
        .unwrap();

        let report = validate(&def);
        assert!(!report.is_valid);
        assert!(report.diagnostics.iter().any(|d| d.code == "E004"));
        assert!(report.diagnostics.iter().any(|d| d.code == "E009"));
    }

    #[test]
    fn validates_port_level_connections() {
        let def = parse_file("examples/blackwell-sm-ports.chip").unwrap();
        let report = validate(&def);
        assert!(report.is_valid);
        let elaborated = report.elaborated.unwrap();
        assert_eq!(elaborated.connections.len(), 3);
        assert_eq!(
            elaborated.connections[0].from.port.as_deref(),
            Some("selected_warp")
        );
        assert_eq!(
            elaborated.connections[0].to.port.as_deref(),
            Some("operand_a")
        );
    }

    #[test]
    fn attaches_spans_to_diagnostics() {
        let def = parse_file("examples/invalid-sm.chip").unwrap();
        let report = validate(&def);
        assert!(report.diagnostics.iter().all(|d| d.span.is_some()));
    }

    #[test]
    fn warns_on_unconnected_instantiated_modules() {
        let def = parse(
            "# CHIP v2 dangling\nDescription:\nDangling module.\nModules:\nDefine module A:\nSummary: Source\nOperations: emit\nInputs: trigger\nOutputs: out\nDefine module B:\nSummary: Idle\nOperations: wait\nInputs: in\nOutputs: out\nInstantiate:\nCreate 1 instances of A\nCreate 1 instances of B\nConnect:\nA.out -> A.trigger\nOutput: done\n",
        )
        .unwrap();

        let report = validate(&def);
        assert!(report.diagnostics.iter().any(|d| d.code == "W008"));
    }

    #[test]
    fn warns_on_disconnected_subgraphs() {
        let def = parse(
            "# CHIP v2 split\nDescription:\nTwo subgraphs.\nModules:\nDefine module A:\nSummary: a\nOperations: op\nInputs: in\nOutputs: out\nDefine module B:\nSummary: b\nOperations: op\nInputs: in\nOutputs: out\nDefine module C:\nSummary: c\nOperations: op\nInputs: in\nOutputs: out\nDefine module D:\nSummary: d\nOperations: op\nInputs: in\nOutputs: out\nInstantiate:\nCreate 1 instances of A\nCreate 1 instances of B\nCreate 1 instances of C\nCreate 1 instances of D\nConnect:\nA.out -> B.in\nC.out -> D.in\nOutput: split\n",
        )
        .unwrap();

        let report = validate(&def);
        assert!(report.diagnostics.iter().any(|d| d.code == "W012"));
    }
}
