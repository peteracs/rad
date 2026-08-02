use std::cell::UnsafeCell;
use std::collections::HashSet;

// Floor for the collection trigger. Programs whose live set stays tiny
// (most: the ECS world lives in the persistent store, not this heap) would
// otherwise collect every few KB of transient garbage now that the VM
// polls `should_collect` at back-edges — measured as a 3-6x slowdown on
// payload-heavy loops (wire encode benches) with an 8 KB floor.
const INITIAL_THRESHOLD: usize = 256 * 1024;
const GC_GROW_FACTOR: usize = 2;

/// A mutable capture cell for closures.
///
/// Replaces `Rc<RefCell<Value>>` with a GC-managed raw cell.  Multiple
/// closures sharing the same capture just hold copies of the same raw
/// `*mut CaptureCell` pointer (the GC keeps the cell alive).
///
/// Interior mutability is safe because Rad is single-threaded and the
/// borrow discipline is enforced by the bytecode (each SetUpvalue /
/// GetUpvalue touches exactly one slot).
pub struct CaptureCell {
    inner: UnsafeCell<crate::value::Value>,
}

impl CaptureCell {
    pub fn new(val: crate::value::Value) -> Self {
        CaptureCell {
            inner: UnsafeCell::new(val),
        }
    }

    #[inline(always)]
    pub fn get(&self) -> crate::value::Value {
        unsafe { *self.inner.get() }
    }

    #[inline(always)]
    pub fn set(&self, val: crate::value::Value) {
        unsafe {
            *self.inner.get() = val;
        }
    }

    #[inline(always)]
    pub fn get_ref(&self) -> &crate::value::Value {
        unsafe { &*self.inner.get() }
    }
}

/// Mark-sweep garbage collector for the Rad VM.
///
/// Objects are allocated via `Box::into_raw` and tracked as raw pointers.
/// The GC is the **sole owner** of all heap objects; `Value::Clone` is a
/// plain bit-copy and `Value::Drop` is a no-op.
///
/// During collection the VM builds a `HashSet<usize>` of all reachable
/// payload addresses, then the GC sweeps (drops + deallocates) every
/// tracked object whose address is absent from that set.
pub struct GcHeap {
    /// (payload pointer, drop function, layout) for each tracked object.
    objects: Vec<GcEntry>,
    bytes_allocated: usize,
    next_gc: usize,
}

struct GcEntry {
    ptr: *mut u8,
    drop_fn: unsafe fn(*mut u8),
    layout: std::alloc::Layout,
}

unsafe fn drop_typed<T>(ptr: *mut u8) {
    std::ptr::drop_in_place(ptr as *mut T);
}

impl GcHeap {
    pub const fn new() -> Self {
        GcHeap {
            objects: Vec::new(),
            bytes_allocated: 0,
            next_gc: INITIAL_THRESHOLD,
        }
    }

    /// Allocate a `T` on the GC heap.  Returns a raw pointer to `T`.
    /// The GC owns the allocation; callers must **never** free it.
    pub fn alloc<T>(&mut self, value: T) -> *mut T {
        let ptr = Box::into_raw(Box::new(value));
        let layout = std::alloc::Layout::new::<T>();
        self.objects.push(GcEntry {
            ptr: ptr as *mut u8,
            drop_fn: drop_typed::<T>,
            layout,
        });
        self.bytes_allocated += layout.size();
        ptr
    }

    pub fn should_collect(&self) -> bool {
        self.bytes_allocated > self.next_gc
    }

    /// Test hook: force the next `should_collect` poll to fire so regression
    /// tests can stage a collection at an exact execution point.
    pub fn set_collect_threshold_for_test(&mut self, bytes: usize) {
        self.next_gc = bytes;
    }

    /// Sweep every object whose address is **not** in `reachable`.
    ///
    /// # Safety
    /// All pointers in `self.objects` must be valid (only this method frees them).
    /// `reachable` must contain payload-pointer addresses from `Value::trace`.
    pub unsafe fn sweep(&mut self, reachable: &HashSet<usize>) -> usize {
        let mut swept = 0usize;
        let mut bytes_freed = 0usize;

        self.objects.retain(|entry| {
            if reachable.contains(&(entry.ptr as usize)) {
                true
            } else {
                unsafe {
                    (entry.drop_fn)(entry.ptr);
                    std::alloc::dealloc(entry.ptr, entry.layout);
                }
                bytes_freed += entry.layout.size();
                swept += 1;
                false
            }
        });

        self.bytes_allocated = self.bytes_allocated.saturating_sub(bytes_freed);
        self.next_gc = (self.bytes_allocated * GC_GROW_FACTOR).max(INITIAL_THRESHOLD);
        swept
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    /// Append all allocations from `other` into `self`. Pointers in `Value`s that referred to
    /// `other` remain valid because object addresses are unchanged.
    pub fn merge(&mut self, mut other: GcHeap) {
        self.bytes_allocated = self.bytes_allocated.saturating_add(other.bytes_allocated);
        self.objects.append(&mut other.objects);
        other.objects.clear();
        other.bytes_allocated = 0;
    }
}

impl crate::value::Allocator for GcHeap {
    fn alloc_object(&mut self, obj: crate::value::Object) -> *mut crate::value::Object {
        self.alloc(obj)
    }
}

impl Default for GcHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GcHeap {
    fn drop(&mut self) {
        for entry in &self.objects {
            unsafe {
                (entry.drop_fn)(entry.ptr);
                std::alloc::dealloc(entry.ptr, entry.layout);
            }
        }
        self.objects.clear();
    }
}
