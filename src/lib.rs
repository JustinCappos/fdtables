mod threei;

use lazy_static::lazy_static;

use std::sync::Mutex;

use std::collections::HashMap;

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
//      iterate_over_fdtable(cageid) -> Values<'_, K, V>
//          returns an iterator over the elements in the cage.
//
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

const FD_PER_PROCESS_MAX: u64 = 1024;

// BUG / TODO: Use this in some sane way...
#[allow(dead_code)]
const TOTAL_FD_MAX: u64 = 4096;

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

// This lets me initialize the code as a global.
// BUG / TODO: Use a DashMap instead of a Mutex for this?
lazy_static! {

  #[derive(Debug)]
  static ref GLOBALFDTABLE: Mutex<HashMap<u64, HashMap<u64,FDTableEntry>>> = {
    let mut m = HashMap::new();
    // Insert a cage so that I have something to fork / test later, if need
    // be. Otherwise, I'm not sure how I get this started. I think this
    // should be invalid from a 3i standpoint, etc. Could this mask an
    // error in the future?
    m.insert(threei::TESTING_CAGEID,HashMap::new());
    Mutex::new(m)
  };
}

// These are the values we look up with at the end...
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FDTableEntry {
    realfd: u64, // underlying fd (may be a virtual fd below us or
    // a kernel fd)
    should_cloexec: bool, // should I close this when exec is called?
    optionalinfo: u64,    // user specified / controlled data
}

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

    return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
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

// This is used for things like dup2, which need a specific fd...
// NOTE: I will assume that the requested_virtualfd isn't used.  If it is, I
// will return ELIND
// virtual and realfds are different
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

// We're just setting a flag here, so this should be pretty straightforward.
pub fn set_cloexec(cageid: u64, virtualfd: u64, is_cloexec: bool) -> Result<(), threei::RetVal> {
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
    };
}

// Super easy, just return the optionalinfo field...
pub fn get_optionalinfo(cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
    let fdtable = GLOBALFDTABLE.lock().unwrap();
    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
        Some(tableentry) => Ok(tableentry.optionalinfo),
        None => Err(threei::Errno::EBADFD as u64),
    };
}

// We're setting an opaque value here. This should be pretty straightforward.
pub fn set_optionalinfo(
    cageid: u64,
    virtualfd: u64,
    optionalinfo: u64,
) -> Result<(), threei::RetVal> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

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

// Helper function used for fork...  Copies an fdtable for another process
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
    assert!(fdtable.insert(newcageid, hmcopy).is_none());
    Ok(())
    // I'm not going to bother to check the number of fds used overall yet...
    //    Err(threei::Errno::EMFILE as u64),
}

// This is mostly used in handling exit, etc.  Returns the HashMap
// for the cage.
pub fn remove_cage_from_fdtable(cageid: u64) -> HashMap<u64, FDTableEntry> {
    let mut fdtable = GLOBALFDTABLE.lock().unwrap();

    if !fdtable.contains_key(&cageid) {
        panic!("Unknown cageid in fdtable access");
    }

    fdtable.remove(&cageid).unwrap()
}

// To add:
//
//
//      empty_fds_for_exec(cageid) -> HashMap<virt_fd:u64,FDTableEntry>
//          This handles exec by removing all of FDTableEntries with cloexec
//          set.  Those are returned in a HashMap
//
//      iterate_over_fdtable(cageid) -> Values<'_, K, V>
//          returns an iterator over the elements in the cage.
//
//

/***************************** TESTS FOLLOW ******************************/

// I'm including my unit tests in-line, in this code.  Integration tests will
// exist in the tests/ directory.
#[cfg(test)]
mod tests {

    use std::sync::Mutex;

    // I'm having a global testing mutex because otherwise the tests will
    // run concurrently.  This messes up some tests, especially testing
    // that tries to get all FDs, etc.
    lazy_static! {
        // This has a junk value (a bool).  Could be anything...
        #[derive(Debug)]
        static ref TESTMUTEX: Mutex<bool> = {
            Mutex::new(true)
        };
    }

    // Import the symbols, etc. in this file...
    use super::*;

    // Helper to empty out state so we can test with a clean system...
    fn flush_fdtable() {
        let mut fdtable = GLOBALFDTABLE.lock().unwrap();
        _ = fdtable.drain();
        fdtable.insert(threei::TESTING_CAGEID, HashMap::new());
    }

    #[test]
    // Basic test to ensure that I can get a virtual fd for a real fd and
    // find the value in the table afterwards...
    fn get_and_translate_work() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        const REALFD: u64 = 10;
        // Acquire a virtual fd...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        let _ = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        let _ = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        let _ = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        assert_eq!(
            REALFD,
            translate_virtual_fd(threei::TESTING_CAGEID, my_virt_fd).unwrap()
        );
    }

    #[test]
    // Let's see if I can change the cloexec flag...
    fn try_set_cloexec() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        const REALFD: u64 = 10;
        // Acquire a virtual fd...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        set_cloexec(threei::TESTING_CAGEID, my_virt_fd, true).unwrap();
    }

    #[test]
    // Get and set optionalinfo
    fn try_get_and_set_optionalinfo() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        // Acquire two virtual fds...
        let my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();
        let my_virt_fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 4, true, 250).unwrap();
        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
        assert_eq!(
            250,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd2).unwrap()
        );
        set_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1, 500).unwrap();
        assert_eq!(
            500,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
        assert_eq!(
            250,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd2).unwrap()
        );
    }

    #[test]
    fn test_remove_cage_from_fdtable() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        // Acquire two virtual fds...
        let my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();

        // let's drop this fdtable...
        let mytable = remove_cage_from_fdtable(threei::TESTING_CAGEID);
        // And check what we got back...
        assert_eq!(
            *(mytable.get(&my_virt_fd1).unwrap()),
            FDTableEntry {
                realfd: 10,
                should_cloexec: false,
                optionalinfo: 150
            }
        );
    }

    #[test]
    fn test_copy_fdtable_for_cage() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        // Acquire two virtual fds...
        let my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();
        let my_virt_fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 4, true, 250).unwrap();

        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
        assert_eq!(
            250,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd2).unwrap()
        );

        // Copy the fdtable over to a new cage...
        copy_fdtable_for_cage(threei::TESTING_CAGEID, threei::TESTING_CAGEID1).unwrap();

        // Check the elements exist...
        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID1, my_virt_fd1).unwrap()
        );
        assert_eq!(
            250,
            get_optionalinfo(threei::TESTING_CAGEID1, my_virt_fd2).unwrap()
        );
        // ... and are independent...
        set_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1, 500).unwrap();
        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID1, my_virt_fd1).unwrap()
        );
        assert_eq!(
            500,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
    }

    #[test]
    // Let's test to see our functions error gracefully with badfds...
    fn get_specific_virtual_fd_tests() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();

        // Choose an unused new_fd
        let my_new_fd: u64;
        if my_virt_fd == 0 {
            my_new_fd = 100;
        } else {
            my_new_fd = 0;
        }
        get_specific_virtual_fd(threei::TESTING_CAGEID, my_new_fd, 1, true, 5).unwrap();
        assert_eq!(
            get_optionalinfo(threei::TESTING_CAGEID, my_new_fd).unwrap(),
            5
        );
        assert_eq!(
            translate_virtual_fd(threei::TESTING_CAGEID, my_new_fd).unwrap(),
            1
        );

        // Check if I get an error using a used fd
        assert!(get_specific_virtual_fd(threei::TESTING_CAGEID, my_new_fd, 1, true, 5).is_err());
        // Check if I get an error going out of range...
        assert!(get_specific_virtual_fd(
            threei::TESTING_CAGEID,
            FD_PER_PROCESS_MAX + 1,
            1,
            true,
            5
        )
        .is_err());
    }

    #[test]
    // Let's test to see our functions error gracefully with badfds...
    fn badfd_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        // some made up number...
        let my_virt_fd = 135;
        assert!(translate_virtual_fd(threei::TESTING_CAGEID, my_virt_fd).is_err());
        assert!(set_cloexec(threei::TESTING_CAGEID, my_virt_fd, true).is_err());
        assert!(get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd).is_err());
        assert!(set_optionalinfo(threei::TESTING_CAGEID, my_virt_fd, 37).is_err());
    }

    // Let's use up all the fds and verify we get an error...
    #[test]
    fn use_all_fds_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        flush_fdtable();

        const REALFD: u64 = 10;
        for current in 0..FD_PER_PROCESS_MAX {
            // check to make sure that the number of items is equal to the
            // number of times through this loop...
            //
            // Note: if this test is failing on the next line, it is likely
            // because some extra fds are allocated for the cage (like stdin,
            // stdout, and stderr).
            assert_eq!(
                GLOBALFDTABLE
                    .lock()
                    .unwrap()
                    .get(&threei::TESTING_CAGEID)
                    .unwrap()
                    .len(),
                current as usize
            );

            let _ = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        }
        // If the test is failing by not triggering here, we're not stopping
        // at the limit...
        if get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).is_err() {
            flush_fdtable();
        } else {
            panic!("Should have raised an error...");
        }
    }

    #[test]
    #[should_panic]
    #[ignore] // Don't run by default because it poisons the GLOBALFDTABLE
              // when panicking
              // Let's check to make sure we panic with an invalid cageid
    fn translate_panics_on_bad_cageid_test() {
        // Should only run individually, so no need to lock...

        let _ = translate_virtual_fd(threei::INVALID_CAGEID, 10);
    }

    #[test]
    #[should_panic]
    #[ignore] // Don't run by default because it poisons the GLOBALFDTABLE
              // when panicking
              // Let's check to make sure we panic with an invalid cageid
    fn get_unused_virtual_fd_panics_on_bad_cageid_test() {
        // Should only run individually, so no need to lock...

        let _ = get_unused_virtual_fd(threei::INVALID_CAGEID, 10, false, 100);
    }

    #[test]
    #[should_panic]
    #[ignore] // Don't run by default because it poisons the GLOBALFDTABLE
              // when panicking
              // Let's check to make sure we panic with an invalid cageid
    fn set_cloexec_panics_on_bad_cageid_test() {
        // Should only run individually, so no need to lock...

        let _ = set_cloexec(threei::INVALID_CAGEID, 10, true);
    }
}
