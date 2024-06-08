// This file exists to make it easier to vary a single file of constants
// instead of editing each implementation...

/// Per-process maximum number of fds...
pub const FD_PER_PROCESS_MAX: u64 = 1024;

/// Use this to indicate there isn't a real fd backing an item
pub const NO_REAL_FD: u64 = 0xff_abcd_ef01;

/// Use to indicate this is an EPOLLFD
pub const EPOLLFD: u64 = 0xff_abcd_ef02;

/// Use this to indicate that a FD is invalid... Usually an error will be
/// returned instead, but this is needed for rare cases like poll.
pub const INVALID_FD: u64 = 0xff_abcd_ef00;

// These are the values we look up with at the end...
#[doc = include_str!("../docs/fdtableentry.md")]
#[derive(Clone, Copy, Debug, PartialEq)]
/// This is a table entry, looked up by virtual fd.
pub struct FDTableEntry {
    /// underlying fd (may be a virtual fd below us or a kernel fd).  In
    /// some cases is also `NO_REAL_FD` or EPOLLFD to indicate it isn't backed
    /// by an underlying fd.
    pub realfd: u64,
    /// Should I close this on exec?  [/`empty_fds_for_exec`]
    pub should_cloexec: bool,
    /// Used for `NO_REAL_FD` and EPOLLFD types to store extra info.  User
    /// defined data can be added here.
    pub optionalinfo: u64,
}

#[allow(non_snake_case)]
/// A function used when registering close handlers which does nothing...
pub const fn NULL_FUNC(_: u64) {}

// BUG / TODO: Use this in some sane way...
#[allow(dead_code)]
/// Global maximum number of fds... (checks may not be implemented)
pub const TOTAL_FD_MAX: u64 = 4096;

// replicating these constants here so this can compile on systems other than
// Linux...  Copied from Rust's libc.
/// copied from libc
pub const EPOLL_CTL_ADD: i32 = 1;
/// copied from libc
pub const EPOLL_CTL_MOD: i32 = 2;
/// copied from libc
pub const EPOLL_CTL_DEL: i32 = 3;

#[allow(non_camel_case_types)]
/// i32 copied from libc.  used in EPOLL event flags even though events are u32
pub type c_int = i32;

/// copied from libc
pub const EPOLLIN: c_int = 0x1;
/// copied from libc
pub const EPOLLPRI: c_int = 0x2;
/// copied from libc
pub const EPOLLOUT: c_int = 0x4;
/// copied from libc
pub const EPOLLERR: c_int = 0x8;
/// copied from libc
pub const EPOLLHUP: c_int = 0x10;
/// copied from libc
pub const EPOLLRDNORM: c_int = 0x40;
/// copied from libc
pub const EPOLLRDBAND: c_int = 0x80;
/// copied from libc
pub const EPOLLWRNORM: c_int = 0x100;
/// copied from libc
pub const EPOLLWRBAND: c_int = 0x200;
/// copied from libc
pub const EPOLLMSG: c_int = 0x400;
/// copied from libc
pub const EPOLLRDHUP: c_int = 0x2000;
/// copied from libc
pub const EPOLLEXCLUSIVE: c_int = 0x1000_0000;
/// copied from libc
pub const EPOLLWAKEUP: c_int = 0x2000_0000;
/// copied from libc
pub const EPOLLONESHOT: c_int = 0x4000_0000;
// Turning this on here because we copied from Rust's libc and I assume they
// intended this...
#[allow(overflowing_literals)]
/// copied from libc
pub const EPOLLET: c_int = 0x8000_0000;

// use libc::epoll_event;
// Note, I'm not using libc's version because this isn't defined on Windows
// or Mac.  Hence, I can't compile, etc. on those systems.  Of course any
// system actually running epoll, will need to be on Mac, but that doesn't mean
// we can't parse those calls.
#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
/// matches libc in Rust.  Copied exactly.
pub struct epoll_event {
    /// copied from libc.  Event types to look at.
    pub events: u32, // So weird that this is a u32, while the constants
    // defined to work with it are i32s...
    /// copied from libc.  Not used.
    pub u64: u64,
}
