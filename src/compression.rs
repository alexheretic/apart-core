use std::process::{Command, Stdio};
use std::io::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Compression {
    /// reported name, also used as file extension
    pub name: &'static str,
    pub command: &'static str,
    pub write_args: &'static str,
    pub read_args: &'static str,
}

const PIGZ: Compression = Compression {
    name: "gz",
    command: "pigz",
    write_args: "-1c",
    read_args: "-dc",
};
const LZ4: Compression = Compression {
    name: "lz4",
    command: "lz4",
    write_args: "-c",
    read_args: "-dc",
};
const ZSTD: Compression = Compression {
    name: "zstd",
    command: "zstdmt",
    write_args: "-c",
    read_args: "-dc",
};
const NONE: Compression = Compression {
    name: "uncompressed",
    command: "cat",
    write_args: "-",
    read_args: "-",
};

const ALL: &[Compression] = &[PIGZ, NONE, ZSTD, LZ4];

impl Compression {
    pub fn from_name(name: &str) -> Result<Compression, String> {
        for z in ALL {
            if z.name == name {
                return Ok(*z);
            }
        }
        Err(format!("Unknown compression name `{}`", name))
    }

    pub fn from_file_name(file: &str) -> Result<Compression, String> {
        for z in ALL {
            if file.ends_with(&format!(".{}", z.name)) {
                return Ok(*z);
            }
        }
        Err(format!("Unknown compression used in file `{}`", file))
    }

    pub fn all_installed() -> Vec<Compression> {
        let mut available = vec![];
        for z in ALL {
            if z.is_installed() {
                available.push(*z);
            }
        }
        available
    }

    fn is_installed(self) -> bool {
        match Command::new(self.command).arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(_) => true,
            Err(e) => {
                if e.kind() != ErrorKind::NotFound {
                    warn!("Error checking if `{}` is installed: {}", self.command, e);
                }
                false
            },
        }
    }
}

impl Default for Compression {
    fn default() -> Self { PIGZ }
}

#[cfg(test)]
mod compression_tests {
    use super::*;

    #[test]
    fn from_gz_file_name() {
        let z = Compression::from_file_name("some-backup-2017-08-09G1106.apt.f2fs.gz");
        assert_eq!(z, Ok(PIGZ));
    }
}
