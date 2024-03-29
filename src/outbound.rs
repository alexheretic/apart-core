use crate::{clone::*, compression::Compression, restore::*, server::DeleteResult};
use chrono::prelude::*;
use json::JsonValue;
use std::io::ErrorKind;
use yaml_rust::{emitter::YamlEmitter, yaml, yaml::Yaml};

pub trait ToYaml {
    fn to_yaml(&self) -> String;
}

fn common_yaml(start: DateTime<Utc>, source: &str, destination: &str, id: &str) -> String {
    format!(
        "id: {id}\n\
         source: {source}\n\
         destination: {destination}\n\
         start: {start:?}",
        id = id,
        start = start,
        source = source,
        destination = destination
    )
}

fn complete_yaml_str(complete: f64) -> String {
    let complete = complete.to_string();
    if complete.len() == 1 {
        complete + ".0"
    } else {
        complete
    }
}

impl ToYaml for CloneStatusCommon {
    fn to_yaml(&self) -> String {
        let &CloneStatusCommon {
            start,
            ref source,
            ref destination,
            ref id,
            ref source_uuid,
            ..
        } = self;
        let mut yaml = common_yaml(start, source, destination, id);
        if let Some(ref uuid) = *source_uuid {
            yaml.push('\n');
            yaml.push_str("source_uuid: ");
            yaml.push_str(uuid);
        }
        yaml
    }
}

impl ToYaml for RestoreStatusCommon<'_> {
    fn to_yaml(&self) -> String {
        let &RestoreStatusCommon {
            start,
            source,
            destination,
            id,
            ..
        } = self;
        common_yaml(start, source, destination, id)
    }
}

impl ToYaml for CloneStatus {
    fn to_yaml(&self) -> String {
        match *self {
            CloneStatus::Running {
                complete,
                ref rate,
                ref common,
                ref estimated_finish,
            } => {
                let estimated_finish =
                    estimated_finish.map_or_else(|| "~".to_owned(), |d| format!("{:?}", d));
                let rate = rate.clone().unwrap_or_else(|| "~".to_owned());
                format!(
                    "type: clone\n\
                     {common_yaml}\n\
                     complete: {complete}\n\
                     syncing: false\n\
                     rate: {rate}\n\
                     estimated_finish: {finish}",
                    common_yaml = common.to_yaml(),
                    complete = complete_yaml_str(complete),
                    rate = rate,
                    finish = estimated_finish
                )
            }
            CloneStatus::Syncing { ref common } => format!(
                "type: clone\n\
                 {common_yaml}\n\
                 complete: 0.9999\n\
                 syncing: true",
                common_yaml = common.to_yaml()
            ),
            CloneStatus::Finished {
                ref finish,
                ref common,
                image_size,
            } => format!(
                "type: clone\n\
                 {common_yaml}\n\
                 complete: 1.0\n\
                 syncing: false\n\
                 finish: {finish:?}\n\
                 image_size: {image_size}",
                common_yaml = common.to_yaml(),
                finish = finish,
                image_size = image_size
            ),
            CloneStatus::Failed {
                ref finish,
                ref common,
                ref reason,
            } => format!(
                "type: clone-failed\n\
                 {common_yaml}\n\
                 finish: {finish:?}\n\
                 error: {error}",
                common_yaml = common.to_yaml(),
                finish = finish,
                error = reason
            ),
        }
    }
}

impl ToYaml for RestoreStatus<'_> {
    fn to_yaml(&self) -> String {
        match *self {
            RestoreStatus::Running {
                ref common,
                complete,
                syncing,
                ref rate,
                estimated_finish,
            } => {
                let estimated_finish =
                    estimated_finish.map_or_else(|| "~".to_owned(), |d| format!("{:?}", d));
                let rate = rate.clone().unwrap_or_else(|| "~".to_owned());
                format!(
                    "type: restore\n\
                     {common_yaml}\n\
                     complete: {complete}\n\
                     syncing: {syncing}\n\
                     rate: {rate}\n\
                     estimated_finish: {finish}",
                    common_yaml = common.to_yaml(),
                    complete = complete_yaml_str(complete),
                    rate = rate,
                    finish = estimated_finish,
                    syncing = syncing
                )
            }
            RestoreStatus::Finished { ref common, finish } => format!(
                "type: restore\n\
                 {common_yaml}\n\
                 complete: 1.0\n\
                 syncing: false\n\
                 finish: {finish:?}",
                finish = finish,
                common_yaml = common.to_yaml()
            ),
            RestoreStatus::Failed {
                ref common,
                ref reason,
                finish,
            } => format!(
                "type: restore-failed\n\
                 {common_yaml}\n\
                 finish: {finish:?}\n\
                 error: {error}",
                common_yaml = common.to_yaml(),
                finish = finish,
                error = reason
            ),
        }
    }
}

pub fn status_yaml(status: &str, lsblk: Vec<JsonValue>) -> String {
    let mut yaml = yaml::Hash::new();
    yaml.insert(Yaml::from_str("type"), Yaml::from_str("status"));
    yaml.insert(Yaml::from_str("status"), Yaml::from_str(status));

    if !lsblk.is_empty() {
        let mut sources = yaml::Array::new();

        fn parse_size(size: &JsonValue) -> Option<u64> {
            size.as_u64()
                .or_else(|| size.as_str().and_then(|s| s.parse::<u64>().ok()))
        }

        for device in lsblk {
            match (
                device["name"].as_str(),
                parse_size(&device["size"]),
                &device["children"],
            ) {
                (Some(name), Some(size), JsonValue::Array(children)) if !children.is_empty() => {
                    let mut source = yaml::Hash::new();
                    source.insert(Yaml::from_str("name"), Yaml::from_str(name));
                    source.insert(Yaml::from_str("size"), Yaml::from_str(&format!("{}", size)));

                    let mut parts = yaml::Array::new();
                    for p in children {
                        if let (Some(name), Some(size), fstype, label, mountpoint, uuid) = (
                            p["name"].as_str(),
                            parse_size(&p["size"]),
                            p["fstype"].as_str(),
                            p["label"].as_str(),
                            p["mountpoint"].as_str(),
                            p["uuid"].as_str(),
                        ) {
                            let mut part = yaml::Hash::new();
                            part.insert(Yaml::from_str("name"), Yaml::from_str(name));
                            part.insert(
                                Yaml::from_str("size"),
                                Yaml::from_str(&format!("{}", size)),
                            );
                            part.insert(
                                Yaml::from_str("mounted"),
                                Yaml::Boolean(mountpoint.is_some()),
                            );
                            if let Some(t) = fstype {
                                part.insert(Yaml::from_str("fstype"), Yaml::from_str(t));
                            }
                            if let Some(l) = label {
                                part.insert(Yaml::from_str("label"), Yaml::from_str(l));
                            }
                            if let Some(id) = uuid {
                                part.insert(Yaml::from_str("uuid"), Yaml::from_str(id));
                            }

                            parts.push(Yaml::Hash(part));
                        }
                    }
                    source.insert(Yaml::from_str("parts"), Yaml::Array(parts));

                    sources.push(Yaml::Hash(source));
                }
                _ => (),
            }
        }

        yaml.insert(Yaml::from_str("sources"), Yaml::Array(sources));
    }

    let mut compression_options = yaml::Array::new();
    for z in Compression::all_installed() {
        compression_options.push(Yaml::from_str(z.name));
    }
    yaml.insert(
        Yaml::from_str("compression_options"),
        Yaml::Array(compression_options),
    );

    let mut yaml_str = String::new();
    YamlEmitter::new(&mut yaml_str)
        .dump(&Yaml::Hash(yaml))
        .unwrap();
    yaml_str
}

impl ToYaml for DeleteResult {
    fn to_yaml(&self) -> String {
        match *self {
            DeleteResult(ref file, Ok(_)) => format!(
                "type: deleted-clone\n\
                 file: {}",
                file
            ),
            DeleteResult(ref file, Err(ref err)) => {
                let reason = match err.kind() {
                    ErrorKind::NotFound => "No such file".to_owned(),
                    _ => err.to_string(),
                };
                format!(
                    "type: delete-clone-failed\n\
                     file: {}\n\
                     error: {}",
                    file, reason
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust::yaml::*;

    #[test]
    fn status_started_yaml() {
        let lsblk_json = vec!(json::parse(r#"
      {"name": "sda", "size": 750156374016, "fstype": null, "label": null, "mountpoint": null,
        "children": [
          {"name": "sda2", "size": 536766054400, "fstype": null, "label": null, "mountpoint": null},
          {"name": "sda3", "size": 181070200832, "fstype": "ext4", "label": "Arch", "mountpoint": "/"}
        ]
      }"#).unwrap());
        let yaml = YamlLoader::load_from_str(&status_yaml("started", lsblk_json))
            .unwrap()
            .remove(0);
        assert_eq!(yaml["type"].as_str(), Some("status"));
        assert_eq!(yaml["status"].as_str(), Some("started"));

        let sda = &yaml["sources"][0];
        assert_eq!(sda["name"].as_str(), Some("sda"));
        assert_eq!(sda["size"].as_i64(), Some(750_156_374_016));

        assert_eq!(sda["parts"][0]["name"].as_str(), Some("sda2"));
        assert_eq!(sda["parts"][0]["size"].as_i64(), Some(536_766_054_400));
        assert_eq!(sda["parts"][0]["fstype"].as_str(), None, "sda2.fstype");
        assert_eq!(sda["parts"][0]["label"].as_str(), None, "sda2.label");
        assert_eq!(
            sda["parts"][0]["mounted"].as_bool(),
            Some(false),
            "sda2.mounted"
        );

        assert_eq!(sda["parts"][1]["name"].as_str(), Some("sda3"));
        assert_eq!(sda["parts"][1]["size"].as_i64(), Some(181_070_200_832));
        assert_eq!(sda["parts"][1]["fstype"].as_str(), Some("ext4"));
        assert_eq!(sda["parts"][1]["label"].as_str(), Some("Arch"));
        assert_eq!(
            sda["parts"][1]["mounted"].as_bool(),
            Some(true),
            "sda3.mounted"
        );
    }

    #[test]
    fn clone_running_to_yaml() {
        let yaml_str = CloneStatus::Running {
            common: CloneStatusCommon {
                source: "/dev/ars2".to_owned(),
                destination: "/mnt/backups/ars2.gz".to_owned(),
                inprogress_destination: "/mnt/backups/ars2.gz.inprogress".to_owned(),
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id".to_owned(),
                source_uuid: Some("123-234-345".to_owned()),
            },
            estimated_finish: Some(Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 00).unwrap()),
            complete: 0.123,
            rate: Some("1GB/s".to_owned()),
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["type"].as_str(), Some("clone"));
        assert_eq!(yaml["complete"].as_f64(), Some(0.123));
        assert_eq!(yaml["id"].as_str(), Some("some-id"));
        assert_eq!(yaml["rate"].as_str(), Some("1GB/s"));
        assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
        assert_eq!(yaml["source"].as_str(), Some("/dev/ars2"));
        assert_eq!(yaml["destination"].as_str(), Some("/mnt/backups/ars2.gz"));
        assert_eq!(yaml["source_uuid"].as_str(), Some("123-234-345"));
    }

    #[test]
    fn restore_running_to_yaml() {
        let yaml_str = RestoreStatus::Running {
            common: RestoreStatusCommon {
                source: "/mnt/backups/ars2.gz",
                destination: "/dev/ars2",
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id",
            },
            estimated_finish: Some(Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 00).unwrap()),
            complete: 0.123,
            syncing: false,
            rate: Some("1GB/s".to_owned()),
        }
        .to_yaml();

        println!("{}", yaml_str);

        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["type"].as_str(), Some("restore"));
        assert_eq!(yaml["complete"].as_f64(), Some(0.123));
        assert_eq!(yaml["syncing"].as_bool(), Some(false));
        assert_eq!(yaml["id"].as_str(), Some("some-id"));
        assert_eq!(yaml["rate"].as_str(), Some("1GB/s"));
        assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
        assert_eq!(yaml["destination"].as_str(), Some("/dev/ars2"));
        assert_eq!(yaml["source"].as_str(), Some("/mnt/backups/ars2.gz"));
    }

    #[test]
    fn job_running_none_options() {
        let yaml_str = CloneStatus::Running {
            common: CloneStatusCommon {
                source: "/dev/ars2".to_owned(),
                destination: "/mnt/backups/ars2.gz".to_owned(),
                inprogress_destination: "/mnt/backups/ars2.gz.inprogress".to_owned(),
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id".to_owned(),
                source_uuid: None,
            },
            estimated_finish: None,
            complete: 0.123,
            rate: None,
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["rate"].as_str(), None);
        assert_eq!(yaml["estimated_finish"].as_str(), None);
    }

    #[test]
    fn clone_finished_to_yaml() {
        let yaml_str = CloneStatus::Finished {
            common: CloneStatusCommon {
                source: "/dev/ars3".to_owned(),
                destination: "/mnt/backups/ars3.gz".to_owned(),
                inprogress_destination: "/mnt/backups/ars2.gz.inprogress".to_owned(),
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id".to_owned(),
                source_uuid: None,
            },
            finish: Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 34).unwrap(),
            image_size: 123_123,
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["type"].as_str(), Some("clone"));
        assert_eq!(yaml["complete"].as_f64(), Some(1.0));
        assert_eq!(yaml["id"].as_str(), Some("some-id"));
        assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
        assert_eq!(yaml["finish"].as_str(), Some("2017-04-18T15:45:34Z"));
        assert_eq!(yaml["source"].as_str(), Some("/dev/ars3"));
        assert_eq!(yaml["destination"].as_str(), Some("/mnt/backups/ars3.gz"));
        assert_eq!(yaml["image_size"].as_i64(), Some(123_123));
    }

    #[test]
    fn restore_finished_to_yaml() {
        let yaml_str = RestoreStatus::Finished {
            common: RestoreStatusCommon {
                source: "/mnt/backups/ars3.gz",
                destination: "/dev/ars3",
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id",
            },
            finish: Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 34).unwrap(),
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["type"].as_str(), Some("restore"));
        assert_eq!(yaml["complete"].as_f64(), Some(1.0));
        assert_eq!(yaml["id"].as_str(), Some("some-id"));
        assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
        assert_eq!(yaml["finish"].as_str(), Some("2017-04-18T15:45:34Z"));
        assert_eq!(yaml["source"].as_str(), Some("/mnt/backups/ars3.gz"));
        assert_eq!(yaml["destination"].as_str(), Some("/dev/ars3"));
    }

    #[test]
    fn job_failed_to_yaml() {
        let yaml_str = CloneStatus::Failed {
            common: CloneStatusCommon {
                source: "/dev/ars3".to_owned(),
                destination: "/mnt/backups/ars3.gz".to_owned(),
                inprogress_destination: "/mnt/backups/ars2.gz.inprogress".to_owned(),
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id".to_owned(),
                source_uuid: None,
            },
            finish: Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 34).unwrap(),
            reason: "something went wrong".to_owned(),
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["type"].as_str(), Some("clone-failed"));
        assert_eq!(yaml["id"].as_str(), Some("some-id"));
        assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
        assert_eq!(yaml["finish"].as_str(), Some("2017-04-18T15:45:34Z"));
        assert_eq!(yaml["source"].as_str(), Some("/dev/ars3"));
        assert_eq!(yaml["destination"].as_str(), Some("/mnt/backups/ars3.gz"));
    }

    #[test]
    fn job_running_to_yaml_ensure_float() {
        let yaml_str = CloneStatus::Running {
            common: CloneStatusCommon {
                source: "/dev/ars3".to_owned(),
                destination: "/mnt/backups/ars3.gz".to_owned(),
                inprogress_destination: "/mnt/backups/ars2.gz.inprogress".to_owned(),
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id".to_owned(),
                source_uuid: None,
            },
            estimated_finish: Some(Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 00).unwrap()),
            complete: 1.0,
            rate: Some("2GB/s".to_owned()),
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["complete"].as_f64(), Some(1.0));
        let yaml_str = CloneStatus::Running {
            common: CloneStatusCommon {
                source: "/dev/ars3".to_owned(),
                destination: "/mnt/backups/ars3.gz".to_owned(),
                inprogress_destination: "/mnt/backups/ars2.gz.inprogress".to_owned(),
                start: Utc.with_ymd_and_hms(2017, 4, 18, 15, 44, 12).unwrap(),
                id: "some-id".to_owned(),
                source_uuid: None,
            },
            estimated_finish: Some(Utc.with_ymd_and_hms(2017, 4, 18, 15, 45, 00).unwrap()),
            complete: 0.0,
            rate: Some("3GB/s".to_owned()),
        }
        .to_yaml();
        let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
        assert_eq!(yaml["complete"].as_f64(), Some(0.0));
    }
}
