use std::alloc::Layout;

const DEFAULT_ARENA_CAPACITY: usize = 256;

unsafe fn drop_typed<T>(ptr: *mut u8) {
    std::ptr::drop_in_place(ptr as *mut T);
}

struct ArenaEntry {
    ptr: *mut u8,
    drop_fn: unsafe fn(*mut u8),
    layout: Layout,
}

/// Bump-style arena allocator for ephemeral per-system temporaries.
///
/// Objects are individually heap-allocated (like `GcHeap`) but tracked in a
/// flat vector so they can all be destroyed in one O(n) pass via `reset()`.
/// No mark phase, no sweep decisions — everything dies together.
pub struct BumpArena {
    entries: Vec<ArenaEntry>,
    bytes_allocated: usize,
}

impl BumpArena {
    pub fn new() -> Self {
        BumpArena {
            entries: Vec::with_capacity(DEFAULT_ARENA_CAPACITY),
            bytes_allocated: 0,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        BumpArena {
            entries: Vec::with_capacity(capacity),
            bytes_allocated: 0,
        }
    }

    /// Allocate a `T` on the arena. Returns a raw pointer.
    /// The arena owns the allocation; callers must **never** free it.
    /// Signature matches `GcHeap::alloc` for trait compatibility.
    pub fn alloc<T>(&mut self, value: T) -> *mut T {
        let ptr = Box::into_raw(Box::new(value));
        let layout = Layout::new::<T>();
        self.entries.push(ArenaEntry {
            ptr: ptr as *mut u8,
            drop_fn: drop_typed::<T>,
            layout,
        });
        self.bytes_allocated += layout.size();
        ptr
    }

    /// Drop all allocations and reset the arena for reuse. O(n) where n is
    /// the number of live allocations — but no reachability analysis needed.
    pub fn reset(&mut self) {
        for entry in self.entries.drain(..) {
            unsafe {
                (entry.drop_fn)(entry.ptr);
                std::alloc::dealloc(entry.ptr, entry.layout);
            }
        }
        self.bytes_allocated = 0;
    }

    pub fn object_count(&self) -> usize {
        self.entries.len()
    }

    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }
}

impl crate::value::Allocator for BumpArena {
    fn alloc_object(&mut self, obj: crate::value::Object) -> *mut crate::value::Object {
        self.alloc(obj)
    }
}

impl Default for BumpArena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BumpArena {
    fn drop(&mut self) {
        self.reset();
    }
}
