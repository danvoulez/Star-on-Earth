pub mod ir;
pub mod parser;
pub mod semantic;
pub mod state;

pub use ir::{
    build_runtime_ir, runtime_ir_hash, runtime_ir_text, IrConnection, IrInstance, IrMemoryBlock,
    IrModule, MemoryClass, RuntimeIr,
};

pub use parser::{
    canonical_hash, canonical_text, parse, parse_file, resolve_imports_from_file, ConnectionSpec,
    Definition, Instance, MemoryBlock, Module, SourceSpan,
};

pub use semantic::{
    elaborate, validate, ConnectionEndpoint, ConnectionTargetKind, Diagnostic, DiagnosticSeverity,
    ElaboratedConnection, ElaboratedDesign, ElaboratedModule, ValidationReport,
};

pub use state::{
    checkpoint_state, diff_states, initialize_state, load_state, save_state, state_hash,
    write_page, ChangedPages, ChipState, RegionResize, StateDiff, StateLineage, StateRegion,
};
