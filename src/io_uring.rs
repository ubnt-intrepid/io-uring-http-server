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

    unsafe fn prep_readv(&mut self, fd: RawFd, bufs: &mut [libc::iovec], flags: libc::off_t);
}

impl SubmissionQueueEventExt for SubmissionQueueEvent<'_> {
    unsafe fn prep_accept(
        &mut self,
        sock: RawFd,
        addr: *mut sockaddr,
        addrlen: *mut socklen_t,
        flags: i32,
    ) {
        uring_sys::io_uring_prep_accept(
            self.raw_mut(), //
            sock,
            addr,
            addrlen,
            flags,
        );
    }

    unsafe fn prep_readv(&mut self, fd: RawFd, iovecs: &mut [libc::iovec], offset: libc::off_t) {
        uring_sys::io_uring_prep_readv(
            self.raw_mut(),
            fd,
            iovecs.as_mut_ptr(),
            iovecs.len() as libc::c_uint,
            offset,
        )
    }
}
