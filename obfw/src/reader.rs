use crate::{DumpItem, MAGIC};
use core::error::Error;
use std::boxed::Box;
use std::io::{ErrorKind, Read};
use thiserror::Error;

/// Provides methods to read a firmware dump.
pub struct DumpReader<F>(F);

impl<F: Read> DumpReader<F> {
    pub fn new(mut file: F) -> Result<Self, ReaderError> {
        // Check magic.
        let mut magic = [0u8; MAGIC.len()];

        file.read_exact(&mut magic).map_err(|e| match e.kind() {
            ErrorKind::UnexpectedEof => ReaderError::NotFirmwareDump,
            _ => ReaderError::Read(e),
        })?;

        if magic != *MAGIC {
            return Err(ReaderError::NotFirmwareDump);
        }

        Ok(Self(file))
    }

    pub fn next(&mut self) -> Result<Option<ItemReader<F>>, ReaderError> {
        // Read item type.
        let mut ty = 0u8;

        self.0
            .read_exact(std::slice::from_mut(&mut ty))
            .map_err(ReaderError::Read)?;

        // Read item version.
        let mut ver = 0u8;

        self.0
            .read_exact(std::slice::from_mut(&mut ver))
            .map_err(ReaderError::Read)?;

        // Check type.
        let ty = DumpItem::try_from(ty).map_err(|_| ReaderError::UnknownItem(ty))?;
        let r = match ty {
            DumpItem::End => return Ok(None),
            DumpItem::Ps4Part => match crate::ps4::PartReader::new(&mut self.0, ver) {
                Ok(v) => ItemReader::Ps4Part(v),
                Err(e) => return Err(ReaderError::ItemReader(ty, Box::new(e))),
            },
        };

        Ok(Some(r))
    }
}

/// Encapsulates a reader for dump item.
pub enum ItemReader<'a, F> {
    Ps4Part(crate::ps4::PartReader<'a, F>),
}

/// Represents an error when [`DumpReader`] fails to read the dump.
#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("the specified file is not a firmware dump")]
    NotFirmwareDump,

    #[error("couldn't read the specified file")]
    Read(#[source] std::io::Error),

    #[error("unknown item type {0}")]
    UnknownItem(u8),

    #[error("couldn't create reader for {0}")]
    ItemReader(DumpItem, #[source] Box<dyn Error>),
}
