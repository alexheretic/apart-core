use clone::{JobStatus, JobStatusCommon};
use json::JsonValue;
use yaml_rust::yaml;
use yaml_rust::yaml::Yaml;
use yaml_rust::emitter::YamlEmitter;

pub trait ToYaml {
  fn to_yaml(&self) -> String;
}

impl ToYaml for JobStatusCommon {
  fn to_yaml(&self) -> String {
    let &JobStatusCommon { ref start, ref source, ref destination, ref id } = self;
    format!("id: {id}
source: {source}
destination: {destination}
start: {start:?}", id = id, start = start, source = source, destination = destination)
  }
}

impl ToYaml for JobStatus {
  fn to_yaml(&self) -> String {
    match self {
      &JobStatus::Running { ref complete, ref rate, ref common } => {
        let complete_yaml_float = match complete.to_string() {
          ref s if s.len() == 1 => s.to_owned() + ".0",
          s => s
        };
        format!("type: clone
{common_yaml}
complete: {complete}
rate: \"{rate}\"", common_yaml = common.to_yaml(), complete = complete_yaml_float, rate = rate)
      },

      &JobStatus::Finished { ref finish, ref rate, ref common } => {
        format!("type: clone
{common_yaml}
complete: {complete}
rate: \"{rate}\"
finish: {finish:?}", common_yaml = common.to_yaml(), complete = "1.0", rate = rate, finish = finish)
      },

      &JobStatus::Failed { ref common, ref reason } => {
        format!("type: clone-failed
{common_yaml}
error: {error}", common_yaml = common.to_yaml(), error = reason)
      }
    }
  }
}

pub fn status_yaml(status: &str, lsblk: Vec<JsonValue>) -> String {
  let mut yaml = yaml::Hash::new();
  yaml.insert(Yaml::from_str("type"), Yaml::from_str("status"));
  yaml.insert(Yaml::from_str("status"), Yaml::from_str(status));

  if !lsblk.is_empty() {
    let mut sources = yaml::Array::new();

    for device in lsblk {
      match (device["name"].as_str(), device["size"].as_str(), &device["children"]) {
        (Some(name), Some(size), &JsonValue::Array(ref children)) if children.len() > 0 => {
          let mut source = yaml::Hash::new();
          source.insert(Yaml::from_str("name"), Yaml::from_str(name));
          source.insert(Yaml::from_str("size"), Yaml::from_str(size));

          let mut parts = yaml::Array::new();
          for p in children {
            if let (Some(name), Some(size), fstype, label, mountpoint) = (
                p["name"].as_str(),
                p["size"].as_str(),
                p["fstype"].as_str(),
                p["label"].as_str(),
                p["mountpoint"].as_str()) {
              let mut part = yaml::Hash::new();
              part.insert(Yaml::from_str("name"), Yaml::from_str(name));
              part.insert(Yaml::from_str("size"), Yaml::from_str(size));
              part.insert(Yaml::from_str("mounted"), Yaml::Boolean(mountpoint.is_some()));
              if let Some(t) = fstype {
                part.insert(Yaml::from_str("fstype"), Yaml::from_str(t));
              }
              if let Some(l) = label {
                part.insert(Yaml::from_str("label"), Yaml::from_str(l));
              }

              parts.push(Yaml::Hash(part));
            }
          }
          source.insert(Yaml::from_str("parts"), Yaml::Array(parts));

          sources.push(Yaml::Hash(source));
        },
        _ => ()
      }
    }

    yaml.insert(Yaml::from_str("sources"), Yaml::Array(sources));
  }

  let mut yaml_str = String::new();
  YamlEmitter::new(&mut yaml_str).dump(&Yaml::Hash(yaml)).unwrap();
  yaml_str
}


#[cfg(test)]
mod tests {
  use super::*;
  use chrono::prelude::*;
  use json;
  use yaml_rust::yaml::*;

  #[test]
  fn status_started_yaml() {
    let lsblk_json = vec!(json::parse(r#"
      {"name": "sda", "size": "750156374016", "fstype": null, "label": null, "mountpoint": null,
        "children": [
          {"name": "sda2", "size": "536766054400", "fstype": null, "label": null, "mountpoint": null},
          {"name": "sda3", "size": "181070200832", "fstype": "ext4", "label": "Arch", "mountpoint": "/"}
        ]
      }"#).unwrap());
    let yaml = YamlLoader::load_from_str(&status_yaml("started", lsblk_json)).unwrap().remove(0);
    assert_eq!(yaml["type"].as_str(), Some("status"));
    assert_eq!(yaml["status"].as_str(), Some("started"));

    let sda = &yaml["sources"][0];
    assert_eq!(sda["name"].as_str(), Some("sda"));
    assert_eq!(sda["size"].as_i64(), Some(750156374016));

    assert_eq!(sda["parts"][0]["name"].as_str(), Some("sda2"));
    assert_eq!(sda["parts"][0]["size"].as_i64(), Some(536766054400));
    assert_eq!(sda["parts"][0]["fstype"].as_str(), None, "sda2.fstype");
    assert_eq!(sda["parts"][0]["label"].as_str(), None, "sda2.label");
    assert_eq!(sda["parts"][0]["mounted"].as_bool(), Some(false), "sda2.mounted");

    assert_eq!(sda["parts"][1]["name"].as_str(), Some("sda3"));
    assert_eq!(sda["parts"][1]["size"].as_i64(), Some(181070200832));
    assert_eq!(sda["parts"][1]["fstype"].as_str(), Some("ext4"));
    assert_eq!(sda["parts"][1]["label"].as_str(), Some("Arch"));
    assert_eq!(sda["parts"][1]["mounted"].as_bool(), Some(true), "sda3.mounted");
  }

  #[test]
  fn job_running_to_yaml() {
    let yaml_str = JobStatus::Running {
      common: JobStatusCommon {
        source: "/dev/ars2".to_owned(),
        destination: "/mnt/backups/ars2.gz".to_owned(),
        start: UTC.ymd(2017, 4, 18).and_hms(15, 44, 12),
        id: "some-id".to_owned()
      },
      complete: 0.123,
      rate: "1GB/s".to_owned() }.to_yaml();
    let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
    assert_eq!(yaml["type"].as_str(), Some("clone"));
    assert_eq!(yaml["complete"].as_f64(), Some(0.123));
    assert_eq!(yaml["id"].as_str(), Some("some-id"));
    assert_eq!(yaml["rate"].as_str(), Some("1GB/s"));
    assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
    assert_eq!(yaml["source"].as_str(), Some("/dev/ars2"));
    assert_eq!(yaml["destination"].as_str(), Some("/mnt/backups/ars2.gz"));
  }

  #[test]
  fn job_finished_to_yaml() {
    let yaml_str = JobStatus::Finished {
      common: JobStatusCommon {
        source: "/dev/ars3".to_owned(),
        destination: "/mnt/backups/ars3.gz".to_owned(),
        start: UTC.ymd(2017, 4, 18).and_hms(15, 44, 12),
        id: "some-id".to_owned()
      },
      finish: UTC.ymd(2017, 4, 18).and_hms(15, 45, 34),
      rate: "1GB/s".to_owned() }.to_yaml();
    let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
    assert_eq!(yaml["type"].as_str(), Some("clone"));
    assert_eq!(yaml["complete"].as_f64(), Some(1.0));
    assert_eq!(yaml["id"].as_str(), Some("some-id"));
    assert_eq!(yaml["rate"].as_str(), Some("1GB/s"));
    assert_eq!(yaml["start"].as_str(), Some("2017-04-18T15:44:12Z"));
    assert_eq!(yaml["finish"].as_str(), Some("2017-04-18T15:45:34Z"));
    assert_eq!(yaml["source"].as_str(), Some("/dev/ars3"));
    assert_eq!(yaml["destination"].as_str(), Some("/mnt/backups/ars3.gz"));
  }

  #[test]
  fn job_running_to_yaml_ensure_float() {
    let yaml_str = JobStatus::Running {
      common: JobStatusCommon {
        source: "/dev/ars3".to_owned(),
        destination: "/mnt/backups/ars3.gz".to_owned(),
        start: UTC.ymd(2017, 4, 18).and_hms(15, 44, 12),
        id: "some-id".to_owned()
      },
      complete: 1.0,
      rate: "2GB/s".to_owned() }.to_yaml();
    let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
    assert_eq!(yaml["complete"].as_f64(), Some(1.0));
    let yaml_str = JobStatus::Running {
      common: JobStatusCommon {
        source: "/dev/ars3".to_owned(),
        destination: "/mnt/backups/ars3.gz".to_owned(),
        start: UTC.ymd(2017, 4, 18).and_hms(15, 44, 12),
        id: "some-id".to_owned()
      },
      complete: 0.0,
      rate: "3GB/s".to_owned() }.to_yaml();
    let yaml = YamlLoader::load_from_str(&yaml_str).unwrap().remove(0);
    assert_eq!(yaml["complete"].as_f64(), Some(0.0));
  }
}
