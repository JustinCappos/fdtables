Waits on epoll.   Only unrealfds will be returned

This call handles `epoll_wait`.  It returns a tuple of two values.  The first
is the realfd of the underlying epollfd.  The second is the set of unrealfds
in the epollfd.  It only returns unrealfds because `epoll_ctl` doesn't support 
realfds (those are known by the interface below).  It returns a hashmap of 
\<virtfd,`epoll_events`\> for the epollfd.  See [`epoll_create_helper`] and 
[`try_epoll_ctl`] for more details.


# Panics
  cageid does not exist

# Errors
  EBADF  the epollfd doesn't exist.

  EINVAL the epollfd isn't an epoll file descriptor.


# Example
```
# use fdtables::*;
# let cage_id = threei::TESTING_CAGEID4;
# init_empty_cage(cage_id);
# let realepollfd:u64 = EPOLLFD;
// make an unreal fd...
let unrealfd = get_unused_virtual_fd(cage_id,NO_REAL_FD, false, 123).unwrap();

// let's create an epollfd which will watch it...
let myepollfd = epoll_create_helper(cage_id,realepollfd,false).unwrap();

let myevent = epoll_event {
    events: (EPOLLIN + EPOLLOUT) as u32,
    u64: 0,
};

// Add the unreal fd...
assert_eq!(try_epoll_ctl(cage_id,myepollfd,EPOLL_CTL_ADD,unrealfd,myevent.clone()).unwrap(),(realepollfd,NO_REAL_FD));

// This should return the unrealfd's info!
assert_eq!(get_epoll_wait_data(cage_id,myepollfd).unwrap().1[&unrealfd].events,myevent.events);
```
