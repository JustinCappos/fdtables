Convert real fds back to virtual, likely after a poll or ppoll command.

This is a helper function for poll / ppoll which is called after the actual
call is made.  It uses the mapping table from the previous 
[convert_virtualfds_to_real] command to translate a vector of realfds
back to virtual.  The vector should not contain any NO_REAL_FD values or 
INVALID_FD values.

Panics:
    Invalid mappingtable
    NO_REAL_FD, INVALID_FD, or unknown value provided

Errors:
    None

Example:
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
// get_specific_virtual_fd(cage_id, VIRTFD, REALFD, CLOEXEC, OPTINFO)
get_specific_virtual_fd(cage_id, 3, 7, false, 10).unwrap();
get_specific_virtual_fd(cage_id, 5, NO_REAL_FD, false, 123).unwrap();

let (mut realfds, unrealfds, invalidfds, mappingtable) = convert_virtualfds_to_real(cage_id, vec!(1,3,5));

// Toss out the unreal and invalid ones...
realfds.retain(|&realfd| realfd != NO_REAL_FD && realfd != INVALID_FD);

// poll(...)  // let's pretend that realfd 7 had its event triggered...
let newrealfds = convert_realfds_back_to_virtual(vec!(7),mappingtable);
// virtfd 3 should be returned
assert_eq!(newrealfds,vec!(3))

```
