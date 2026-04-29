#[macro_export]
macro_rules! posix_call {
    ( $err:expr, $call:expr ) => {
        {
            let result = $call;
            if result < 0 {
                let error = std::io::Error::last_os_error();
                $err.push(format!("Posix call error: {}", error.to_string()));
            }

            result
        }
    };
}


pub mod epoll {
    use std::os::fd::RawFd;
    use std::collections::HashSet;

    use libc::{epoll_create1, epoll_ctl, epoll_wait};
    use libc::{epoll_event};
    use libc::{EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL};

    pub use libc::EPOLLIN;

    pub fn create() -> RawFd {
        let fd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            panic!("Unable to create epoll fd: {}", error.to_string());
        }

        fd
    }

    pub fn close(fd: RawFd) {
        unsafe { libc::close(fd) };
    }

    pub fn wait_on_fds(epoll_fd: RawFd, fds: Vec<RawFd>, events: Option<i32>, timeout: i32) -> Result<HashSet<RawFd>, String> {
        let monitored_events = (events.unwrap_or(EPOLLIN)) as u32;
        let mut received_events = vec![epoll_event { events: 0, u64: 0 }; fds.len()];

        let mut event = epoll_event {events: monitored_events, u64: 0};

        let awakened_fds = unsafe {
            let mut errors = Vec::<String>::new();

            for fd in fds.iter() {
                event.u64 = *fd as u64;
                posix_call!(errors, epoll_ctl(epoll_fd, EPOLL_CTL_ADD, *fd, &raw mut event));
            }

            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }

            posix_call!(errors, epoll_wait(epoll_fd, received_events.as_mut_ptr(), fds.len() as i32, timeout));

            for fd in fds {
                posix_call!(errors, epoll_ctl(epoll_fd, EPOLL_CTL_DEL, fd, &raw mut event));
            }

            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }

            let mut received_fds = HashSet::new();
            for recv_event in received_events {
                if (recv_event.events & monitored_events) != 0 {
                    let fd = recv_event.u64 as libc::c_int;
                    received_fds.insert(fd);
                }
            }

            received_fds
        };

        Ok(awakened_fds)
    }
}
