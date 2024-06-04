//  DashMap<u64,[Option<FDTableEntry>;FD_PER_PROCESSS_MAX]>  Space is ~24KB 
//  per cage w/ 1024 fds?!?
//      Static DashMap.  Let's see if having the FDTableEntries be a static
//      array is any faster...

use crate::threei;

use dashmap::DashMap;

use lazy_static::lazy_static;

use std::collections::HashMap;

use std::sync::Mutex;

// This uses a Dashmap (for cages) with an array of FDTableEntry items.

// Get constants about the fd table sizes, etc.
pub use super::commonconstants::*;

// algorithm name.  Need not be listed.  Used in benchmarking output
#[doc(hidden)]
pub const ALGONAME: &str = "DashMapArrayGlobal";

// These are the values we look up with at the end...
#[doc = include_str!("../docs/fdtableentry.md")]
#[derive(Clone, Copy, Debug, PartialEq,Eq, Hash)]
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

lazy_static! {
    // This is needed for close and similar functionality.  I need track the
    // number of times a realfd is open
    #[derive(Debug)]
    static ref REALFDCOUNT: DashMap<u64, u64> = {
        DashMap::new()
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

    // I moved this up so that if I decrement the same realfd, it calls
    // the intermediate handler instead of the final one.
    _increment_realfd(realfd);
    if let Some(entry) = FDTABLE.get(&cageid).unwrap()[requested_virtualfd as usize] {
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
    FDTABLE.get_mut(&cageid).unwrap()[requested_virtualfd as usize] = Some(myentry);
    Ok(())
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
    // BUG: Is this a copy!?!  Am I passing a ref to the same thing!?!?!?
    let hmcopy = *FDTABLE.get(&srccageid).unwrap();

    // Increment copied items
    for entry in FDTABLE.get(&srccageid).unwrap().iter() {
        if entry.is_some() {
            let thisrealfd = entry.unwrap().realfd;
            if thisrealfd != NO_REAL_FD {
                _increment_realfd(thisrealfd);
            }
        }
    }

    assert!(FDTABLE.insert(newcageid, hmcopy).is_none());
    Ok(())
    // I'm not going to bother to check the number of fds used overall yet...
    //    Err(threei::Errno::EMFILE as u64),
}

// This is mostly used in handling exit, etc.  Returns the HashMap
// for the cage.
#[doc = include_str!("../docs/remove_cage_from_fdtable.md")]
pub fn remove_cage_from_fdtable(cageid: u64) {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }


    let myfdarray = FDTABLE.get(&cageid).unwrap();
    for item in 0..FD_PER_PROCESS_MAX as usize {
        if myfdarray[item].is_some() {
            let therealfd = myfdarray[item].unwrap().realfd;
            if therealfd != NO_REAL_FD {
                _decrement_realfd(therealfd);
            }
            else{
                // Let their code know this has been closed...
                let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
                (closehandlers.unreal_handler)(myfdarray[item].unwrap().optionalinfo);
            }
        }
    }
    // I need to do this or else I'll try to double claim the lock and
    // deadlock...
    drop(myfdarray);

    FDTABLE.remove(&cageid);

}

// This removes all fds with the should_cloexec flag set.  They are returned
// in a new hashmap...
#[doc = include_str!("../docs/empty_fds_for_exec.md")]
pub fn empty_fds_for_exec(cageid: u64) {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut myfdarray = FDTABLE.get_mut(&cageid).unwrap();
    for item in 0..FD_PER_PROCESS_MAX as usize {
        if myfdarray[item].is_some() && myfdarray[item].unwrap().should_cloexec {
            let therealfd = myfdarray[item].unwrap().realfd;
            if therealfd != NO_REAL_FD {
                _decrement_realfd(therealfd);
            }
            else{
                // Let their code know this has been closed...
                let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
                (closehandlers.unreal_handler)(myfdarray[item].unwrap().optionalinfo);
            }
            myfdarray[item] = None;
        }
    }

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



/******************* CLOSE SPECIFIC FUNCTIONALITY *******************/

// Helper for close.  Returns a tuple of realfd, number of references
// remaining.
#[doc = include_str!("../docs/close_virtualfd.md")]
pub fn close_virtualfd(cageid:u64, virtfd:u64) -> Result<(),threei::RetVal> {
    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut myfdarray = FDTABLE.get_mut(&cageid).unwrap();


    if myfdarray[virtfd as usize].is_some() {
        let therealfd = myfdarray[virtfd as usize].unwrap().realfd;

        if therealfd == NO_REAL_FD {
            // Let their code know this has been closed...
            let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
            (closehandlers.unreal_handler)(myfdarray[virtfd as usize].unwrap().optionalinfo);
            // Zero out this entry...
            myfdarray[virtfd as usize] = None;
            return Ok(());
        }
        _decrement_realfd(therealfd);
        // Zero out this entry...
        myfdarray[virtfd as usize] = None;
        return Ok(());
    }
    Err(threei::Errno::EBADFD as u64)
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


// Helpers to track the count of times each realfd is used
#[doc(hidden)]
fn _decrement_realfd(realfd:u64) -> u64 {
    if realfd == NO_REAL_FD {
        panic!("Called _decrement_realfd with NO_REAL_FD");
    }

    let newcount:u64 = REALFDCOUNT.get(&realfd).unwrap().value() - 1;
    let closehandlers = CLOSEHANDLERTABLE.lock().unwrap();
    if newcount > 0 {
        (closehandlers.intermediate_handler)(realfd);
        REALFDCOUNT.insert(realfd,newcount);
    }
    else{
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

    // Get a mutable reference to the entry so we can update it.
    return match REALFDCOUNT.get_mut(&realfd) {
        Some(mut count) => {
            *count += 1;
            *count
        }
        None => {
            REALFDCOUNT.insert(realfd, 1);
            1
        }
    }
}



/***************   Code for handling select() ****************/

use libc::fd_set;
use std::collections::HashSet;
use std::cmp;
use std::mem;

// Helper to get an empty fd_set.  Helper function to isolate unsafe code, 
// etc.
pub fn _init_fd_set() -> fd_set {
    let raw_fd_set:fd_set;
    unsafe {
        let mut this_fd_set = mem::MaybeUninit::<libc::fd_set>::uninit();
        libc::FD_ZERO(this_fd_set.as_mut_ptr());
        raw_fd_set = this_fd_set.assume_init()
    }
    raw_fd_set
}

// Helper to get a null pointer.
pub fn _get_null_fd_set() -> fd_set {
    //unsafe{ptr::null_mut()}
    // BUG!!!  Need to fix this later.
    _init_fd_set()
}

pub fn _fd_set(fd:u64, thisfdset:&mut fd_set) {
    unsafe{libc::FD_SET(fd as i32,thisfdset)}
}

pub fn _fd_isset(fd:u64, thisfdset:&fd_set) -> bool {
    unsafe{libc::FD_ISSET(fd as i32,thisfdset)}
}

// Computes the bitmodifications and returns a (maxnfds, unrealset) tuple...
fn _do_bitmods(myfdarray:[Option<FDTableEntry>;FD_PER_PROCESS_MAX as usize], nfds:u64, infdset: fd_set, thisfdset: &mut fd_set, mappingtable: &mut HashMap<u64,u64>) -> Result<(u64,HashSet<(u64,u64)>),threei::RetVal> {
    let mut unrealhashset:HashSet<(u64,u64)> = HashSet::new();
    // Iterate through the infdset and set those values as is appropriate
    let mut highestpos = 0;

    // Clippy is somehow missing how pos is using bit.  
    #[allow(clippy::needless_range_loop)]
    for bit in 0..nfds as usize {
        let pos = bit as u64;
        if _fd_isset(pos,&infdset) {
            if let Some(entry) = myfdarray[bit] {
                if entry.realfd == NO_REAL_FD {
                    unrealhashset.insert((pos,entry.optionalinfo));
                }
                else {
                    mappingtable.insert(entry.realfd, pos);
                    _fd_set(entry.realfd,thisfdset);
                    // I add one because select expects nfds to be the max+1
                    highestpos = cmp::max(highestpos, entry.realfd+1);
                }
            }
            else {
                return Err(threei::Errno::EINVAL as u64);
            }
        }
    }
    Ok((highestpos,unrealhashset))
}

// helper to call before calling select beneath you.  Translates your virtfds 
// to realfds.
// See: https://man7.org/linux/man-pages/man2/select.2.html for details / 
// corner cases about the arguments.

// I hate doing these, but don't know how to make this interface better...
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
#[doc = include_str!("../docs/get_real_bitmasks_for_select.md")]
pub fn get_real_bitmasks_for_select(cageid:u64, nfds:u64, readbits:Option<fd_set>, writebits:Option<fd_set>, exceptbits:Option<fd_set>) -> Result<(u64, fd_set, fd_set, fd_set, [HashSet<(u64,u64)>;3], HashMap<u64,u64>),threei::RetVal> {
    
    if nfds >= FD_PER_PROCESS_MAX {
        return Err(threei::Errno::EINVAL as u64);
    }

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut unrealarray:[HashSet<(u64,u64)>;3] = [HashSet::new(),HashSet::new(),HashSet::new()];
    let mut mappingtable:HashMap<u64,u64> = HashMap::new();
    let mut newnfds = 0;

    // dashmaps are lockless, but usually I would grab a lock on the fdtable
    // here...  
    let binding = FDTABLE.get(&cageid).unwrap();
    let thefdvec = *binding.value();

        // putting results in a vec was the cleanest way I found to do this..
    let mut resultvec = Vec::new();

    for (unrealoffset, inset) in [readbits,writebits, exceptbits].into_iter().enumerate() {
        match inset {
            Some(virtualbits) => {
                let mut retset = _init_fd_set();
                let (thisnfds,myunrealhashset) = _do_bitmods(thefdvec,nfds,virtualbits, &mut retset,&mut mappingtable)?;
                resultvec.push(retset);
                newnfds = cmp::max(thisnfds, newnfds);
                unrealarray[unrealoffset] = myunrealhashset;
            }
            None => {
                // This item is null.  No unreal items
                // BUG: Need to actually return null!
                resultvec.push(_get_null_fd_set());
                unrealarray[unrealoffset] = HashSet::new();
            }
        }
    }

    Ok((newnfds, resultvec[0], resultvec[1], resultvec[2], unrealarray, mappingtable))
    
}


// helper to call after calling select beneath you.  returns the fd_sets you 
// need for your return from a select call and the number of unique flags
// set...

// I hate doing these, but don't know how to make this interface better...
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
#[doc = include_str!("../docs/get_virtual_bitmasks_from_select_result.md")]
pub fn get_virtual_bitmasks_from_select_result(nfds:u64, readbits:fd_set, writebits:fd_set, exceptbits:fd_set,unrealreadset:HashSet<u64>, unrealwriteset:HashSet<u64>, unrealexceptset:HashSet<u64>, mappingtable:HashMap<u64,u64>) -> Result<(u64, fd_set, fd_set, fd_set),threei::RetVal> {

    // Note, I don't need the cage_id here because I have the mappingtable...

    if nfds >= FD_PER_PROCESS_MAX {
        panic!("This shouldn't be possible because we shouldn't have returned this previously")
    }

    let mut flagsset = 0;
    let mut retvec = Vec::new();

        for (inset,unrealset) in [(readbits,unrealreadset), (writebits,unrealwriteset), (exceptbits,unrealexceptset)] {
        let mut retbits = _init_fd_set();
        for bit in 0..nfds as usize {
            let pos = bit as u64;
            if _fd_isset(pos,&inset)&& !_fd_isset(*mappingtable.get(&pos).unwrap(),&retbits) {
                flagsset+=1;
                _fd_set(*mappingtable.get(&pos).unwrap(),&mut retbits);
            }
        }
        for virtfd in unrealset {
            if !_fd_isset(virtfd,&retbits) {
                flagsset+=1;
                _fd_set(virtfd,&mut retbits);
            }
        }
        retvec.push(retbits);
    }

    Ok((flagsset,retvec[0],retvec[1],retvec[2]))

}



/********************** POLL SPECIFIC FUNCTIONS **********************/

// helper to call before calling poll beneath you.  replaces the fds in 
// the poll struct with virtual versions and returns the items you need
// to check yourself...
#[allow(clippy::type_complexity)]
#[doc = include_str!("../docs/convert_virtualfds_to_real.md")]
pub fn convert_virtualfds_to_real(cageid:u64, virtualfds:Vec<u64>) -> (Vec<u64>, Vec<(u64,u64)>, Vec<u64>, HashMap<u64,u64>) {

    if !FDTABLE.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    let mut unrealvec = Vec::new();
    let mut realvec = Vec::new();
    let mut invalidvec = Vec::new();
    let thefdarray = *FDTABLE.get(&cageid).unwrap();
    let mut mappingtable:HashMap<u64,u64> = HashMap::new();

    // BUG?: I'm ignoring the fact that virtualfds can show up multiple times.
    // I'm not sure this actually matters, but I didn't think hard about it.
    for virtfd in virtualfds {
        match thefdarray[virtfd as usize] {
            Some(entry) => {
                // always append the value here.  NO_REAL_FD will be added
                // in the appropriate places to tell them to handle those calls
                // themself.  
                realvec.push(entry.realfd);
                if entry.realfd == NO_REAL_FD {
                    unrealvec.push((virtfd,entry.optionalinfo));
                }
                else{
                    mappingtable.insert(entry.realfd, virtfd);
                }
            }
            None => {
                // Add this because they need to handle it if POLLNVAL is set.
                // An exception should not be raised!!!
                realvec.push(INVALID_FD);
                invalidvec.push(virtfd);
            }
        }
    }

    (realvec, unrealvec, invalidvec, mappingtable)
}



// helper to call after calling poll.  replaces the realfds the vector
// with virtual ones...
#[doc = include_str!("../docs/convert_realfds_back_to_virtual.md")]
pub fn convert_realfds_back_to_virtual(realfds:Vec<u64>, mappingtable:HashMap<u64,u64>) -> Vec<u64> {

    // I don't care what cage was used, and don't need to lock anything...
    // I have the mappingtable!
    
    let mut virtvec = Vec::new();

    for realfd in realfds {
        virtvec.push(*mappingtable.get(&realfd).unwrap());
    }

    virtvec
}






/********************** TESTING HELPER FUNCTION **********************/

#[doc(hidden)]
// Helper to initialize / empty out state so we can test with a clean system...
// This is only used in tests, thus is hidden...
pub fn refresh() {
    FDTABLE.clear();
    FDTABLE.insert(threei::TESTING_CAGEID,[Option::None;FD_PER_PROCESS_MAX as usize]);
    let mut closehandlers = CLOSEHANDLERTABLE.lock().unwrap_or_else(|e| {
        CLOSEHANDLERTABLE.clear_poison();
        e.into_inner()
    });
    closehandlers.intermediate_handler = NULL_FUNC;
    closehandlers.final_handler = NULL_FUNC;
    closehandlers.unreal_handler = NULL_FUNC;
    // Note, it doesn't seem that Dashmaps can be poisoned...
}
