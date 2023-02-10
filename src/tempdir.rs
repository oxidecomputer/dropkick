use camino::{FromPathError, Utf8Path, Utf8PathBuf};
use std::io;
use std::path::Path;
use tempfile::TempDir;

#[derive(Debug)]
pub(crate) struct Utf8TempDir {
    utf8: Utf8PathBuf,
    _tempdir: TempDir,
}

impl Utf8TempDir {
    pub(crate) fn new() -> io::Result<Utf8TempDir> {
        TempDir::new()?
            .try_into()
            .map_err(FromPathError::into_io_error)
    }

    pub(crate) fn new_in(dir: impl AsRef<Path>) -> io::Result<Utf8TempDir> {
        TempDir::new_in(dir)?
            .try_into()
            .map_err(FromPathError::into_io_error)
    }
}

impl TryFrom<TempDir> for Utf8TempDir {
    type Error = FromPathError;

    fn try_from(tempdir: TempDir) -> Result<Utf8TempDir, FromPathError> {
        Ok(Utf8TempDir {
            utf8: <&Utf8Path>::try_from(tempdir.path())?.to_owned(),
            _tempdir: tempdir,
        })
    }
}

impl AsRef<Utf8Path> for Utf8TempDir {
    fn as_ref(&self) -> &Utf8Path {
        &self.utf8
    }
}
