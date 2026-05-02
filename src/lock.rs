#[macro_export]
macro_rules! generate_rw_accessors {
    ( $var:ident $backtrace:ident $read:ident $write:ident $panic:ident $state:ty ) => {
        use std::backtrace::Backtrace;
        use std::sync::{RwLockReadGuard, RwLockWriteGuard, TryLockError};

        static $backtrace: RwLock<Option<Backtrace>> = RwLock::new(None);

        pub fn $read() -> RwLockReadGuard<'static, $state> {
            match $var.try_read() {
                Ok(guard) => guard,
                Err(TryLockError::WouldBlock) => {
                    $panic();
                }
                Err(TryLockError::Poisoned(error)) => {
                    panic!(
                        "Poisoned read lock while fetching state: {}",
                        error.to_string()
                    );
                }
            }
        }

        pub fn $write() -> RwLockWriteGuard<'static, $state> {
            match $var.try_write() {
                Ok(guard) => {
                    $backtrace.try_write().unwrap().replace(Backtrace::capture());

                    guard
                }
                Err(TryLockError::WouldBlock) => {
                    $panic();
                }
                Err(TryLockError::Poisoned(error)) => {
                    panic!(
                        "Poisoned write lock while fetching state: {}",
                        error.to_string()
                    );
                }
            }
        }

        fn $panic() -> ! {
            let backtrace_opt = $backtrace.try_read().unwrap();

            match backtrace_opt.as_ref() {
                Some(backtrace) => {
                    let backtrace_str = backtrace.to_string();
                    let backtrace_split: Vec<&str> = backtrace_str.split('\n').collect();

                    let backtrace_trimmed = {
                        if backtrace_split.len() > 5 {
                            backtrace_split[2..4].join("\n")
                        } else {
                            backtrace_split.join("\n")
                        }
                    };

                    panic!(
                        "Unable to acquire lock while fetching state. Last write lock acquisition place:\n\n{}\n\n",
                        backtrace_trimmed
                    );
                }
                None => {
                    panic!(
                        "Unable to acquire lock while fetching state. No write lock backtrace is available.",
                    );
        
                }
            };
        }
    };
}
