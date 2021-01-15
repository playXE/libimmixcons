use super::{allocation::ImmixSpace, block::ImmixBlock, constants::*, CollectionType};
use crate::{large_object_space::LargeObjectSpace, object::*, util::*};
use alloc::collections::VecDeque;
use core::ptr::NonNull;
use vec_map::VecMap;

pub struct ImmixCollector;
pub struct Visitor<'a> {
    immix_space: &'a mut ImmixSpace,
    queue: &'a mut VecDeque<*mut RawGc>,
    defrag: bool,
    next_live_mark: bool,
}

impl<'a> Tracer for Visitor<'a> {
    fn trace(&mut self, reference: &mut NonNull<RawGc>) {
        unsafe {
            let mut child = &mut *reference.as_ptr();
            if child.is_forwarded() {
                debug!("Child {:p} is forwarded to 0x{:x}", child, child.vtable());
                // when forwarded pointer to vtable is set to forwarding pointer
                *reference = NonNull::new_unchecked(child.vtable() as *mut _);
            } else if (&*child).get_mark() != self.next_live_mark {
                // if there are some blocks that needs evacuation try to evacuate object.
                if self.defrag && self.immix_space.filter_fast(Address::from_ptr(child)) {
                    if let Some(new_child) = self.immix_space.maybe_evacuate(child) {
                        *reference = NonNull::new_unchecked(new_child.to_mut_ptr());
                        debug!("Evacuated child {:p} to {}", child, new_child);
                        child = &mut *new_child.to_mut_ptr::<RawGc>();
                    }
                }
                debug!("Push child {:p} into object queue", child);
                self.queue.push_back(child);
            }
        }
    }
}

impl ImmixCollector {
    pub fn collect(
        collection_type: &CollectionType,
        roots: &[*mut RawGc],
        precise_roots: &[*mut *mut RawGc],
        immix_space: &mut ImmixSpace,
        next_live_mark: bool,
    ) -> usize {
        let mut object_queue: VecDeque<*mut RawGc> = roots.iter().copied().collect();
        for root in precise_roots.iter() {
            unsafe {
                let root = &mut **root;
                let mut raw = &mut **root;
                if immix_space.filter_fast(Address::from_ptr(raw)) {
                    if raw.is_forwarded() {
                        raw = &mut *(raw.vtable() as *mut RawGc);
                    } else if *collection_type == CollectionType::ImmixEvacCollection {
                        if let Some(new_object) = immix_space.maybe_evacuate(raw) {
                            *root = new_object.to_mut_ptr::<RawGc>();
                            raw.set_forwarded(new_object.to_usize());
                            raw = &mut *new_object.to_mut_ptr::<RawGc>();
                        }
                    }
                }
                object_queue.push_back(raw);
            }
        }
        let mut visited = 0;

        while let Some(object) = object_queue.pop_front() {
            unsafe {
                //debug!("Process object {:p} in Immix closure", object);
                let object_addr = Address::from_ptr(object);
                if !(&mut *object).mark(next_live_mark) {
                    if immix_space.filter_fast(object_addr) {
                        let block = ImmixBlock::get_block_ptr(object_addr);
                        immix_space.set_gc_object(object_addr); // Mark object in bitmap
                        (&mut *block).line_object_mark(object_addr); // Mark block line
                    }
                    debug!("Object {:p} was unmarked: visit their children", object);
                    visited += (&*object).object_size();
                    let visitor_fn = (&*object).rtti().visit_references;
                    {
                        let mut visitor = core::mem::transmute::<_, Visitor<'static>>(Visitor {
                            immix_space,
                            next_live_mark,
                            queue: &mut object_queue,
                            defrag: *collection_type == CollectionType::ImmixEvacCollection,
                        });

                        visitor_fn(
                            object as *mut u8,
                            TracerPtr {
                                tracer: core::mem::transmute(&mut visitor as &mut dyn Tracer),
                            },
                        );
                    }
                }
            }
        }
        debug!("Completed collection with {} bytes visited", visited);
        visited
    }
}
use alloc::vec::Vec;

pub struct Collector {
    all_blocks: Vec<*mut ImmixBlock>,
    mark_histogram: VecMap<usize>,
}
impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}
impl Collector {
    pub fn new() -> Self {
        Self {
            all_blocks: Vec::new(),
            mark_histogram: VecMap::with_capacity(NUM_LINES_PER_BLOCK),
        }
    }
    /// Store the given blocks into the buffer for use during the collection.
    pub fn extend_all_blocks(&mut self, blocks: Vec<*mut ImmixBlock>) {
        self.all_blocks.extend(blocks);
    }

    /// Prepare a collection.
    ///
    /// This function decides if a evacuating and/or cycle collecting
    /// collection will be performed. If `evacuation` is set the collectors
    /// will try to evacuate. If `cycle_collect` is set the immix tracing
    /// collector will be used.
    pub fn prepare_collection(
        &mut self,
        evacuation: bool,
        _cycle_collect: bool,
        available_blocks: usize,
        evac_headroom: usize,
        total_blocks: usize,
        emergency: bool,
    ) -> CollectionType {
        if emergency && USE_EVACUATION {
            for block in &mut self.all_blocks {
                unsafe {
                    (**block).evacuation_candidate = true;
                }
            }
            return CollectionType::ImmixEvacCollection;
        }
        let mut perform_evac = evacuation;

        let evac_threshhold = (total_blocks as f64 * EVAC_TRIGGER_THRESHHOLD) as usize;

        let available_evac_blocks = available_blocks + evac_headroom;
        debug!(
            "total blocks={},evac threshold={}, available evac blocks={}",
            total_blocks, evac_threshhold, available_evac_blocks
        );
        if evacuation || available_evac_blocks < evac_threshhold {
            let hole_threshhold = self.establish_hole_threshhold(evac_headroom);
            debug!("evac threshold={}", hole_threshhold);
            perform_evac = USE_EVACUATION && hole_threshhold > 0;
            if perform_evac {
                for block in &mut self.all_blocks {
                    unsafe {
                        (**block).evacuation_candidate =
                            (**block).hole_count as usize >= hole_threshhold;
                    }
                }
            }
        }

        match (false, perform_evac, true) {
            (true, false, true) => CollectionType::ImmixCollection,
            (true, true, true) => CollectionType::ImmixEvacCollection,
            (false, false, _) => CollectionType::ImmixCollection,
            (false, true, _) => CollectionType::ImmixEvacCollection,
            _ => CollectionType::ImmixCollection,
        }
    }

    pub fn collect(
        &mut self,
        collection_type: &CollectionType,
        roots: &[*mut RawGc],
        precise_roots: &[*mut *mut RawGc],
        immix_space: &mut ImmixSpace,
        large_object_space: &mut LargeObjectSpace,
        next_live_mark: bool,
    ) -> usize {
        // TODO: maybe use immix_space.bitmap.clear_range(immix_space.begin,immix_space.block_cursor)?
        for block in &mut self.all_blocks {
            unsafe {
                immix_space
                    .bitmap
                    .clear_range((*block) as usize, (*block) as usize + BLOCK_SIZE);
                (**block).line_map.clear_all();
            }
        }
        let visited = ImmixCollector::collect(
            collection_type,
            roots,
            precise_roots,
            immix_space,
            next_live_mark,
        );
        self.mark_histogram.clear();
        let (recyclable_blocks, free_blocks) = self.sweep_all_blocks();
        immix_space.set_recyclable_blocks(recyclable_blocks);

        // XXX We should not use a constant here, but something that
        // XXX changes dynamically (see rcimmix: MAX heuristic).
        let evac_headroom = if USE_EVACUATION {
            EVAC_HEADROOM - immix_space.evac_headroom()
        } else {
            0
        };
        immix_space.extend_evac_headroom(free_blocks.iter().take(evac_headroom).copied());
        immix_space.return_blocks(free_blocks.iter().skip(evac_headroom).copied());
        large_object_space.sweep();
        visited
    }
    /// Sweep all blocks in the buffer after the collection.
    ///
    /// This function returns a list of recyclable blocks and a list of free
    /// blocks.
    fn sweep_all_blocks(&mut self) -> (Vec<*mut ImmixBlock>, Vec<*mut ImmixBlock>) {
        let mut unavailable_blocks = Vec::new();
        let mut recyclable_blocks = Vec::new();
        let mut free_blocks = Vec::new();
        for block in self.all_blocks.drain(..) {
            if unsafe { (*block).is_empty() } {
                unsafe {
                    (*block).reset();
                }
                debug!("Push block {:p} into free_blocks", block);
                free_blocks.push(block);
            } else {
                unsafe {
                    (*block).count_holes();
                }
                let (holes, marked_lines) = unsafe { (*block).count_holes_and_marked_lines() };
                if self.mark_histogram.contains_key(holes) {
                    if let Some(val) = self.mark_histogram.get_mut(holes) {
                        *val += marked_lines;
                    }
                } else {
                    self.mark_histogram.insert(holes, marked_lines);
                }
                debug!(
                    "Found {} holes and {} marked lines in block {:p}",
                    holes, marked_lines, block
                );
                match holes {
                    0 => {
                        debug!("Push block {:p} into unavailable_blocks", block);
                        unavailable_blocks.push(block);
                    }
                    _ => {
                        debug!("Push block {:p} into recyclable_blocks", block);
                        recyclable_blocks.push(block);
                    }
                }
            }
        }
        self.all_blocks = unavailable_blocks;
        (recyclable_blocks, free_blocks)
    }

    /// Calculate how many holes a block needs to have to be selected as a
    /// evacuation candidate.
    fn establish_hole_threshhold(&self, evac_headroom: usize) -> usize {
        let mut available_histogram: VecMap<usize> = VecMap::with_capacity(NUM_LINES_PER_BLOCK);
        for &block in &self.all_blocks {
            let (holes, free_lines) = unsafe { (*block).count_holes_and_available_lines() };
            if available_histogram.contains_key(holes) {
                if let Some(val) = available_histogram.get_mut(holes) {
                    *val += free_lines;
                }
            } else {
                available_histogram.insert(holes, free_lines);
            }
        }
        let mut required_lines = 0;
        let mut available_lines = evac_headroom * (NUM_LINES_PER_BLOCK - 1);

        for threshold in 0..NUM_LINES_PER_BLOCK {
            required_lines += *self.mark_histogram.get(threshold).unwrap_or(&0);
            available_lines =
                available_lines.saturating_sub(*available_histogram.get(threshold).unwrap_or(&0));
            if available_lines <= required_lines {
                return threshold;
            }
        }
        NUM_LINES_PER_BLOCK
    }
}
