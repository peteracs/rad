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
/// Every object and its tracking header share one allocation. The intrusive
/// list avoids a second metadata allocation and, critically, avoids the
/// unbounded temporary peak produced when a tracking `Vec` grows. The GC is
/// the **sole owner** of all heap objects; `Value::Clone` is a plain bit-copy
/// and `Value::Drop` is a no-op.
///
/// During collection the VM builds a `HashSet<usize>` of all reachable
/// payload addresses, then the GC sweeps (drops + deallocates) every
/// tracked object whose address is absent from that set.
pub struct GcHeap {
    head: *mut GcEntry,
    tail: *mut GcEntry,
    object_count: usize,
    bytes_allocated: usize,
    total_allocations: u64,
    total_allocated_bytes: u64,
    next_gc: usize,
}

struct GcEntry {
    next: *mut GcEntry,
    ptr: *mut u8,
    drop_fn: unsafe fn(*mut u8),
    allocation_layout: std::alloc::Layout,
    payload_layout: std::alloc::Layout,
    accounted_size: usize,
}

unsafe fn drop_typed<T>(ptr: *mut u8) {
    std::ptr::drop_in_place(ptr as *mut T);
}

impl GcHeap {
    pub const fn new() -> Self {
        GcHeap {
            head: std::ptr::null_mut(),
            tail: std::ptr::null_mut(),
            object_count: 0,
            bytes_allocated: 0,
            total_allocations: 0,
            total_allocated_bytes: 0,
            next_gc: INITIAL_THRESHOLD,
        }
    }

    /// Allocate a `T` on the GC heap.  Returns a raw pointer to `T`.
    /// The GC owns the allocation; callers must **never** free it.
    pub fn alloc<T>(&mut self, value: T) -> *mut T {
        self.alloc_accounted(value, 0)
    }

    /// Exact retained bytes for the GC header and a payload of `T`, excluding
    /// any backing storage owned by `T` itself.
    pub(crate) fn allocation_bytes_for<T>() -> usize {
        std::alloc::Layout::new::<GcEntry>()
            .extend(std::alloc::Layout::new::<T>())
            .expect("GC allocation layout overflow")
            .0
            .pad_to_align()
            .size()
    }

    fn alloc_accounted<T>(&mut self, value: T, retained_bytes: usize) -> *mut T {
        let payload_layout = std::alloc::Layout::new::<T>();
        let (allocation_layout, payload_offset) = std::alloc::Layout::new::<GcEntry>()
            .extend(payload_layout)
            .expect("GC allocation layout overflow");
        let allocation_layout = allocation_layout.pad_to_align();
        // SAFETY: `allocation_layout` is non-zero because it includes the
        // tracking header. `handle_alloc_error` establishes a non-null base.
        let allocation = crate::allocation_meter::managed_allocation(|| unsafe {
            std::alloc::alloc(allocation_layout)
        });
        if allocation.is_null() {
            std::alloc::handle_alloc_error(allocation_layout);
        }
        let entry = allocation.cast::<GcEntry>();
        // SAFETY: `Layout::extend` supplied an aligned in-bounds payload
        // offset for T inside this allocation.
        let ptr = unsafe { allocation.add(payload_offset).cast::<T>() };
        unsafe {
            ptr.write(value);
            entry.write(GcEntry {
                next: std::ptr::null_mut(),
                ptr: ptr.cast::<u8>(),
                drop_fn: drop_typed::<T>,
                allocation_layout,
                payload_layout,
                accounted_size: allocation_layout.size().saturating_add(retained_bytes),
            });
            if self.tail.is_null() {
                self.head = entry;
            } else {
                (*self.tail).next = entry;
            }
        }
        self.tail = entry;
        self.object_count = self.object_count.saturating_add(1);
        let accounted_size = allocation_layout.size().saturating_add(retained_bytes);
        self.bytes_allocated = self.bytes_allocated.saturating_add(accounted_size);
        self.total_allocations = self.total_allocations.saturating_add(1);
        self.total_allocated_bytes = self
            .total_allocated_bytes
            .saturating_add(accounted_size as u64);
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
    /// All pointers in the intrusive list must be valid (only this method
    /// frees them).
    /// `reachable` must contain payload-pointer addresses from `Value::trace`.
    pub unsafe fn sweep(&mut self, reachable: &HashSet<usize>) -> usize {
        let mut swept = 0usize;
        let mut bytes_freed = 0usize;

        let mut previous = std::ptr::null_mut();
        let mut current = self.head;
        while !current.is_null() {
            let next = unsafe { (*current).next };
            if reachable.contains(&(unsafe { (*current).ptr } as usize)) {
                previous = current;
            } else {
                if previous.is_null() {
                    self.head = next;
                } else {
                    unsafe { (*previous).next = next };
                }
                if self.tail == current {
                    self.tail = previous;
                }
                let ptr = unsafe { (*current).ptr };
                let drop_fn = unsafe { (*current).drop_fn };
                let layout = unsafe { (*current).allocation_layout };
                let accounted_size = unsafe { (*current).accounted_size };
                unsafe {
                    drop_fn(ptr);
                    std::alloc::dealloc(current.cast::<u8>(), layout);
                }
                bytes_freed = bytes_freed.saturating_add(accounted_size);
                self.object_count = self.object_count.saturating_sub(1);
                swept += 1;
            }
            current = next;
        }

        self.bytes_allocated = self.bytes_allocated.saturating_sub(bytes_freed);
        self.next_gc = (self.bytes_allocated * GC_GROW_FACTOR).max(INITIAL_THRESHOLD);
        swept
    }

    pub fn object_count(&self) -> usize {
        self.object_count
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    /// Monotonic counters for exact scoped runtime metrics. Collection
    /// changes live bytes, never these allocation totals.
    pub fn total_allocations(&self) -> u64 {
        self.total_allocations
    }

    pub fn total_allocated_bytes(&self) -> u64 {
        self.total_allocated_bytes
    }

    /// Return the heap accounting that would result from replacing an
    /// already-tracked `Object` while preserving its allocation address.
    ///
    /// Replay graph cloning reserves object identities with placeholders so
    /// cycles can close before their outgoing edges are populated. The
    /// placeholder's retained-size charge is not a valid charge for the
    /// eventual list/map/closure object; callers use this method to preflight
    /// the replacement before allocating its native backing storage.
    pub(crate) fn projected_object_replacement_bytes(
        &self,
        ptr: *mut crate::value::Object,
        retained_bytes: usize,
    ) -> Result<usize, String> {
        let mut entry = self.head;
        while !entry.is_null() && unsafe { (*entry).ptr } != ptr.cast::<u8>() {
            entry = unsafe { (*entry).next };
        }
        if entry.is_null() {
            return Err("GC replacement target is not owned by this heap".into());
        }
        let replacement_size = unsafe { (*entry).allocation_layout }
            .size()
            .saturating_add(retained_bytes);
        Ok(self
            .bytes_allocated
            .saturating_sub(unsafe { (*entry).accounted_size })
            .saturating_add(replacement_size))
    }

    /// Replace a tracked `Object` in place and update retained-byte
    /// accounting atomically with the replacement.
    pub(crate) fn replace_accounted_object(
        &mut self,
        ptr: *mut crate::value::Object,
        replacement: crate::value::Object,
    ) -> Result<(), String> {
        let retained_bytes = replacement.accounted_heap_bytes();
        let mut entry = self.head;
        while !entry.is_null() && unsafe { (*entry).ptr } != ptr.cast::<u8>() {
            entry = unsafe { (*entry).next };
        }
        if entry.is_null() {
            return Err("GC replacement target is not owned by this heap".into());
        }
        if unsafe { (*entry).payload_layout } != std::alloc::Layout::new::<crate::value::Object>() {
            return Err("GC replacement target is not an Object allocation".into());
        }
        let replacement_size = unsafe { (*entry).allocation_layout }
            .size()
            .saturating_add(retained_bytes);
        self.bytes_allocated = self
            .bytes_allocated
            .saturating_sub(unsafe { (*entry).accounted_size })
            .saturating_add(replacement_size);
        unsafe { (*entry).accounted_size = replacement_size };
        // SAFETY: the entry proves `ptr` is a live Object owned exclusively by
        // this heap. `replace` drops the placeholder after installing the new
        // value without changing the stable address recorded by graph edges.
        drop(unsafe { std::ptr::replace(ptr, replacement) });
        Ok(())
    }

    /// Append all allocations from `other` into `self`. Pointers in `Value`s that referred to
    /// `other` remain valid because object addresses are unchanged.
    pub fn merge(&mut self, mut other: GcHeap) {
        if !other.head.is_null() {
            if self.tail.is_null() {
                self.head = other.head;
            } else {
                unsafe { (*self.tail).next = other.head };
            }
            self.tail = other.tail;
        }
        self.bytes_allocated = self.bytes_allocated.saturating_add(other.bytes_allocated);
        self.object_count = self.object_count.saturating_add(other.object_count);
        self.total_allocations = self
            .total_allocations
            .saturating_add(other.total_allocations);
        self.total_allocated_bytes = self
            .total_allocated_bytes
            .saturating_add(other.total_allocated_bytes);
        other.head = std::ptr::null_mut();
        other.tail = std::ptr::null_mut();
        other.object_count = 0;
        other.bytes_allocated = 0;
        other.total_allocations = 0;
        other.total_allocated_bytes = 0;
    }
}

impl crate::value::Allocator for GcHeap {
    fn alloc_object(&mut self, obj: crate::value::Object) -> *mut crate::value::Object {
        let retained_bytes = obj.accounted_heap_bytes();
        self.alloc_accounted(obj, retained_bytes)
    }
}

impl Default for GcHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GcHeap {
    fn drop(&mut self) {
        let mut current = self.head;
        while !current.is_null() {
            let next = unsafe { (*current).next };
            let ptr = unsafe { (*current).ptr };
            let drop_fn = unsafe { (*current).drop_fn };
            let layout = unsafe { (*current).allocation_layout };
            unsafe {
                drop_fn(ptr);
                std::alloc::dealloc(current.cast::<u8>(), layout);
            }
            current = next;
        }
        self.head = std::ptr::null_mut();
        self.tail = std::ptr::null_mut();
        self.object_count = 0;
    }
}
