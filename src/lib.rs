//! This module provides an fdtable, an abstraction which makes it easy
//! to separate out file descriptors for different cages.  There are several
//! reasons why this is needed.  First, different cages are not permitted to
//! see or access each others' descriptors, hence one needs a means to track
//! this somehow.  Second, different cages may each want to have something
//! like their STDERR or STDOUT directed to different locations.  Third,
//! when a cage forks, its fds are inherited by the child, but operations on
//! those file descriptors (e.g., close) may happen independenty and must not
//! impact the other cage.
//!
//! As such, this is a general library meant to handle those issues.  It has
//! the primary function of letting set up virtual (child cage) to real 
//! (the underlying system) fd mappings.
//!
//! Note that the code re-exports an implementation from a specific submodule.
//! This was done to make the algorithmic options easier to benchmark and
//! compare.  You, the caller, should only use the base fdtables::XXX API and
//! not fdtables::algorithmname::XXX, as the latter will not be stable over 
//! time.

// I likely should remove this...  I only use it to make empty_fds_for_exec
// slightly more efficient in some implementations...
#![feature(hash_extract_if)]

// NOTE: This setup is a bit odd, I know.  I'm creating different
// implementations of the same algorithm and I'd like to test them.  Originally
// I was going to have a struct interface where I switched between them by
// swapping out structs with the same trait.  This was a pain-in-the-butt, but
// it worked for single threaded things or multi-threaded readable things.
// However, I couldn't figure out how to make this work with having threads
// share a struct where the underlying things which were mutable (even though
// the underlying items were locked appropriately in a generic way).
//
// This makes things like the doc strings very odd as well.  I am extracting
// these out to separate files instead of having them in-line, since the
// different implementations will have the same doc strings.
//
// How this works is that I will import a single implementation as a mod here
// and this is what the benchmarker will use.  If you want to change the
// implementation you benchmark / test / use, you need to change the lines
// below...
//

/*  ------------ SET OF IMPLEMENTATIONS OF FDTABLES ------------ */

// --- Solution without locking ---
//  HashMap<u64,HashMap<u64,FDTableEntry>>
//      Done: Unlocked
//
//
//  Broken!!!!  Don't know how to have a static global without a mutex.
//mod nolockbasic;
//pub use crate::nolockbasic::*;

// --- Solutions with global locking ---
//  Mutex<HashMap<u64,HashMap<u64,FDTableEntry>>>
//      This is the default thing I implemented.
//      Done: GlobalVanilla

//mod vanillaglobal;
//pub use crate::vanillaglobal::*;

//  DashMap<u64,HashMap<u64,FDTableEntry>>
//      Just a basic solution with a dashmap instead of a mutex + hashmap
//      Done: GlobalDashMap
//

mod dashmapglobal;
pub use crate::dashmapglobal::*;

//
//  DashMap<u64,[FDTableEntry;1024]>  Space is ~24KB per cage?!?
//      Static DashMap.  Let's see if having the FDTableEntries be a static
//      array is any faster...
//
//  DashMap<u64,vec!(FDTableEntry;1024)>  Space is ~30KB per cage?!?
//      Static DashMap.  Let's see if having the FDTableEntries be a Vector
//      is any different than a static array...
//
//  Mutex<Box<[[FDTableEntry;1024];256]>>  Space here is ~6MB total!?
//
//  struct PerCageFDTable {
//      largest_fd_ever_used: u64,
//      this_cage_fdtable: HashMap<u64,FDTableEntry>,
//  }
//  Mutex<HashMap<u64,PerCageFDTable>>
//
//  DashMap<u64,PerCageFDTable>

// --- Solutions with per-fd locking ---
//  Vec<Arc<parking_lot::RwLock<Option<FDTableEntry>>>> Space is ~40KB per cage
//      Quite similar to the initial RustPOSIX implementation.  The vector is
//      populated with 1024 entries at all times.  The locks exist at all
//      times, even when the fds are not allocated.  This lets different
//      threads access the same fd without a race, etc.
//

//
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
// To make this work, this library provides the following funtionality (these
// must all be implemented by any party wishing to add functionality):
//
//      pub const ALGONAME: &str = XXX;
//          Where XXX is a string for the name of the algorithm.  Printed
//          during benchmarking...
//
//      refresh()
//          Sets up / clears out the information.  Useful between tests.
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
//      get_optionalinfo(cageid,virtualfd) -> Result<optionalinfo, EBADFD>
//      set_optionalinfo(cageid,virtualfd,optionalinfo) -> Result<(), EBADFD>
//          The above two are useful if you want to track other things like
//          an id for other in-memory data structures
//
//      return_fdtable_copy(cageid: u64) -> HashMap<u64, FDTableEntry>
//          returns a copy of the fdtable for a cage.  Useful helper function
//          for a caller that needs to examine the table.  Likely could be
//          more efficient by letting the caller borrow this...
//
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
//
//
// [*] This isn't possible because fork causes the same fd in the parent and
// child to have separate file pointers (e.g., read / write to separate
// locations in the file).
//
// [**] This is only the 'real' fd from the standpoint of the user of this
// library.  If another part of the system below it, such as another grate or
// the microvisor, is using this library, it will get translated again.
//

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
//              has been reached. (mostly unimplemented)
//

// This is used everywhere...  Should I re-export more of these symbols?
pub mod threei;
/// Error values (matching errno in Linux) for the various call Results
pub use threei::Errno;

/***************************** TESTS FOLLOW ******************************/

// I'm including my unit tests in-line, in this code.  Integration tests will
// exist in the tests/ directory.
#[cfg(test)]
mod tests {

    use lazy_static::lazy_static;

    use std::sync::Mutex;

    use std::thread;

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

    #[test]
    // Basic test to ensure that I can get a virtual fd for a real fd and
    // find the value in the table afterwards...
    fn get_and_translate_work() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        refresh();

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
        refresh();

        const REALFD: u64 = 10;
        // Acquire a virtual fd...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        set_cloexec(threei::TESTING_CAGEID, my_virt_fd, true).unwrap();
    }

    #[test]
    // Get and set optionalinfo
    fn try_get_and_set_optionalinfo() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        refresh();

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
        refresh();

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
    fn test_empty_fds_for_exec() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        refresh();

        // Acquire two virtual fds...
        let my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();
        let my_virt_fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 4, true, 250).unwrap();

        let myhm = empty_fds_for_exec(threei::TESTING_CAGEID);

        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
        // Should be missing...
        assert!(get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd2).is_err());

        // Should be missing...
        assert!(myhm.get(&my_virt_fd1).is_none());
        // Should be in this hash map now...
        assert_eq!(myhm.get(&my_virt_fd2).unwrap().realfd, 4);
    }

    #[test]
    fn return_fdtable_copy_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        refresh();
        // Acquire two virtual fds...
        let my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();
        let my_virt_fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 4, true, 250).unwrap();

        // Copy the fdtable over to a new cage...
        let mut myhm = return_fdtable_copy(threei::TESTING_CAGEID);

        // Check we got what we expected...
        assert_eq!(
            *(myhm.get(&my_virt_fd1).unwrap()),
            FDTableEntry {
                realfd: 10,
                should_cloexec: false,
                optionalinfo: 150
            }
        );
        assert_eq!(
            *(myhm.get(&my_virt_fd2).unwrap()),
            FDTableEntry {
                realfd: 4,
                should_cloexec: true,
                optionalinfo: 250
            }
        );

        myhm.insert(
            my_virt_fd1,
            FDTableEntry {
                realfd: 2,
                should_cloexec: false,
                optionalinfo: 15,
            },
        )
        .unwrap();

        // has my hashmap been updated?
        assert_eq!(
            *(myhm.get(&my_virt_fd1).unwrap()),
            FDTableEntry {
                realfd: 2,
                should_cloexec: false,
                optionalinfo: 15
            }
        );

        // Check to make sure the actual table is still intact...
        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
        assert_eq!(
            250,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd2).unwrap()
        );
    }

    #[test]
    fn test_copy_fdtable_for_cage() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        refresh();

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
        refresh();

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
        refresh();

        // some made up number...
        let my_virt_fd = 135;
        assert!(translate_virtual_fd(threei::TESTING_CAGEID, my_virt_fd).is_err());
        assert!(set_cloexec(threei::TESTING_CAGEID, my_virt_fd, true).is_err());
        assert!(get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd).is_err());
        assert!(set_optionalinfo(threei::TESTING_CAGEID, my_virt_fd, 37).is_err());
    }

    #[test]
    // Let's do a multithreaded test...
    fn multithreaded_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap();

        refresh();
        let fd = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
        let fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 20, true, 200).unwrap();
        let fd3 = get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 300).unwrap();
        for threadcount in [1, 2, 4, 8, 16].iter() {
            let mut thread_handle_vec: Vec<thread::JoinHandle<()>> = Vec::new();
            for _numthreads in 0..*threadcount {
                let thisthreadcount = *threadcount;

                thread_handle_vec.push(thread::spawn(move || {
                    // Do 10K / threadcount of 10 requests each.  100K total
                    for _ in 0..10000 / thisthreadcount {
                        translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd2).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd3).unwrap();
                    }
                }));
            }
            for handle in thread_handle_vec {
                handle.join().unwrap();
            }
        }
    }

    #[test]
    // Let's do a multithreaded test...
    fn multithreaded_write_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap();

        refresh();
        for threadcount in [1, 2, 4, 8, 16].iter() {
            let mut thread_handle_vec: Vec<thread::JoinHandle<()>> = Vec::new();
            for _numthreads in 0..*threadcount {
                let thisthreadcount = *threadcount;

                thread_handle_vec.push(thread::spawn(move || {
                    // Do 1000 writes, then flush it out...
                    for _ in 0..1000 / thisthreadcount {
                        let fd =
                            get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
                        translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                    }
                }));
            }
            for handle in thread_handle_vec {
                handle.join().unwrap();
            }
            refresh();
        }
    }

    // Let's use up all the fds and verify we get an error...
    #[test]
    fn use_all_fds_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap();
        refresh();

        const REALFD: u64 = 10;
        for _current in 0..FD_PER_PROCESS_MAX {
            // check to make sure that the number of items is equal to the
            // number of times through this loop...
            //
            // Note: if this test is failing on the next line, it is likely
            // because some extra fds are allocated for the cage (like stdin,
            // stdout, and stderr).
            //
            // I removed this because it lifts the veil of the interface by
            // peeking into the GLOBALFDTABLE
            /*            assert_eq!(
                GLOBALFDTABLE
                    .lock()
                    .unwrap()
                    .get(&threei::TESTING_CAGEID)
                    .unwrap()
                    .len(),
                current as usize
            ); */

            let _ = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        }
        // If the test is failing by not triggering here, we're not stopping
        // at the limit...
        if get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).is_err() {
            refresh();
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
