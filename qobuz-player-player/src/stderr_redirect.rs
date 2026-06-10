pub use stderr_redirect_impl::silence_stderr;

#[cfg(unix)]
mod stderr_redirect_impl {
    use std::{
        fs::OpenOptions,
        io,
        os::fd::{AsRawFd, RawFd},
    };

    pub struct StderrRedirect {
        original_fd: RawFd,
    }

    impl StderrRedirect {
        pub fn new() -> io::Result<Self> {
            let devnull = OpenOptions::new().write(true).open("/dev/null")?;
            let devnull_fd = devnull.as_raw_fd();

            let original_fd = unsafe { libc::dup(libc::STDERR_FILENO) };
            if original_fd < 0 {
                return Err(io::Error::last_os_error());
            }

            if unsafe { libc::dup2(devnull_fd, libc::STDERR_FILENO) } < 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self { original_fd })
        }
    }

    impl Drop for StderrRedirect {
        fn drop(&mut self) {
            unsafe {
                libc::dup2(self.original_fd, libc::STDERR_FILENO);
                libc::close(self.original_fd);
            }
        }
    }

    pub fn silence_stderr<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _redirect = StderrRedirect::new().unwrap();
        f()
    }
}

#[cfg(not(unix))]
mod stderr_redirect_impl {
    pub fn silence_stderr<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        f()
    }
}
