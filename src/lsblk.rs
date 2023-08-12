use json::JsonValue;
use std::{
    env,
    io::{Error, ErrorKind, Result},
    process::{Command, Stdio},
    str,
};

fn lsblk_cmd() -> String {
    env::var("APART_LSBLK_CMD").unwrap_or_else(|_| "lsblk".to_owned())
}

/**
 * example json output
 * [{"name": "sda", "size": 750156374016, "fstype": null, "label": null, "mountpoint": null,
 *   "children": [
 *      {"name": "sda1", "size": 104857600, "fstype": "ntfs", "label": "Win Reserved",
 * "mountpoint": null, "uuid": null},      {"name": "sda2", "size": 536766054400, "fstype":
 * "ntfs", "label": "SSD", "mountpoint": null, "uuid": null},      ...
 *   ]
 * }]
 */
pub fn blockdevices() -> Result<Vec<json::JsonValue>> {
    let cmd = lsblk_cmd();
    let lsblk = Command::new(&cmd)
        .arg("-Jbo")
        .arg("name,size,fstype,label,mountpoint,uuid")
        .stdout(Stdio::piped())
        .output()?;

    match json::parse(&String::from_utf8_lossy(&lsblk.stdout)) {
        Ok(mut json) => match json.remove("blockdevices") {
            JsonValue::Array(devs) => Ok(devs),
            _ => Ok(Vec::new()),
        },
        Err(err) => Err(Error::new(
            ErrorKind::InvalidData,
            format!("{cmd} invalid json output: {err}"),
        )),
    }
}

/// expecting something like "/dev/sda1"
fn partition_matching(source: &str) -> Option<JsonValue> {
    match blockdevices() {
        Err(_) => None,
        Ok(devices) => {
            for mut device in devices {
                if let JsonValue::Array(parts) = device["children"].take() {
                    for part in parts {
                        if part["name"].is_string() && format!("/dev/{}", part["name"]) == source {
                            return Some(part);
                        }
                    }
                }
            }
            None
        }
    }
}

/// expecting something like "/dev/sda1"
pub fn fstype(source: &str) -> Option<String> {
    match partition_matching(source) {
        Some(mut part) => part["fstype"].take_string(),
        _ => None,
    }
}

/// expecting something like "/dev/sda1"
pub fn uuid(source: &str) -> Option<String> {
    match partition_matching(source) {
        Some(mut part) => part["uuid"].take_string(),
        _ => None,
    }
}
