use yaml_rust::{YamlLoader};
use self::Request::*;

#[derive(PartialEq, Debug)]
pub enum Request {
  StatusRequest,
  KillRequest,

  CloneRequest { source: String, destination: String, name: String },
  CancelCloneRequest { id: String },

  RestoreRequest { source: String, destination: String },
  CancelRestoreRequest { id: String }
}

impl Request {
  /// Parses a yaml string to a Request struct, all errors -> None
  pub fn parse(yaml: &str) -> Option<Request> {
    match YamlLoader::load_from_str(yaml) {
      Ok(docs) => match docs[0]["type"].as_str() {
        Some("status-request") => Some(StatusRequest),
        Some("kill-request") => Some(KillRequest),
        Some("clone") => {
          let msg = &docs[0];
          match (msg["source"].as_str(), msg["destination"].as_str(), msg["name"].as_str()) {
            (Some(s), Some(d), Some(n)) => Some(CloneRequest {
              source: s.to_owned(),
              destination: d.to_owned(),
              name: n.to_owned()
            }),
            _ => None
          }
        },
        Some("restore") => {
          let msg = &docs[0];
          match (msg["source"].as_str(), msg["destination"].as_str()) {
            (Some(s), Some(d)) => Some(RestoreRequest {
              source: s.to_owned(),
              destination: d.to_owned()
            }),
            _ => None
          }
        },
        Some("cancel-clone") => match docs[0]["id"].as_str() {
          Some(id) => Some(CancelCloneRequest{ id: id.to_owned() }),
          _ => None
        },
        Some("cancel-restore") => match docs[0]["id"].as_str() {
          Some(id) => Some(CancelRestoreRequest{ id: id.to_owned() }),
          _ => None
        },
        _ => None
      },
      _ => None
    }
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_status_request() {
    assert_eq!(Request::parse("type: status-request"), Some(StatusRequest))
  }

  #[test]
  fn parse_kill_request() {
    assert_eq!(Request::parse("type: kill-request"), Some(KillRequest));
  }

  #[test]
  fn parse_clone_request() {
    let message = Request::parse("type: clone\n\
                                 source: /dev/abc12\n\
                                 destination: /mnt/backups/\n\
                                 name: alex");
    assert_eq!(message, Some(CloneRequest {
      source: "/dev/abc12".to_owned(),
      destination: "/mnt/backups/".to_owned(),
      name: "alex".to_owned()
    }));
  }

  #[test]
  fn parse_restore_request() {
    let message = Request::parse("type: restore\n\
                                 source: /mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz\n\
                                 destination: /dev/abc123");
    assert_eq!(message, Some(RestoreRequest {
      source: "/mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz".to_owned(),
      destination: "/dev/abc123".to_owned()
    }));
  }

  #[test]
  fn parse_cancel_restore() {
    let message = Request::parse("type: cancel-restore\n\
                                 id: uid13213");
    assert_eq!(message, Some(CancelRestoreRequest {
      id: "uid13213".to_owned()
    }));
  }
}
