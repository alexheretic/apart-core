use std::process::Child;

/// Handle a child process no longer desired running
pub fn drop_log_errors(cmd: &mut Child, log_name: &str) {
    trace!("drop_log_errors(cmd, {})", log_name);
    match cmd.try_wait() {
        Ok(out) => {
            match out {
                None => {
                    if let Err(x) = cmd.kill() {
                        error!("Failed to kill {}: {}", log_name, x);
                    }
                }
                Some(status) => {
                    if !status.success() {
                        warn!("{} finished with != 0 exit", log_name);
                    }
                }
            };
            // after finish / kill use #wait to cleanup
            if let Err(err) = cmd.wait() {
                match err.raw_os_error() {
                    Some(10) => debug!("{}.wait(): {}", log_name, err), // no child process
                    _ => error!("!{}.wait(): {}, kind: {:?}", log_name, err, err.kind()),
                };
            }
        }
        Err(err) => match err.raw_os_error() {
            Some(10) => debug!("{}.try_wait(): {}", log_name, err), // no child process
            _ => error!("Failed to get status {}: {}", log_name, err),
        },
    }
}
