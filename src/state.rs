use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ir::{MemoryClass, RuntimeIr};

const CHIPSTATE_MAGIC: &[u8; 12] = b"CHIPSTATEv1\0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChipState {
    pub version: u32,
    pub design_name: String,
    pub design_hash: String,
    pub page_size: u32,
    pub created_unix_seconds: u64,
    pub lineage: StateLineage,
    pub regions: Vec<StateRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateLineage {
    pub parent_state_hash: Option<String>,
    pub checkpoint_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateRegion {
    pub name: String,
    pub class: MemoryClass,
    pub size_bytes: u64,
    pub mapped: bool,
    pub hot_pages: BTreeSet<u64>,
    pub dirty_pages: BTreeMap<u64, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateDiff {
    pub same_design_hash: bool,
    pub added_regions: Vec<String>,
    pub removed_regions: Vec<String>,
    pub resized_regions: Vec<RegionResize>,
    pub changed_pages: Vec<ChangedPages>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegionResize {
    pub name: String,
    pub before: u64,
    pub after: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedPages {
    pub region: String,
    pub modified_page_indices: Vec<u64>,
}

pub fn initialize_state(ir: &RuntimeIr) -> ChipState {
    let created_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let regions = ir
        .memory_blocks
        .iter()
        .map(|block| StateRegion {
            name: block.name.clone(),
            class: block.class.clone(),
            size_bytes: block.size_bytes,
            mapped: false,
            hot_pages: BTreeSet::new(),
            dirty_pages: BTreeMap::new(),
        })
        .collect::<Vec<_>>();

    ChipState {
        version: 1,
        design_name: ir.name.clone(),
        design_hash: ir.ir_hash.clone(),
        page_size: 4096,
        created_unix_seconds,
        lineage: StateLineage {
            parent_state_hash: None,
            checkpoint_label: None,
        },
        regions,
    }
}

pub fn write_page(
    state: &mut ChipState,
    region_name: &str,
    page_index: u64,
    data: Vec<u8>,
) -> Result<(), String> {
    if data.is_empty() {
        return Err("Page data cannot be empty.".to_string());
    }

    let page_size = state.page_size as usize;
    if data.len() > page_size {
        return Err(format!(
            "Page payload exceeds page size ({} > {}).",
            data.len(),
            page_size
        ));
    }

    let region = state
        .regions
        .iter_mut()
        .find(|region| region.name == region_name)
        .ok_or_else(|| format!("Unknown state region '{}'.", region_name))?;

    let max_pages = if region.size_bytes == 0 {
        0
    } else {
        ((region.size_bytes - 1) / state.page_size as u64) + 1
    };

    if page_index >= max_pages {
        return Err(format!(
            "Page index {} out of bounds for region '{}' ({} pages).",
            page_index, region_name, max_pages
        ));
    }

    region.dirty_pages.insert(page_index, data);
    region.hot_pages.insert(page_index);
    region.mapped = true;
    Ok(())
}

pub fn checkpoint_state(state: &ChipState, label: impl Into<String>) -> ChipState {
    let mut checkpoint = state.clone();
    checkpoint.lineage.parent_state_hash = Some(state_hash(state));
    checkpoint.lineage.checkpoint_label = Some(label.into());
    checkpoint.created_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(checkpoint.created_unix_seconds);
    checkpoint
}

pub fn save_state(path: impl AsRef<Path>, state: &ChipState) -> Result<(), String> {
    let metadata = serde_json::to_vec(state)
        .map_err(|err| format!("Failed to encode chipstate metadata: {}", err))?;
    let metadata_len = u32::try_from(metadata.len())
        .map_err(|_| "Chipstate metadata is too large for v1 container.".to_string())?;

    let mut output = Vec::with_capacity(CHIPSTATE_MAGIC.len() + 4 + metadata.len());
    output.extend_from_slice(CHIPSTATE_MAGIC);
    output.extend_from_slice(&metadata_len.to_le_bytes());
    output.extend_from_slice(&metadata);

    fs::write(path.as_ref(), output)
        .map_err(|err| format!("Failed to write '{}': {}", path.as_ref().display(), err))
}

pub fn load_state(path: impl AsRef<Path>) -> Result<ChipState, String> {
    let bytes = fs::read(path.as_ref())
        .map_err(|err| format!("Failed to read '{}': {}", path.as_ref().display(), err))?;

    if bytes.len() < CHIPSTATE_MAGIC.len() + 4 {
        return Err("Invalid chipstate file: too small to contain header.".to_string());
    }

    if &bytes[..CHIPSTATE_MAGIC.len()] != CHIPSTATE_MAGIC {
        return Err("Invalid chipstate file: bad magic header.".to_string());
    }

    let mut len_bytes = [0_u8; 4];
    len_bytes.copy_from_slice(&bytes[CHIPSTATE_MAGIC.len()..CHIPSTATE_MAGIC.len() + 4]);
    let metadata_len = u32::from_le_bytes(len_bytes) as usize;
    let metadata_start = CHIPSTATE_MAGIC.len() + 4;
    let metadata_end = metadata_start + metadata_len;

    if metadata_end > bytes.len() {
        return Err("Invalid chipstate file: metadata length exceeds file size.".to_string());
    }

    serde_json::from_slice(&bytes[metadata_start..metadata_end])
        .map_err(|err| format!("Failed to decode chipstate metadata: {}", err))
}

pub fn state_hash(state: &ChipState) -> String {
    let canonical = serde_json::to_vec(state).expect("chipstate should always serialize");
    let hash = blake3::hash(&canonical);
    hash.to_hex().to_string()
}

pub fn diff_states(before: &ChipState, after: &ChipState) -> StateDiff {
    let before_regions = before
        .regions
        .iter()
        .map(|region| (region.name.clone(), region))
        .collect::<BTreeMap<_, _>>();
    let after_regions = after
        .regions
        .iter()
        .map(|region| (region.name.clone(), region))
        .collect::<BTreeMap<_, _>>();

    let added_regions = after_regions
        .keys()
        .filter(|name| !before_regions.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();

    let removed_regions = before_regions
        .keys()
        .filter(|name| !after_regions.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();

    let mut resized_regions = Vec::new();
    let mut changed_pages = Vec::new();

    for (name, before_region) in &before_regions {
        let Some(after_region) = after_regions.get(name) else {
            continue;
        };

        if before_region.size_bytes != after_region.size_bytes {
            resized_regions.push(RegionResize {
                name: name.clone(),
                before: before_region.size_bytes,
                after: after_region.size_bytes,
            });
        }

        let pages = before_region
            .dirty_pages
            .keys()
            .chain(after_region.dirty_pages.keys())
            .copied()
            .collect::<BTreeSet<_>>();

        let modified = pages
            .into_iter()
            .filter(|page| {
                before_region.dirty_pages.get(page) != after_region.dirty_pages.get(page)
            })
            .collect::<Vec<_>>();

        if !modified.is_empty() {
            changed_pages.push(ChangedPages {
                region: name.clone(),
                modified_page_indices: modified,
            });
        }
    }

    StateDiff {
        same_design_hash: before.design_hash == after.design_hash,
        added_regions,
        removed_regions,
        resized_regions,
        changed_pages,
    }
}

#[cfg(test)]
mod tests {
    use crate::{build_runtime_ir, parse_file};

    use super::{
        checkpoint_state, diff_states, initialize_state, load_state, save_state, state_hash,
        write_page,
    };

    #[test]
    fn round_trips_chipstate_binary() {
        let def = parse_file("examples/sm-memory.chip").unwrap();
        let ir = build_runtime_ir(&def).unwrap();
        let mut state = initialize_state(&ir);
        write_page(&mut state, "Shared Memory", 0, vec![1, 2, 3]).unwrap();

        let path = std::env::temp_dir().join("chipstate_roundtrip_test.chipstate");
        save_state(&path, &state).unwrap();
        let loaded = load_state(&path).unwrap();

        assert_eq!(state_hash(&state), state_hash(&loaded));
    }

    #[test]
    fn creates_checkpoint_lineage() {
        let def = parse_file("examples/sm-memory.chip").unwrap();
        let ir = build_runtime_ir(&def).unwrap();
        let state = initialize_state(&ir);

        let checkpoint = checkpoint_state(&state, "after-step-1");
        assert_eq!(
            checkpoint.lineage.checkpoint_label.as_deref(),
            Some("after-step-1")
        );
        assert_eq!(
            checkpoint.lineage.parent_state_hash,
            Some(state_hash(&state))
        );
    }

    #[test]
    fn diffs_changed_pages() {
        let def = parse_file("examples/sm-memory.chip").unwrap();
        let ir = build_runtime_ir(&def).unwrap();
        let mut a = initialize_state(&ir);
        let mut b = initialize_state(&ir);

        write_page(&mut b, "L1 Cache", 0, vec![9]).unwrap();

        let diff = diff_states(&a, &b);
        assert!(diff.same_design_hash);
        assert_eq!(diff.changed_pages.len(), 1);
        assert_eq!(diff.changed_pages[0].region, "L1 Cache");
        assert_eq!(diff.changed_pages[0].modified_page_indices, vec![0]);

        write_page(&mut a, "L1 Cache", 0, vec![9]).unwrap();
        let no_diff = diff_states(&a, &b);
        assert!(no_diff.changed_pages.is_empty());
    }
}
