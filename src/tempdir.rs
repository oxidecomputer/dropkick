// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use camino::{FromPathError, Utf8Path, Utf8PathBuf};
use std::io;
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

    pub(crate) fn path(&self) -> &Utf8Path {
        &self.utf8
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
