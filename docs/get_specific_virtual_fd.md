This is used to get a specific virtualfd mapping.

Useful for implementing something like dup2.  Use this only if you care 
which virtualfd you get.  Otherwise use [get_unused_virtual_fd].

Panics:
    if the cageid does not exist

Errors:
    returns ELIND if you're picking an already used virtualfd.  If you
    want to mimic dup2's behavior, you need to close it first, which the
    caller should handle.
    returns EBADF if it's not in the range of valid fds.

Example:
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
# let realfd: u64 = 10;
# let virtfd: u64 = 1000;
// Should not error...
assert!(get_specific_virtual_fd(cage_id, virtfd, realfd, false, 100).is_ok());
// Check that you get the real fd back here...
assert_eq!(realfd,translate_virtual_fd(cage_id, virtfd).unwrap());
```
