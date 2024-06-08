This is the main virtual -> realfd lookup function for fdtables.  

Converts a virtualfd, which is used in a cage, into the realfd, which 
is known to whatever is below us, possibly the OS kernel.

# Panics
  if the cageid does not exist

# Errors
  if the virtualfd does not exist

# Example
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
# let realfd: u64 = 10;
let my_virt_fd = get_unused_virtual_fd(cage_id, realfd, false, 100).unwrap();
// Check that you get the real fd back here...
assert_eq!(realfd,translate_virtual_fd(cage_id, my_virt_fd).unwrap());
```
