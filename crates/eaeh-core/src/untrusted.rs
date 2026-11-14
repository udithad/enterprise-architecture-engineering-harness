//! Type-level marking of untrusted data (model output, tool output, retrieved content).
//!
//! `Untrusted<T>` deliberately does not implement `Deref`. Code must call
//! [`Untrusted::into_data`] / [`Untrusted::as_data`] to obtain the value, and the
//! context assembler only accepts untrusted values in data slots — never as
//! instructions (see `eaeh-intelligence::context`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn as_data(&self) -> &T {
        &self.0
    }

    pub fn into_data(self) -> T {
        self.0
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Untrusted<U> {
        Untrusted(f(self.0))
    }
}
