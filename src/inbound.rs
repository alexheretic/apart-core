use yaml_rust::{YamlLoader};
use self::Request::*;

#[derive(PartialEq, Debug)]
pub enum Request {
  StatusRequest,
  KillRequest,
  CloneRequest { source: String, destination: String, name: String }
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
  fn parse_job_request() {
    let message = Request::parse("type: clone\nsource: /dev/abc12\ndestination: /mnt/backups/\nname: alex");
    assert_eq!(message, Some(CloneRequest {
      source: "/dev/abc12".to_owned(),
      destination: "/mnt/backups/".to_owned(),
      name: "alex".to_owned()
    }));
  }
}
