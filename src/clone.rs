use child;
use chrono::prelude::*;
use compression::Compression;
use lsblk;
use partclone;
use partclone::*;
use regex::Regex;
use std::{fmt, fs, str, thread};
use std::cell::{RefCell, Cell};
use std::error::Error;
use std::fs::{File, Metadata};
use std::io::{Error as IoError, ErrorKind, Result as IoResult};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use uuid::Uuid;
use async;

#[derive(Clone, PartialEq, Debug)]
pub struct CloneStatusCommon {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub inprogress_destination: String,
    pub start: DateTime<Utc>,
}

#[derive(PartialEq, Debug)]
pub enum CloneStatus {
    Running {
        common: CloneStatusCommon,
        complete: f64,
        rate: Option<String>,
        estimated_finish: Option<DateTime<Utc>>,
    },
    Syncing {
        common: CloneStatusCommon,
    },
    Finished {
        common: CloneStatusCommon,
        finish: DateTime<Utc>,
        image_size: u64,
    },
    Failed {
        common: CloneStatusCommon,
        reason: String,
        finish: DateTime<Utc>,
    },
}

#[derive(Debug)]
pub struct CloneJob {
    source: String,
    destination: String,
    id: Uuid,
    start: DateTime<Utc>,
    partclone_cmd: RefCell<Child>,
    compress_cmd: RefCell<Child>,
    sent_first_msg: Cell<bool>,
    partclone_status: Receiver<PartcloneStatus>,
    partclone_finished: Cell<bool>,
    rename_task: RefCell<Option<Receiver<IoResult<Metadata>>>>,
}

fn destination_raw_fd(dir: &str, name: &str, partclone_variant: &str, z: Compression)
    -> IoResult<(String, RawFd)>
{
    // something like: "/mnt/backups/mypart-2017-01-25T1245.apt.gz.inprogress"
    let file = format!(
        "{directory}/{name}-{timestamp}.apt.{partclone_variant}.{z_name}.inprogress",
        directory = dir,
        name = name,
        timestamp = Local::now().format("%Y-%m-%dT%H%M"),
        partclone_variant = partclone_variant,
        z_name = z.name
    );
    let path = Path::new(&file);
    if path.exists() {
        return Err(IoError::new(ErrorKind::AlreadyExists, format!("{} already exists", file)));
    }
    Ok((file.to_owned(), File::create(path)?.into_raw_fd()))
}

impl CloneJob {
    pub fn try_recv(&self) -> Result<CloneStatus, Box<Error>> {

        if !self.sent_first_msg.get() {
            // bosh out an initial running message to show the clone has started
            self.sent_first_msg.set(true);
            return Ok(CloneStatus::Running {
                common: self.clone_status_common(),
                complete: 0.0,
                rate: None,
                estimated_finish: None,
            });
        }

        if self.partclone_finished.get() {
            return match self.try_wait() {
                Ok(Some(_)) => {
                    if self.rename_task.borrow().is_none() {
                        let from = self.destination.clone();
                        let to = self.successful_destination().to_owned();
                        *self.rename_task.borrow_mut() = Some(async::receiver(move|| {
                            fs::rename(&from, &to)?;
                            fs::metadata(&to)
                        }));
                    }
                    match self.rename_task.borrow_mut().as_ref().unwrap().try_recv()? {
                        Ok(meta) => {
                            Ok(CloneStatus::Finished {
                                common: self.clone_status_common(),
                                finish: Utc::now(),
                                image_size: meta.len(),
                            })
                        },
                        Err(err) => {
                            error!("Failed to rename {}: {}", self.destination, err);
                            Ok(CloneStatus::Failed {
                                common: self.clone_status_common(),
                                finish: Utc::now(),
                                reason: format!("Failed to rename {}", self.destination)
                            })
                        }
                    }
                },
                Ok(None) => Err("Waiting for commands to finish".into()),
                Err(err) => {
                    error!("Clone failed: {:?}", err);
                    Ok(CloneStatus::Failed {
                        common: self.clone_status_common(),
                        finish: Utc::now(),
                        reason: "Failed".to_owned(),
                    })
                }
            };
        }

        Ok(match self.partclone_status.try_recv()? {
            PartcloneStatus::Running { rate, estimated_finish, complete, } => {
                CloneStatus::Running {
                    common: self.clone_status_common(),
                    complete: if complete > 0.9999 { 0.9999 } else { complete },
                    rate: Some(rate),
                    estimated_finish: Some(estimated_finish),
                }
            },
            PartcloneStatus::Synced { .. } => {
                self.partclone_finished.set(true);
                CloneStatus::Syncing { common: self.clone_status_common() }
            },
            PartcloneStatus::Failed { finish } => {
                CloneStatus::Failed {
                    common: self.clone_status_common(),
                    finish,
                    reason: "Failed".to_owned(),
                }
            },
        })
    }

    /// Returns `Ok(Some(()))` when both partclone & compress commands have exitted successfully
    fn try_wait(&self) -> (Result<Option<()>, Box<Error>>) {
        let pcl = match self.partclone_cmd.borrow_mut().try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    Some(())
                }
                else {
                    return Err("Clone failed".into());
                }
            },
            Ok(None) => None,
            Err(_) => return Err("Clone failed".into()),
        };
        let cmp = match self.compress_cmd.borrow_mut().try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    Some(())
                }
                else {
                    return Err("Compress failed".into());
                }
            },
            Ok(None) => None,
            Err(_) => return Err("Compress failed".into()),
        };

        Ok(match (pcl, cmp) {
            (Some(_), Some(_)) => Some(()),
            _ => None,
        })
    }

    pub fn clone_status_common(&self) -> CloneStatusCommon {
        CloneStatusCommon {
            id: self.id(),
            source: self.source.clone(),
            destination: self.successful_destination().to_owned(),
            inprogress_destination: self.destination.clone(),
            start: self.start,
        }
    }

    pub fn id(&self) -> String {
        format!("{}", self.id)
    }

    pub fn successful_destination(&self) -> &str {
        let (without_inprogress, _) = self.destination.split_at(self.destination.len() - ".inprogress".len());
        without_inprogress
    }

    pub fn fail_status(&self, reason: &str) -> CloneStatus {
        CloneStatus::Failed {
            common: self.clone_status_common(),
            reason: reason.to_owned(),
            finish: Utc::now(),
        }
    }

    pub fn new(source: String, destination: String, name: String, z: Compression) -> IoResult<CloneJob> {
        let (partclone_variant, partclone_cmd) = match lsblk::fstype(&source) {
            Some(fstype) => {
                match partclone::cmd(&fstype) {
                    Ok(cmd) => (fstype, cmd),
                    Err(_) => {
                        info!("No partclone command found for fstype '{}', using dd...", fstype);
                        ("dd".to_owned(), partclone::cmd("dd")?)
                    },
                }
            },
            _ => {
                info!("fstype not found for source '{}', using dd...", source);
                ("dd".to_owned(), partclone::cmd("dd")?)
            },
        };
        let (dest_file, dest_raw_fd) = destination_raw_fd(&destination, &name, &partclone_variant, z)?;

        let mut partclone_cmd = {
            let mut args = Vec::new();
            if partclone_variant != "dd" {
                args.push("-c");
            }
            args.push("-s");
            args.push(&source);

            Command::new(partclone_cmd)
                .args(&args)
                .stdout(Stdio::piped())
                .stdin(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?
        };

        let compress_cmd = Command::new(z.command)
            .arg(z.write_args)
            .stdin(unsafe {
                Stdio::from_raw_fd(partclone_cmd.stdout.take().unwrap().into_raw_fd())
            })
            .stdout(unsafe { Stdio::from_raw_fd(dest_raw_fd) })
            .stderr(Stdio::null())
            .spawn()?;

        let stderr = partclone_cmd.stderr.take().unwrap();
        let (tx, partclone_status) = mpsc::channel();
        thread::Builder::new()
            .name(format!("partclone-stderr-reader {}->{}", source, dest_file))
            .spawn(move || if let Err(e) = partclone::read_output(stderr, &tx) {
                warn!("partclone::read_output failed: {}", e);
            })?;

        Ok(CloneJob {
            source,
            destination: dest_file,
            start: Utc::now(),
            partclone_cmd: RefCell::new(partclone_cmd),
            compress_cmd: RefCell::new(compress_cmd),
            partclone_status,
            id: Uuid::new_v4(),
            sent_first_msg: Cell::new(false),
            partclone_finished: Cell::new(false),
            rename_task: RefCell::new(None),
        })
    }
}

impl Drop for CloneJob {
    fn drop(&mut self) {
        child::drop_log_errors(&mut self.partclone_cmd.borrow_mut(), "CloneJob#partclone_cmd");
        child::drop_log_errors(&mut self.compress_cmd.borrow_mut(), "CloneJob#compress_cmd");

        let inprogress_file = Path::new(&self.destination);
        if inprogress_file.exists() {
            if let Err(err) = fs::remove_file(inprogress_file) {
                error!("Could not rm inprogress clone: {}", err);
            }
        }
    }
}

impl fmt::Display for CloneJob {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CloneJob({}->{})", self.source, self.successful_destination())
    }
}

pub fn partclone_variant_from_image(filename: &str) -> Result<String, Box<Error>> {
    let image_re = Regex::new(r"^.*/?[^/]+-\d{4,}-\d\d-\d\dT\d{4}\.apt\.(.+)\..+$").expect("!image_re");

    for caps in image_re.captures_iter(filename) {
        return Ok(caps[1].parse::<String>()?);
    }
    Err(Box::new(OutputInvalidError(format!("Invalid image file: {}", filename))))
}

pub fn is_valid_image_name(filename: &str) -> bool {
    partclone_variant_from_image(filename).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ext2_variant_from_image() {
        assert_eq!(
            partclone_variant_from_image("mockimg-2017-04-20T1500.apt.ext2.gz").unwrap(),
            "ext2".to_owned()
        );
    }

    #[test]
    fn dd_variant_from_image() {
        assert_eq!(
            partclone_variant_from_image("/mnt/backups/mockimg-2017-04-20T1500.apt.dd.gz")
                .unwrap(),
            "dd".to_owned()
        );
    }

    #[test]
    fn image_valid() {
        assert_eq!(is_valid_image_name("/mnt/backups/mockimg-2017-04-20T1500.apt.dd.gz"), true);
    }

    #[test]
    fn image_invalid() {
        assert_eq!(is_valid_image_name("/mnt/backups/mockimg-2017-04-20T1500.gz"), false);
    }
}
