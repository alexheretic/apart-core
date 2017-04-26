use chrono::prelude::*;
use std::{thread, str, fmt, env, fs};
use std::path::Path;
use std::process::{Command, Child, Stdio, ChildStderr};
use wait_timeout::ChildExt;
use std::time::Duration;
use chrono::Duration as OldDuration;
use std::io::{ErrorKind, Error, BufReader, Result, BufRead};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::fs::File;
use regex::Regex;
use uuid::Uuid;

#[derive(Clone, PartialEq, Debug)]
pub struct JobStatusCommon {
  pub id: String,
  pub source: String,
  pub destination: String,
  pub start: DateTime<UTC>
}

#[derive(PartialEq, Debug)]
pub enum JobStatus {
  Running {
    common: JobStatusCommon,
    complete: f64,
    rate: Option<String>,
    estimated_finish: Option<DateTime<UTC>>
  },
  Finished { common: JobStatusCommon, rate: String, finish: DateTime<UTC> },
  Failed { common: JobStatusCommon, reason: String, finish: DateTime<UTC> }
}

#[derive(Debug)]
pub struct CloneJob {
  source: String,
  destination: String,
  id: Uuid,
  partclone_cmd: Child,
  start: DateTime<UTC>,
  pub rx: Receiver<JobStatus>
}

fn check_exists(command: &str) -> Result<()> {
  let cmd_path = Path::new(command);
  if !cmd_path.exists() {
    return Err(Error::new(ErrorKind::NotFound, format!("{} not found", command)));
  }
  Ok(())
}

fn partclone_cmd(variant: &str) -> Result<String> {
  let partclone_cmd = env::var("PARTCLONE_CMD").unwrap_or("partclone".to_owned());
  let partclone_dd = format!("{}.{}", partclone_cmd, variant);
  check_exists(&partclone_dd)?;
  Ok(partclone_dd.to_string())
}

fn destination_raw_fd(dir: &str, name: &str, partclone_variant: &str) -> Result<(String, RawFd)> {
  // something like: "/mnt/backups/mypart-2017-01-25T1245.apt.gz.inprogress"
  let file = format!("{directory}/{name}-{timestamp}.apt.{partclone_variant}.gz.inprogress",
    directory = dir, name = name, timestamp = Local::now().format("%Y-%m-%dT%H%M"),
    partclone_variant = partclone_variant);
  let path = Path::new(&file);
  if path.exists() {
    return Err(Error::new(ErrorKind::AlreadyExists, format!("{} already exists", file)));
  }
  Ok((file.to_owned(), File::create(path)?.into_raw_fd()))
}

fn read_partclone_output(stderr: ChildStderr, tx: Sender<JobStatus>, info: JobStatusCommon) {
  let progress_re = Regex::new(
    r"Remaining:\s*(\d{2,}:\d{2}:\d{2}), Completed:\s*(\d{1,3}\.?\d?\d?)%,\s*([^,]+)").unwrap();

  let (mut started_main_output, mut synced) = (false, false);
  let mut last_rate = None;

  // send initial status update
  if let Err(_) = tx.send(JobStatus::Running {
      common: info.clone(),
      rate: None,
      estimated_finish: None,
      complete: 0.0 }) {
    warn!("tx.send failed, finishing");
    return;
  }

  let duration_re = Regex::new(r"^(\d{2,}):(\d{2}):(\d{2})$").unwrap();

  for line in BufReader::new(stderr).lines() {
    match line {
      Ok(out) => {
        if started_main_output {
          if !synced {
            for cap in progress_re.captures_iter(&out) {
              let mut estimated_finish = None;
              for cap in duration_re.captures_iter(&cap[1]) {
                if let (Ok(hours), Ok(minutes), Ok(seconds)) =
                    (cap[1].parse::<i64>(), cap[2].parse::<i64>(), cap[3].parse::<i64>()) {
                  let remaining = OldDuration::hours(hours) +
                    OldDuration::minutes(minutes) + OldDuration::seconds(seconds);
                  estimated_finish = Some(UTC::now() + remaining);
                }
              }

              let mut complete = cap[2].parse::<f64>().expect("!parse complete") / 100.0;
              if complete == 1.0 {
                // only return 100% when synced
                complete = 0.9999;
              }

              let rate = cap[3].to_owned();
              last_rate = Some(rate.clone());
              if let Err(_) = tx.send(JobStatus::Running {
                  common: info.clone(),
                  estimated_finish: estimated_finish,
                  rate: Some(rate),
                  complete: complete }) {
                warn!("tx.send failed, finishing");
                break;
              }
            }
            if out.contains("Syncing... OK!") {
              synced = true;
            }
          }
        }
        else if out.starts_with("File system:") {
          started_main_output = true;
        }
      },
      _ => ()
    }
  }
  if synced {
    if let Err(_) = tx.send(JobStatus::Finished {
        common: info,
        rate: last_rate.unwrap_or("?".to_owned()),
        finish: UTC::now() }) {
      warn!("tx.send failed (final), finishing");
    }
  }
}

impl CloneJob {
  pub fn new(source: String, destination: String, name: String) -> Result<CloneJob> {
    let partclone_variant = "dd"; // TODO detect best variant to use
    let (dest_file, dest_raw_fd) = destination_raw_fd(&destination, &name, partclone_variant)?;

    let mut partclone_cmd = Command::new(partclone_cmd(partclone_variant)?)
      .arg("-s").arg(&source)
      .stdout(Stdio::piped())
      .stdin(Stdio::null())
      .stderr(Stdio::piped())
      .spawn()?;

    Command::new("pigz").arg("-1c")
      .stdin(unsafe { Stdio::from_raw_fd(partclone_cmd.stdout.take().unwrap().into_raw_fd()) })
      .stdout(unsafe { Stdio::from_raw_fd(dest_raw_fd) })
      .spawn()?;

    let stderr = partclone_cmd.stderr.take().unwrap();
    let thread_name = format!("partclone-stderr-reader {}->{}", source, dest_file);
    let (tx, rx) = mpsc::channel();
    let job = CloneJob {
      source: source,
      destination: dest_file,
      partclone_cmd: partclone_cmd,
      rx: rx,
      start: UTC::now(),
      id: Uuid::new_v4()
    };
    let info = JobStatusCommon {
      source: job.source.to_owned(),
      destination: job.successful_destination().to_owned(),
      id: job.id(),
      start: job.start
    };

    thread::Builder::new()
      .name(thread_name)
      .spawn(move|| {
        read_partclone_output(stderr, tx, info)
      })?;

    Ok(job)
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

  pub fn fail_status(&self, reason: &str) -> JobStatus {
    JobStatus::Failed {
      common: JobStatusCommon {
        source: self.source.to_owned(),
        destination: self.successful_destination().to_owned(),
        id: self.id(),
        start: self.start
      },
      reason: reason.to_owned(),
      finish: UTC::now()
    }
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
