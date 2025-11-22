use std::ops::Deref;
use std::sync::OnceLock;

pub struct OnceCell<T>(OnceLock<T>);

unsafe impl<T: Sync + Send> Sync for OnceCell<T> {}
unsafe impl<T: Send> Send for OnceCell<T> {}

impl<T> OnceCell<T> {
    pub const fn new() -> OnceCell<T> {
        OnceCell(OnceLock::new())
    }

    pub fn set(&self, value: T) {
        if self.0.set(value).is_err() {
            panic!("OnceCell hasn't been initialized.");
        }
    }
}

impl<T> Deref for OnceCell<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.get().unwrap()
    }
}