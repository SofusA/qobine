use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;

use stream_download::storage::StorageProvider;

pub struct PassthroughStorageProvider {
    pub partial_path: PathBuf,
}

impl StorageProvider for PassthroughStorageProvider {
    type Reader = File;
    type Writer = File;

    fn into_reader_writer(self, _content_length: Option<u64>) -> io::Result<(File, File)> {
        if let Some(parent) = self.partial_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let writer = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.partial_path)?;
        // Independent fd with its own position. `try_clone` shares position
        // with the writer, which would corrupt streamed reads.
        let reader = OpenOptions::new().read(true).open(&self.partial_path)?;
        Ok((reader, writer))
    }
}
