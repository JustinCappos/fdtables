//  DashMap<u64,[Option<FDTableEntry>;FD_PER_PROCESSS_MAX]>  Space is ~24KB 
//  per cage w/ 1024 fds?!?
//      Static DashMap.  Let's see if having the FDTableEntries be a static
//      array is any faster...

use crate::threei;

use dashmap::DashMap;

use lazy_static::lazy_static;

use std::collections::HashMap;

// This uses a Dashmap (for cages) with an array of FDTableEntry items.

/// Per-process maximum number of fds...
pub const FD_PER_PROCESS_MAX: u64 = 1024;

// BUG / TODO: Use this in some sane way...
#[allow(dead_code)]
/// Global maximum number of fds... (checks may not be implemented)
pub const TOTAL_FD_MAX: u64 = 4096;

// algorithm name.  Need not be listed.  Used in benchmarking output
#[doc(hidden)]
pub const ALGONAME: &str = "DashMapArrayGlobal";

/// Use this to indicate there isn't a real fd backing an item
pub const NO_REAL_FD: u64 = 0xffabcdef01;

// These are the values we look up with at the end...
#[doc = include_str!("../docs/fdtableentry.md")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FDTableEntry {
    pub realfd: u64, // underlying fd (may be a virtual fd below us or
    // a kernel fd)
    pub should_cloexec: bool, // should I close this when exec is called?
    pub optionalinfo: u64,    // user specified / controlled data
}

// It's fairly easy to check the fd count on a per-process basis (I just check
// when I would add a new fd).
//
// BUG: I will ignore the total limit for now.  I would ideally do this on
// every creation, close, fork, etc. but it's a PITA to track this.

// We will raise a panic anywhere we receive an unknown cageid.  This frankly
// should not be possible and indicates some sort of internal error in our
// code.  However, it is expected we could receive an invalid file descriptor
// when a cage makes a call.

// In order to store this information, I'm going to use a DashMap which
// has keys of (cageid:u64) and values that are an array of FD_PER_PROCESS_MAX
// Option<FDTableEntry> items. 
//
//

// This lets me initialize the code as a global.
lazy_static! {

  #[derive(Debug)]
  // Usually I would care more about this, but I'm keeping this close to
  // the vanilla implementation...
  static ref FDTABLE: DashMap<u64, [Option<FDTableEntry>;FD_PER_PROCESS_MAX as usize]> = {
    let m = DashMap::new();
    // Insert a cage so that I have something to fork / test later, if need
    // be. Otherwise, I'm not sure how I get this started. I think this
    // should be invalid from a 3i standpoint, etc. Could this mask an
    // error in the future?
    m.insert(threei::TESTING_CAGEID,[Option::None;FD_PER_PROCESS_MAX as usize]);
    m
  };
}

#[doc = include_str!("../docs/translate_virtual_fd.md")]
pub fn translate_virtual_fd(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {

    // They should not be able to pass a new cage I don't know.  I should
    // always have a table for each cage because each new cage is added at fork
    // time
    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match FDTABLE.get(&cageid).unwrap()[virtualfd as usize] {
        Some(tableentry) => Ok(tableentry.realfd),
        None => Err(threei::Errno::EBADFD as u64),
    };
}


// This is fairly slow if I just iterate sequentially through numbers.
// However there are not that many to choose from.  I could pop from a list
// or a set as well...  Likely the best solution is to keep a count of the
// largest fd handed out and to just use this until you wrap.  This will be
// super fast for a normal cage and will be correct in the weird case.
// Right now, I'll just implement the slow path and will speed this up
// later, if needed.
#[doc = include_str!("../docs/get_unused_virtual_fd.md")]
pub fn get_unused_virtual_fd(
    cageid: u64,
    realfd: u64,
    should_cloexec: bool,
    optionalinfo: u64,
) -> Result<u64, threei::RetVal> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }
    // Set up the entry so it has the right info...
    // Note, a HashMap stores its data on the heap!  No need to box it...
    // https://doc.rust-lang.org/book/ch08-03-hash-maps.html#creating-a-new-hash-map
    let myentry = FDTableEntry {
        realfd,
        should_cloexec,
        optionalinfo,
    };

    let mut myfdarray = FDTABLE.get_mut(&cageid).unwrap();

    // Check the fds in order.
    for fdcandidate in 0..FD_PER_PROCESS_MAX {
        // FIXME: This is likely very slow.  Should do something smarter...
        if myfdarray[fdcandidate as usize].is_none() {
            // I just checked.  Should not be there...
            myfdarray[fdcandidate as usize] = Some(myentry);
            return Ok(fdcandidate);
        }
    }

    // I must have checked all fds and failed to find one open.  Fail!
    Err(threei::Errno::EMFILE as u64)
}

// This is used for things like dup2, which need a specific fd...
// NOTE: I will assume that the requested_virtualfd isn't used.  If it is, I
// will return ELIND
#[doc = include_str!("../docs/get_specific_virtual_fd.md")]
pub fn get_specific_virtual_fd(
    cageid: u64,
    requested_virtualfd: u64,
    realfd: u64,
    should_cloexec: bool,
    optionalinfo: u64,
) -> Result<(), threei::RetVal> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // If you ask for a FD number that is too large, I'm going to reject it.
    // Note that, I need to use the FD_PER_PROCESS_MAX setting because this
    // is also how I'm tracking how many values you have open.  If this
    // changed, then these constants could be decoupled...
    if requested_virtualfd > FD_PER_PROCESS_MAX {
        return Err(threei::Errno::EBADF as u64);
    }

    // Set up the entry so it has the right info...
    // Note, a HashMap stores its data on the heap!  No need to box it...
    // https://doc.rust-lang.org/book/ch08-03-hash-maps.html#creating-a-new-hash-map
    let myentry = FDTableEntry {
        realfd,
        should_cloexec,
        optionalinfo,
    };

    if FDTABLE
        .get(&cageid)
        .unwrap()[requested_virtualfd as usize].is_some()
    {
        Err(threei::Errno::ELIND as u64)
    } else {
        FDTABLE.get_mut(&cageid).unwrap()[requested_virtualfd as usize] = Some(myentry);
        Ok(())
    }
}

// We're just setting a flag here, so this should be pretty straightforward.
#[doc = include_str!("../docs/set_cloexec.md")]
pub fn set_cloexec(cageid: u64, virtualfd: u64, is_cloexec: bool) -> Result<(), threei::RetVal> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // return EBADFD, if the fd is missing...
    if FDTABLE.get(&cageid).unwrap()[virtualfd as usize].is_none() {
        return Err(threei::Errno::EBADFD as u64);
    }
    // Set the is_cloexec flag
    FDTABLE.get_mut(&cageid).unwrap()[virtualfd as usize].as_mut().unwrap().should_cloexec = is_cloexec;
    Ok(())
}

// Super easy, just return the optionalinfo field...
#[doc = include_str!("../docs/get_optionalinfo.md")]
pub fn get_optionalinfo(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match FDTABLE.get(&cageid).unwrap()[virtualfd as usize] {
        Some(tableentry) => Ok(tableentry.optionalinfo),
        None => Err(threei::Errno::EBADFD as u64),
    };
}

// We're setting an opaque value here. This should be pretty straightforward.
#[doc = include_str!("../docs/set_optionalinfo.md")]
pub fn set_optionalinfo(
    cageid: u64,
    virtualfd: u64,
    optionalinfo: u64,
) -> Result<(), threei::RetVal> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // return EBADFD, if the fd is missing...
    if FDTABLE.get(&cageid).unwrap()[virtualfd as usize].is_none() {
        return Err(threei::Errno::EBADFD as u64);
    }

    // Set optionalinfo or return EBADFD, if that's missing...
    FDTABLE.get_mut(&cageid).unwrap()[virtualfd as usize].as_mut().unwrap().optionalinfo = optionalinfo;
    Ok(())
}

// Helper function used for fork...  Copies an fdtable for another process
#[doc = include_str!("../docs/copy_fdtable_for_cage.md")]
pub fn copy_fdtable_for_cage(srccageid: u64, newcageid: u64) -> Result<(), threei::Errno> {

    if !FDTABLE.contains_key(&srccageid) {
        panic!("Unknown srccageid in fdtable access");
    }
    if FDTABLE.contains_key(&newcageid) {
        panic!("Known newcageid in fdtable access");
    }

    // Insert a copy and ensure it didn't exist...
    let hmcopy = *FDTABLE.get(&srccageid).unwrap();
    assert!(FDTABLE.insert(newcageid, hmcopy).is_none());
    Ok(())
    // I'm not going to bother to check the number of fds used overall yet...
    //    Err(threei::Errno::EMFILE as u64),
}

// This is mostly used in handling exit, etc.  Returns the HashMap
// for the cage.
#[doc = include_str!("../docs/remove_cage_from_fdtable.md")]
pub fn remove_cage_from_fdtable(cageid: u64) -> HashMap<u64, FDTableEntry> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut myhashmap = HashMap::new();

    let myfdarray = FDTABLE.get(&cageid).unwrap();
    for item in 0..FD_PER_PROCESS_MAX as usize {
        if myfdarray[item].is_some() {
            myhashmap.insert(item as u64,myfdarray[item].unwrap());
        }
    }
    // I need to do this or else I'll try to double claim the lock and
    // deadlock...
    drop(myfdarray);

    FDTABLE.remove(&cageid);

    myhashmap

}

// This removes all fds with the should_cloexec flag set.  They are returned
// in a new hashmap...
#[doc = include_str!("../docs/empty_fds_for_exec.md")]
pub fn empty_fds_for_exec(cageid: u64) -> HashMap<u64, FDTableEntry> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut myhashmap = HashMap::new();

    let mut myfdarray = FDTABLE.get_mut(&cageid).unwrap();
    for item in 0..FD_PER_PROCESS_MAX as usize {
        if myfdarray[item].is_some() && myfdarray[item].unwrap().should_cloexec {
            myhashmap.insert(item as u64,myfdarray[item].unwrap());
            myfdarray[item] = None;
        }
    }

    myhashmap

}

// Returns the HashMap returns a copy of the fdtable for a cage.  Useful 
// helper function for a caller that needs to examine the table.  Likely could
// be more efficient by letting the caller borrow this...
#[doc = include_str!("../docs/return_fdtable_copy.md")]
pub fn return_fdtable_copy(cageid: u64) -> HashMap<u64, FDTableEntry> {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut myhashmap = HashMap::new();

    let myfdarray = FDTABLE.get(&cageid).unwrap();
    for item in 0..FD_PER_PROCESS_MAX as usize {
        if myfdarray[item].is_some() {
            myhashmap.insert(item as u64,myfdarray[item].unwrap());
        }
    }
    myhashmap
}


#[doc(hidden)]
// Helper to initialize / empty out state so we can test with a clean system...
// This is only used in tests, thus is hidden...
pub fn refresh() {
    FDTABLE.clear();
    FDTABLE.insert(threei::TESTING_CAGEID,[Option::None;FD_PER_PROCESS_MAX as usize]);
}

