use crate::backend::{Backend, RustBackend};
use crate::convert::LeanType;
use crate::value::LeanValue;
use lean2rust_runtime::Obj;
use std::fmt;

/// Marker type for Lean's `IO.Error`.
pub enum IoErrorType {}

/// A Lean `IO.Error`, with its message rendered by Lean's `IO.Error.toString`.
pub struct IoError<B: Backend = RustBackend> {
    value: LeanValue<IoErrorType, B>,
    message: String,
}

impl<B: Backend> IoError<B> {
    /// An `IO.userError` with `message`, as raised by Lean's `throw (IO.userError msg)`.
    pub fn user(message: impl Into<String>) -> Self {
        let message = message.into();
        unsafe {
            let e = B::mk_io_user_error(B::mk_string(&message));
            IoError { value: LeanValue::from_owned(e), message }
        }
    }

    /// The error message, as `IO.Error.toString` renders it.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The underlying Lean `IO.Error` value.
    pub fn value(&self) -> &LeanValue<IoErrorType, B> {
        &self.value
    }
}

impl<B: Backend> Clone for IoError<B> {
    fn clone(&self) -> Self {
        IoError { value: self.value.clone(), message: self.message.clone() }
    }
}

unsafe impl<B: Backend> LeanType<B> for IoError<B> {
    fn into_lean(self) -> Obj {
        self.value.into_obj()
    }

    unsafe fn from_lean(o: Obj) -> Self {
        unsafe { IoError { message: B::io_error_to_string(o), value: LeanValue::from_borrowed(o) } }
    }
}

impl<B: Backend> fmt::Debug for IoError<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IoError").field("message", &self.message).finish()
    }
}

impl<B: Backend> fmt::Display for IoError<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl<B: Backend> std::error::Error for IoError<B> {}
