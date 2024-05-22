/* Here I'm testing a few basic design choices around how to implement
 * the fdtables library.  In this way, another person can later look and
 * see what I tried and why I decided to do this in the way I did.
 * The main part of this testing will focus on the virtual fd -> real fd 
 * translation as well as virtual fd acquisition will be the focus since
 * they are the most common operations.  */

use criterion::{criterion_group, criterion_main, Criterion};

use std::sync::{Mutex,Arc};

use std::thread;

use std::collections::HashMap;

use dashmap;

// We will get / put FDTableEntry structures for each...
// I hate doing this, but I'm going to drop the winning implementation into
// fdtables, so I might as well avoid writing fdtables::... everywhere so that
// I don't need to change it back later
use fdtables::*;

// I will test the following different things:
//
// --- Solution without locking ---
//  HashMap<u64,HashMap<u64,FDTableEntry>>
//      Done: Unlocked
//
// --- Solutions with global locking ---
//  Mutex<HashMap<u64,HashMap<u64,FDTableEntry>>>
//      This is the default thing I implemented.
//      Done: GlobalVanilla
//
//  DashMap<u64,HashMap<u64,FDTableEntry>>
//      Just a basic solution with a dashmap instead of a mutex + hashmap
//      Done: GlobalDashMap
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

pub trait FDTableTestable: Send + Sync {
    fn refresh(&mut self); //both initializes and cleans up, as is needed...
    fn translate_virtual_fd(&self,cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal>;
    fn get_unused_virtual_fd(&mut self, cageid: u64, realfd: u64, should_cloexec: bool, optionalinfo: u64,) -> Result<u64, threei::RetVal>;
    fn get_optionalinfo(&self, cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal>;
    fn set_optionalinfo(&mut self, cageid: u64, virtualfd: u64, optionalinfo: u64,) -> Result<(), threei::RetVal>;
    fn copy_fdtable_for_cage(&mut self, srccageid: u64, newcageid: u64) -> Result<(), threei::Errno>;
}

//unsafe impl Send for FDTableTestable {}
//unsafe impl Sync for FDTableTestable {}

// ------------------ !!!!!    Unlocked    !!!!! ------------------ //

//  HashMap<u64,HashMap<u64,FDTableEntry>>
pub struct UnlockedComparison {
    fdtable:HashMap<u64,HashMap<u64,FDTableEntry>>,
}

unsafe impl Send for UnlockedComparison {}
unsafe impl Sync for UnlockedComparison {}


// This is basically all copied from the locked version of this code...
impl FDTableTestable for UnlockedComparison {
    // Setup or destroy and recreate the hashmap by creating a new one and 
    // throwing away the old.  I'll use this before the first test and between
    // sets of tests...
    fn refresh(&mut self) {
        self.fdtable = HashMap::new();
        self.fdtable.insert(threei::TESTING_CAGEID,HashMap::new());
    }

    fn translate_virtual_fd(&self,cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        return match self.fdtable.get(&cageid).unwrap().get(&virtualfd) {
            Some(tableentry) => Ok(tableentry.realfd),
            None => Err(threei::Errno::EBADFD as u64),
        };
    }

    fn get_unused_virtual_fd(&mut self, cageid: u64, realfd: u64, should_cloexec: bool, optionalinfo: u64,) -> Result<u64, threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        let myentry = FDTableEntry {
            realfd,
            should_cloexec,
            optionalinfo,
        };

        // Check the fds in order.
        for fdcandidate in 0..FD_PER_PROCESS_MAX {
            if !self.fdtable.get(&cageid).unwrap().contains_key(&fdcandidate) {
                // I just checked.  Should not be there...
                self.fdtable
                    .get_mut(&cageid)
                    .unwrap()
                    .insert(fdcandidate, myentry);
                return Ok(fdcandidate);
            }
        }

        // I must have checked all fds and failed to find one open.  Fail!
        Err(threei::Errno::EMFILE as u64)

    }

    fn get_optionalinfo(&self, cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        return match self.fdtable.get(&cageid).unwrap().get(&virtualfd) {
            Some(tableentry) => Ok(tableentry.optionalinfo), 
            None => Err(threei::Errno::EBADFD as u64),
        };
    }

    fn set_optionalinfo(&mut self, cageid: u64, virtualfd: u64, optionalinfo: u64,) -> Result<(), threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        // Set the is_cloexec flag or return EBADFD, if that's missing...
        return match self.fdtable.get_mut(&cageid).unwrap().get_mut(&virtualfd) {
            Some(tableentry) => {
                tableentry.optionalinfo = optionalinfo;
                Ok(())
            }
            None => Err(threei::Errno::EBADFD as u64),
        };

    }

    fn copy_fdtable_for_cage(&mut self, srccageid: u64, newcageid: u64) -> Result<(), threei::Errno> {
        if !self.fdtable.contains_key(&srccageid) {
            panic!("Unknown srccageid in fdtable access");
        }
        if self.fdtable.contains_key(&newcageid) {
            panic!("Known newcageid in fdtable access");
        }
    
        // Insert a copy and ensure it didn't exist...
        let hmcopy = self.fdtable.get(&srccageid).unwrap().clone();
        assert!(self.fdtable.insert(newcageid, hmcopy).is_none());
        Ok(())
        // I'm not going to bother to check the number of fds used overall yet...
        //    Err(threei::Errno::EMFILE as u64),
    }

}

// ------------------ !!!!!    Global Vanilla    !!!!! ------------------ //
    
//  Mutex<HashMap<u64,HashMap<u64,FDTableEntry>>>
struct GlobalVanilla {
    globalfdtable: Mutex<HashMap<u64, HashMap<u64,FDTableEntry>>>,
}

unsafe impl Send for GlobalVanilla {}
unsafe impl Sync for GlobalVanilla {}

// This is basically all copied from the locked version of this code...
impl FDTableTestable for GlobalVanilla {
    // Setup or destroy and recreate the hashmap by creating a new one and 
    // throwing away the old.  I'll use this before the first test and between
    // sets of tests...
    fn refresh(&mut self) {
        let mut fdtable = self.globalfdtable.lock().unwrap();
        _ = fdtable.drain();
        *fdtable = HashMap::new();
        fdtable.insert(threei::TESTING_CAGEID,HashMap::new());
    }

    fn translate_virtual_fd(&self,cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
        let fdtable = self.globalfdtable.lock().unwrap();
        if !fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
            Some(tableentry) => Ok(tableentry.realfd),
            None => Err(threei::Errno::EBADFD as u64),
        };
    }

    fn get_unused_virtual_fd(&mut self, cageid: u64, realfd: u64, should_cloexec: bool, optionalinfo: u64,) -> Result<u64, threei::RetVal> {
        let mut fdtable = self.globalfdtable.lock().unwrap();
        if !fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

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

    fn get_optionalinfo(&self, cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
        let fdtable = self.globalfdtable.lock().unwrap();
        if !fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        return match fdtable.get(&cageid).unwrap().get(&virtualfd) {
            Some(tableentry) => Ok(tableentry.optionalinfo), 
            None => Err(threei::Errno::EBADFD as u64),
        };
    }

    fn set_optionalinfo(&mut self, cageid: u64, virtualfd: u64, optionalinfo: u64,) -> Result<(), threei::RetVal> {
        let mut fdtable = self.globalfdtable.lock().unwrap();
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

    fn copy_fdtable_for_cage(&mut self, srccageid: u64, newcageid: u64) -> Result<(), threei::Errno> {
        let mut fdtable = self.globalfdtable.lock().unwrap();
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

}

// ------------------ !!!!!    Global Dashmap    !!!!! ------------------ //

//  DashMap<u64,HashMap<u64,FDTableEntry>>
pub struct DashMapComparison {
    pub fdtable: dashmap::DashMap<u64,HashMap<u64,FDTableEntry>>,
}

unsafe impl Send for DashMapComparison {}
unsafe impl Sync for DashMapComparison {}

// This is basically all copied from the locked version of this code...
impl FDTableTestable for DashMapComparison {
    // Setup or destroy and recreate the hashmap by creating a new one and 
    // throwing away the old.  I'll use this before the first test and between
    // sets of tests...
    fn refresh(&mut self) {
        self.fdtable = dashmap::DashMap::new();
        self.fdtable.insert(threei::TESTING_CAGEID,HashMap::new());
    }

    fn translate_virtual_fd(&self,cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        return match self.fdtable.get(&cageid).unwrap().get(&virtualfd) {
            Some(tableentry) => Ok(tableentry.realfd),
            None => Err(threei::Errno::EBADFD as u64),
        };
    }

    fn get_unused_virtual_fd(&mut self, cageid: u64, realfd: u64, should_cloexec: bool, optionalinfo: u64,) -> Result<u64, threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        let myentry = FDTableEntry {
            realfd,
            should_cloexec,
            optionalinfo,
        };

        // Check the fds in order.
        for fdcandidate in 0..FD_PER_PROCESS_MAX {
            if !self.fdtable.get(&cageid).unwrap().contains_key(&fdcandidate) {
                // I just checked.  Should not be there...
                self.fdtable
                    .get_mut(&cageid)
                    .unwrap()
                    .insert(fdcandidate, myentry);
                return Ok(fdcandidate);
            }
        }

        // I must have checked all fds and failed to find one open.  Fail!
        Err(threei::Errno::EMFILE as u64)

    }

    fn get_optionalinfo(&self, cageid: u64, virtualfd: u64) -> Result<u64, threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        return match self.fdtable.get(&cageid).unwrap().get(&virtualfd) {
            Some(tableentry) => Ok(tableentry.optionalinfo), 
            None => Err(threei::Errno::EBADFD as u64),
        };
    }

    fn set_optionalinfo(&mut self, cageid: u64, virtualfd: u64, optionalinfo: u64,) -> Result<(), threei::RetVal> {
        if !self.fdtable.contains_key(&cageid) {
            panic!("Unknown cageid in fdtable access");
        }

        // Set the is_cloexec flag or return EBADFD, if that's missing...
        return match self.fdtable.get_mut(&cageid).unwrap().get_mut(&virtualfd) {
            Some(tableentry) => {
                tableentry.optionalinfo = optionalinfo;
                Ok(())
            }
            None => Err(threei::Errno::EBADFD as u64),
        };

    }

    fn copy_fdtable_for_cage(&mut self, srccageid: u64, newcageid: u64) -> Result<(), threei::Errno> {
        if !self.fdtable.contains_key(&srccageid) {
            panic!("Unknown srccageid in fdtable access");
        }
        if self.fdtable.contains_key(&newcageid) {
            panic!("Known newcageid in fdtable access");
        }
    
        // Insert a copy and ensure it didn't exist...
        let hmcopy = self.fdtable.get(&srccageid).unwrap().clone();
        assert!(self.fdtable.insert(newcageid, hmcopy).is_none());
        Ok(())
        // I'm not going to bother to check the number of fds used overall yet...
        //    Err(threei::Errno::EMFILE as u64),
    }

}


// ---*****----- !!!!! BENCHMARKS BENCHMARKS BENCHMARKS !!!!! -----*****--- //

// This is a horrible hack because Rust doesn't have a good way to let you
// name traits in the same way you do objects...  For some dumb reason I've 
// read one can put these on the heap and it works fine, likely because the 
// compiler doesn't know ahead of time how much space to allocate on the stack 
// for this object...  Anyways, I'm going to sidestep this nonsense completely
pub fn run_benchmark(c: &mut Criterion) {
    // I'll focus on different data structures and techniques here...
    do_a_benchmark(c,UnlockedComparison{fdtable:HashMap::new()},"Unlocked");
    do_a_benchmark(c,GlobalVanilla{globalfdtable:Mutex::new(HashMap::new())},"GlobalVanilla");
    do_a_benchmark(c,DashMapComparison{fdtable:dashmap::DashMap::new()},"GlobalDashMap");

}

pub fn do_a_benchmark(c: &mut Criterion,mut algorithm: impl FDTableTestable + 'static, algoname:&str) {

    let mut group = c.benchmark_group("primitives basics");
    // Set it up...
    algorithm.refresh();

    let fd = algorithm.get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
    group.bench_function(format!("{}: translate_virtual_fd (10000)",algoname),
            |b| b.iter(|| {
                for _ in [0..1000].iter() {
                    algorithm.translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                }
            })
        );

    algorithm.refresh();
    group.bench_function(format!("{}: get_translate_refresh (1000)",algoname),
            |b| b.iter(|| {
                for _ in [0..1000].iter() {
                    let fd = algorithm.get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
                    algorithm.translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                }
                algorithm.refresh();
            })
        );


    if algoname == "Unlocked" {
        println!("--------Skipping multi-threaded tests--------!");
        group.finish();
        return;

    }

    //use std::time::Duration;


//    let newalgoname = algoname.clone();

    /*
    let handle = thread::spawn(move|| {
        for _i in 1..5 {
            print!("c {}",algoname.to_string());
            thread::sleep(Duration::from_millis(1));
        }
    });

    for _i in 1..5 {
//            print!("m {}",algoname);
            thread::sleep(Duration::from_millis(1));
        }

    handle.join().unwrap(); */

    // ---------------- MULTI-THREADED TESTS ------------------  //

    let fd = algorithm.get_unused_virtual_fd(threei::TESTING_CAGEID, 10, true, 100).unwrap();
    let _fd2 = algorithm.get_unused_virtual_fd(threei::TESTING_CAGEID, 20, true, 200).unwrap();
    let _fd3 = algorithm.get_unused_virtual_fd(threei::TESTING_CAGEID, 30, true, 300).unwrap();

    let mut thread_handle_vec:Vec<thread::JoinHandle<()>> = Vec::new();
    let algwrapper = Arc::new(algorithm);

    group.bench_function(format!("{}: translate_virtual_fd (10000)",algoname), |b| b.iter({ 
        || {
                let newalgorithm  = Arc::clone(&algwrapper);
                thread_handle_vec.push(thread::spawn(move || {
                    for _ in [0..1000].iter() {
                        newalgorithm.translate_virtual_fd(threei::TESTING_CAGEID, fd).unwrap();
                    }
                }));
            }}
        )
        );

    for handle in thread_handle_vec {
        handle.join().unwrap();
    }
    //algorithm.refresh();

    group.finish();
}


criterion_group!(benches, run_benchmark);
criterion_main!(benches);
