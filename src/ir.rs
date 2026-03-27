use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::parser::{Definition, MemoryBlock};
use crate::semantic::{elaborate, ConnectionTargetKind, Diagnostic};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeIr {
    pub kind: String,
    pub name: String,
    pub modules: Vec<IrModule>,
    pub instances: Vec<IrInstance>,
    pub memory_blocks: Vec<IrMemoryBlock>,
    pub connections: Vec<IrConnection>,
    pub source_hash: String,
    pub ir_hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IrModule {
    pub name: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IrInstance {
    pub id: String,
    pub module_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum MemoryClass {
    Hbm,
    Sram,
    Cache,
    Unified,
    DiskBacked,
    Generic,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IrMemoryBlock {
    pub name: String,
    pub class: MemoryClass,
    pub size_bytes: u64,
    pub source_size: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IrConnection {
    pub from_component: String,
    pub from_kind: ConnectionTargetKind,
    pub from_port: Option<String>,
    pub to_component: String,
    pub to_kind: ConnectionTargetKind,
    pub to_port: Option<String>,
}

pub fn build_runtime_ir(def: &Definition) -> Result<RuntimeIr, Vec<Diagnostic>> {
    let elaborated = elaborate(def)?;

    let modules = elaborated
        .modules
        .iter()
        .map(|m| IrModule {
            name: m.name.clone(),
            inputs: m.inputs.clone(),
            outputs: m.outputs.clone(),
            operations: m.operations.clone(),
        })
        .collect::<Vec<_>>();

    let module_counts = elaborated
        .modules
        .iter()
        .map(|m| (m.name.clone(), m.instance_count))
        .collect::<BTreeMap<_, _>>();

    let mut instances = Vec::new();
    for (module_name, count) in module_counts {
        for index in 0..count {
            instances.push(IrInstance {
                id: format!("{}#{}", module_name, index + 1),
                module_name: module_name.clone(),
            });
        }
    }

    let mut seen = BTreeSet::new();
    let mut memory_blocks = Vec::new();
    for block in &elaborated.memory_blocks {
        if !seen.insert(block.name.clone()) {
            continue;
        }
        memory_blocks.push(ir_memory_block(block)?);
    }

    let connections = elaborated
        .connections
        .iter()
        .map(|c| IrConnection {
            from_component: c.from.component.clone(),
            from_kind: c.from.kind.clone(),
            from_port: c.from.port.clone(),
            to_component: c.to.component.clone(),
            to_kind: c.to.kind.clone(),
            to_port: c.to.port.clone(),
        })
        .collect::<Vec<_>>();

    let mut ir = RuntimeIr {
        kind: elaborated.kind,
        name: elaborated.name,
        modules,
        instances,
        memory_blocks,
        connections,
        source_hash: elaborated.canonical_hash,
        ir_hash: String::new(),
    };

    ir.ir_hash = runtime_ir_hash(&ir);
    Ok(ir)
}

fn ir_memory_block(block: &MemoryBlock) -> Result<IrMemoryBlock, Vec<Diagnostic>> {
    let class = classify_memory(&block.name, &block.size);
    let size_bytes = match parse_size_to_bytes(&block.size) {
        Some(value) => value,
        None => {
            return Err(vec![Diagnostic {
                code: "E013".to_string(),
                severity: crate::semantic::DiagnosticSeverity::Error,
                message: "Memory block size must use a supported unit (B, KB, MB, GB, TB, KiB, MiB, GiB, TiB).".to_string(),
                section: Some("memory".to_string()),
                subject: Some(block.name.clone()),
                span: block.span.clone(),
            }])
        }
    };

    Ok(IrMemoryBlock {
        name: block.name.clone(),
        class,
        size_bytes,
        source_size: block.size.clone(),
    })
}

fn classify_memory(name: &str, size: &str) -> MemoryClass {
    let haystack = format!("{} {}", name, size).to_ascii_lowercase();
    if haystack.contains("hbm") {
        MemoryClass::Hbm
    } else if haystack.contains("sram") {
        MemoryClass::Sram
    } else if haystack.contains("cache") {
        MemoryClass::Cache
    } else if haystack.contains("disk") || haystack.contains("ssd") {
        MemoryClass::DiskBacked
    } else if haystack.contains("unified") || haystack.contains("uma") {
        MemoryClass::Unified
    } else {
        MemoryClass::Generic
    }
}

fn parse_size_to_bytes(raw: &str) -> Option<u64> {
    let compact = raw.replace(' ', "");
    if compact.is_empty() {
        return None;
    }

    let split_index = compact
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(compact.len());

    let (number_str, unit_str) = compact.split_at(split_index);
    let number = number_str.parse::<f64>().ok()?;
    let unit = if unit_str.is_empty() { "B" } else { unit_str };

    let multiplier = match unit.to_ascii_uppercase().as_str() {
        "B" => 1_f64,
        "KB" => 1_000_f64,
        "MB" => 1_000_000_f64,
        "GB" => 1_000_000_000_f64,
        "TB" => 1_000_000_000_000_f64,
        "KIB" => 1024_f64,
        "MIB" => 1024_f64.powi(2),
        "GIB" => 1024_f64.powi(3),
        "TIB" => 1024_f64.powi(4),
        _ => return None,
    };

    let bytes = number * multiplier;
    if !bytes.is_finite() || bytes < 0.0 || bytes > u64::MAX as f64 {
        return None;
    }

    Some(bytes.round() as u64)
}

pub fn runtime_ir_text(ir: &RuntimeIr) -> String {
    let mut out = String::new();
    out.push_str(&format!("kind:{}\n", ir.kind));
    out.push_str(&format!("name:{}\n", ir.name));
    out.push_str(&format!("source_hash:{}\n", ir.source_hash));

    for module in &ir.modules {
        out.push_str(&format!("module:{}\n", module.name));
        for input in &module.inputs {
            out.push_str(&format!("module_input:{}:{}\n", module.name, input));
        }
        for output in &module.outputs {
            out.push_str(&format!("module_output:{}:{}\n", module.name, output));
        }
        for op in &module.operations {
            out.push_str(&format!("module_op:{}:{}\n", module.name, op));
        }
    }

    for instance in &ir.instances {
        out.push_str(&format!(
            "instance:{}:{}\n",
            instance.id, instance.module_name
        ));
    }

    for block in &ir.memory_blocks {
        out.push_str(&format!(
            "memory:{}:{:?}:{}\n",
            block.name, block.class, block.size_bytes
        ));
    }

    for edge in &ir.connections {
        out.push_str(&format!(
            "connect:{}:{:?}:{:?}->{}:{:?}:{:?}\n",
            edge.from_component,
            edge.from_kind,
            edge.from_port,
            edge.to_component,
            edge.to_kind,
            edge.to_port
        ));
    }

    out
}

pub fn runtime_ir_hash(ir: &RuntimeIr) -> String {
    let hash = blake3::hash(runtime_ir_text(ir).as_bytes());
    hash.to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use crate::parser::parse_file;

    use super::{build_runtime_ir, parse_size_to_bytes, MemoryClass};

    #[test]
    fn builds_runtime_ir_with_instance_ids_and_hash() {
        let def = parse_file("examples/blackwell-sm-imported.chip").unwrap();
        let ir = build_runtime_ir(&def).unwrap();

        assert_eq!(ir.instances.len(), 13);
        assert!(ir
            .instances
            .iter()
            .any(|instance| instance.id == "Tensor Core#8"));
        assert!(!ir.ir_hash.is_empty());
        assert_eq!(ir.memory_blocks.len(), 0);
    }

    #[test]
    fn parses_size_units() {
        assert_eq!(parse_size_to_bytes("1GB"), Some(1_000_000_000));
        assert_eq!(parse_size_to_bytes("1 GiB"), Some(1_073_741_824));
        assert_eq!(parse_size_to_bytes("128MB"), Some(128_000_000));
        assert_eq!(parse_size_to_bytes("bad"), None);
    }

    #[test]
    fn classifies_memory() {
        let def = parse_file("examples/sm-memory.chip").unwrap();
        let ir = build_runtime_ir(&def).unwrap();
        let names_to_class = ir
            .memory_blocks
            .iter()
            .map(|b| (b.name.as_str(), b.class.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(names_to_class.get("L1 Cache"), Some(&MemoryClass::Cache));
    }
}
