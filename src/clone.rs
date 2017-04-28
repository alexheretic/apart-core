use chrono::prelude::*;
use std::{thread, str, fmt, fs};
use std::path::Path;
use regex::Regex;
use std::process::{Command, Child, Stdio};
use wait_timeout::ChildExt;
use std::time::Duration;
use std::io::{ErrorKind, Error as IoError, Result as IoResult};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::fs::File;
use std::cell::{RefCell, Cell};
use uuid::Uuid;
use std::error::Error;
use lsblk;
use partclone;
use partclone::*;

#[derive(Clone, PartialEq, Debug)]
pub struct CloneStatusCommon {
  pub id: String,
  pub source: String,
  pub destination: String,
  pub inprogress_destination: String,
  pub start: DateTime<UTC>
}

#[derive(PartialEq, Debug)]
pub enum CloneStatus {
  Running {
    common: CloneStatusCommon,
    complete: f64,
    rate: Option<String>,
    estimated_finish: Option<DateTime<UTC>>
  },
  Finished { common: CloneStatusCommon, rate: String, finish: DateTime<UTC>, image_size: u64 },
  Failed { common: CloneStatusCommon, reason: String, finish: DateTime<UTC> }
}

#[derive(Debug)]
pub struct CloneJob {
  source: String,
  destination: String,
  id: Uuid,
  partclone_cmd: Child,
  start: DateTime<UTC>,
  last_rate: RefCell<Option<String>>,
  sent_first_msg: Cell<bool>,
  pub rx: Receiver<PartcloneStatus>
}

fn destination_raw_fd(dir: &str, name: &str, partclone_variant: &str) -> IoResult<(String, RawFd)> {
  // something like: "/mnt/backups/mypart-2017-01-25T1245.apt.gz.inprogress"
  let file = format!("{directory}/{name}-{timestamp}.apt.{partclone_variant}.gz.inprogress",
    directory = dir, name = name, timestamp = Local::now().format("%Y-%m-%dT%H%M"),
    partclone_variant = partclone_variant);
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
        estimated_finish: None
      })
    }

    Ok(match self.rx.try_recv()? {
      PartcloneStatus::Running { rate, estimated_finish, complete } => {
        *self.last_rate.try_borrow_mut()? = Some(rate.clone());
        CloneStatus::Running {
          common: self.clone_status_common(),
          complete,
          rate: Some(rate),
          estimated_finish: Some(estimated_finish)
        }
      },
      PartcloneStatus::Synced { finish } => {
        let meta = fs::metadata(&self.destination)?;
        let last_rate = self.last_rate.borrow().clone();
        CloneStatus::Finished {
          common: self.clone_status_common(),
          rate: last_rate.unwrap_or("?".to_owned()),
          finish,
          image_size: meta.len()
        }
      }
    })
  }

  pub fn clone_status_common(&self) -> CloneStatusCommon {
    CloneStatusCommon {
      id: self.id(),
      source: self.source.clone(),
      destination: self.successful_destination().to_owned(),
      inprogress_destination: self.destination.clone(),
      start: self.start
    }
  }

  pub fn id(&self) -> String {
    format!("{}", self.id)
  }

  fn rm_inprogress_file(&self) {
    if let Err(err) = fs::remove_file(&self.destination) {
      error!("Could not rm inprogress clone: {}", err);
    }
  }

  pub fn successful_destination(&self) -> &str {
    let (without_inprogress, _) = self.destination.split_at(self.destination.len() - ".inprogress".len());
    without_inprogress
  }

  pub fn fail_status(&self, reason: &str) -> CloneStatus {
    CloneStatus::Failed {
      common: self.clone_status_common(),
      reason: reason.to_owned(),
      finish: UTC::now()
    }
  }

  pub fn new(source: String, destination: String, name: String) -> IoResult<CloneJob> {
    let (partclone_variant, partclone_cmd) = match lsblk::fstype(&source) {
      Some(fstype) => match partclone::cmd(&fstype) {
        Ok(cmd) => (fstype, cmd),
        Err(_) => {
          warn!("No partclone command found for fstype '{}', using dd...", fstype);
          ("dd".to_owned(), partclone::cmd("dd")?)
        }
      },
      _ => {
        info!("fstype not found for source '{}', using dd...", source);
        ("dd".to_owned(), partclone::cmd("dd")?)
      }
    };
    let (dest_file, dest_raw_fd) = destination_raw_fd(&destination, &name, &partclone_variant)?;

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

    Command::new("pigz").arg("-1c")
      .stdin(unsafe { Stdio::from_raw_fd(partclone_cmd.stdout.take().unwrap().into_raw_fd()) })
      .stdout(unsafe { Stdio::from_raw_fd(dest_raw_fd) })
      .spawn()?;

    let stderr = partclone_cmd.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
      .name(format!("partclone-stderr-reader {}->{}", source, dest_file))
      .spawn(move|| {
        if let Err(e) = partclone::read_output(stderr, tx) {
          error!("partclone::read_output failed: {}", e);
        }
      })?;

    let job = CloneJob {
      source: source,
      destination: dest_file,
      partclone_cmd: partclone_cmd,
      rx: rx,
      start: UTC::now(),
      id: Uuid::new_v4(),
      last_rate: RefCell::new(None),
      sent_first_msg: Cell::new(false)
    };

    Ok(job)
  }
}

impl Drop for CloneJob {
  fn drop(&mut self) {
    match self.partclone_cmd.wait_timeout(Duration::from_secs(0)) {
      Ok(None) => {
        if let Err(x) = self.partclone_cmd.kill() {
          error!("Failed to kill CloneJob#cmd: {}", x);
        }
        self.rm_inprogress_file();
      },
      Ok(Some(status)) => {
        if status.success() {
          if let Err(err) = fs::rename(&self.destination, self.successful_destination()) {
            error!("Failed to rename {}: {}", self.destination, err);
          }
        }
        else {
          warn!("CloneJob finished with != 0 exit");
          self.rm_inprogress_file();
        }
      },
      Err(x) => {
        error!("Failed to drop CloneJob: {}", x);
        self.rm_inprogress_file();
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
  let image_re = Regex::new(r"^.*/?[^/]+-\d{4,}-\d\d-\d\dT\d{4}\.apt\.(.+)\..+$")
    .expect("!image_re");

  for caps in image_re.captures_iter(filename) {
    return Ok(caps[1].parse::<String>()?);
  }
  Err(Box::new(OutputInvalidError(format!("Invalid image file: {}", filename))))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn local_ext2_variant_from_image() {
    assert_eq!(partclone_variant_from_image("mockimg-2017-04-20T1500.apt.ext2.gz").unwrap(),
      "ext2".to_owned());
  }

  #[test]
  fn dd_variant_from_image() {
    assert_eq!(partclone_variant_from_image("/mnt/backups/mockimg-2017-04-20T1500.apt.dd.gz").unwrap(),
      "dd".to_owned());
  }
}
