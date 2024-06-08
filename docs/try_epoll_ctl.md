Modifies an epoll fd to add, remove, or modify a fd.

This is a helper function for `epoll_ctl`.  It only really operates on the
fds which are `NO_REAL_FD` and (?possibly?) EPOLLFD types.  It returns 
`NO_REAL_FD` if it was able to do `epoll_ctl` on a locally managed (e.g., a 
`NO_REAL_FD`) fd.  If called with a virtual fd which maps to a real fd, this 
returns the realfd.  See [`epoll_create_helper`] and [`get_epoll_wait_data`] 
for more details.

# Panics
  cageid does not exist

# Errors
  EBADF  epfd or fd is not a valid file descriptor.

  EEXIST op was `EPOLL_CTL_ADD`, and the supplied file descriptor fd
         is already registered with this epoll instance.

  EINVAL epfd is not an epoll file descriptor, or fd is the same as
         epfd, or the requested operation op is not supported by
         this interface.

  ELOOP  fd refers to an epoll instance and this `EPOLL_CTL_ADD`
         operation would result in a circular loop of epoll
         instances monitoring one another or a nesting depth of
         epoll instances greater than 5.

  ENOENT op was `EPOLL_CTL_MOD` or `EPOLL_CTL_DEL`, and fd is not
         registered with this epoll instance.

  Note, all error conditions are not checked for a realfd.  It is expected
that the caller will call the underlying epoll call which will itself error.

# Example
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID4;
# init_empty_cage(cage_id);
// make an unreal fd...
let unrealfd = get_unused_virtual_fd(cage_id,NO_REAL_FD, false, 123).unwrap();

// let's create an epollfd which will watch it...
let myepollfd = epoll_create_helper(cage_id,false).unwrap();

let myevent = epoll_event {
    events: (EPOLLIN + EPOLLOUT) as u32,
    u64: 0,
};

// Add the unreal fd...
assert_eq!(try_epoll_ctl(cage_id,myepollfd,EPOLL_CTL_ADD,unrealfd,myevent.clone()).unwrap(),NO_REAL_FD);

// This should return the unrealfd's info!
assert_eq!(get_epoll_wait_data(cage_id,myepollfd).unwrap()[&unrealfd].events,myevent.events);
```
