pub const BLOCK_SIZE: usize = 32 * 1024;
#[cfg(feature = "line-size-128")]
pub const LINE_SIZE: usize = 128;
#[cfg(feature = "line-size-256")]
pub const LINE_SIZE: usize = 256;
#[cfg(feature = "line-size-512")]
pub const LINE_SIZE: usize = 512;
#[cfg(feature = "line-size-1024")]
pub const LINE_SIZE: usize = 1024;
pub const NUM_LINES_PER_BLOCK: usize = BLOCK_SIZE / LINE_SIZE;
// Objects smaller than MEDIUM_OBJECT are allocated with the
/// `NormalAllocator`, otherwise the `OverflowAllocator` is used.
pub const MEDIUM_OBJECT: usize = LINE_SIZE;

/// Objects larger than LARGE_OBJECT are allocated using the `LargeObjectSpace`.
pub const LARGE_OBJECT: usize = 8 * 1024;
/// Whether evacuation should be used or not.
pub const USE_EVACUATION: bool = true;

/// The number of blocks stored into the `EvacAllocator` for evacuation.
pub const EVAC_HEADROOM: usize = 5;

/// Ratio when to trigger evacuation collection.
pub const EVAC_TRIGGER_THRESHHOLD: f64 = 0.25;
