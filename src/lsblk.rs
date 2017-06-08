use json;
use json::JsonValue;
use std::{env, str};
use std::io::{Result, Error, ErrorKind};
use std::process::{Command, Stdio};

fn lsblk_cmd() -> String {
  env::var("APART_LSBLK_CMD").unwrap_or_else(|_| "lsblk".to_owned())
}

/**
 * example json output
 * [{"name": "sda", "size": "750156374016", "fstype": null, "label": null, "mountpoint": null,
 *   "children": [
 *      {"name": "sda1", "size": "104857600", "fstype": "ntfs", "label": "Win Reserved", "mountpoint": null},
 *      {"name": "sda2", "size": "536766054400", "fstype": "ntfs", "label": "SSD", "mountpoint": null},
 *      ...
 *   ]
 * }]
 */
pub fn blockdevices() -> Result<Vec<json::JsonValue>> {
  let lsblk = Command::new(lsblk_cmd())
    .arg("-Jbo").arg("name,size,fstype,label,mountpoint")
    .stdout(Stdio::piped())
    .output()?;

  match json::parse(&String::from_utf8_lossy(&lsblk.stdout)) {
    Ok(mut json) => match json.remove("blockdevices") {
      JsonValue::Array(devs) => Ok(devs),
      _ => Ok(Vec::new())
    },
    Err(err) => Err(Error::new(ErrorKind::InvalidData, format!("json invalid: {}", err)))
  }
}

/// expecting something like "/dev/sda1"
pub fn fstype(source: &str) -> Option<String> {
  match blockdevices() {
    Err(_) => None,
    Ok(devices) => {
      for device in devices {
        if let JsonValue::Array(ref parts) = device["children"] {
          for part in parts {
            if let (Some(name), Some(fstype)) = (part["name"].as_str(), part["fstype"].as_str()) {
              if source.ends_with(&format!("/{}", name)) {
                return Some(fstype.to_owned())
              }
            }
          }
        }
      }
      None
    }
  }
}
