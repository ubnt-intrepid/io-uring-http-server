use iou::SubmissionQueueEvent;
use libc::{sockaddr, socklen_t};
use std::os::unix::prelude::*;

pub trait SubmissionQueueEventExt {
    unsafe fn prep_accept(
        &mut self,
        sock: RawFd,
        addr: *mut sockaddr,
        addrlen: *mut socklen_t,
        flags: i32,
    );
}

impl SubmissionQueueEventExt for SubmissionQueueEvent<'_> {
    unsafe fn prep_accept(
        &mut self,
        sock: RawFd,
        addr: *mut sockaddr,
        addrlen: *mut socklen_t,
        flags: i32,
    ) {
        uring_sys::io_uring_prep_accept(self.raw_mut(), sock, addr, addrlen, flags);
    }
}
