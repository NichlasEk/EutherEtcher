use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::error::{EutherError, Result};

#[derive(Debug, Clone, Default)]
pub struct CancelFlag {
    cancelled: Arc<AtomicBool>,
}

impl CancelFlag {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err(EutherError::Cancelled)
        } else {
            Ok(())
        }
    }
}
