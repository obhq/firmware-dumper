#![no_std]

#[cfg(feature = "read")]
pub use self::reader::*;

use core::fmt::{Display, Formatter};
use num_enum::{IntoPrimitive, TryFromPrimitive};

pub mod ps4;

#[cfg(feature = "read")]
mod reader;

#[cfg(feature = "read")]
extern crate std;

pub const MAGIC: &'static [u8; 4] = b"\x7FOBF";

/// Type of top-level item in the dump file.
#[repr(u8)]
#[derive(Debug, Clone, Copy, IntoPrimitive, TryFromPrimitive)]
pub enum DumpItem {
    End = 0,
    Ps4Part = 1,
}

impl Display for DumpItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let v = match self {
            Self::End => "",
            Self::Ps4Part => "PlayStation 4 partition",
        };

        f.write_str(v)
    }
}
