use std::boxed::Box;

use libc::SA_SIGINFO;
use libc::{c_int, sigset_t};

macro_rules! posix_signal {
    ($name:ident $value:expr) => {
        pub const $name: PosixSignal = PosixSignal($value);
    };
}

macro_rules! posix_call {
    ( $call:expr ) => {
        unsafe {
            let result = $call;
            if result < 0 {
                let error = std::io::Error::last_os_error();
                eprintln!("Posix call error: {}", error.to_string());
            }

            result
        }
    };
}

pub struct PosixSignal(c_int);

impl PosixSignal {
    posix_signal!(SIGINT libc::SIGINT);
}

pub fn install_signal_handler(signal: PosixSignal, handler: unsafe fn() -> ()) {
    let mut mask = Box::<sigset_t>::new_zeroed();
    posix_call!(libc::sigemptyset(mask.as_mut_ptr()));

    let action = libc::sigaction {
        sa_sigaction: handler as usize,
        sa_mask: unsafe { *mask.assume_init() },
        sa_flags: SA_SIGINFO,
        sa_restorer: None,
    };

    let old_action = std::ptr::null_mut();
    posix_call!(libc::sigaction(signal.0, &raw const action, old_action));
}

#[test]
fn test_sigint() {
    use std::sync::atomic::{AtomicBool, Ordering};

    static CONDITION: AtomicBool = AtomicBool::new(false);
    unsafe fn handler() {
        CONDITION.store(true, Ordering::Relaxed);
    }

    install_signal_handler(PosixSignal::SIGINT, handler);

    posix_call!(libc::raise(libc::SIGINT));

    let time_now = std::time::Instant::now();
    let mut elapsed_time = 0;

    while !CONDITION.load(Ordering::Relaxed) {
        elapsed_time = time_now.elapsed().as_millis();
        if elapsed_time > 500 {
            break;
        }

        std::hint::spin_loop();
    }

    assert!(elapsed_time <= 500);
    assert!(CONDITION.load(Ordering::Relaxed))
}
