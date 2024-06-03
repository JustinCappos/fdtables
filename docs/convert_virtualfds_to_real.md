Convert virtual fds to real, likely to handle the poll or ppoll command.

This is a helper function for poll / ppoll which is called before the actual
call is made.  It is given a cageid and a vector of virtualfds.  It returns
a vector of virtualfds of the same size.  Each entry is either replaced by
the realfd, NO_REAL_FD (if it is unreal), or INVALID_FD (if invalid).  The
unrealvector returned has a length equal to the number of NO_REAL_FD elements
with each containing a (virtfd, optionalinfo) tuple, ordered by the original
vector.  A vector of the virtual fds used that were invalid is also returned 
(with the entries in the same order as the original vector).  There is also a 
mapping table (hashmap) returned, which is used to reverse this call.  For 
more info, see [convert_realfds_back_to_virtual].  (Note, you must use 
the same mapping table from your prior call when using this function.)

Panics:
    unknown cageid

Errors:
    None

Example:
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
// get_specific_virtual_fd(cage_id, VIRTFD, REALFD, CLOEXEC, OPTINFO)
get_specific_virtual_fd(cage_id, 3, 7, false, 10).unwrap();
get_specific_virtual_fd(cage_id, 5, NO_REAL_FD, false, 123).unwrap();

let (realfds, unrealfds, invalidfds, mappingtable) = convert_virtualfds_to_real(cage_id, vec!(1,3,5));

assert_eq!(realfds,vec!(INVALID_FD,7,NO_REAL_FD));

```
