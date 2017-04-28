use chrono::prelude::*;
use std::{thread, str, fmt, env, fs};
use std::path::Path;
use std::process::{Command, Child, Stdio, ChildStderr};
use wait_timeout::ChildExt;
use std::time::Duration;
use chrono::Duration as OldDuration;
use std::io::{ErrorKind, BufReader, BufRead};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, SendError};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::fs::File;
use std::rc::Rc;
use regex::Regex;
use uuid::Uuid;
use lsblk;
use partclone;
use partclone::*;
use std::error::Error;
use std::cell::{RefCell, Cell};
use clone::partclone_variant_from_image;

#[derive(Debug)]
pub struct RestoreStatusCommon<'a> {
  pub id: &'a str,
  pub source: &'a str,
  pub destination: &'a str,
  pub start: DateTime<UTC>
}

#[derive(Debug)]
pub enum RestoreStatus<'a> {
  Running {
    common: RestoreStatusCommon<'a>,
    complete: f64,
    rate: Option<String>,
    estimated_finish: Option<DateTime<UTC>>
  },
  Finished { common: RestoreStatusCommon<'a>, finish: DateTime<UTC> },
  Failed { common: RestoreStatusCommon<'a>, reason: String, finish: DateTime<UTC> }
}

#[derive(Debug)]
pub struct RestoreJob {
  source: String,
  destination: String,
  id: String,
  partclone_cmd: Child,
  start: DateTime<UTC>,
  sent_first_msg: Cell<bool>,
  pub rx: Receiver<PartcloneStatus>
}

impl<'j> RestoreJob {
  pub fn try_recv(&'j self) -> Result<RestoreStatus<'j>, Box<Error>> {
    if !self.sent_first_msg.get() {
      // bosh out an initial running message to show the clone has started
      self.sent_first_msg.set(true);
      return Ok(RestoreStatus::Running {
        common: self.clone_status_common(),
        complete: 0.0,
        rate: None,
        estimated_finish: None
      })
    }

    Ok(match self.rx.try_recv()? {
      PartcloneStatus::Running { rate, estimated_finish, complete } => {
        warn!("{}, {}", rate, estimated_finish);
        RestoreStatus::Running {
          common: self.clone_status_common(),
          complete,
          rate: Some(rate),
          estimated_finish: Some(estimated_finish)
        }
      },
      PartcloneStatus::Synced { finish } => {
        RestoreStatus::Finished {
          common: self.clone_status_common(),
          finish
        }
      }
    })
  }

  pub fn clone_status_common(&'j self) -> RestoreStatusCommon<'j> {
    RestoreStatusCommon {
      id: &self.id,
      source: &self.source,
      destination: &self.destination,
      start: self.start
    }
  }

  pub fn id(&self) -> &str {
    &self.id
  }

  pub fn fail_status(&self, reason: &str) -> RestoreStatus {
    RestoreStatus::Failed {
      common: self.clone_status_common(),
      reason: reason.to_owned(),
      finish: UTC::now()
    }
  }

  pub fn new(source: String, destination: String) -> Result<RestoreJob, Box<Error>> {
    let partclone_cmd = partclone::cmd(&partclone_variant_from_image(&source)?)?;

    let mut cat = Command::new("cat").arg(&source)
      .stdout(Stdio::piped())
      .stdin(Stdio::null())
      .stderr(Stdio::null())
      .spawn()?;

    let mut pigz = Command::new("pigz").arg("-dc")
      .stdout(Stdio::piped())
      .stdin(unsafe { Stdio::from_raw_fd(cat.stdout.take().expect("!cat.stdout").into_raw_fd()) })
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
      .stdin(unsafe { Stdio::from_raw_fd(pigz.stdout.take().expect("!pigz.stdout").into_raw_fd()) })
      .stderr(Stdio::piped())
      .spawn()?
    };

    let stderr = partclone_cmd.stderr.take().expect("!partclone.stderr");
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
      .name(format!("partclone-stderr-reader {}->{}", source, destination))
      .spawn(move|| {
        if let Err(e) = partclone::read_output(stderr, tx) {
          error!("partclone::read_output failed: {}", e);
        }
      })?;

    let job = RestoreJob {
      source: source,
      destination: destination,
      partclone_cmd: partclone_cmd,
      rx: rx,
      start: UTC::now(),
      sent_first_msg: Cell::new(false),
      id: Uuid::new_v4().to_string()
    };

    Ok(job)
  }
}

impl fmt::Display for RestoreJob {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "RestoreJob({}->{})", self.source, self.destination)
  }
}
