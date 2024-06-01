use crate::threei;

use lazy_static::lazy_static;

use std::sync::Mutex;

use std::collections::HashMap;

// This fdtables library tracks the maxfd so it can more quickly get an unused
// file descriptor.  


// Get constants about the fd table sizes, etc.
pub use super::commonconstants::*;

// algorithm name.  Need not be listed in the docs.
#[doc(hidden)]
pub const ALGONAME: &str = "MutHashMaxGlobal";

// These are the values we look up with at the end...
#[doc = include_str!("../docs/fdtableentry.md")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FDTableEntry {
    pub realfd: u64, // underlying fd (may be a virtual fd below us or
    // a kernel fd)
    pub should_cloexec: bool, // should I close this when exec is called?
    pub optionalinfo: u64,    // user specified / controlled data
}

#[derive(Clone, Debug)]
struct FDTable {
    highestneverusedfd: u64, // Never resets (even after close).  Used to 
                            // let us quickly get an unused fd
    thisfdtable: HashMap<u64,FDTableEntry>, // the virtfd -> entry map
}

// It's fairly easy to check the fd count on a per-process basis (I just check
// when I would
// add a new fd).
//
// BUG: I will ignore the total limit for now.  I would ideally do this on
// every creation, close, fork, etc. but it's a PITA to track this.

// We will raise a panic anywhere we receive an unknown cageid.  This frankly
// should not be possible and indicates some sort of internal error in our
// code.  However, it is expected we could receive an invalid file descriptor
// when a cage makes a call.

// In order to store this information, I'm going to use a HashMap which
// has keys of (cageid:u64) and values that are a table with a HashMap and
// a counter of the highestneverusedfd.
// HashMap has keys of (virtualfd:64) and values of (realfd:u64,
// should_cloexec:bool, optionalinfo:u64).
//
// I thought also about having different tables for the entries
// since they aren't always used together, but this seemed needlessly complex
// (at least at first).
//

// This lets me initialize the code as a global.
lazy_static! {

  #[derive(Debug)]
  static ref GLOBALFDTABLE: Mutex<HashMap<u64, FDTable>> = {
    let mut m = HashMap::new();
    // Insert a cage so that I have something to fork / test later, if need
    // be. Otherwise, I'm not sure how I get this started. I think this
    // should be invalid from a 3i standpoint, etc. Could this mask an
    // error in the future?
    //
    //
    let newmap = HashMap::new();
    let emptytab = FDTable{
        highestneverusedfd:0,
        thisfdtable:newmap,
    };

    m.insert(threei::TESTING_CAGEID,emptytab);
    Mutex::new(m)
  };
}

lazy_static! {
  // This is needed for close and similar functionality.  I need track the
  // number of times a realfd is open
  #[derive(Debug)]
  static ref GLOBALREALFDCOUNT: Mutex<HashMap<u64, u64>> = {
    Mutex::new(HashMap::new())
  };

}

// Internal helper to hold the close handlers...
struct CloseHandlers {
    intermediate_handler: fn(u64),
    final_handler: fn(u64),
    unreal_handler: fn(u64),
}

// Seems sort of like a constant...  I'm not sure if this is bad form or not...
#[allow(non_snake_case)]
pub fn NULL_FUNC(_:u64) { }

lazy_static! {
    // This holds the user registered handlers they want to have called when
    // a close occurs.  I did this rather than return messy data structures
    // from the close, exec, and exit handlers because it seemed cleaner...
    #[derive(Debug)]
    static ref CLOSEHANDLERTABLE: Mutex<CloseHandlers> = {
        let c = CloseHandlers {
            intermediate_handler:NULL_FUNC,
            final_handler:NULL_FUNC,
            unreal_handler:NULL_FUNC,
        };
        Mutex::new(c)
    };
}

#[doc = include_str!("../docs/translate_virtual_fd.md")]
pub fn translate_virtual_fd(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
    // Get the lock on the fdtable...  I'm not handling "poisoned locks" now
    // where a thread holding the lock died...
    let fdtable = GLOBALFDTABLE.lock().unwrap();

    // They should not be able to pass a new cage I don't know.  I should
    // always have a table for each cage because each new cage is added at fork
    // time
    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match fdtable.get(&cageid).unwrap().thisfdtable.get(&virtualfd) {
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
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
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

    let myfdentry = fdtable.get_mut(&cageid).unwrap();

    if myfdentry.highestneverusedfd < FD_PER_PROCESS_MAX {
        _increment_realfd(realfd);
        // We have an entry we've never touched!
        myfdentry.thisfdtable.insert(myfdentry.highestneverusedfd, myentry);
        myfdentry.highestneverusedfd += 1;
        return Ok(myfdentry.highestneverusedfd-1);
    }

    let myfdentry = fdtable.get_mut(&cageid).unwrap();
    let myfdmap = &mut myfdentry.thisfdtable;

    // Check the fds in order.
    for fdcandidate in 0..FD_PER_PROCESS_MAX {
        // Get the entry if it's Vacant and assign it to e (so I can fill
        // it in).
        if let std::collections::hash_map::Entry::Vacant(e) = myfdmap.entry(fdcandidate) {
            e.insert(myentry);
            _increment_realfd(realfd);
            return Ok(fdcandidate);
        }
    }

    // I must have checked all fds and failed to find one open.  Fail!
    Err(threei::Errno::EMFILE as u64)
}

// This is used for things like dup2, which need a specific fd...
// If the requested_virtualfd is used, I close it...
#[doc = include_str!("../docs/get_specific_virtual_fd.md")]
pub fn get_specific_virtual_fd(
    cageid: u64,
    requested_virtualfd: u64,
    realfd: u64,
    should_cloexec: bool,
    optionalinfo: u64,
) -> Result<(), threei::RetVal> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
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

    // I moved this up so that if I decrement the same realfd, it calls
    // the intermediate handler instead of the final one.
    _increment_realfd(realfd);
    if let Some(entry) = fdtable.get(&cageid).unwrap().thisfdtable.get(&requested_virtualfd)  {
        if entry.realfd != NO_REAL_FD {
                        _decrement_realfd(entry.realfd);
        }
        else {
            // Let their code know this has been closed...
            let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
            (closehandlers.unreal_handler)(entry.optionalinfo);
        }
    }

    // always add the new entry
    fdtable.get_mut(&cageid).unwrap().thisfdtable.insert(requested_virtualfd,myentry);
    Ok(())
}

// We're just setting a flag here, so this should be pretty straightforward.
#[doc = include_str!("../docs/set_cloexec.md")]
pub fn set_cloexec(cageid: u64, virtualfd: u64, is_cloexec: bool) -> Result<(), threei::RetVal> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // Set the is_cloexec flag or return EBADFD, if that's missing...
    return match fdtable.get_mut(&cageid).unwrap().thisfdtable.get_mut(&virtualfd) {
        Some(tableentry) => {
            tableentry.should_cloexec = is_cloexec;
            Ok(())
        }
        None => Err(threei::Errno::EBADFD as u64),
    };
}

// Super easy, just return the optionalinfo field...
#[doc = include_str!("../docs/get_optionalinfo.md")]
pub fn get_optionalinfo(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
    let fdtable = GLOBALFDTABLE.lock().unwrap();
    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match fdtable.get(&cageid).unwrap().thisfdtable.get(&virtualfd) {
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
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // Set optionalinfo or return EBADFD, if that's missing...
    return match fdtable.get_mut(&cageid).unwrap().thisfdtable.get_mut(&virtualfd) {
        Some(tableentry) => {
            tableentry.optionalinfo = optionalinfo;
            Ok(())
        }
        None => Err(threei::Errno::EBADFD as u64),
    };
}

// Helper function used for fork...  Copies an fdtable for another process
#[doc = include_str!("../docs/copy_fdtable_for_cage.md")]
pub fn copy_fdtable_for_cage(srccageid: u64, newcageid: u64) -> Result<(), threei::Errno> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&srccageid) {
        panic!("Unknown srccageid in fdtable access");
    }
    if fdtable.contains_key(&newcageid) {
        panic!("Known newcageid in fdtable access");
    }

    // Insert a copy and ensure it didn't exist...
    let hmcopy = fdtable.get(&srccageid).unwrap().clone();

    // increment the reference to items in the fdtable appropriately...
    for v in fdtable.get(&srccageid).unwrap().thisfdtable.values() {
        if v.realfd != NO_REAL_FD {
            _increment_realfd(v.realfd);
        }
    }

    // insert the new table...
    assert!(fdtable.insert(newcageid, hmcopy).is_none());
    Ok(())
    // I'm not going to bother to check the number of fds used overall yet...
    //    Err(threei::Errno::EMFILE as u64),
}

// This is mostly used in handling exit, etc.  Returns the HashMap
// for the cage.
#[doc = include_str!("../docs/remove_cage_from_fdtable.md")]
pub fn remove_cage_from_fdtable(cageid: u64) {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // decrement the reference to items in the fdtable appropriately...
    for v in fdtable.get(&cageid).unwrap().thisfdtable.values() {
        if v.realfd != NO_REAL_FD {
            _decrement_realfd(v.realfd);
        }
        else {
            // Let their code know this has been closed...
            let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
            (closehandlers.unreal_handler)(v.optionalinfo);
        }
    }


    fdtable.remove(&cageid).unwrap();
}

// This removes all fds with the should_cloexec flag set.  They are returned
// in a new hashmap...
#[doc = include_str!("../docs/empty_fds_for_exec.md")]
pub fn empty_fds_for_exec(cageid: u64) {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // Create this hashmap through an lambda that checks should_cloexec...
    // See: https://doc.rust-lang.org/std/collections/struct.HashMap.html#method.extract_if

/*    fdtable
        .get_mut(&cageid)
        .unwrap()
        .extract_if(|_k, v| v.should_cloexec)
        .collect()*/

    // I'm writing the below code to avoid using the extract_if experimental 
    // nightly function...
    let thiscagefdtable = &mut fdtable.get_mut(&cageid).unwrap().thisfdtable;

    let mut without_cloexec_hm:HashMap<u64,FDTableEntry> = HashMap::new();
    for (k,v) in thiscagefdtable.drain() {
        if v.should_cloexec {
            if v.realfd == NO_REAL_FD {
                // Let their code know this has been closed...
                let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
                (closehandlers.unreal_handler)(v.optionalinfo);
            }
            else {
                // Let the helper tell the user and decrement the count
                _decrement_realfd(v.realfd);
            }
        }
        else{
            without_cloexec_hm.insert(k,v);
        }

    }

    let newhighest = fdtable.get(&cageid).unwrap().highestneverusedfd;
    let newfdtable = FDTable {
        highestneverusedfd:newhighest,
        thisfdtable:without_cloexec_hm,
    };

    // Put the ones without_cloexec back in the hashmap...
    fdtable.insert(cageid,newfdtable);

}

// Helper for close.  Returns a tuple of realfd, number of references
// remaining.
#[doc = include_str!("../docs/close_virtualfd.md")]
pub fn close_virtualfd(cageid:u64, virtfd:u64) -> Result<(),threei::RetVal> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let thiscagesfdtable = &mut fdtable.get_mut(&cageid).unwrap().thisfdtable;

    match thiscagesfdtable.remove(&virtfd) {
        Some(entry) =>
            if entry.realfd == NO_REAL_FD {
                // Let their code know this has been closed...
                let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
                (closehandlers.unreal_handler)(entry.optionalinfo);
                Ok(())
            }
            else {
                _decrement_realfd(entry.realfd);
                Ok(())
            }
        None => Err(threei::Errno::EBADFD as u64),
    }
}

// returns a copy of the fdtable for a cage.  Useful helper function for a
// caller that needs to examine the table.  Likely could be more efficient by
// letting the caller borrow this...
#[doc = include_str!("../docs/return_fdtable_copy.md")]
pub fn return_fdtable_copy(cageid: u64) -> HashMap<u64, FDTableEntry> {
    let fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    fdtable.get(&cageid).unwrap().thisfdtable.clone()
}

// Register a series of helpers to be called for close.  Can be called
// multiple times to override the older helpers.
#[doc = include_str!("../docs/register_close_handlers.md")]
pub fn register_close_handlers(intermediate_handler: fn(u64), final_handler: fn(u64), unreal_handler: fn(u64)) {
    // Unlock the table and set the handlers...
    let mut closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
    closehandlers.intermediate_handler = intermediate_handler;
    closehandlers.final_handler = final_handler;
    closehandlers.unreal_handler = unreal_handler;
}

// Helper to initialize / empty out state so we can test with a clean system...
// only used when testing...
//
// I'm cleaning up "poisoned" mutexes here so that I can handle tests that 
// panic
#[doc(hidden)]
pub fn refresh() {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap_or_else(|e| {
        GLOBALFDTABLE.clear_poison();
        e.into_inner()
    });

    fdtable.clear();

    let newmap = HashMap::new();
    let emptytab = FDTable{
        highestneverusedfd:0,
        thisfdtable:newmap,
    };

    fdtable.insert(threei::TESTING_CAGEID, emptytab);
    let mut closehandlers = CLOSEHANDLERTABLE.lock().unwrap_or_else(|e| {
        CLOSEHANDLERTABLE.clear_poison();
        e.into_inner()
    });

    closehandlers.intermediate_handler = NULL_FUNC;
    closehandlers.final_handler = NULL_FUNC;
    closehandlers.unreal_handler = NULL_FUNC;

    let mut _realfdcount = GLOBALREALFDCOUNT.lock().unwrap_or_else(|e| {
        GLOBALREALFDCOUNT.clear_poison();
        e.into_inner()
    });
}

// Helpers to track the count of times each realfd is used
#[doc(hidden)]
fn _decrement_realfd(realfd:u64) -> u64 {
    // Do nothing if it's not a realfd...
    if realfd == NO_REAL_FD {
        panic!("Called _decrement_realfd with NO_REAL_FD");
    }

    // Get this table's lock...
    let mut realfdcount = GLOBALREALFDCOUNT.lock().unwrap();

    let newcount:u64 = realfdcount.get(&realfd).unwrap() - 1;
    let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
    if newcount > 0 {
        (closehandlers.intermediate_handler)(realfd);
        realfdcount.insert(realfd,newcount);
    }
    else {
        (closehandlers.final_handler)(realfd);
    }
    newcount
}

// Helpers to track the count of times each realfd is used
#[doc(hidden)]
fn _increment_realfd(realfd:u64) -> u64 {
    if realfd == NO_REAL_FD {
        return 0
    }

    // Get this table's lock...
    let mut realfdcount = GLOBALREALFDCOUNT.lock().unwrap();

    // Get a mutable reference to the entry so we can update it.
    return match realfdcount.get_mut(&realfd) {
        Some(count) => {
            *count += 1;
            *count
        }
        None => {
            realfdcount.insert(realfd, 1);
            1
        }
    }
}