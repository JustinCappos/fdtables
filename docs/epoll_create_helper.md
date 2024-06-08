Creates a new fd for epoll and eppoll (only for `NO_REAL_FD` fds at this level)

This is a helper function for `epoll_create` and related functions.  It creates 
a new epoll fd which can be used with other calls like `epoll_ctl` and 
`epoll_wait`.  It also determines if cloexec should be set or not (it is not
on `epoll_create`, but can be set on `epoll_create1`).   See the calls 
[`try_epoll_ctl`] and [`get_epoll_wait_data`] for more details.


# Panics
  cageid does not exist

# Errors
  EMFILE if there are no open file descriptors

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
