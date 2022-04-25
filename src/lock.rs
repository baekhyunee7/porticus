use crossbeam::utils::Backoff;
use spin::mutex::{SpinMutex, SpinMutexGuard};

pub fn guard<T>(spin_lock: &SpinMutex<T>) -> SpinMutexGuard<T> {
    let backoff = Backoff::new();
    loop {
        if let Some(guard) = spin_lock.try_lock() {
            return guard;
        }
        backoff.spin();
    }
}
