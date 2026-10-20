//! Poison-tolerant locking. A panic inside one job must not take the whole
//! queue down with it: `std::sync::Mutex` marks itself poisoned when a holder
//! panics, and `.lock().unwrap()` would then panic in every later caller.
//! The data guarded here (job list, caches, recorder handle) stays valid after
//! a panic, so recovering the guard is always safe.

use std::sync::{Condvar, Mutex, MutexGuard};

pub trait LockExt<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn wait<'a, T>(cv: &Condvar, g: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    cv.wait(g).unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn recovers_from_poison() {
        let m = Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("boom");
        })
        .join();
        assert!(m.is_poisoned());
        *m.lock_safe() += 1;
        assert_eq!(*m.lock_safe(), 2);
    }
}
