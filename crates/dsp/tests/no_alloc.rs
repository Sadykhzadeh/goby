//! The block path must not touch the heap: once a voice is set up, processing blocks
//! allocates nothing. Counted with a global allocator that only counts on this thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use goby_dsp::fx::Noise;

#[path = "support/voices.rs"]
mod voices;

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const SR: u32 = 48_000;
const BLOCK: usize = 480;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

#[test]
fn voices_do_not_allocate_per_block() {
    let mut noise = Noise::default();
    let mut block = vec![0.0f32; BLOCK];
    for (name, recipe, _) in voices::CASES {
        let mut voice = goby_dsp::dsp::Voice::new(SR);
        voice.set(recipe, 0.0, 1.0);
        for _ in 0..200 {
            block.iter_mut().for_each(|s| *s = 0.3 * noise.next());
            voice.process(&mut block);
        }
        let before = allocations();
        for _ in 0..1000 {
            block.iter_mut().for_each(|s| *s = 0.3 * noise.next());
            voice.process(&mut block);
        }
        let made = allocations() - before;
        assert_eq!(made, 0, "{name} allocated {made} times in 1000 blocks");
    }
}
