Translate select's bitmasks from virtual to realfds.

This is a helper function for select, which is called before you make the
call to select.  It takes a set of virtual bitmasks and translates them to 
real bitmasks.  For each non-real fd mentioned, it returns the virtual fd / 
optionalinfo tuple so the caller can process them.  After receiving these real 
bitmasks, the caller should call select underneath.  The mapping table return 
value is needed by [`get_virtual_bitmasks_from_select_result`] to revert the 
realfds back to virtualfds.


NOTE: If the same realfd is behind multiple virtualfds, only one of those
virtualfds will be triggered.  I need to investigate how Linux behaves, but
from what I can see from a quick search, the behavior here is undefined.

# Panics
  Invalid cageid

# Errors
  This will return EBADF if any fd isn't valid
  This will return EINVAL if nfds is >= the maximum file descriptor limit

# Example
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
// get_specific_virtual_fd(cage_id, VIRTFD, REALFD, CLOEXEC, OPTINFO)
get_specific_virtual_fd(cage_id, 3, 7, false, 10).unwrap();
get_specific_virtual_fd(cage_id, 5, NO_REAL_FD, false, 123).unwrap();
let mut fds_to_check= _init_fd_set();
_fd_set(3,&mut fds_to_check);
_fd_set(5,&mut fds_to_check);
// map these into the right sets...
let (newnfds, realreadbits, realwritebits, realexceptbits, unrealset, mappingtable) = get_real_bitmasks_for_select(cage_id, 6, Some(fds_to_check), None, None).unwrap();
// Should set the read to have bit 7 set...
assert!(_fd_isset(7,& realreadbits));
assert_eq!(newnfds, 8);
// and return the set: (5,123) which is the virtualfd, optionalinfo tuple.
assert_eq!(*unrealset[0].iter().next().unwrap(), (5 as u64,123 as u64));
```
