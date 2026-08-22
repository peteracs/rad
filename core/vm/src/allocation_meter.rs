//! Exact allocator-call metering for benchmarked system bodies.
//!
//! `guest` metrics are semantic GC-object counts owned by [`crate::gc`].
//! This module separately counts native allocator calls made by the VM,
//! direct GC backing allocations, and the RAD-managed portion of a native
//! host-call boundary. Plugin-internal allocators are outside this process
//! allocator contract and must be reported by the plugin verifier.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeAllocationCounters {
    pub runtime_calls: u64,
    pub runtime_bytes: u64,
    pub managed_calls: u64,
    pub managed_bytes: u64,
    pub host_boundary_calls: u64,
    pub host_boundary_bytes: u64,
}

impl NativeAllocationCounters {
    fn difference(self, earlier: Self) -> Self {
        Self {
            runtime_calls: self.runtime_calls.saturating_sub(earlier.runtime_calls),
            runtime_bytes: self.runtime_bytes.saturating_sub(earlier.runtime_bytes),
            managed_calls: self.managed_calls.saturating_sub(earlier.managed_calls),
            managed_bytes: self.managed_bytes.saturating_sub(earlier.managed_bytes),
            host_boundary_calls: self
                .host_boundary_calls
                .saturating_sub(earlier.host_boundary_calls),
            host_boundary_bytes: self
                .host_boundary_bytes
                .saturating_sub(earlier.host_boundary_bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AllocationClass {
    Runtime,
    Managed,
    HostBoundary,
}

#[derive(Clone, Copy)]
struct MeterState {
    active_depth: u32,
    class: AllocationClass,
    counters: NativeAllocationCounters,
}

thread_local! {
    static STATE: Cell<MeterState> = const { Cell::new(MeterState {
        active_depth: 0,
        class: AllocationClass::Runtime,
        counters: NativeAllocationCounters {
            runtime_calls: 0,
            runtime_bytes: 0,
            managed_calls: 0,
            managed_bytes: 0,
            host_boundary_calls: 0,
            host_boundary_bytes: 0,
        },
    }) };
}

static ALLOCATOR_INSTALLED: AtomicBool = AtomicBool::new(false);

/// How many [`AllocationScope`]s are open across all threads.
///
/// `record_allocation` runs on every allocation in the process, so reaching
/// the thread-local just to discover metering is off costs a TLS lookup per
/// allocation — a cost the dispatch60 frame path pays millions of times for a
/// counter nobody is reading. This answers "is anyone metering at all?" with
/// one relaxed load; the thread-local stays authoritative for "is *this*
/// thread metering?", so accounting is unchanged.
///
/// Relaxed is sufficient: a scope opened on this thread is ordered before this
/// thread's later allocations by program order, and allocations on other
/// threads were never part of this thread's per-thread counters.
static ACTIVE_SCOPES: AtomicUsize = AtomicUsize::new(0);

/// Marks that the executable installed [`MeteredSystemAllocator`]. Embedding
/// applications that use another allocator intentionally get an unsupported
/// benchmark result instead of a fabricated zero.
pub fn mark_allocator_installed() {
    ALLOCATOR_INSTALLED.store(true, Ordering::Release);
}

pub fn allocator_installed() -> bool {
    cfg!(test) || ALLOCATOR_INSTALLED.load(Ordering::Acquire)
}

/// Hook called by the process global allocator. It is allocation-free and
/// inactive outside an explicit [`AllocationScope`].
#[inline]
pub fn record_allocation(bytes: usize) {
    if ACTIVE_SCOPES.load(Ordering::Relaxed) == 0 {
        return;
    }
    let _ = STATE.try_with(|state| {
        let mut current = state.get();
        if current.active_depth == 0 {
            return;
        }
        let bytes = bytes as u64;
        match current.class {
            AllocationClass::Runtime => {
                current.counters.runtime_calls = current.counters.runtime_calls.saturating_add(1);
                current.counters.runtime_bytes =
                    current.counters.runtime_bytes.saturating_add(bytes);
            }
            AllocationClass::Managed => {
                current.counters.managed_calls = current.counters.managed_calls.saturating_add(1);
                current.counters.managed_bytes =
                    current.counters.managed_bytes.saturating_add(bytes);
            }
            AllocationClass::HostBoundary => {
                current.counters.host_boundary_calls =
                    current.counters.host_boundary_calls.saturating_add(1);
                current.counters.host_boundary_bytes =
                    current.counters.host_boundary_bytes.saturating_add(bytes);
            }
        }
        state.set(current);
    });
}

pub struct MeteredSystemAllocator;

unsafe impl GlobalAlloc for MeteredSystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !pointer.is_null() {
            record_allocation(new_size);
        }
        pointer
    }
}

pub struct AllocationScope {
    supported: bool,
    start: NativeAllocationCounters,
    finished: bool,
}

impl AllocationScope {
    pub fn begin() -> Self {
        let supported = allocator_installed();
        ACTIVE_SCOPES.fetch_add(1, Ordering::Relaxed);
        let start = STATE
            .try_with(|state| {
                let mut current = state.get();
                let start = current.counters;
                current.active_depth = current.active_depth.saturating_add(1);
                state.set(current);
                start
            })
            .unwrap_or_default();
        Self {
            supported,
            start,
            finished: false,
        }
    }

    pub fn finish(mut self) -> AllocationMeasurement {
        let end = STATE
            .try_with(|state| {
                let mut current = state.get();
                current.active_depth = current.active_depth.saturating_sub(1);
                current.class = AllocationClass::Runtime;
                state.set(current);
                current.counters
            })
            .unwrap_or_default();
        self.finished = true;
        ACTIVE_SCOPES.fetch_sub(1, Ordering::Relaxed);
        AllocationMeasurement {
            supported: self.supported,
            counters: end.difference(self.start),
        }
    }
}

impl Drop for AllocationScope {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        ACTIVE_SCOPES.fetch_sub(1, Ordering::Relaxed);
        let _ = STATE.try_with(|state| {
            let mut current = state.get();
            current.active_depth = current.active_depth.saturating_sub(1);
            current.class = AllocationClass::Runtime;
            state.set(current);
        });
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AllocationMeasurement {
    pub supported: bool,
    pub counters: NativeAllocationCounters,
}

fn with_class<T>(class: AllocationClass, operation: impl FnOnce() -> T) -> T {
    struct Restore(AllocationClass);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = STATE.try_with(|state| {
                let mut current = state.get();
                current.class = self.0;
                state.set(current);
            });
        }
    }

    let previous = STATE
        .try_with(|state| {
            let mut current = state.get();
            let previous = current.class;
            current.class = class;
            state.set(current);
            previous
        })
        .unwrap_or(AllocationClass::Runtime);
    let restore = Restore(previous);
    let result = operation();
    drop(restore);
    result
}

pub(crate) fn managed_allocation<T>(operation: impl FnOnce() -> T) -> T {
    with_class(AllocationClass::Managed, operation)
}

pub(crate) fn host_boundary<T>(operation: impl FnOnce() -> T) -> T {
    with_class(AllocationClass::HostBoundary, operation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_allocations_are_separated_by_owner_category() {
        let scope = AllocationScope::begin();
        let runtime = std::hint::black_box(vec![1_u8; 64]);
        let managed = managed_allocation(|| std::hint::black_box(vec![2_u8; 96]));
        let host = host_boundary(|| std::hint::black_box(vec![3_u8; 128]));
        let measured = scope.finish();
        std::hint::black_box((runtime, managed, host));

        assert!(measured.supported);
        assert!(measured.counters.runtime_calls >= 1);
        assert!(measured.counters.runtime_bytes >= 64);
        assert!(measured.counters.managed_calls >= 1);
        assert!(measured.counters.managed_bytes >= 96);
        assert!(measured.counters.host_boundary_calls >= 1);
        assert!(measured.counters.host_boundary_bytes >= 128);
    }
}
