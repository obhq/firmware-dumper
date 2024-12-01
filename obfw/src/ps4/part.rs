use super::PartItem;
use core::cmp::min;
use std::boxed::Box;
use std::io::{ErrorKind, Read};
use std::vec::Vec;
use thiserror::Error;

/// Provides methods to read PlayStation 4 partition dump.
pub struct PartReader<'a, F> {
    dump: &'a mut F,
    fs: Vec<u8>,
    dev: Vec<u8>,
}

impl<'a, F: Read> PartReader<'a, F> {
    pub(crate) fn new(file: &'a mut F, ver: u8) -> Result<Self, PartError> {
        match ver {
            0 => Self::read_v0(file),
            v => Err(PartError::UnknownVersion(v)),
        }
    }

    pub fn fs(&self) -> &[u8] {
        &self.fs
    }

    pub fn dev(&self) -> &[u8] {
        &self.dev
    }

    pub fn next(&mut self) -> Result<Option<PartData>, PartError> {
        // Read item type.
        let mut ty = 0;

        self.dump
            .read_exact(std::slice::from_mut(&mut ty))
            .map_err(PartError::Read)?;

        // Create item reader.
        let ty = PartItem::try_from(ty).map_err(|_| PartError::UnknownItem(ty))?;
        let data = match ty {
            PartItem::End => return Ok(None),
            PartItem::Directory => Self::read_str(self.dump).map(PartData::Directory)?,
            PartItem::File => self.read_file()?,
        };

        Ok(Some(data))
    }

    fn read_v0(file: &'a mut F) -> Result<Self, PartError> {
        // Read filesystem type and device name.
        let fs = Self::read_str(file)?;
        let dev = Self::read_str(file)?;

        // Read device name.
        Ok(Self {
            dump: file,
            fs,
            dev,
        })
    }

    fn read_file(&mut self) -> Result<PartData, PartError> {
        // Read name and block type.
        let name = Self::read_str(self.dump)?;
        let mut btype = 0;

        self.dump
            .read_exact(std::slice::from_mut(&mut btype))
            .map_err(PartError::Read)?;

        // Create block reader.
        let r = match btype {
            0 => Box::new(UncompressedFile {
                dump: Some(self.dump),
                buf: Vec::with_capacity(0xFFFF),
                off: 0,
            }),
            v => return Err(PartError::UnknownFileBlock(v)),
        };

        Ok(PartData::File(name, r))
    }

    fn read_str(file: &mut F) -> Result<Vec<u8>, PartError> {
        // Read length.
        let mut len = [0u8; 8];

        file.read_exact(&mut len).map_err(PartError::Read)?;

        // Read data.
        let len = usize::from_le_bytes(len);
        let mut data = Vec::new();

        file.by_ref()
            .take(len.try_into().unwrap())
            .read_to_end(&mut data)
            .map_err(PartError::Read)?;

        if data.len() != len {
            Err(PartError::Read(ErrorKind::UnexpectedEof.into()))
        } else {
            Ok(data)
        }
    }
}

/// Encapsulates data of a partition item.
pub enum PartData<'a> {
    Directory(Vec<u8>),
    File(Vec<u8>, Box<dyn Read + 'a>),
}

/// Provides [`Read`] implementation to read uncompressed file.
struct UncompressedFile<'a, F> {
    dump: Option<&'a mut F>,
    buf: Vec<u8>,
    off: usize,
}

impl<'a, F: Read> Read for UncompressedFile<'a, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        // Check if we need to read next block.
        if self.off == self.buf.len() {
            // Read block length.
            let mut len = [0u8; 2];
            let dump = match &mut self.dump {
                Some(v) => v,
                None => return Ok(0),
            };

            dump.read_exact(&mut len)?;

            // Check if EOF.
            let len = u16::from_le_bytes(len);

            if len == 0 {
                self.dump = None;
                return Ok(0);
            }

            // Read block.
            self.buf.clear();
            self.off = 0;

            if dump.take(len.into()).read_to_end(&mut self.buf)? != len.into() {
                return Err(ErrorKind::UnexpectedEof.into());
            }
        }

        // Copy data.
        let src = &self.buf[self.off..];
        let len = min(buf.len(), src.len());

        buf[..len].copy_from_slice(&src[..len]);
        self.off += len;

        Ok(len)
    }
}

/// Represents an error when [`PartReader`] fails to read partition dump.
#[derive(Debug, Error)]
pub enum PartError {
    #[error("unknown version {0}")]
    UnknownVersion(u8),

    #[error("couldn't read the specified file")]
    Read(#[source] std::io::Error),

    #[error("unknown item type {0}")]
    UnknownItem(u8),

    #[error("unknown file block type {0}")]
    UnknownFileBlock(u8),
}
