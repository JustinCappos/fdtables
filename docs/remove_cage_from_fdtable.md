discards a cage -- likely for handling exit()

This is mostly used in handling exit, etc.  Returns the HashMap for the 
cage, so that the caller can close realfds, etc. as is needed.

Panics:
    Invalid cageid

Errors:
    None

Example:
```
# use fdtables::*;
# let src_cage_id = threei::TESTING_CAGEID;
# let cage_id = threei::TESTING_CAGEID2;
# copy_fdtable_for_cage(src_cage_id,cage_id).unwrap();
let my_virt_fd = get_unused_virtual_fd(cage_id, 10, false, 10).unwrap();
let my_cages_fdtable = remove_cage_from_fdtable(cage_id);
assert!(my_cages_fdtable.get(&my_virt_fd).is_some());
//   If we do the following line, it would panic, since the cage_id has 
//   been removed from the table...
// get_unused_virtual_fd(cage_id, 10, false, 10)
```
