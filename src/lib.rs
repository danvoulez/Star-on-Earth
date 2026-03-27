pub mod ir;
pub mod parser;
pub mod semantic;

pub use parser::{
    canonical_hash, canonical_text, parse, parse_file, resolve_imports_from_file, ConnectionSpec,
    Definition, Instance, MemoryBlock, Module, SourceSpan,
};

pub use ir::{
    build_runtime_ir, runtime_ir_hash, runtime_ir_text, IrConnection, IrInstance, IrMemoryBlock,
    IrModule, MemoryClass, RuntimeIr,
};

pub use semantic::{
    elaborate, validate, ConnectionEndpoint, ConnectionTargetKind, Diagnostic, DiagnosticSeverity,
    ElaboratedConnection, ElaboratedDesign, ElaboratedModule, ValidationReport,
};
