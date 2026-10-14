//! A world file held in memory as bytes: a redb backend for a copy of a file (debug tools that
//! read or rewrite a browser world file) and for tests.

use std::sync::{Arc, Mutex};

use redb::StorageBackend;

/// The bytes of a world file; clones share them.
#[derive(Debug, Clone, Default)]
pub struct BytesFile(Arc<Mutex<Vec<u8>>>);

impl BytesFile {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Arc::new(Mutex::new(bytes)))
    }
    /// A copy of the file's bytes.
    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().expect("bytes").clone()
    }
}

impl StorageBackend for BytesFile {
    fn len(&self) -> Result<u64, std::io::Error> {
        Ok(self.0.lock().expect("bytes").len() as u64)
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), std::io::Error> {
        let b = self.0.lock().expect("bytes");
        let o = offset as usize;
        out.copy_from_slice(
            b.get(o..o + out.len())
                .ok_or(std::io::ErrorKind::UnexpectedEof)?,
        );
        Ok(())
    }
    fn set_len(&self, len: u64) -> Result<(), std::io::Error> {
        self.0.lock().expect("bytes").resize(len as usize, 0);
        Ok(())
    }
    fn sync_data(&self) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), std::io::Error> {
        let mut b = self.0.lock().expect("bytes");
        let o = offset as usize;
        if b.len() < o + data.len() {
            b.resize(o + data.len(), 0);
        }
        b[o..o + data.len()].copy_from_slice(data);
        Ok(())
    }
}
