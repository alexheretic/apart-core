use crate::include::*;
use std::{
    io::ErrorKind,
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Compression {
    /// reported name, also used as file extension
    pub name: &'static str,
    pub command: &'static str,
    pub write_args: &'static [&'static str],
    pub read_args: &'static [&'static str],
}

// ~110 MB/s per core, compression 100->25
const PIGZ: Compression = Compression {
    name: "gz",
    command: "pigz",
    write_args: &["-1c"],
    read_args: &["-dc"],
};
// ~1250 MB/s single threaded, compression 100->30
const LZ4: Compression = Compression {
    name: "lz4",
    command: "lz4",
    write_args: &["-c"],
    read_args: &["-dc"],
};
// ~450 MB/s per core, compression 100->22
const ZSTD: Compression = Compression {
    name: "zst",
    command: "zstd",
    write_args: &["-T0", "-c"],
    // support up to --long=31 recompression
    read_args: &["-T0", "--long=31", "-dc"],
};
const NONE: Compression = Compression {
    name: "uncompressed",
    command: "cat",
    write_args: &["-"],
    read_args: &["-"],
};

const ALL: &[Compression] = &[PIGZ, NONE, ZSTD, LZ4];

impl Compression {
    pub fn from_name(name: &str) -> Result<Compression, String> {
        ALL.iter()
            .find(|z| z.name == name)
            .copied()
            .ok_or_else(|| format!("Unknown compression name `{}`", name))
    }

    pub fn from_file_name(file: &str) -> Result<Compression, String> {
        for z in ALL {
            if file.ends_with(&format!(".{}", z.name)) {
                return Ok(*z);
            }
        }
        if file.ends_with(".zstd") {
            // used as zstd name/extension in v0.2
            return Ok(ZSTD);
        }
        Err(format!("Unknown compression used in file `{}`", file))
    }

    pub fn all_installed() -> impl Iterator<Item = Compression> {
        ALL.iter().filter(|z| z.is_installed()).copied()
    }

    fn is_installed(self) -> bool {
        match Command::new(self.command)
            .arg("--version")
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
            }
        }
    }
}

impl Default for Compression {
    fn default() -> Self {
        PIGZ
    }
}

#[cfg(test)]
mod compression_tests {
    use super::*;

    #[test]
    fn from_gz_file_name() {
        let z = Compression::from_file_name("some-backup-2017-08-09G1106.apt.f2fs.gz");
        assert_eq!(z, Ok(PIGZ));
    }

    #[test]
    fn from_zst_file_name() {
        let z = Compression::from_file_name("some-backup-2017-08-09G1106.apt.f2fs.zst");
        assert_eq!(z, Ok(ZSTD));
        // also support old extension
        let old_z = Compression::from_file_name("some-backup-2017-08-09G1106.apt.f2fs.zstd");
        assert_eq!(old_z, Ok(ZSTD));
    }
}
