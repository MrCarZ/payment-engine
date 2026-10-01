//! Filesystem operations for run artifacts, without payment processing policy.
use std::{
    fs::{File, OpenOptions, create_dir, create_dir_all, read_dir, rename},
    io::{BufWriter, Result as IoResult, Write},
    path::{Path, PathBuf},
};

pub fn create_run_directory(root: &Path, run_id: &str) -> IoResult<PathBuf> {
    let directory = root.join(run_id);
    create_dir_all(root)?;
    create_dir(&directory)?;
    Ok(directory)
}
pub fn create_file(path: &Path) -> IoResult<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}
pub fn publish(partial: &Path, completed: &Path) -> IoResult<()> {
    rename(partial, completed)
}
pub fn trace_files(directory: &Path) -> IoResult<Vec<PathBuf>> {
    let mut paths = read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<IoResult<Vec<_>>>()?;
    paths.retain(|path| path.to_string_lossy().ends_with(".trace.csv"));
    paths.sort();
    Ok(paths)
}
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub struct Tee<W> {
    file: BufWriter<File>,
    output: W,
}
impl<W> Tee<W> {
    pub fn new(path: &Path, output: W) -> IoResult<Self> {
        Ok(Self {
            file: BufWriter::new(create_file(path)?),
            output,
        })
    }
}
impl<W: Write> Write for Tee<W> {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        self.file.write_all(bytes)?;
        self.output.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> IoResult<()> {
        self.file.flush()?;
        self.output.flush()
    }
}
