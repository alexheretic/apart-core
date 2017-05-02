use chrono::prelude::*;
use std::{str, fmt, env};
use std::path::Path;
use std::process::{ChildStderr};
use chrono::Duration as OldDuration;
use std::io::{ErrorKind, BufReader, BufRead, Error as IoError};
use std::sync::mpsc::{Sender};
use regex::Regex;
use std::error::Error;

#[derive(Debug)]
pub enum PartcloneStatus {
  Running { complete: f64, rate: String, estimated_finish: DateTime<UTC> },
  Synced { finish: DateTime<UTC> }
}

#[derive(Debug)]
pub struct OutputInvalidError(pub String);

impl fmt::Display for OutputInvalidError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for OutputInvalidError {
  fn description(&self) -> &str { &self.0 }
  fn cause(&self) -> Option<&Error> { None }
}

pub fn cmd(variant: &str) -> Result<String, IoError> {
  let partclone_cmd = env::var("APART_PARTCLONE_CMD").unwrap_or("partclone".to_owned());
  let partclone_dd = format!("{}.{}", partclone_cmd, variant);
  let cmd_path = Path::new(&partclone_dd);
  if !cmd_path.exists() {
    return Err(IoError::new(ErrorKind::NotFound, format!("{} not found", partclone_dd)));
  }
  Ok(partclone_dd.to_string())
}

pub fn read_output(stderr: ChildStderr, tx: Sender<PartcloneStatus>)
                         -> Result<(), Box<Error>> {
  let progress_re = Regex::new(
    r"Remaining:\s*(\d{2,}:\d{2}:\d{2}), Completed:\s*(\d{1,3}\.?\d?\d?)%,\s*([^,]+)").unwrap();

  let (mut started_main_output, mut synced) = (false, false);
  let duration_re = Regex::new(r"^(\d{2,}):(\d{2}):(\d{2})$").unwrap();

  'read: for line in BufReader::new(stderr).lines() {
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
              let estimated_finish = estimated_finish
                .ok_or(OutputInvalidError("!estimated_finish".to_owned()))?;

              let mut complete = cap[2].parse::<f64>()? / 100.0;
              if complete == 1.0 {
                // only 100% when synced
                complete = 0.9999;
              }

              let rate = cap[3].to_owned();
              if let Err(err) = tx.send(PartcloneStatus::Running { estimated_finish, rate, complete }) {
                // this can be expected if, for example, the job is cancelled
                debug!("Could not send, job dropped?: {}", err);
                break 'read;
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
    if let Err(err) = tx.send(PartcloneStatus::Synced { finish: UTC::now() }) {
      debug!("Could not send, job dropped?: {}", err);
    }
  }
  Ok(())
}
