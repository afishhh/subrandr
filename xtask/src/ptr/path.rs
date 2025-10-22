use std::{
    borrow::Borrow,
    ffi::OsStr,
    fmt::Display,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::Result;

use super::PtrInfo;

#[repr(transparent)]
#[derive(Debug)]
pub struct PtrPath(Path);

impl PtrPath {
    fn from_path_unchecked(path: &Path) -> &Self {
        unsafe { std::mem::transmute::<&Path, &Self>(path) }
    }

    pub fn new(path: &Path) -> Option<&Self> {
        if path.as_os_str().as_encoded_bytes().ends_with(b".ptr") {
            Some(Self::from_path_unchecked(path))
        } else {
            None
        }
    }

    pub fn read(&self) -> Result<PtrInfo> {
        PtrInfo::read(self)
    }

    pub fn write(&self, info: PtrInfo) -> std::io::Result<()> {
        info.write(&std::fs::File::create(self.path())?)
    }

    pub fn data_path(&self) -> &Path {
        Path::new(unsafe {
            OsStr::from_encoded_bytes_unchecked(
                self.0
                    .as_os_str()
                    .as_encoded_bytes()
                    .strip_suffix(b".ptr")
                    .expect("PtrPath must end in .ptr"),
            )
        })
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn display(&self) -> impl Display {
        self.0.display()
    }
}

impl ToOwned for PtrPath {
    type Owned = PtrPathBuf;

    fn to_owned(&self) -> Self::Owned {
        PtrPathBuf(self.0.to_owned())
    }
}

#[repr(transparent)]
#[derive(Debug, Clone)]
pub struct PtrPathBuf(PathBuf);

impl PtrPathBuf {
    pub fn from_data_path(path: &Path) -> Option<Self> {
        path.file_name().map(|_| {
            let mut string = path.to_owned().into_os_string();
            string.push(".ptr");
            Self(PathBuf::from(string))
        })
    }
}

impl FromStr for PtrPathBuf {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        PtrPath::new(Path::new(s))
            .ok_or(r#"ptr path must end in ".ptr""#)
            .map(PtrPath::to_owned)
    }
}

impl Borrow<PtrPath> for PtrPathBuf {
    fn borrow(&self) -> &PtrPath {
        PtrPath::from_path_unchecked(&self.0)
    }
}

impl Deref for PtrPathBuf {
    type Target = PtrPath;

    fn deref(&self) -> &Self::Target {
        self.borrow()
    }
}
