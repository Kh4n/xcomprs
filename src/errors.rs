use std::{ffi::NulError, str::Utf8Error};

use derive_more::From;
use x11rb::rust_connection::{ConnectionError, ReplyError, ReplyOrIdError};

#[derive(Debug, From)]
pub enum CompError {
    Reply(ReplyError),
    ReplyOrId(ReplyOrIdError),
    Conn(ConnectionError),
    Str(String),
    Utf8Err(Utf8Error),
    IoErr(std::io::Error),
    NulErr(NulError),
}
