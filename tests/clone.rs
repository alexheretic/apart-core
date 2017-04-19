extern crate flate2;
extern crate uuid;
extern crate zmq;
extern crate yaml_rust;
extern crate wait_timeout;
extern crate chrono;
mod coreutil;

use chrono::prelude::*;
use coreutil::*;
use std::path::Path;
use std::time::{Duration, Instant};

// Tests asserting from a client's perspective performing a partition clone

#[test]
fn do_clone_job() {
  let core = CoreHandle::new().unwrap();

  let clone_msg = format!("type: clone
source: /dev/abc12
destination: {destination}
name: do_clone_job", destination = core.tmp_dir());
  core.send(&clone_msg);
  let expected_filename = format!("do_clone_job-{}.apt.dd.gz", Local::now().format("%Y-%m-%dT%H%M"));

  let ref msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("clone"));
  let id = msg["id"].as_str();
  let start = msg["start"].as_str();
  assert!(id.is_some(), "missing clone.id");
  assert!(start.is_some(), "missing clone.start");
  assert_eq!(msg["complete"].as_f64(), Some(0.0));
  assert_eq!(msg["rate"].as_str(), Some("9.99GB/min"));
  assert_eq!(msg["finish"].as_str(), None);
  assert_eq!(msg["source"].as_str(), Some("/dev/abc12"));
  assert_eq!(msg["destination"].as_str(), Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref()));
  assert_eq!(core.get_mock_partclone_last_source_of("dd").expect("!last source"), "/dev/abc12");

  assert!(!core.path_of(&format!("{}/{}", core.tmp_dir(), expected_filename)).unwrap().exists());

  core.set_mock_partclone(MockPartcloneState{ complete: 0.5634, rate: "0.01GB/min".to_owned() })
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(0.5634));
  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["rate"].as_str(), Some("0.01GB/min"));
  assert_eq!(msg["start"].as_str(), start);
  assert_eq!(msg["finish"].as_str(), None);

  assert!(!core.path_of(&format!("{}/{}", core.tmp_dir(), expected_filename)).unwrap().exists());

  core.set_mock_partclone(MockPartcloneState{ complete: 1.0, rate: "12.23GB/min".to_owned() })
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["rate"].as_str(), Some("12.23GB/min"));
  assert_eq!(msg["start"].as_str(), start);
  assert!(msg["finish"].as_str().is_some(), "missing clone.finish");

  let output = core.get_tmp_file_contents_bytes(&expected_filename).expect("!read $tmp_dir/do_clone_job.apt.gz");
  assert_eq!(decompress(&output).expect("!decompress"), "mock-partition-/dev/abc12-data");
}

#[test]
fn cancel_clone_job() {
  let core = CoreHandle::new().unwrap();

  let clone_msg = format!("type: clone
source: /dev/abc12
destination: {destination}
name: cancel_clone_job", destination = core.tmp_dir());
  core.send(&clone_msg);

  let ref msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("clone"));
  let id = msg["id"].as_str();
  let destination = msg["destination"].as_str().unwrap();

  core.set_mock_partclone(MockPartcloneState{ complete: 0.7865, rate: "9.00GB/min".to_owned() })
    .expect("!set_mock_partclone");

  let ref msg = core.expect_message_with(|msg| msg["rate"].as_str() == Some("9.00GB/min"));
  assert_eq!(msg["id"].as_str(), id);

  let inprogress_path = format!("{}.inprogress", destination);
  assert!(Path::new(&inprogress_path).exists());

  let cancel_msg = format!("type: cancel-clone\nid: {id}", id = id.unwrap());
  core.send(&cancel_msg);

  let ref msg = core.expect_message_with(|msg| msg["error"].as_str().is_some());
  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["error"].as_str(), Some("Cancelled"));

  assert!(!Path::new(&destination).exists());

  let start = Instant::now();
  loop {
    assert!(Instant::now().duration_since(start) < Duration::from_secs(1),
      "*.inprogress file not deleted");
    if !Path::new(&inprogress_path).exists() { break; }
  }
}
