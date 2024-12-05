use crate::{DumpItem, MAGIC};
use core::error::Error;
use core::fmt::{Display, Formatter};
use std::boxed::Box;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use thiserror::Error;

/// Provides methods to read a firmware dump.
pub struct DumpReader<F> {
    file: F,
    items: u32,
}

impl<F: Read + Seek> DumpReader<F> {
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

        // Read item count.
        let mut items = [0u8; 4];

        file.seek(SeekFrom::End(-4))
            .map_err(ReaderError::SeekItemCount)?;
        file.read_exact(&mut items).map_err(ReaderError::Read)?;
        file.seek(SeekFrom::Start(4))
            .map_err(ReaderError::SeekFirstItem)?;

        Ok(Self {
            file,
            items: u32::from_le_bytes(items),
        })
    }

    /// Returns total items in this dump, including nested items.
    pub fn items(&self) -> u32 {
        self.items
    }

    pub fn next_item(&mut self) -> Result<Option<ItemReader<F>>, ReaderError> {
        // Read item type.
        let mut ty = 0u8;

        self.file
            .read_exact(std::slice::from_mut(&mut ty))
            .map_err(ReaderError::Read)?;

        // Read item version.
        let mut ver = 0u8;

        self.file
            .read_exact(std::slice::from_mut(&mut ver))
            .map_err(ReaderError::Read)?;

        // Check type.
        let ty = DumpItem::try_from(ty).map_err(|_| ReaderError::UnknownItem(ty))?;
        let r = match ty {
            DumpItem::End => return Ok(None),
            DumpItem::Ps4Part => match crate::ps4::PartReader::new(&mut self.file, ver) {
                Ok(v) => ItemReader::Ps4Part(v),
                Err(e) => return Err(ReaderError::ItemReader(ty, Box::new(e))),
            },
        };

        Ok(Some(r))
    }
}

/// Encapsulates a reader for dump item.
#[derive(Debug)]
pub enum ItemReader<'a, F> {
    Ps4Part(crate::ps4::PartReader<'a, F>),
}

impl<F> Display for ItemReader<'_, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            Self::Ps4Part(_) => "PlayStation 4 partition",
        };

        f.write_str(name)
    }
}

/// Represents an error when [`DumpReader`] fails to read the dump.
#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("the specified file is not a firmware dump")]
    NotFirmwareDump,

    #[error("couldn't read the specified file")]
    Read(#[source] std::io::Error),

    #[error("couldn't seek to item count")]
    SeekItemCount(#[source] std::io::Error),

    #[error("couldn't seek to first item")]
    SeekFirstItem(#[source] std::io::Error),

    #[error("unknown item type {0}")]
    UnknownItem(u8),

    #[error("couldn't create reader for {0}")]
    ItemReader(DumpItem, #[source] Box<dyn Error>),
}
