use crate::thunk::{HeadThunk, TailThunk};
use crate::{backend, plt, HookError};
use std::collections::BTreeMap;
use std::os::raw::c_void;
use std::sync::{Mutex, OnceLock};

pub const MAX_PROLOGUE_SIGNATURE_LEN: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HookKey {
    Inline(usize),
    Import(String, String),
}
enum Physical {
    Inline(backend::InlinePhysical),
    Import(plt::ImportPhysical),
}
struct HookNode {
    replacement: usize,
    original: TailThunk,
    sequence: u64,
}
struct HookChain {
    physical: Physical,
    head: HeadThunk,
    nodes: Vec<HookNode>,
    pristine: Option<Vec<u8>>,
    thumb: bool,
}
struct Registry {
    chains: BTreeMap<HookKey, HookChain>,
    next_sequence: u64,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            chains: BTreeMap::new(),
            next_sequence: 0,
        })
    })
}

fn append(
    registry: &mut Registry,
    key: &HookKey,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> Result<(), HookError> {
    if replacement.is_null() {
        return Err(HookError::InvalidArgument);
    }
    backend::validate_replacement(replacement)?;
    let sequence = registry.next_sequence;
    let chain = registry
        .chains
        .get_mut(key)
        .ok_or(HookError::InstallationConflict)?;
    let tail = TailThunk::new_for_mode(chain.head.destination(), chain.thumb)?;
    if !original.is_null() {
        unsafe {
            *original = tail.entry();
        }
    }
    chain.nodes.push(HookNode {
        replacement: replacement as usize,
        original: tail,
        sequence,
    });
    chain.head.publish(replacement as usize);
    registry.next_sequence += 1;
    Ok(())
}

pub unsafe fn hook_inline(
    target: *mut c_void,
    replacement: *mut c_void,
    original: *mut *mut c_void,
    expected: Option<&[u8]>,
) -> Result<(), HookError> {
    if target.is_null() || replacement.is_null() {
        return Err(HookError::InvalidArgument);
    }
    backend::validate_replacement(replacement)?;
    if let Some(bytes) = expected {
        if bytes.len() < backend::PATCH_LEN {
            return Err(HookError::InvalidArgument);
        }
        if bytes.len() > MAX_PROLOGUE_SIGNATURE_LEN {
            return Err(HookError::SignatureTooLong);
        }
    }
    let key = HookKey::Inline(backend::code_address(target));
    let mut registry = registry().lock().map_err(|_| HookError::Poisoned)?;
    if let Some(chain) = registry.chains.get(&key) {
        if let Some(expected) = expected {
            if chain.pristine.as_deref().map(|p| &p[..expected.len()]) != Some(expected) {
                return Err(HookError::PrologueMismatch);
            }
        }
        return append(&mut registry, &key, replacement, original);
    }
    let pristine = backend::snapshot_prologue(target, MAX_PROLOGUE_SIGNATURE_LEN)?;
    if let Some(expected) = expected {
        if pristine.get(..expected.len()) != Some(expected) {
            return Err(HookError::PrologueMismatch);
        }
    }
    let thumb = target as usize & 1 != 0;
    let head = HeadThunk::new_for_mode(target as usize, thumb)?;
    // Allocate every fallible resource before changing the target's code.  The
    // tail initially points at target only as a placeholder; it is private
    // until Dobby returns the relocated original below.
    let tail = TailThunk::new_for_mode(target as usize, thumb)?;
    let physical = backend::install_inline_physical(target, head.entry())?;
    head.publish(physical.relocated_original as usize);
    tail.publish(physical.relocated_original as usize);
    if !original.is_null() {
        unsafe { *original = tail.entry() };
    }
    let sequence = registry.next_sequence;
    registry.chains.insert(
        key.clone(),
        HookChain {
            physical: Physical::Inline(physical),
            head,
            nodes: vec![HookNode {
                replacement: replacement as usize,
                original: tail,
                sequence,
            }],
            pristine: Some(pristine),
            thumb,
        },
    );
    registry.next_sequence += 1;
    // Publishing last prevents a hook invocation from reaching a partially
    // initialized logical chain.
    registry
        .chains
        .get(&key)
        .expect("inserted chain")
        .head
        .publish(replacement as usize);
    Ok(())
}

pub fn hook_import(
    image: Option<&str>,
    symbol: &str,
    replacement: *mut c_void,
    original: *mut *mut c_void,
) -> Result<(), HookError> {
    if symbol.is_empty() || replacement.is_null() || image.is_none() {
        return Err(HookError::InvalidArgument);
    }
    backend::validate_replacement(replacement)?;
    let identity = plt::image_identity(image.unwrap())?;
    let key = HookKey::Import(identity.clone(), symbol.to_owned());
    let mut registry = registry().lock().map_err(|_| HookError::Poisoned)?;
    if registry.chains.contains_key(&key) {
        return append(&mut registry, &key, replacement, original);
    }
    let discovery = plt::discover_import(&identity, symbol)?;
    let thumb = discovery.true_original as usize & 1 != 0;
    let head = HeadThunk::new_for_mode(discovery.true_original as usize, thumb)?;
    // As with inline hooks, complete allocation before the physical GOT write.
    let tail = TailThunk::new_for_mode(discovery.true_original as usize, thumb)?;
    let physical = plt::install_import_physical(discovery, head.entry())?;
    if !original.is_null() {
        unsafe { *original = tail.entry() };
    }
    let sequence = registry.next_sequence;
    registry.chains.insert(
        key.clone(),
        HookChain {
            physical: Physical::Import(physical),
            head,
            nodes: vec![HookNode {
                replacement: replacement as usize,
                original: tail,
                sequence,
            }],
            pristine: None,
            thumb,
        },
    );
    registry.next_sequence += 1;
    registry
        .chains
        .get(&key)
        .expect("inserted chain")
        .head
        .publish(replacement as usize);
    Ok(())
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    let _ = registry().lock().map(|mut r| r.chains.clear());
}
