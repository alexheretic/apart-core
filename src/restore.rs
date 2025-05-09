use crate::{
    child, clone::partclone_variant_from_image, compression::Compression, include::*, partclone,
    partclone::*,
};
use chrono::prelude::*;
use std::{
    cell::Cell,
    error::Error,
    fmt,
    os::unix::io::{FromRawFd, IntoRawFd},
    process::{Child, Command, Stdio},
    str,
    sync::{mpsc, mpsc::Receiver},
    thread,
};
use uuid::Uuid;

#[derive(Debug)]
pub struct RestoreStatusCommon<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub destination: &'a str,
    pub start: DateTime<Utc>,
}

#[derive(Debug)]
pub enum RestoreStatus<'a> {
    Running {
        common: RestoreStatusCommon<'a>,
        complete: f64,
        syncing: bool,
        rate: Option<String>,
        estimated_finish: Option<DateTime<Utc>>,
    },
    Finished {
        common: RestoreStatusCommon<'a>,
        finish: DateTime<Utc>,
    },
    Failed {
        common: RestoreStatusCommon<'a>,
        reason: String,
        finish: DateTime<Utc>,
    },
}

#[derive(Debug)]
pub struct RestoreJob {
    source: String,
    destination: String,
    id: String,
    cat_cmd: Child,
    compress_cmd: Child,
    partclone_cmd: Child,
    start: DateTime<Utc>,
    sent_first_msg: Cell<bool>,
    partclone_status: Receiver<PartcloneStatus>,
}

impl<'j> RestoreJob {
    pub fn try_recv(&'j self) -> Result<RestoreStatus<'j>, Box<dyn Error>> {
        if !self.sent_first_msg.get() {
            // bosh out an initial running message to show the clone has started
            self.sent_first_msg.set(true);
            return Ok(RestoreStatus::Running {
                common: self.clone_status_common(),
                complete: 0.0,
                syncing: false,
                rate: None,
                estimated_finish: None,
            });
        }

        Ok(match self.partclone_status.try_recv()? {
            PartcloneStatus::Running {
                rate,
                estimated_finish,
                complete,
            } => RestoreStatus::Running {
                common: self.clone_status_common(),
                complete: if complete > 0.9999 { 0.9999 } else { complete },
                syncing: complete > 0.9999,
                rate: Some(rate),
                estimated_finish: Some(estimated_finish),
            },
            PartcloneStatus::Synced { finish } => RestoreStatus::Finished {
                common: self.clone_status_common(),
                finish,
            },
            PartcloneStatus::Failed { finish } => RestoreStatus::Failed {
                common: self.clone_status_common(),
                finish,
                reason: "Failed".to_owned(),
            },
        })
    }

    pub fn clone_status_common(&'j self) -> RestoreStatusCommon<'j> {
        RestoreStatusCommon {
            id: &self.id,
            source: &self.source,
            destination: &self.destination,
            start: self.start,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn fail_status(&self, reason: &str) -> RestoreStatus<'_> {
        RestoreStatus::Failed {
            common: self.clone_status_common(),
            reason: reason.to_owned(),
            finish: Utc::now(),
        }
    }

    pub fn new(source: String, destination: String) -> Result<RestoreJob, Box<dyn Error>> {
        let partclone_cmd = partclone::cmd(&partclone_variant_from_image(&source)?)?;

        let z = Compression::from_file_name(&source)?;

        let mut cat = Command::new("cat")
            .arg(&source)
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        let mut z_process = Command::new(z.command)
            .args(z.read_args)
            .stdout(Stdio::piped())
            .stdin(unsafe {
                Stdio::from_raw_fd(cat.stdout.take().expect("!cat.stdout").into_raw_fd())
            })
            .stderr(Stdio::null())
            .spawn()?;

        let mut partclone_cmd = {
            let mut args = Vec::new();
            if !partclone_cmd.ends_with("dd") {
                args.push("-r");
            }
            args.push("-o");
            args.push(&destination);

            Command::new(partclone_cmd)
                .args(&args)
                .stdout(Stdio::null())
                .stdin(unsafe {
                    Stdio::from_raw_fd(z_process.stdout.take().expect("!z.stdout").into_raw_fd())
                })
                .stderr(Stdio::piped())
                .spawn()?
        };

        let stderr = partclone_cmd.stderr.take().expect("!partclone.stderr");
        let (tx, partclone_status) = mpsc::channel();
        thread::Builder::new()
            .name(format!(
                "partclone-stderr-reader {}->{}",
                source, destination
            ))
            .spawn(move || {
                if let Err(e) = partclone::read_output(stderr, &tx) {
                    error!("partclone::read_output failed: {}", e);
                }
            })?;

        let job = RestoreJob {
            source,
            destination,
            cat_cmd: cat,
            compress_cmd: z_process,
            partclone_cmd,
            partclone_status,
            start: Utc::now(),
            sent_first_msg: Cell::new(false),
            id: Uuid::new_v4().to_string(),
        };

        Ok(job)
    }
}

impl fmt::Display for RestoreJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RestoreJob({}->{})", self.source, self.destination)
    }
}

impl Drop for RestoreJob {
    fn drop(&mut self) {
        child::drop_log_errors(&mut self.cat_cmd, "RestoreJob#cat_cmd");
        child::drop_log_errors(&mut self.compress_cmd, "RestoreJob#compress_cmd");
        child::drop_log_errors(&mut self.partclone_cmd, "RestoreJob#partclone_cmd");
    }
}
