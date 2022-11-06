use crate::include::*;
use chrono::{prelude::*, Duration as OldDuration};
use regex::Regex;
use std::{
    env,
    error::Error,
    fmt,
    io::{BufRead, BufReader, Error as IoError, ErrorKind},
    path::Path,
    process::ChildStderr,
    rc::Rc,
    str,
    sync::mpsc::Sender,
};

#[derive(Debug)]
pub enum PartcloneStatus {
    Running {
        complete: f64,
        rate: String,
        estimated_finish: DateTime<Utc>,
    },
    Synced {
        finish: DateTime<Utc>,
    },
    Failed {
        finish: DateTime<Utc>,
    },
}

#[derive(Debug)]
pub struct OutputInvalidError(pub String);

impl fmt::Display for OutputInvalidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for OutputInvalidError {
    fn description(&self) -> &str {
        &self.0
    }
    fn cause(&self) -> Option<&dyn Error> {
        None
    }
}

fn default_partclone_path() -> Option<&'static str> {
    [
        "/usr/bin/partclone",
        "/usr/sbin/partclone",
        "/bin/partclone",
        "/sbin/partclone",
        "/usr/local/partclone",
        "/usr/local/bin/partclone",
        "/usr/local/sbin/partclone",
    ]
    .into_iter()
    .find(|p| Path::new(p).with_extension("dd").exists())
}

pub fn cmd(variant: &str) -> Result<String, IoError> {
    let partclone_cmd = match env::var("APART_PARTCLONE_CMD") {
        Ok(env_partclone) => Some(format!("{}.{}", env_partclone, variant)),
        _ => default_partclone_path().map(|path| format!("{}.{}", path, variant)),
    };

    if partclone_cmd.is_none() {
        return Err(IoError::new(
            ErrorKind::NotFound,
            "partclone not found on system".to_owned(),
        ));
    }

    let partclone_cmd = partclone_cmd.unwrap();
    if Path::new(&partclone_cmd).exists() {
        Ok(partclone_cmd)
    } else {
        Err(IoError::new(
            ErrorKind::NotFound,
            format!("{} not found", partclone_cmd),
        ))
    }
}

static PARTCLONE_LOG_TAIL: usize = 4;

pub fn read_output(
    stderr: ChildStderr,
    tx: &Sender<PartcloneStatus>,
) -> Result<(), Box<dyn Error>> {
    let progress_re = Regex::new(
    r"Remaining:\s*(\d{2,}:\d{2}:\d{2}), Completed:\s*(\d{1,3}\.?\d?\d?)%,\s*R?a?t?e?:?\s*([0-9][^,]+)").unwrap();

    let (mut started_main_output, mut synced) = (false, false);
    let duration_re = Regex::new(r"^(\d{2,}):(\d{2}):(\d{2})$").unwrap();

    let mut partclone_out_tail = Vec::new();

    for line in BufReader::new(stderr).lines().flatten() {
        let out = Rc::new(line);
        partclone_out_tail.push(out.clone());
        if partclone_out_tail.len() > PARTCLONE_LOG_TAIL {
            partclone_out_tail.remove(0);
        }
        debug!("partclone: {}", out);
        if started_main_output {
            if !synced {
                for cap in progress_re.captures_iter(&out) {
                    let mut estimated_finish = None;
                    for cap in duration_re.captures_iter(&cap[1]) {
                        if let (Ok(hours), Ok(minutes), Ok(seconds)) = (
                            cap[1].parse::<i64>(),
                            cap[2].parse::<i64>(),
                            cap[3].parse::<i64>(),
                        ) {
                            let remaining = OldDuration::hours(hours)
                                + OldDuration::minutes(minutes)
                                + OldDuration::seconds(seconds);
                            estimated_finish = Some(Utc::now() + remaining);
                        }
                    }
                    let estimated_finish = estimated_finish
                        .ok_or_else(|| OutputInvalidError("!estimated_finish".to_owned()))?;
                    let complete = cap[2].parse::<f64>()? / 100.0;
                    let rate = cap[3].to_owned();
                    debug!(
                        "Partclone output: complete: {}, finish: {}, rate: {}",
                        complete, estimated_finish, rate
                    );
                    if let Err(err) = tx.send(PartcloneStatus::Running {
                        estimated_finish,
                        rate,
                        complete,
                    }) {
                        // this can be expected if, for example, the job is cancelled
                        debug!("Could not send, job dropped?: {}", err);
                        return Ok(());
                    }
                }
                if out.contains("Syncing... OK!") {
                    synced = true;
                }
            }
        } else if out.starts_with("File system:") {
            started_main_output = true;
        }
    }
    if synced {
        if let Err(err) = tx.send(PartcloneStatus::Synced { finish: Utc::now() }) {
            debug!("Could not send, job dropped?: {}", err);
        }
    } else {
        for tail_line in partclone_out_tail {
            error!("Partclone-failed: {}", tail_line);
        }
        if let Err(err) = tx.send(PartcloneStatus::Failed { finish: Utc::now() }) {
            debug!("Could not send, job dropped?: {}", err);
        }
    }
    Ok(())
}
