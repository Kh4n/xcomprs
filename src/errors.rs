use std::{error::Error, ffi::NulError, num::TryFromIntError, str::Utf8Error};

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
    IntConvertError(TryFromIntError),
}

// pub struct TestErr<'a, T: Error> {
//     msg: &'a str,
//     error: T,
// }

// fn test() {
//     let t = TestErr {
//         msg: "e",
//         error: ReplyError::ConnectionError(ConnectionError::FDPassingFailed),
//     };
// }
