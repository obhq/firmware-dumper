#![no_std]

mod io;

pub use self::io::*;

#[cfg(feature = "read")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

/// Encapsulates a firmware dump.
pub struct FirmwareDump<F> {
    file: F,
}

impl<F> FirmwareDump<F> {
    pub const MAGIC: &'static [u8; 4] = b"\x7FOBF";
    pub const ITEM_END: u8 = 0;
    pub const ITEM_PARTITION: u8 = 1;

    pub fn new(file: F) -> Self {
        Self { file }
    }
}

#[cfg(feature = "read")]
impl<F: DumpRead> FirmwareDump<F> {
    pub fn read(&mut self) -> Result<FirmwareItems<F>, ItemError<F::Err>> {
        Ok(FirmwareItems {
            file: &mut self.file,
        })
    }
}

/// Iterator to numerate items in a firmware dump.
///
/// This type does not implement [`Iterator`] due to its [`Iterator::Item`] has incompatible
/// lifetime.
#[cfg(feature = "read")]
pub struct FirmwareItems<'a, F> {
    file: &'a mut F,
}

#[cfg(feature = "read")]
impl<'a, F: DumpRead> FirmwareItems<'a, F> {
    #[allow(clippy::should_implement_trait)] // We want to be able to drop-in implement Iterator.
    pub fn next(&mut self) -> Option<Result<FirmwareItem<F>, ItemError<F::Err>>> {
        // Read item type.
        let mut ty = 0;

        if let Err(e) = self.file.read(core::slice::from_mut(&mut ty)) {
            return Some(Err(ItemError::ReadFailed(e)));
        }

        if ty == FirmwareDump::<F>::ITEM_END {
            return None;
        }

        // Read item version.
        let mut ver = 0;

        if let Err(e) = self.file.read(core::slice::from_mut(&mut ver)) {
            return Some(Err(ItemError::ReadFailed(e)));
        }

        // Read item data.
        Some(match (ty, ver) {
            (FirmwareDump::<F>::ITEM_PARTITION, 0) => self
                .read_partition_v0()
                .map(|v| FirmwareItem::Partition(v.0, v.1)),
            (FirmwareDump::<F>::ITEM_PARTITION, _) => Err(ItemError::UnknownVersion(ty, ver)),
            _ => Err(ItemError::UnknownItem(ty)),
        })
    }

    fn read_partition_v0(
        &mut self,
    ) -> Result<(alloc::string::String, ItemReader<F>), ItemError<F::Err>> {
        // Read name length.
        let mut len = 0;

        self.file
            .read(core::slice::from_mut(&mut len))
            .map_err(ItemError::ReadFailed)?;

        // Read name.
        let mut name = alloc::vec![0; len.into()];

        self.file.read(&mut name).map_err(ItemError::ReadFailed)?;

        // Check if name valid.
        let name = alloc::string::String::from_utf8(name).map_err(|_| ItemError::FileCorrupted)?;

        // Read data length.
        let mut len = [0; 8];
        let len = match self.file.read(&mut len) {
            Ok(_) => u64::from_le_bytes(len),
            Err(e) => return Err(ItemError::ReadFailed(e)),
        };

        Ok((name, ItemReader::new(self.file, len)))
    }
}

/// Encapsulates an item in a firmware dump.
#[cfg(feature = "read")]
pub enum FirmwareItem<'a, F: DumpRead> {
    Partition(alloc::string::String, ItemReader<'a, F>),
}

/// Struct to read a raw item in a firmware dump.
///
/// [`Drop`] implementation on this struct may panic if there are some unread data.
#[cfg(feature = "read")]
pub struct ItemReader<'a, F: DumpRead> {
    file: &'a mut F,
    pos: u64,
    len: u64,
}

#[cfg(feature = "read")]
impl<'a, F: DumpRead> ItemReader<'a, F> {
    fn new(file: &'a mut F, len: u64) -> Self {
        Self { file, pos: 0, len }
    }
}

#[cfg(feature = "read")]
impl<'a, F: DumpRead> Drop for ItemReader<'a, F> {
    fn drop(&mut self) {
        if self.pos != self.len {
            self.file.seek(self.pos).unwrap();
        }
    }
}

/// Represents an error when [`FirmwareDump::read()`] or [`FirmwareItems`] fails to enumerate a next
/// item.
#[cfg(feature = "read")]
#[derive(Debug)]
pub enum ItemError<F> {
    FileCorrupted,
    ReadFailed(F),
    UnknownItem(u8),
    UnknownVersion(u8, u8),
}

#[cfg(all(feature = "read", feature = "std"))]
impl<F: std::error::Error + 'static> std::error::Error for ItemError<F> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadFailed(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(feature = "read")]
impl<F> core::fmt::Display for ItemError<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FileCorrupted => f.write_str("file corrupted"),
            Self::ReadFailed(_) => f.write_str("couldn't read the file"),
            Self::UnknownItem(ty) => write!(f, "unknown item type {ty}"),
            Self::UnknownVersion(ty, ver) => write!(f, "unknown version {ver} on item type {ty}"),
        }
    }
}
