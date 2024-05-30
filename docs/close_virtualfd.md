Close a virtual file descriptor, returning (realfd, remaining count)

This is a helper function for close.  It returns the realfd and the remaining
count of times this fd is open.  This is useful for letting many virtualfds
all map to the same real fd and then only closing it when the last fd is 
closed.

If the realfd is NO_REAL_FD, then it always returns (NO_REAL_FD, 0) regardless
of how many NO_REAL_FD entries there are.

Panics:
    Invalid cageid for srccageid

Errors:
    This will return EBADF if the fd isn't valid

Example:
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
# const REALFD:u64 = 209;
let my_virt_fd = get_unused_virtual_fd(cage_id, REALFD, false, 10).unwrap();
// dup2 call made for fd 15...
get_specific_virtual_fd(cage_id, 15, REALFD, false, 10).unwrap();
// Now they close the original fd...
let (realfd, count) = close_virtualfd(cage_id,my_virt_fd).unwrap();
assert_eq!(realfd,REALFD);
// ... but one reference remains!
assert_eq!(count,1);
```
