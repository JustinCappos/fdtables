removes and returns a hashmap of all entries with should_cloexec set

This goes through every entry in a cage's fdtable and removes all entries
that have should_cloexec set to true.  These entries are all added to a
new hashmap which is returend.  This is useful for handling exec, as the
caller can now decide how to handle each fd.

Panics:
    Invalid cageid

Errors:
    None

Example:
```
# use fdtables::*;
# let src_cage_id = threei::TESTING_CAGEID;
# let cage_id = threei::TESTING_CAGEID3;
# copy_fdtable_for_cage(src_cage_id,cage_id).unwrap();
let my_virt_fd = get_unused_virtual_fd(cage_id, 20, true, 17).unwrap();
let my_virt_fd2 = get_unused_virtual_fd(cage_id, 33, false, 16).unwrap();
let cloexec_fdtable = empty_fds_for_exec(cage_id);
// The first fd should be closed and returned...
assert!(cloexec_fdtable.get(&my_virt_fd).is_some());
// So isn't in the original table anymore...
assert!(translate_virtual_fd(cage_id, my_virt_fd).is_err());
// The second fd isn't returned...
assert!(cloexec_fdtable.get(&my_virt_fd2).is_none());
// Because it is still in the original table...
assert!(translate_virtual_fd(cage_id, my_virt_fd2).is_ok());
```
