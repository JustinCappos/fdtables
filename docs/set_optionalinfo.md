Set optional information needed by the library importer.  

This is useful if you want to assign some sort of index to virtualfds,
often if there is no realfd backing them.  For example, if you are 
implementing in-memory pipe buffers, this could be the position in an 
array where a ring buffer lives.   See also [get_optionalinfo].

Panics:
    Invalid cageid

Errors:
    BADFD if the virtualfd doesn't exist

Example:
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
let my_virt_fd = get_unused_virtual_fd(cage_id, 10, false, 10).unwrap();
set_optionalinfo(cage_id, my_virt_fd,12345).unwrap();
assert_eq!(get_optionalinfo(cage_id, my_virt_fd).unwrap(),12345);
```
