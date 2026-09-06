//! 文件字节读写。业务序列化不放在这里。

use std::fs;
use std::io;
use std::path::Path;

pub fn read_bytes(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    fs::read(path)
}

pub fn write_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.as_ref().parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

pub fn write_bytes_atomic(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    let tmp = path.with_extension("tmp");
    write_bytes(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}
