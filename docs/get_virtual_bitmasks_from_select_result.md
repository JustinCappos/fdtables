Translate the real bitmasks returned by select into virtual ones for the caller

This is a helper function for select called after select is called.  After 
a select call returns, there are a series of realfd bitmasks which need to be 
translated to virtualfd bitmasks (as this is what the caller expects).  Also, 
three `HashSet`s of virtualfds may be provided, which allows handling of 
non-realfds.  See also: [`get_real_bitmasks_for_select`].  (Note, you must use 
the same mapping table from your prior call when using this function.)

# Panics
  `mapping_table` is missing elements from the realfd's.
  nfds is larger than `FD_PER_PROCESS_MAX`

# Errors
  None

# Example
```
# use fdtables::*;
# use std::collections::HashSet;
# let cage_id = threei::TESTING_CAGEID;
// get_specific_virtual_fd(cage_id, VIRTFD, REALFD, CLOEXEC, OPTINFO)
get_specific_virtual_fd(cage_id, 3, 7, false, 10).unwrap();
get_specific_virtual_fd(cage_id, 5, NO_REAL_FD, false, 123).unwrap();

let mut fds_to_check= _init_fd_set();
_fd_set(3,&mut fds_to_check);
_fd_set(5,&mut fds_to_check);

// map these into the right sets...
let (newnfds, realreadbits, realwritebits, realexceptbits, unrealset, mappingtable) = get_real_bitmasks_for_select(cage_id, 6, Some(fds_to_check), None, None).unwrap();
 
// select(....)  Suppose that fd 7 was readable...

// and let's say our unreal handler was too...
# let mut unrealreadhashes = HashSet::new();
# unrealreadhashes.insert(5);
// we would call:
let (amount, virtread, virtwrite, virtexcept) = get_virtual_bitmasks_from_select_result(8,realreadbits,realwritebits,realexceptbits, unrealreadhashes, HashSet::new(), HashSet::new(),&mappingtable).unwrap();
```
