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
//! compare.  You, the caller, should only use the base `fdtables::XXX` API and
//! not `fdtables::algorithmname::XXX`, as the latter will not be stable over
//! time.

// Copied from Tom Buckley-Houston
// =========================================================================
//                  Canonical lints for whole crate
// =========================================================================
// Official docs:
//   https://doc.rust-lang.org/nightly/clippy/lints.html
// Useful app to lookup full details of individual lints:
//   https://rust-lang.github.io/rust-clippy/master/index.html
//
// We set base lints to give the fullest, most pedantic feedback possible.
// Though we prefer that they are just warnings during development so that build-denial
// is only enforced in CI.
//
#![warn(
    // `clippy::all` is already on by default. It implies the following:
    //   clippy::correctness code that is outright wrong or useless
    //   clippy::suspicious code that is most likely wrong or useless
    //   clippy::complexity code that does something simple but in a complex way
    //   clippy::perf code that can be written to run faster
    //   clippy::style code that should be written in a more idiomatic way
    clippy::all,

    // It's always good to write as much documentation as possible
    missing_docs,

    // > clippy::pedantic lints which are rather strict or might have false positives
    clippy::pedantic,

    // > new lints that are still under development"
    // (so "nursery" doesn't mean "Rust newbies")
//    clippy::nursery,

    // > The clippy::cargo group gives you suggestions on how to improve your Cargo.toml file.
    // > This might be especially interesting if you want to publish your crate and are not sure
    // > if you have all useful information in your Cargo.toml.
    clippy::cargo
)]
// > The clippy::restriction group will restrict you in some way.
// > If you enable a restriction lint for your crate it is recommended to also fix code that
// > this lint triggers on. However, those lints are really strict by design and you might want
// > to #[allow] them in some special cases, with a comment justifying that.
#![allow(clippy::blanket_clippy_restriction_lints)]
// JAC: I took a look at these and it seems like these are mostly uninteresting
// false positives.
//#![warn(clippy::restriction)]

// I do a fair amount of casting to usize so that I can index values in arrays.
// I can't annotate them all separately because I can't assign attributes to
// expressions.  So I'll turn this off.
#![allow(clippy::cast_possible_truncation)]
// TODO: This is to disable a warning in threei's reversible enum definition.
// I'd like to revisit that clippy warning later and see if we want to handle
// it differently
#![allow(clippy::result_unit_err)]

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

//mod dashmapglobal;
//pub use crate::dashmapglobal::*;

//
//  DashMap<u64,[Option<FDTableEntry>;1024]>  Space is ~24KB per cage?!?
//      Static DashMap.  Let's see if having the FDTableEntries be a static
//      array is any faster...
//

//mod dashmaparrayglobal;
//pub use crate::dashmaparrayglobal::*;

//
//  DashMap<u64,vec!(FDTableEntry;1024)>  Space is ~30KB per cage?!?
//      Vector DashMap.  Let's see if having the FDTableEntries be a Vector
//      is any different than a static array...
//

//mod dashmapvecglobal;
//pub use crate::dashmapvecglobal::*;

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
//      get_specific_virtual_fd(cageid,virtualfd,realfd,is_cloexec,optionalinfo) -> Result<(), EBADF>
//          This is mostly used for dup2/3.  I'm assuming the caller got the
//          entry already and wants to put it in a location.  Closes the fd if
//          the entry is occupied.  Raises EBADF if out of range...
//
//      copy_fdtable_for_cage(srccageid, newcageid) -> Result<(), ENFILE>
//          This is effectively just making a copy of a specific cage's
//          fdtable, for use in fork()
//
//      remove_cage_from_fdtable(cageid)
//          This is mostly used in handling exit, etc.  Calls the appropriate
//          close handlers.
//
//      empty_fds_for_exec(cageid)
//          This handles exec by removing all of FDTableEntries with cloexec
//          set.  It calls the appropriate close handler for each fd.
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
//      close_virtualfd(cageid: u64) -> Result<(),EBADF>
//          removes an entry from the virtual fd table.  It calls the
//          appropriate close handlers.
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
// [*] This isn't possible because the parent and child can close, open, dup,
// etc. their file descriptors independently.
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

include!("current_impl");

mod commonconstants;
pub use commonconstants::*;

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

    use std::sync::{Mutex, MutexGuard};

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

    fn do_panic(_: u64) {
        panic!("do_panic!");
    }

    #[test]
    // Basic test to ensure that I can get a virtual fd for a real fd and
    // find the value in the table afterwards...
    fn get_and_translate_work() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = 10;
        // Acquire a virtual fd...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap();
        set_cloexec(threei::TESTING_CAGEID, my_virt_fd, true).unwrap();
    }

    #[test]
    // Get and set optionalinfo
    fn try_get_and_set_optionalinfo() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        // Acquire two virtual fds...
        let _my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();

        // let's drop this fdtable...
        remove_cage_from_fdtable(threei::TESTING_CAGEID);
        // Likely should have a better test, but everything will panic...
    }

    #[test]
    fn test_empty_fds_for_exec() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        // Acquire two virtual fds...
        let my_virt_fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, 10, false, 150).unwrap();
        let my_virt_fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, 4, true, 250).unwrap();

        empty_fds_for_exec(threei::TESTING_CAGEID);

        assert_eq!(
            150,
            get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd1).unwrap()
        );
        // Should be missing...
        assert!(get_optionalinfo(threei::TESTING_CAGEID, my_virt_fd2).is_err());
    }

    #[test]
    fn return_fdtable_copy_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
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
    // Do close_virtualfd(...) testing...
    fn test_close_virtualfd() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = 57;

        const ANOTHERREALFD: u64 = 101;

        const SPECIFICVIRTUALFD: u64 = 15;

        // None of my closes (until the end) will be the last...
        register_close_handlers(NULL_FUNC, do_panic, NULL_FUNC);

        // use the same realfd a few times in different ways...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 10).unwrap();
        get_specific_virtual_fd(threei::TESTING_CAGEID, SPECIFICVIRTUALFD, REALFD, false, 10)
            .unwrap();
        let cloexecfd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, true, 10).unwrap();
        // and a different realfd
        let _my_virt_fd3 =
            get_unused_virtual_fd(threei::TESTING_CAGEID, ANOTHERREALFD, false, 10).unwrap();

        // let's close one (should have two left...)
        close_virtualfd(threei::TESTING_CAGEID, my_virt_fd).unwrap();

        // Let's fork (to double the count)!
        copy_fdtable_for_cage(threei::TESTING_CAGEID, threei::TESTING_CAGEID7).unwrap();

        // let's simulate exec, which should close one of these...
        empty_fds_for_exec(threei::TESTING_CAGEID7);

        // but the copy in the original cage table should remain, so this
        // shouldn't error...
        translate_virtual_fd(threei::TESTING_CAGEID, cloexecfd).unwrap();

        // However, the other should be gone and should error...
        assert!(translate_virtual_fd(threei::TESTING_CAGEID7, cloexecfd).is_err());

        // Let's simulate exit on the initial cage, to close two of them...
        remove_cage_from_fdtable(threei::TESTING_CAGEID);

        // panic if this isn't the last one (from now on)
        register_close_handlers(do_panic, NULL_FUNC, NULL_FUNC);

        // Now this is the last one!
        close_virtualfd(threei::TESTING_CAGEID7, SPECIFICVIRTUALFD).unwrap();
    }

    #[test]
    #[should_panic]
    // Check for duplicate uses of the same realfd...
    fn test_dup_close() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = 57;

        // get the realfd...  I tested this in the test above, so should not
        // panic...
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 10).unwrap();
        close_virtualfd(threei::TESTING_CAGEID, my_virt_fd).unwrap();

        // Panic on this one...
        register_close_handlers(NULL_FUNC, do_panic, NULL_FUNC);

        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 10).unwrap();
        close_virtualfd(threei::TESTING_CAGEID, my_virt_fd).unwrap();
    }

    // Helper for the close handler recursion tests...
    fn _test_close_handler_recursion_helper(_: u64) {
        // reset helpers
        register_close_handlers(NULL_FUNC, NULL_FUNC, NULL_FUNC);

        const REALFD: u64 = 57;
        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 10).unwrap();
        close_virtualfd(threei::TESTING_CAGEID, my_virt_fd).unwrap();
    }

    #[test]
    // check to see what happens if close handlers call other operations...
    fn test_close_handler_recursion() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = 57;

        // Register my helper to be called when I call close...
        register_close_handlers(NULL_FUNC, _test_close_handler_recursion_helper, NULL_FUNC);

        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 10).unwrap();
        // Call this which calls the close handler
        close_virtualfd(threei::TESTING_CAGEID, my_virt_fd).unwrap();
    }

    #[test]
    // get_specific_virtual_fd closehandler recursion... likely deadlock on
    // fail.
    fn test_gsvfd_handler_recursion() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = 57;

        // Register my helper to be called when I call close...
        register_close_handlers(NULL_FUNC, _test_close_handler_recursion_helper, NULL_FUNC);

        let my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 10).unwrap();
        // Call this which calls the close handler
        get_specific_virtual_fd(threei::TESTING_CAGEID, my_virt_fd, 123, true, 0).unwrap();
    }

    #[test]
    // remove_cage_from_fdtable closehandler recursion... likely deadlock on
    // fail.
    fn test_rcffdt_handler_recursion() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = 57;
        // Since I'm removing a cage here, yet doing operations afterwards,
        // I need to have an empty cage first.
        init_empty_cage(threei::TESTING_CAGEID5);

        // Register my helper to be called when I call close...
        register_close_handlers(NULL_FUNC, _test_close_handler_recursion_helper, NULL_FUNC);

        let _my_virt_fd =
            get_unused_virtual_fd(threei::TESTING_CAGEID5, REALFD, false, 10).unwrap();
        // Call this which calls the close handler
        remove_cage_from_fdtable(threei::TESTING_CAGEID5);
    }

    #[test]
    // empty_fds_for_exec closehandler recursion...  likely deadlock on fail.
    fn test_effe_handler_recursion() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        const REALFD: u64 = NO_REAL_FD;

        // Register my helper to be called when I call close...
        register_close_handlers(NULL_FUNC, _test_close_handler_recursion_helper, NULL_FUNC);

        let _my_virt_fd = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, true, 10).unwrap();
        empty_fds_for_exec(threei::TESTING_CAGEID);
    }

    #[test]
    // empty_fds_for_exec closehandler recursion...  likely deadlock on fail.
    fn test_unreal_handler_recursion() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
        refresh();

        // Register my helper to be called when I call close...
        register_close_handlers(NULL_FUNC, NULL_FUNC, _test_close_handler_recursion_helper);

        // close
        let my_virt_fd =
            get_unused_virtual_fd(threei::TESTING_CAGEID, NO_REAL_FD, true, 10).unwrap();
        // Call this which calls the close handler
        close_virtualfd(threei::TESTING_CAGEID, my_virt_fd).unwrap();

        // restore handlers
        register_close_handlers(NULL_FUNC, NULL_FUNC, _test_close_handler_recursion_helper);

        // exec
        let _my_virt_fd =
            get_unused_virtual_fd(threei::TESTING_CAGEID, NO_REAL_FD, true, 10).unwrap();
        empty_fds_for_exec(threei::TESTING_CAGEID);

        // restore handlers
        register_close_handlers(NULL_FUNC, NULL_FUNC, _test_close_handler_recursion_helper);

        // remove
        init_empty_cage(threei::TESTING_CAGEID5);
        let _my_virt_fd =
            get_unused_virtual_fd(threei::TESTING_CAGEID5, NO_REAL_FD, false, 10).unwrap();
        remove_cage_from_fdtable(threei::TESTING_CAGEID5);

        // restore handlers
        register_close_handlers(NULL_FUNC, NULL_FUNC, _test_close_handler_recursion_helper);

        // dup2
        let my_virt_fd =
            get_unused_virtual_fd(threei::TESTING_CAGEID, NO_REAL_FD, false, 10).unwrap();
        get_specific_virtual_fd(threei::TESTING_CAGEID, my_virt_fd, NO_REAL_FD, true, 0).unwrap();
    }

    #[test]
    // check some common poll cases...
    fn check_poll_helpers() {
        let mut _thelock: MutexGuard<bool>;
        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        let cage_id = threei::TESTING_CAGEID;

        // get_specific_virtual_fd(cage_id, VIRTFD, REALFD, CLOEXEC, OPTINFO)
        get_specific_virtual_fd(cage_id, 3, 7, false, 10).unwrap();
        get_specific_virtual_fd(cage_id, 5, NO_REAL_FD, false, 123).unwrap();
        get_specific_virtual_fd(cage_id, 9, 20, true, 0).unwrap();

        let (mut realfds, unrealfds, invalidfds, mappingtable) =
            convert_virtualfds_to_real(cage_id, vec![1, 3, 5, 9]);

        assert_eq!(realfds.len(), 4);
        assert_eq!(unrealfds.len(), 1);
        assert_eq!(invalidfds.len(), 1);
        assert_eq!(realfds, vec!(INVALID_FD, 7, NO_REAL_FD, 20));
        assert_eq!(invalidfds, vec!(1));
        assert_eq!(unrealfds, vec!((5, 123)));

        // Toss out the unreal and invalid ones...
        realfds.retain(|&realfd| realfd != NO_REAL_FD && realfd != INVALID_FD);

        // poll(...)  // let's pretend that realfd 7 had its event triggered...
        let newrealfds = convert_realfds_back_to_virtual(vec![7], &mappingtable);
        // virtfd 3 should be returned
        assert_eq!(newrealfds, vec!(3));
    }

    #[test]
    // check some common epoll cases...
    fn check_epoll_helpers() {
        let mut _thelock: MutexGuard<bool>;
        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        let cage_id = threei::TESTING_CAGEID;

        let virtfd1 = 5;
        let virtfd2 = 6;
        let virtfd3 = 10;
        let realfd = 20;
        let epollrealfd = 100;
        // get_specific_virtual_fd(cage_id, VIRTFD, REALFD, CLOEXEC, OPTINFO)
        get_specific_virtual_fd(cage_id, virtfd1, NO_REAL_FD, false, 123).unwrap();
        get_specific_virtual_fd(cage_id, virtfd2, NO_REAL_FD, false, 456).unwrap();
        get_specific_virtual_fd(cage_id, virtfd3, realfd, true, 0).unwrap();

        // get an epollfd...
        let epollfd = epoll_create_helper(cage_id, epollrealfd, false).unwrap();

        let myevent1 = epoll_event {
            events: (EPOLLIN + EPOLLOUT) as u32,
            u64: 0,
        };
        let myevent2 = epoll_event {
            events: (EPOLLIN) as u32,
            u64: 0,
        };

        // try to add the realfd, which should fail and return the realfd
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd, EPOLL_CTL_ADD, virtfd3, myevent1.clone()).unwrap(),
            (epollrealfd, realfd)
        );
        // Nothing should have been added...
        assert_eq!(get_epoll_wait_data(cage_id, epollfd).unwrap().1.len(), 0);

        // Add in one unrealfd...
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd, EPOLL_CTL_ADD, virtfd1, myevent1.clone()).unwrap(),
            (epollrealfd, NO_REAL_FD)
        );

        // Should have one item...
        assert_eq!(get_epoll_wait_data(cage_id, epollfd).unwrap().1.len(), 1);

        // Delete it...
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd, EPOLL_CTL_DEL, virtfd1, myevent1.clone()).unwrap(),
            (epollrealfd, NO_REAL_FD)
        );

        // Back to zero...
        assert_eq!(get_epoll_wait_data(cage_id, epollfd).unwrap().1.len(), 0);

        // Add in two unrealfds...
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd, EPOLL_CTL_ADD, virtfd1, myevent1.clone()).unwrap(),
            (epollrealfd, NO_REAL_FD)
        );
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd, EPOLL_CTL_ADD, virtfd2, myevent2.clone()).unwrap(),
            (epollrealfd, NO_REAL_FD)
        );
        assert_eq!(get_epoll_wait_data(cage_id, epollfd).unwrap().1.len(), 2);

        // Check their event types are correct...
        assert_eq!(
            get_epoll_wait_data(cage_id, epollfd).unwrap().1[&virtfd1].events,
            myevent1.events
        );
        assert_eq!(
            get_epoll_wait_data(cage_id, epollfd).unwrap().1[&virtfd2].events,
            myevent2.events
        );

        // Let's switch one of them...
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd, EPOLL_CTL_MOD, virtfd1, myevent2.clone()).unwrap(),
            (epollrealfd, NO_REAL_FD)
        );
        // not anymore!
        assert_ne!(
            get_epoll_wait_data(cage_id, epollfd).unwrap().1[&virtfd1].events,
            myevent1.events
        );
        // correct!
        assert_eq!(
            get_epoll_wait_data(cage_id, epollfd).unwrap().1[&virtfd1].events,
            myevent2.events
        );
    }

    #[test]
    #[ignore]
    // Add these if I do the complete epoll later.  These tests are amazing!
    // https://github.com/heiher/epoll-wakeup
    // Right now, just check, did I implement epoll of epoll fds?
    #[allow(non_snake_case)]
    fn check_SHOULD_FAIL_FOR_NOW_if_we_support_epoll_of_epoll() {
        let mut _thelock: MutexGuard<bool>;
        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        let cage_id = threei::TESTING_CAGEID;

        // get two epollfds...
        let epollfd1 = epoll_create_helper(cage_id, EPOLLFD, false).unwrap();
        let epollfd2 = epoll_create_helper(cage_id, EPOLLFD, false).unwrap();

        let myevent1 = epoll_event {
            events: (EPOLLIN + EPOLLOUT) as u32,
            u64: 0,
        };

        // try to add an epollfd to an epollfd
        assert_eq!(
            try_epoll_ctl(cage_id, epollfd1, EPOLL_CTL_ADD, epollfd2, myevent1.clone()).unwrap(),
            (EPOLLFD, NO_REAL_FD)
        );
    }

    #[test]
    // check some common select cases...
    fn check_get_real_bitmasks_for_select() {
        let mut _thelock: MutexGuard<bool>;
        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        let cage_id = threei::TESTING_CAGEID;

        get_specific_virtual_fd(cage_id, 3, 7, false, 10).unwrap();
        get_specific_virtual_fd(cage_id, 5, NO_REAL_FD, false, 123).unwrap();

        let mut bad_fds_to_check = _init_fd_set();
        _fd_set(2, &mut bad_fds_to_check);

        // check all of the positions!
        assert!(
            get_real_bitmasks_for_select(cage_id, 6, Some(bad_fds_to_check), None, None).is_err()
        );
        assert!(
            get_real_bitmasks_for_select(cage_id, 6, None, Some(bad_fds_to_check), None).is_err()
        );
        assert!(
            get_real_bitmasks_for_select(cage_id, 6, None, None, Some(bad_fds_to_check)).is_err()
        );

        // but if I drop the nfds too low, it is okay...
        assert!(
            get_real_bitmasks_for_select(cage_id, 2, None, None, Some(bad_fds_to_check)).is_ok()
        );

        // too high also errors...
        assert!(
            get_real_bitmasks_for_select(cage_id, 1024, None, None, Some(bad_fds_to_check))
                .is_err()
        );

        // recall, we set up some actual virtualfds above...
        let mut actual_fds_to_check = _init_fd_set();
        _fd_set(3, &mut actual_fds_to_check);
        _fd_set(5, &mut actual_fds_to_check);

        assert!(get_real_bitmasks_for_select(
            cage_id,
            6,
            Some(actual_fds_to_check),
            Some(actual_fds_to_check),
            None
        )
        .is_ok());

        // let's peek closer at an actual call...
        let (_, realreadbits, realwritebits, realexceptbits, unrealitems, mappingtable) =
            get_real_bitmasks_for_select(cage_id, 6, Some(actual_fds_to_check), None, None)
                .unwrap();
        assert!(realreadbits.is_some());
        assert!(realwritebits.is_none());
        assert!(realexceptbits.is_none());
        assert_eq!(unrealitems[0].len(), 1);
        assert_eq!(mappingtable.len(), 1);
    }

    #[test]
    // Let's test to see our functions error gracefully with badfds...
    fn get_specific_virtual_fd_tests() {
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });

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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });

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
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });
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
    // Do we close a virtualfd when we select it?  (Do nothing, but see the
    // next test.)
    fn check_get_specific_virtual_fd_close_ok_test() {
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        copy_fdtable_for_cage(threei::TESTING_CAGEID, threei::TESTING_CAGEID10).unwrap();

        let virtfd = get_unused_virtual_fd(threei::TESTING_CAGEID10, 10, false, 100).unwrap();
        // Do nothing.  See next test...
        get_specific_virtual_fd(threei::TESTING_CAGEID10, virtfd, 10, false, 100).unwrap();
    }

    #[test]
    #[should_panic]
    // checks that init correctly panics
    fn check_init_panics() {
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        copy_fdtable_for_cage(threei::TESTING_CAGEID, threei::TESTING_CAGEID11).unwrap();
        // panic!
        init_empty_cage(threei::TESTING_CAGEID11);
    }

    #[test]
    #[should_panic]
    // Do we close a virtualfd when we call get_specific on it?
    fn check_get_specific_virtual_fd_close_panic_test() {
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        copy_fdtable_for_cage(threei::TESTING_CAGEID, threei::TESTING_CAGEID11).unwrap();
        // panic in a moment!
        register_close_handlers(do_panic, do_panic, NULL_FUNC);
        let virtfd = get_unused_virtual_fd(threei::TESTING_CAGEID11, 234, false, 100).unwrap();
        // panic!!!
        get_specific_virtual_fd(threei::TESTING_CAGEID11, virtfd, 10, false, 100).unwrap();
    }

    #[test]
    #[should_panic]
    // Let's check to make sure we panic with an invalid cageid
    fn translate_panics_on_bad_cageid_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });

        let _ = translate_virtual_fd(threei::INVALID_CAGEID, 10);
    }

    #[test]
    #[should_panic]
    // Let's check to make sure we panic with an invalid cageid
    fn get_unused_virtual_fd_panics_on_bad_cageid_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });

        let _ = get_unused_virtual_fd(threei::INVALID_CAGEID, 10, false, 100);
    }

    #[test]
    #[should_panic]
    // Let's check to make sure we panic with an invalid cageid
    fn set_cloexec_panics_on_bad_cageid_test() {
        let mut _thelock = TESTMUTEX.lock().unwrap_or_else(|e| {
            refresh();
            TESTMUTEX.clear_poison();
            e.into_inner()
        });

        let _ = set_cloexec(threei::INVALID_CAGEID, 10, true);
    }

    #[test]
    #[should_panic]
    // Let's check that our callback for close is working correctly by having
    // it panic
    fn test_intermediate_handler() {
        // Get the guard in a way that if we unpoison it, we don't end up
        // with multiple runners...
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }

        refresh();

        const REALFD: u64 = 132;
        // I'm using unwrap_or because I don't want a panic here to be
        // considered passing the test
        let fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap_or(1);
        let _fd2 = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap_or(1);

        register_close_handlers(do_panic, NULL_FUNC, NULL_FUNC);

        // should panic here...
        close_virtualfd(threei::TESTING_CAGEID, fd1).unwrap();
    }

    #[test]
    #[should_panic]
    // Check final_handler
    fn test_final_handler() {
        // Get the guard in a way that if we unpoison it, we don't end up
        // with multiple runners...
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        const REALFD: u64 = 109;
        // I'm using unwrap_or because I don't want a panic here to be
        // considered passing the test
        let fd1 = get_unused_virtual_fd(threei::TESTING_CAGEID, REALFD, false, 100).unwrap_or(1);

        register_close_handlers(NULL_FUNC, do_panic, NULL_FUNC);

        // should panic here...
        close_virtualfd(threei::TESTING_CAGEID, fd1).unwrap();
    }

    #[test]
    #[should_panic]
    // Let's check that our callback for close is working correctly by having
    // it panic
    fn test_unreal_handler() {
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        // I'm using unwrap_or because I don't want a panic here to be
        // considered passing the test
        let fd1 =
            get_unused_virtual_fd(threei::TESTING_CAGEID, NO_REAL_FD, false, 100).unwrap_or(1);

        register_close_handlers(NULL_FUNC, NULL_FUNC, do_panic);

        // should panic here...
        close_virtualfd(threei::TESTING_CAGEID, fd1).unwrap();
    }

    #[test]
    // No panics.  Just call a function...
    fn test_close_handlers() {
        let mut _thelock: MutexGuard<bool>;

        loop {
            match TESTMUTEX.lock() {
                Err(_) => {
                    TESTMUTEX.clear_poison();
                }
                Ok(val) => {
                    _thelock = val;
                    break;
                }
            }
        }
        refresh();

        // I'm using unwrap_or because I don't want a panic here to be
        // considered passing the test
        let fd1 =
            get_unused_virtual_fd(threei::TESTING_CAGEID, NO_REAL_FD, false, 100).unwrap_or(1);

        fn myfunc(_: u64) {}

        register_close_handlers(myfunc, myfunc, myfunc);

        // should panic here...
        close_virtualfd(threei::TESTING_CAGEID, fd1).unwrap();
    }
}
