
//#;![feature(lazy_cell)]

mod threei;

use lazy_static::lazy_static;

use std::sync::Mutex;

use std::collections::HashMap;

//use std::sync::LazyLock;



// This is a basic fdtables library.  The purpose is to allow a cage to have
// a set of virtual fds which is translated into real fds.  
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
//      get_unused_virtual_fd(cageid,realfd,is_cloexec,optionalinfo) -> Result<virtualfd, EMFILE>
//      set_cloexec(cageid,virtualfd,is_cloexec) -> Result<(), EBADFD>
//
//
// There are other helper functions:
//  
//      get_optionalinfo(cageid,virtualfd) -> Result<optionalinfo, EBADFD>
//      set_optionalinfo(cageid,virtualfd,optionalinfo) -> Result<(), EBADFD>
//          The above two are useful if you want to track other things like
//          an id for other in-memory data structures
//
//      copy_fdtable_for_cage(srccageid, newcageid) -> Result<(), ENFILE>
//          This is mostly used in handling fork, etc.  
//
//      remove_cage_from_fdtable(cageid) 
//          This is mostly used in handling exit, etc.
//
//      close_all_for_exec(cageid) 
//          This handles exec by removing all of the realfds from the cage.
//
//      get_exec_iter(cageid) -> iter()
//          This handles exec by returning an iterator over the realfds,
//          removing each entry after the next iterator element is returned.
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

const FD_PER_PROCESS_MAX:u64 = 1024;

const TOTAL_FD_MAX:u64 = 1024;


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
// I'm using LazyLock because I think this is how I'm supposed to set up 
// static / global variables.
/*static mut fdtable; : LazyLock<HashMap<u64, HashMap<u64,FDTableEntry>>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // Insert a cage so that I have something to fork / test later, if need
    // be.  Otherwise, I'm not sure how I get this started.  I think this 
    // should be invalid from a 3i standpoint, etc.  Could this mask an error 
    // in the future?
    m.insert(threei::TESTING_CAGEID,HashMap::new());
    m
A); */
//static mut fdtable : HashMap<u64, HashMap<u64,FDTableEntry>> = HashMap::new();


// Okay, so I have 

lazy_static! {

  #[derive(Debug)]
  static ref GLOBALFDTABLE: Mutex<HashMap<u64, HashMap<u64,FDTableEntry>>> = {
    let mut m = HashMap::new();
    // Insert a cage so that I have something to fork / test later, if need
    // be. Otherwise, I'm not sure how I get this started. I think this
    // should be invalid from a 3i standpoint, etc. Could this mask an
    // error in the future?
    m.insert(threei::TESTING_CAGEID,HashMap::new());
    Mutex::new(m.into())
  };
}

/*
lazy_static! {
    static ref fdtable: HashMap<u64, HashMap<u64,FDTableEntry>> = {
        let mut m = HashMap::new();
        // Insert a cage so that I have something to fork / test later, if need
        // be.  Otherwise, I'm not sure how I get this started.  I think this 
        // should be invalid from a 3i standpoint, etc.  Could this mask an 
        // error in the future?
        m.insert(INVALID_CAGEID,HashMap::new());
        m
    };
}
*/

// These are the values we look up with at the end...
#[derive(Clone, Copy)]
struct FDTableEntry {
    realfd:u64, // underlying fd (may be a virtual fd below us or a kernel fd)
    should_cloexec:bool, // should I close this when exec is called?
    optionalinfo:u64, // user specified / controlled data
}


// BUG: Right now none of this is thread safe!  I likely need to lock the
// fdtable or similar or else all sorts of race conditions can occur.

pub fn translate_virtual_fd(cageid:u64, virtualfd:u64) -> Result<u64,threei::RetVal> {
    // Get the lock on the fdtable...  I'm not handling "poisoned locks" now
    // where a thread holding the lock died...
    let fdtable = GLOBALFDTABLE.lock().unwrap();

    // They should not be able to pass a new cage I don't know.  I should
    // always have a table for each cage because each new cage is added at fork
    // time
    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
        Some(tableentry) => Ok(tableentry.realfd),
        None => Err(threei::Errno::EBADFD as u64),
    }
}


// This is fairly slow if I just iterate sequentially through numbers.  
// However there are not that many to choose from.  I could pop from a list
// or a set as well...  Likely the best solution is to keep a count of the 
// largest fd handed out and to just use this until you wrap.  This will be
// super fast for a normal cage and will be correct in the weird case.
// Right now, I'll just implement the slow path and will speed this up
// later, if needed.
pub fn get_unused_virtual_fd(cageid:u64,realfd:u64,should_cloexec:bool,optionalinfo:u64) -> Result<u64, threei::RetVal> {

    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }
    // Set up the entry so it has the right info...
    // Note, a HashMap stores its data on the heap!  No need to box it...
    // https://doc.rust-lang.org/book/ch08-03-hash-maps.html#creating-a-new-hash-map
    let myentry = FDTableEntry{
        realfd,
        should_cloexec,
        optionalinfo,
    };


    // Check the fds in order.  
    for fdcandidate in 0..FD_PER_PROCESS_MAX {
        if !fdtable.get(&cageid).unwrap().contains_key(&fdcandidate) {
            // I just checked.  Should not be there...
            fdtable.get_mut(&cageid).unwrap().insert(fdcandidate, myentry.clone());
            return Ok(fdcandidate);
        }
    }

    // I must have checked all fds and failed to find one open.  Fail!
    return Err(threei::Errno::EMFILE as u64);
}


// We're just setting a flag here, so this should be pretty straightforward.
pub fn set_cloexec(cageid:u64,virtualfd:u64,is_cloexec:bool) -> Result<(), threei::RetVal> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

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
    }

}





// I'm including my unit tests in-line, in this code.  Integration tests will
// exist in the tests/ directory.
#[cfg(test)]
mod tests {

    // Import the symbols, etc. in this file...
    use super::*;

    // Helper to empty out state so we can test with a clean system...
    fn flush_fdtable() {
        let mut fdtable = GLOBALFDTABLE.lock().unwrap();
        _ = fdtable.drain();
        fdtable.insert(threei::TESTING_CAGEID,HashMap::new());
    } 


    #[test]
    // Basic test to ensure that I can get a virtual fd for a real fd and
    // find the value in the table afterwards...
    fn get_and_translate_work() {

        flush_fdtable();

        const REALFD:u64 = 10;
        // Acquire a virtual fd...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).unwrap();
        let _ = get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).unwrap();
        let _ = get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).unwrap();
        let _ = get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).unwrap();
        assert_eq!(REALFD, translate_virtual_fd(threei::TESTING_CAGEID, my_virt_fd).unwrap());
    }


    #[test]
    // Let's see if I can change the cloexec flag...
    fn try_set_cloexec() {

        flush_fdtable();

        const REALFD:u64 = 10;
        // Acquire a virtual fd...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).unwrap();
        set_cloexec(threei::TESTING_CAGEID,my_virt_fd,true).unwrap();
    }

    #[test]
    // Let's test to see our functions error gracefully with badfds...
    fn badfd_test() {

        flush_fdtable();

        // some made up number...
        let my_virt_fd = 135;
        assert!(translate_virtual_fd(threei::TESTING_CAGEID, my_virt_fd).is_err());
        assert!(set_cloexec(threei::TESTING_CAGEID,my_virt_fd,true).is_err());
    }


    // Let's use up all the fds and verify we get an error...
    #[test]
    fn use_all_fds_test() {

        flush_fdtable();

        const REALFD:u64 = 10;
        for current in 0..FD_PER_PROCESS_MAX {
            // check to make sure that the number of items is equal to the
            // number of times through this loop...
            //
            // Note: if this test is failing on the next line, it is likely 
            // because some extra fds are allocated for the cage (like stdin, 
            // stdout, and stderr).
            assert_eq!(GLOBALFDTABLE.lock().unwrap().get(&threei::TESTING_CAGEID).unwrap().len(), current as usize);

            let _ = get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).unwrap();
        }
        // If the test is failing by not triggering here, we're not stopping
        // at the limit...
        if get_unused_virtual_fd(threei::TESTING_CAGEID,REALFD,false,100).is_err() {
            flush_fdtable();
        }
        else {
            panic!("Should have raised an error...");
        }

    }
}


