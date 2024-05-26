//  DashMap<u64,HashMap<u64,FDTableEntry>>
//      Just a basic solution with a dashmap instead of a mutex + hashmap
//      Done: GlobalDashMap

use crate::threei;

use dashmap::DashMap;

use lazy_static::lazy_static;

use std::collections::HashMap;

// This is a slightly more advanced fdtables library using DashMap.  
// The purpose is to allow a cage to have a set of virtual fds which is 
// translated into real fds.
//
// For example, suppose a cage with cageid A, wants to open a file.  That open
// operation needs to return a file descriptor to the cage.  Rather than have
// each cage have the actual underlying numeric fd[*], each cage has its own
// virtual fd numbers.  So cageid A's fd 6 will potentially be different from
// cageid B's fd 6.  When a call from cageid A or B is made, this will need to
// be translated from that virtual fd into the read fd[**].
//
// One other complexity deals with the CLOEXEC flag.  If this is set on a file
// descriptor, then when exec is called, it must be closed.  This library
// provides a few functions to simplify this process.
//
// To make this work, this library provides the following funtionality:
//
//      translate_virtual_fd(cageid, virtualfd) -> Result<realfd,EBADFD>
//
//      get_unused_virtual_fd(cageid,realfd,is_cloexec,optionalinfo) -> Result<virtualfd, EMFILE>
//
//      set_cloexec(cageid,virtualfd,is_cloexec) -> Result<(), EBADFD>
//
//      get_specific_virtual_fd(cageid,virtualfd,realfd,is_cloexec,optionalinfo) -> Result<(), (ELIND|EBADF)>
//          This is mostly used for dup2/3.  I'm assuming the caller got the
//          entry already and wants to put it in a location.  Returns ELIND
//          if the entry is occupied and EBADF if out of range...
//
//      copy_fdtable_for_cage(srccageid, newcageid) -> Result<(), ENFILE>
//          This is effectively just making a copy of a specific cage's
//          fdtable, for use in fork()
//
//      remove_cage_from_fdtable(cageid) -> HashMap<virt_fd:u64,FDTableEntry>
//          This is mostly used in handling exit, etc.  Returns the HashMap
//          for the cage.
//
//      empty_fds_for_exec(cageid) -> HashMap<virt_fd:u64,FDTableEntry>
//          This handles exec by removing all of FDTableEntries with cloexec
//          set.  Those are returned in a HashMap
//
//
// There are other helper functions meant to be used when this is imported
// as a grate library::
//
//      get_optionalinfo(cageid,virtualfd) -> Result<optionalinfo, EBADFD>
//      set_optionalinfo(cageid,virtualfd,optionalinfo) -> Result<(), EBADFD>
//          The above two are useful if you want to track other things like
//          an id for other in-memory data structures
//
// In situations where this will be used by a grate, a few other calls are
// particularly useful:
//
//      threeii::reserve_fd(cageid, Option<fd>) -> Result<fd, EMFILE / ENFILE>
//          Used to have the grate, etc. beneath you reserve (or provide) a fd.
//          This is useful for situatiosn where you want to have most fds
//          handled elsewhere, but need to be able to acquire a few for your
//          purposes (like implementing in-memory pipes, etc.)
//
// [*] This isn't possible because fork causes the same fd in the parent and
// child to have separate file pointers (e.g., read / write to separate
// locations in the file).
//
// [**] This is only the 'real' fd from the standpoint of the user of this
// library.  If another part of the system below it, such as another grate or
// the microvisor, is using this library, it will get translated again.
//

// This library is likely the place in the system where we should consider
// putting in place limits on file descriptors.  Linux does this through two
// error codes, one for a per-process limit and the other for an overall system
// limit.  My thinking currently is that both will be configurable values in
// the library.
//
//       EMFILE The per-process limit on the number of open file
//              descriptors has been reached.
//
//       ENFILE The system-wide limit on the total number of open files
//              has been reached.

pub const FD_PER_PROCESS_MAX: u64 = 1024;

// BUG / TODO: Use this in some sane way...
#[allow(dead_code)]
pub const TOTAL_FD_MAX: u64 = 4096;

pub const ALGONAME: &str = "DashMapGlobal";

// These are the values we look up with at the end...
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FDTableEntry {
    pub realfd: u64, // underlying fd (may be a virtual fd below us or
    // a kernel fd)
    pub should_cloexec: bool, // should I close this when exec is called?
    pub optionalinfo: u64,    // user specified / controlled data
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

// In order to store this information, I'm going to use a DashMap which
// has keys of (cageid:u64) and values that are another HashMap.  The second
// HashMap has keys of (virtualfd:64) and values of (realfd:u64,
// should_cloexec:bool, optionalinfo:u64).
//
// To speed up lookups, I could have used arrays instead of HashMaps.  In
// theory, that space is far too large, but likely each could be bounded to
// smaller values like 1024.  For simplicity I avoided this for now.
//
// I thought also about having different tables for the tuple of values
// since they aren't always used together, but this seemed needlessly complex
// (at least at first).
//

// This lets me initialize the code as a global.
lazy_static! {

  #[derive(Debug)]
  // Usually I would care more about this, but I'm keeping this close to
  // the vanilla implementation...
  static ref fdtable: DashMap<u64, HashMap<u64,FDTableEntry>> = {
    let m = DashMap::new();
    // Insert a cage so that I have something to fork / test later, if need
    // be. Otherwise, I'm not sure how I get this started. I think this
    // should be invalid from a 3i standpoint, etc. Could this mask an
    // error in the future?
    m.insert(threei::TESTING_CAGEID,HashMap::new());
    m
  };
}

/// This is the main virtual -> realfd lookup function for fdtables.  
///
/// Converts a virtualfd, which is used in a cage, into the realfd, which 
/// is known to whatever is below us, possibly the OS kernel.
///
/// Panics:
///     if the cageid does not exist
///
/// Errors:
///     if the virtualfd does not exist
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// # let realfd: u64 = 10;
/// let my_virt_fd = get_unused_virtual_fd(cage_id, realfd, false, 100).unwrap();
/// // Check that you get the real fd back here...
/// assert_eq!(realfd,translate_virtual_fd(cage_id, my_virt_fd).unwrap());
/// ```
///     
pub fn translate_virtual_fd(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
    // Get the lock on the fdtable...  I'm not handling "poisoned locks" now
    // where a thread holding the lock died...
//    let fdtable = GLOBALFDTABLE;

    // They should not be able to pass a new cage I don't know.  I should
    // always have a table for each cage because each new cage is added at fork
    // time
    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
        Some(tableentry) => Ok(tableentry.realfd),
        None => Err(threei::Errno::EBADFD as u64),
    };
}

/// Get a virtualfd mapping to put an item into the fdtable.
///
/// This is the overwhelmingly common way to get a virtualfd and should be 
/// used essentially everywhere except in cases like dup2(), where you do 
/// actually care what fd you are assigned.
///
/// Panics:
///     if the cageid does not exist
///
/// Errors:
///     if the cage has used EMFILE virtual descriptors already, return EMFILE
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// # let realfd: u64 = 10;
/// // Should not error...
/// let my_virt_fd = get_unused_virtual_fd(cage_id, realfd, false, 100).unwrap();
/// // Check that you get the real fd back here...
/// assert_eq!(realfd,translate_virtual_fd(cage_id, my_virt_fd).unwrap());
/// ```
// This is fairly slow if I just iterate sequentially through numbers.
// However there are not that many to choose from.  I could pop from a list
// or a set as well...  Likely the best solution is to keep a count of the
// largest fd handed out and to just use this until you wrap.  This will be
// super fast for a normal cage and will be correct in the weird case.
// Right now, I'll just implement the slow path and will speed this up
// later, if needed.
pub fn get_unused_virtual_fd(
    cageid: u64,
    realfd: u64,
    should_cloexec: bool,
    optionalinfo: u64,
) -> Result<u64, threei::RetVal> {
    //let mut fdtable = GLOBALFDTABLE;

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

    // Check the fds in order.
    for fdcandidate in 0..FD_PER_PROCESS_MAX {
        if !fdtable.get(&cageid).unwrap().contains_key(&fdcandidate) {
            // I just checked.  Should not be there...
            fdtable
                .get_mut(&cageid)
                .unwrap()
                .insert(fdcandidate, myentry);
            return Ok(fdcandidate);
        }
    }

    // I must have checked all fds and failed to find one open.  Fail!
    Err(threei::Errno::EMFILE as u64)
}

/// This is used to get a specific virtualfd mapping.
///
/// Useful for implementing something like dup2.  Use this only if you care 
/// which virtualfd you get.  Otherwise use get_unused_virtual_fd().
///
/// Panics:
///     if the cageid does not exist
///
/// Errors:
///     returns ELIND if you're picking an already used virtualfd.  If you
///     want to mimic dup2's behavior, you need to close it first, which the
///     caller should handle.
///     returns EBADF if it's not in the range of valid fds.
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// # let realfd: u64 = 10;
/// # let virtfd: u64 = 1000;
/// // Should not error...
/// assert!(get_specific_virtual_fd(cage_id, virtfd, realfd, false, 100).is_ok());
/// // Check that you get the real fd back here...
/// assert_eq!(realfd,translate_virtual_fd(cage_id, virtfd).unwrap());
/// ```
// This is used for things like dup2, which need a specific fd...
// NOTE: I will assume that the requested_virtualfd isn't used.  If it is, I
// will return ELIND
pub fn get_specific_virtual_fd(
    cageid: u64,
    requested_virtualfd: u64,
    realfd: u64,
    should_cloexec: bool,
    optionalinfo: u64,
) -> Result<(), threei::RetVal> {
    //let mut fdtable = GLOBALFDTABLE;

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

    if fdtable
        .get(&cageid)
        .unwrap()
        .contains_key(&requested_virtualfd)
    {
        Err(threei::Errno::ELIND as u64)
    } else {
        fdtable
            .get_mut(&cageid)
            .unwrap()
            .insert(requested_virtualfd, myentry);
        Ok(())
    }
}

/// Helper function for setting the close on exec (CLOEXEC) flag.
///
/// The reason this information is needed is because the empty_fds_for_exec()
/// call needs to know which fds should be closed and which should be retained.
///
/// Panics:
///     Unknown cageid
///
/// Errors:
///     EBADFD if the virtual file descriptor is incorrect
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// # let realfd: u64 = 10;
/// // Acquire a virtual fd...
/// let my_virt_fd = get_unused_virtual_fd(cage_id, realfd, false, 100).unwrap();
/// // Swap this so it'll be closed when empty_fds_for_exec is called...
/// set_cloexec(cage_id, my_virt_fd, true).unwrap();
/// ```
// We're just setting a flag here, so this should be pretty straightforward.
pub fn set_cloexec(cageid: u64, virtualfd: u64, is_cloexec: bool) -> Result<(), threei::RetVal> {
    //let mut fdtable = GLOBALFDTABLE;

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // Set the is_cloexec flag or return EBADFD, if that's missing...
    return match fdtable.get_mut(&cageid).unwrap().get_mut(&virtualfd) {
        Some(tableentry) => {
            tableentry.should_cloexec = is_cloexec;
            Ok(())
        }
        None => Err(threei::Errno::EBADFD as u64),
    };
}

/// Used to get optional information needed by the library importer.  
///
/// This is useful if you want to assign some sort of index to virtualfds,
/// often if there is no realfd backing them.  For example, if you are 
/// implementing in-memory pipe buffers, this could be the position in an 
/// array where a ring buffer lives.   See also set_optionalinfo()
///
/// Panics:
///     Invalid cageid
///
/// Errors:
///     BADFD if the virtualfd doesn't exist
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// let my_virt_fd = get_unused_virtual_fd(cage_id, 10, false, 12345).unwrap();
/// assert_eq!(get_optionalinfo(cage_id, my_virt_fd).unwrap(),12345);
/// ```
// Super easy, just return the optionalinfo field...
pub fn get_optionalinfo(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
    //let fdtable = GLOBALFDTABLE;
    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
        Some(tableentry) => Ok(tableentry.optionalinfo),
        None => Err(threei::Errno::EBADFD as u64),
    };
}

/// Set optional information needed by the library importer.  
///
/// This is useful if you want to assign some sort of index to virtualfds,
/// often if there is no realfd backing them.  For example, if you are 
/// implementing in-memory pipe buffers, this could be the position in an 
/// array where a ring buffer lives.   See also get_optionalinfo()
///
/// Panics:
///     Invalid cageid
///
/// Errors:
///     BADFD if the virtualfd doesn't exist
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// let my_virt_fd = get_unused_virtual_fd(cage_id, 10, false, 10).unwrap();
/// set_optionalinfo(cage_id, my_virt_fd,12345).unwrap();
/// assert_eq!(get_optionalinfo(cage_id, my_virt_fd).unwrap(),12345);
/// ```
// We're setting an opaque value here. This should be pretty straightforward.
pub fn set_optionalinfo(
    cageid: u64,
    virtualfd: u64,
    optionalinfo: u64,
) -> Result<(), threei::RetVal> {
    //let mut fdtable = GLOBALFDTABLE;

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // Set the is_cloexec flag or return EBADFD, if that's missing...
    return match fdtable.get_mut(&cageid).unwrap().get_mut(&virtualfd) {
        Some(tableentry) => {
            tableentry.optionalinfo = optionalinfo;
            Ok(())
        }
        None => Err(threei::Errno::EBADFD as u64),
    };
}

/// Duplicate a cage's fdtable -- useful for implementing fork()
///
/// This function is effectively just making a copy of a specific cage's
/// fdtable, for use in fork().  Nothing complicated here.
///
/// Panics:
///     Invalid cageid for srccageid
///     Already used cageid for newcageid
///
/// Errors:
///     This will return ENFILE if too many fds are used, if the implementation
///     supports it...
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let src_cage_id = threei::TESTING_CAGEID;
/// # let new_cage_id = threei::TESTING_CAGEID1;
/// let my_virt_fd = get_unused_virtual_fd(src_cage_id, 10, false, 10).unwrap();
/// copy_fdtable_for_cage(src_cage_id,new_cage_id).unwrap();
/// // Check that this entry exists under the new_cage_id...
/// assert_eq!(get_optionalinfo(new_cage_id, my_virt_fd).unwrap(),10);
/// ```
// Helper function used for fork...  Copies an fdtable for another process
pub fn copy_fdtable_for_cage(srccageid: u64, newcageid: u64) -> Result<(), threei::Errno> {
    //let mut fdtable = GLOBALFDTABLE;

    if !fdtable.contains_key(&srccageid) {
        panic!("Unknown srccageid in fdtable access");
    }
    if fdtable.contains_key(&newcageid) {
        panic!("Known newcageid in fdtable access");
    }

    // Insert a copy and ensure it didn't exist...
    let hmcopy = fdtable.get(&srccageid).unwrap().clone();
    assert!(fdtable.insert(newcageid, hmcopy).is_none());
    Ok(())
    // I'm not going to bother to check the number of fds used overall yet...
    //    Err(threei::Errno::EMFILE as u64),
}

/// discards a cage -- likely for handling exit()
///
/// This is mostly used in handling exit, etc.  Returns the HashMap for the 
/// cage, so that the caller can close realfds, etc. as is needed.
///
/// Panics:
///     Invalid cageid
///
/// Errors:
///     None
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let src_cage_id = threei::TESTING_CAGEID;
/// # let cage_id = threei::TESTING_CAGEID2;
/// # copy_fdtable_for_cage(src_cage_id,cage_id).unwrap();
/// let my_virt_fd = get_unused_virtual_fd(cage_id, 10, false, 10).unwrap();
/// let my_cages_fdtable = remove_cage_from_fdtable(cage_id);
/// assert!(my_cages_fdtable.get(&my_virt_fd).is_some());
/// //   If we do the following line, it would panic, since the cage_id has 
/// //   been removed from the table...
/// // get_unused_virtual_fd(cage_id, 10, false, 10)
/// ```
// This is mostly used in handling exit, etc.  Returns the HashMap
// for the cage.
pub fn remove_cage_from_fdtable(cageid: u64) -> HashMap<u64, FDTableEntry> {
    //let mut fdtable = GLOBALFDTABLE;

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    fdtable.remove(&cageid).unwrap().1
}

/// removes and returns a hashmap of all entries with should_cloexec set
///
/// This goes through every entry in a cage's fdtable and removes all entries
/// that have should_cloexec set to true.  These entries are all added to a
/// new hashmap which is returend.  This is useful for handling exec, as the
/// caller can now decide how to handle each fd.
///
/// Panics:
///     Invalid cageid
///
/// Errors:
///     None
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let src_cage_id = threei::TESTING_CAGEID;
/// # let cage_id = threei::TESTING_CAGEID3;
/// # copy_fdtable_for_cage(src_cage_id,cage_id).unwrap();
/// let my_virt_fd = get_unused_virtual_fd(cage_id, 20, true, 17).unwrap();
/// let my_virt_fd2 = get_unused_virtual_fd(cage_id, 33, false, 16).unwrap();
/// let cloexec_fdtable = empty_fds_for_exec(cage_id);
/// // The first fd should be closed and returned...
/// assert!(cloexec_fdtable.get(&my_virt_fd).is_some());
/// // So isn't in the original table anymore...
/// assert!(translate_virtual_fd(cage_id, my_virt_fd).is_err());
/// // The second fd isn't returned...
/// assert!(cloexec_fdtable.get(&my_virt_fd2).is_none());
/// // Because it is still in the original table...
/// assert!(translate_virtual_fd(cage_id, my_virt_fd2).is_ok());
/// ```
// This removes all fds with the should_cloexec flag set.  They are returned
// in a new hashmap...
pub fn empty_fds_for_exec(cageid: u64) -> HashMap<u64, FDTableEntry> {
    //let mut fdtable = GLOBALFDTABLE;

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    // Create this hashmap through an lambda that checks should_cloexec...
    // See: https://doc.rust-lang.org/std/collections/struct.HashMap.html#method.extract_if

    fdtable
        .get_mut(&cageid)
        .unwrap()
        .extract_if(|_k, v| v.should_cloexec)
        .collect()
}

/// gets a copy of a cage's fdtable hashmap
///
/// Utility function that some callers may want.  I'm not sure why this is 
/// needed exactly
///
/// Panics:
///     Invalid cageid
///
/// Errors:
///     None
///
/// Example:
/// ```
/// # use fdtables::*;
/// # let cage_id = threei::TESTING_CAGEID;
/// let my_virt_fd = get_unused_virtual_fd(cage_id, 10, false, 10).unwrap();
/// let my_cages_fdtable = return_fdtable_copy(cage_id);
/// assert!(my_cages_fdtable.get(&my_virt_fd).is_some());
/// // I can modify the cage table after this and the changes won't show up
/// // in my local HashMap since this is a copy...
/// ```
// Returns the HashMap returns a copy of the fdtable for a cage.  Useful 
// helper function for a caller that needs to examine the table.  Likely could
// be more efficient by letting the caller borrow this...
pub fn return_fdtable_copy(cageid: u64) -> HashMap<u64, FDTableEntry> {
    //let fdtable = GLOBALFDTABLE;

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    fdtable.get(&cageid).unwrap().clone()
}

#[doc(hidden)]
// Helper to initialize / empty out state so we can test with a clean system...
// This is only used in tests, thus is hidden...
pub fn refresh() {
    //let mut fdtable = GLOBALFDTABLE;
    fdtable.clear();
    fdtable.insert(threei::TESTING_CAGEID, HashMap::new());
}