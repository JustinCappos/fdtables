Sets up user defined functions to be called when a `close()` happens.  

This lets a user function register itself to be called with either the
realfd (for realfds) or the optionalinfo (if not a realfd) whenever
something is closed by exec, exit, close, etc.  The first argument is 
called when a close is called on a realfd, but a reference to the realfd
still remains.  The second argument is called when a close on the last
reference to the realfd is called.  The third argument is called when 
a close on an unreal fd is performed.

# Panics
  Never

# Errors
  None

# Example
```should_panic
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID;
# let realfd: u64 = 10;
# const MYVIRTFD:u64 = 123;
fn oh_no(num:u64) {
    panic!("AAAARRRRGGGGHHHH!!!!");
}

// oh_no should be called when all references to the realfd are closed...
register_close_handlers(NULL_FUNC,oh_no,NULL_FUNC);

// Get a fd and dup it...
let my_virt_fd = get_unused_virtual_fd(cage_id, realfd, false, 100).unwrap();
get_specific_virtual_fd(cage_id, MYVIRTFD, realfd, false, 100).unwrap();

// Nothing should happen when I call this, since I'm closing only one reference
// and I registered the NULL_FUNC for this scenario...
close_virtualfd(cage_id,MYVIRTFD);
// However, after this, I will panic..
close_virtualfd(cage_id,my_virt_fd);
```
