use core::fmt::Debug;

/// Provides method to write a dump file.
#[cfg(feature = "write")]
pub trait DumpWrite: DumpFile {}

/// Provides method to read a dump file.
#[cfg(feature = "read")]
pub trait DumpRead: DumpFile {
    fn read(&mut self, buf: &mut [u8]) -> Result<(), Self::Err>;
}

#[cfg(all(feature = "read", feature = "std"))]
impl<T: std::io::Read> DumpRead for T {
    fn read(&mut self, buf: &mut [u8]) -> Result<(), Self::Err> {
        std::io::Read::read_exact(self, buf)
    }
}

/// Provides common methods to work on a dump file.
pub trait DumpFile {
    type Err: Debug + 'static;

    fn seek(&mut self, off: u64) -> Result<(), Self::Err>;
}

#[cfg(feature = "std")]
impl<T: std::io::Seek> DumpFile for T {
    type Err = std::io::Error;

    fn seek(&mut self, off: u64) -> Result<(), Self::Err> {
        let pos = std::io::Seek::seek(self, std::io::SeekFrom::Start(off))?;

        if pos != off {
            Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof))
        } else {
            Ok(())
        }
    }
}
