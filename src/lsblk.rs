use json;
use std::{env, str};
use std::io::{Result, Error, ErrorKind};
use std::process::{Command, Stdio};

fn lsblk_cmd() -> String {
  env::var("LSBLK_CMD").unwrap_or("lsblk".to_owned())
}

pub fn blockdevices() -> Result<Vec<json::JsonValue>> {
  let lsblk = Command::new(lsblk_cmd())
    .arg("-Jbo").arg("name,size,fstype,label,mountpoint")
    .stdout(Stdio::piped())
    .output()?;

  match json::parse(&String::from_utf8_lossy(&lsblk.stdout)) {
    Ok(mut json) => match json.remove("blockdevices") {
      json::JsonValue::Array(devs) => Ok(devs),
      _ => Ok(Vec::new())
    },
    Err(err) => Err(Error::new(ErrorKind::InvalidData, format!("json invalid: {}", err)))
  }
}
