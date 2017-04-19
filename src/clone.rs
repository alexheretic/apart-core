use chrono::prelude::*;
use std::{thread, str, fmt, env, fs};
use std::path::Path;
use std::process::{Command, Child, Stdio, ChildStderr};
use wait_timeout::ChildExt;
use std::time::Duration;
use std::io::{ErrorKind, Error, BufReader, Result, BufRead};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::fs::File;
use regex::Regex;

#[derive(PartialEq, Debug)]
pub enum JobStatus {
  Running { source: String, destination: String, complete: f64, rate: String, start: DateTime<UTC> },
  Finished { source: String, destination: String, rate: String, start: DateTime<UTC>, finish: DateTime<UTC> }
}

#[derive(Debug)]
pub struct CloneJob {
  source: String,
  destination: String,
  partclone_cmd: Child,
  pub rx: Receiver<JobStatus>
}

fn check_exists(command: &str) -> Result<()> {
  let cmd_path = Path::new(command);
  if !cmd_path.exists() {
    return Err(Error::new(ErrorKind::NotFound, format!("{} not found", command)));
  }
  Ok(())
}

fn partclone_cmd_for_source(source: &str) -> Result<String> {
  let partclone_cmd = env::var("PARTCLONE_CMD").unwrap_or("partclone".to_owned());
  let partclone_dd = format!("{}.{}", partclone_cmd, "dd");
  check_exists(&partclone_dd)?;
  Ok(partclone_dd.to_string())
}

fn destination_raw_fd(dir: &str, name: &str) -> Result<(String, RawFd)> {
  // something like: "/mnt/backups/mypart-2017-01-25T1245.apt.gz.inprogress"
  let file = format!("{}/{}-{}.apt.gz.inprogress", dir, name, Local::now().format("%Y-%m-%dT%H%M"));
  let path = Path::new(&file);
  if path.exists() {
    return Err(Error::new(ErrorKind::AlreadyExists, format!("{} already exists", file)));
  }
  Ok((file.to_owned(), File::create(path)?.into_raw_fd()))
}

fn read_partclone_output(stderr: ChildStderr, tx: Sender<JobStatus>, source: String, destination: String) {
  let progress_re = Regex::new(r"Completed:\s*(\d{1,3}\.?\d?\d?)%,\s*([^,]+)").unwrap();
  let start = UTC::now();
  let (mut started_main_output, mut synced) = (false, false);
  let mut last_rate = None;

  for line in BufReader::new(stderr).lines() {
    match line {
      Ok(out) => {
        if started_main_output {
          if !synced {
            for cap in progress_re.captures_iter(&out) {
              let mut complete = cap[1].parse::<f64>().expect("!parse complete") / 100.0;
              if complete == 1.0 {
                // only return 100% when synced
                complete = 0.9999;
              }
              let rate = cap[2].to_owned();
              last_rate = Some(rate.clone());
              if let Err(_) = tx.send(JobStatus::Running {
                  source: source.to_owned(),
                  destination: destination.to_owned(),
                  complete: complete,
                  rate: rate,
                  start: start }) {
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
        source: source.to_owned(),
        destination: destination.to_owned(),
        rate: last_rate.unwrap_or("?".to_owned()),
        start: start,
        finish: UTC::now() }) {
      warn!("tx.send failed (final), finishing");
    }
  }
}

impl CloneJob {
  pub fn new(source: String, destination: String, name: String) -> Result<CloneJob> {
    let (dest_file, dest_raw_fd) = destination_raw_fd(&destination, &name)?;

    let mut partclone_cmd = Command::new(partclone_cmd_for_source(&source)?)
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
      rx: rx
    };
    let (source, destination) = (job.source.to_owned(), job.successful_destination().to_owned());

    thread::Builder::new()
      .name(thread_name)
      .spawn(move|| {
        read_partclone_output(stderr, tx, source, destination)
      })?;

    Ok(job)
  }

  pub fn id(&self) -> String {
    format!("{}->{}", self.source, self.successful_destination())
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
}

impl Drop for CloneJob {
  fn drop(&mut self) {
    match self.partclone_cmd.wait_timeout(Duration::from_secs(0)) {
      Ok(None) => {
        if let Err(x) = self.partclone_cmd.kill() {
          error!("Failed to kill CloneJob#cmd: {}", x);
          self.rm_inprogress_file();
        }
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
