use crate::io_uring::SubmissionQueueEventExt as _;
use anyhow::Context as _;
use iou::IoUring;
use std::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    os::unix::prelude::*,
    ptr,
};

const QUEUE_DEPTH: u32 = 256;

enum UserData {
    Accept,
}

pub struct Server {
    listener: TcpListener,
    io_uring: IoUring,
}

impl Server {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr) //
            .context("failed to open listener socket")?;

        let io_uring = IoUring::new(QUEUE_DEPTH) //
            .context("failed to init io_uring")?;

        Ok(Self { listener, io_uring })
    }

    pub fn run_loop(&mut self) -> anyhow::Result<()> {
        self.add_accept_request()?;

        loop {
            let event = self
                .io_uring
                .wait_for_cqe()
                .context("IoUring::wait_for_cqe")?;

            let user_data = unsafe {
                let ptr = event.user_data() as *mut UserData;
                Box::from_raw(ptr)
            };
            let res = event.result().context("async request failed")?;

            drop(event);

            match *user_data {
                UserData::Accept => {
                    println!("accept");
                    let _sock = unsafe { TcpStream::from_raw_fd(res as _) };
                    self.add_accept_request()?;
                }
            }
        }
    }

    fn add_accept_request(&mut self) -> anyhow::Result<()> {
        let mut sqe = self
            .io_uring
            .next_sqe()
            .context("submission queue is full")?;

        let user_data = Box::new(UserData::Accept);
        unsafe {
            sqe.prep_accept(
                self.listener.as_raw_fd(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
            sqe.set_user_data(Box::into_raw(user_data) as _);
        }

        self.io_uring.submit_sqes()?;

        Ok(())
    }
}
