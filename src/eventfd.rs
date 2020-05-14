use mio::{unix::EventedFd, Poll, PollOpt, Ready, Token};
use std::{io, os::unix::prelude::*};

pub struct EventFd(RawFd);

impl EventFd {
    pub fn new(initval: u32) -> io::Result<Self> {
        unsafe {
            let rc = libc::eventfd(
                initval as libc::c_uint,
                libc::EFD_CLOEXEC | libc::EFD_NONBLOCK,
            );
            if rc < 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self(rc))
        }
    }
}

impl Drop for EventFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

impl io::Read for EventFd {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let rc = libc::read(
                self.0,
                dst.as_mut_ptr().cast(), //
                dst.len(),
            );
            if rc < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(rc as usize)
        }
    }
}

impl mio::Evented for EventFd {
    fn register(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        EventedFd(&self.0).register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &Poll,
        token: Token,
        interest: Ready,
        opts: PollOpt,
    ) -> io::Result<()> {
        EventedFd(&self.0).reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        EventedFd(&self.0).deregister(poll)
    }
}
