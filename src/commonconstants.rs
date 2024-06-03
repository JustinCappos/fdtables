// This file exists to make it easier to vary a single file of constants
// instead of editing each implementation...

/// Per-process maximum number of fds...
pub const FD_PER_PROCESS_MAX: u64 = 1024;

/// Use this to indicate there isn't a real fd backing an item
pub const NO_REAL_FD: u64 = 0xffabcdef01;

/// Use this to indicate that a FD is invalid... Usually an error will be
/// returned instead, but this is needed for rare cases like poll.
pub const INVALID_FD: u64 = 0xffabcdef00;

// BUG / TODO: Use this in some sane way...
#[allow(dead_code)]
/// Global maximum number of fds... (checks may not be implemented)
pub const TOTAL_FD_MAX: u64 = 4096;
