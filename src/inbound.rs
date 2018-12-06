use self::Request::*;
use crate::compression::Compression;
use yaml_rust::YamlLoader;

#[derive(PartialEq, Debug)]
pub enum Request {
    StatusRequest,
    KillRequest,

    CloneRequest {
        source: String,
        destination: String,
        name: String,
        compression: Compression,
    },
    CancelCloneRequest {
        id: String,
    },

    RestoreRequest {
        source: String,
        destination: String,
    },
    CancelRestoreRequest {
        id: String,
    },

    DeleteImageRequest {
        file: String,
    },
}

impl Request {
    /// Parses a yaml string to a Request struct, all errors -> None
    pub fn parse(yaml: &str) -> Option<Request> {
        if let Ok(docs) = YamlLoader::load_from_str(yaml) {
            if let Some(msg) = docs.into_iter().next() {
                let msg_type = msg["type"].as_str();
                if let Some("status-request") = msg_type {
                    return Some(StatusRequest);
                }
                if let Some("kill-request") = msg_type {
                    return Some(KillRequest);
                }
                if let (Some("clone"), Some(source), Some(dest), Some(name), compression) = (
                    msg_type,
                    msg["source"].as_str(),
                    msg["destination"].as_str(),
                    msg["name"].as_str(),
                    msg["compression"].as_str(),
                ) {
                    let z = {
                        if let Some(name) = compression {
                            match Compression::from_name(name) {
                                Ok(z) => z,
                                Err(err) => {
                                    warn!("{}", err);
                                    return None;
                                }
                            }
                        } else {
                            Compression::default()
                        }
                    };

                    return Some(CloneRequest {
                        source: source.to_owned(),
                        destination: dest.to_owned(),
                        name: name.to_owned(),
                        compression: z,
                    });
                }
                if let (Some("restore"), Some(source), Some(dest)) = (
                    msg_type,
                    msg["source"].as_str(),
                    msg["destination"].as_str(),
                ) {
                    return Some(RestoreRequest {
                        source: source.to_owned(),
                        destination: dest.to_owned(),
                    });
                }
                if let (Some("cancel-clone"), Some(id)) = (msg_type, msg["id"].as_str()) {
                    return Some(CancelCloneRequest { id: id.to_owned() });
                }
                if let (Some("cancel-restore"), Some(id)) = (msg_type, msg["id"].as_str()) {
                    return Some(CancelRestoreRequest { id: id.to_owned() });
                }
                if let (Some("delete-clone"), Some(file)) = (msg_type, msg["file"].as_str()) {
                    return Some(DeleteImageRequest {
                        file: file.to_owned(),
                    });
                }
            }
        }
        None
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
        assert_eq!(Request::parse("type: kill-request"), Some(KillRequest))
    }

    #[test]
    fn parse_empty() {
        assert_eq!(Request::parse(""), None)
    }

    #[test]
    fn parse_clone_request() {
        let message = Request::parse(
            "type: clone\n\
             source: /dev/abc12\n\
             destination: /mnt/backups/\n\
             name: alex",
        );
        assert_eq!(
            message,
            Some(CloneRequest {
                source: "/dev/abc12".to_owned(),
                destination: "/mnt/backups/".to_owned(),
                name: "alex".to_owned(),
                compression: Compression::default(),
            })
        );
    }

    #[test]
    fn parse_restore_request() {
        let message = Request::parse(
            "type: restore\n\
             source: /mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz\n\
             destination: /dev/abc123",
        );
        assert_eq!(
            message,
            Some(RestoreRequest {
                source: "/mnt/backups/sda1-2017-04-18T1739.apt.ext4.gz".to_owned(),
                destination: "/dev/abc123".to_owned(),
            })
        );
    }

    #[test]
    fn parse_cancel_restore() {
        let message = Request::parse(
            "type: cancel-restore\n\
             id: uid13213",
        );
        assert_eq!(
            message,
            Some(CancelRestoreRequest {
                id: "uid13213".to_owned()
            })
        );
    }
}
