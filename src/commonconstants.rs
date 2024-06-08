// This file exists to make it easier to vary a single file of constants
// instead of editing each implementation...

/// Per-process maximum number of fds...
pub const FD_PER_PROCESS_MAX: u64 = 1024;

/// Use this to indicate there isn't a real fd backing an item
pub const NO_REAL_FD: u64 = 0xffabcdef01;

/// Use to indicate this is an EPOLLFD
pub const EPOLLFD: u64 = 0xffabcdef02;

/// Use this to indicate that a FD is invalid... Usually an error will be
/// returned instead, but this is needed for rare cases like poll.
pub const INVALID_FD: u64 = 0xffabcdef00;

// BUG / TODO: Use this in some sane way...
#[allow(dead_code)]
/// Global maximum number of fds... (checks may not be implemented)
pub const TOTAL_FD_MAX: u64 = 4096;

// replicating these constants here so this can compile on systems other than
// Linux...  Copied from Rust's libc.
pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_MOD: i32 = 2;
pub const EPOLL_CTL_DEL: i32 = 3;

#[allow(non_camel_case_types)]
pub type c_int = i32;

pub const EPOLLIN: c_int = 0x1;
pub const EPOLLPRI: c_int = 0x2;
pub const EPOLLOUT: c_int = 0x4;
pub const EPOLLERR: c_int = 0x8;
pub const EPOLLHUP: c_int = 0x10;
pub const EPOLLRDNORM: c_int = 0x40;
pub const EPOLLRDBAND: c_int = 0x80;
pub const EPOLLWRNORM: c_int = 0x100;
pub const EPOLLWRBAND: c_int = 0x200;
pub const EPOLLMSG: c_int = 0x400;
pub const EPOLLRDHUP: c_int = 0x2000;
pub const EPOLLEXCLUSIVE: c_int = 0x10000000;
pub const EPOLLWAKEUP: c_int = 0x20000000;
pub const EPOLLONESHOT: c_int = 0x40000000;
// Turning this on here because we copied from Rust's libc and I assume they
// intended this...
#[allow(overflowing_literals)]
pub const EPOLLET: c_int = 0x80000000;

// use libc::epoll_event;
// Note, I'm not using libc's version because this isn't defined on Windows
// or Mac.  Hence, I can't compile, etc. on those systems.  Of course any
// system actually running epoll, will need to be on Mac, but that doesn't mean
// we can't parse those calls.
#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub struct epoll_event {
    pub events: u32, // So weird that this is a u32, while the constants
    // defined to work with it are i32s...
    pub u64: u64,
}
