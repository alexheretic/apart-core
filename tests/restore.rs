extern crate flate2;
extern crate uuid;
extern crate zmq;
extern crate yaml_rust;
extern crate wait_timeout;
extern crate chrono;
mod coreutil;

use chrono::prelude::*;
use chrono::Duration as OldDuration;
use coreutil::*;

// Tests asserting from a client's perspective performing a partition restore

#[test]
fn restore_success() {
  let core = CoreHandle::new().unwrap();
  /// default estimated remaining duration in mock partclone
  let mock_duration = OldDuration::minutes(3) + OldDuration::seconds(2);

  let source_image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.dd.gz");
  let clone_msg = format!("type: restore\n\
                          source: {source}\n\
                          destination: /dev/abc123", source = source_image);
  core.send(&clone_msg);

  let ref msg = core.expect_message_with(|msg|
    msg["type"].as_str() == Some("restore") && msg["rate"].as_str().is_some());
  let id = msg["id"].as_str();
  let start = msg["start"].as_str();
  assert!(id.is_some(), "missing restore.id");
  assert!(start.is_some(), "missing restore.start");
  assert_eq!(msg["complete"].as_f64(), Some(0.0));
  assert_eq!(msg["finish"].as_str(), None);
  assert_eq!(msg["source"].as_str(), Some(source_image.as_ref()));
  assert_eq!(msg["destination"].as_str(), Some("/dev/abc123"));
  assert_eq!(core.get_mock_partclone_last_destination_of("dd").expect("!last source"), "/dev/abc123");
  assert!(!core.get_mock_partclone_last_arg_r_set_for("dd"), "partclone.dd invoked with '-r'");

  core.set_mock_partclone(MockPartcloneState{ complete: 0.5634, rate: "0.01GB/min".to_owned() })
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(0.5634));
  let expected_estimated_finished_time = UTC::now() + mock_duration;

  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["rate"].as_str(), Some("0.01GB/min"));

  let estimated_finish = msg["estimated_finish"].as_str().expect("missing estimated_finish");
  let estimated_finish_time: DateTime<UTC> = estimated_finish.parse().expect("!parse estimated_finish");

  let finish_time_diff = estimated_finish_time.signed_duration_since(expected_estimated_finished_time);
  if abs(finish_time_diff) > OldDuration::seconds(1) {
    assert_eq!(estimated_finish_time, expected_estimated_finished_time, "expected within a second");
  }
  assert_eq!(msg["start"].as_str(), start);
  assert_eq!(msg["finish"].as_str(), None);

  core.set_mock_partclone(MockPartcloneState{ complete: 1.0, rate: "12.23GB/min".to_owned() })
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
  assert_eq!(msg["id"].as_str(), id);
  // assert_eq!(msg["rate"].as_str(), Some("12.23GB/min"));
  assert_eq!(msg["start"].as_str(), start);
  assert!(msg["finish"].as_str().is_some(), "missing restore.finish");

  assert_eq!("check actual data", "mock-partition-/dev/sda5-data");
}

fn abs(duration: OldDuration) -> OldDuration {
  if duration < OldDuration::zero() {
    return duration * -1
  }
  duration
}

#[test]
fn restore_using_partclone_fstype_variant_f2fs() {
  let core = CoreHandle::new().unwrap();

  let source_image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.f2fs.gz");
  let clone_msg = format!("type: restore\n\
                          source: {source}\n\
                          destination: /dev/abc122", source = source_image);
  core.send(&clone_msg);

  let ref msg = core.expect_message_with(|msg|
    msg["type"].as_str() == Some("restore") && msg["rate"].as_str().is_some());
  assert_eq!(msg["source"].as_str(), Some(source_image.as_ref()));
  assert_eq!(msg["destination"].as_str(), Some("/dev/abc122"));
  assert_eq!(core.get_mock_partclone_last_destination_of("f2fs").expect("!last destination"),
    "/dev/abc122");
  assert!(core.get_mock_partclone_last_arg_r_set_for("f2fs"),
    "partclone.f2fs not invoked with '-r'");
}

#[test]
fn restore_using_partclone_fstype_variant_ext2() {
  let core = CoreHandle::new().unwrap();

  let source_image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.ext2.gz");
  let clone_msg = format!("type: restore\n\
                          source: {source}\n\
                          destination: /dev/abc124", source = source_image);
  core.send(&clone_msg);

  let ref msg = core.expect_message_with(|msg|
    msg["type"].as_str() == Some("restore") && msg["rate"].as_str().is_some());
  assert_eq!(msg["source"].as_str(), Some(source_image.as_ref()));
  assert_eq!(msg["destination"].as_str(), Some("/dev/abc124"));
  assert_eq!(core.get_mock_partclone_last_destination_of("ext2").expect("!last destination"),
    "/dev/abc124");
  assert!(core.get_mock_partclone_last_arg_r_set_for("ext2"),
    "partclone.ext2 not invoked with '-r'");
}

#[test]
fn restore_then_cancel() {
  let core = CoreHandle::new().unwrap();

  let source_image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.dd.gz");
  let clone_msg = format!("type: restore\n\
                          source: {source}\n\
                          destination: /dev/abc124", source = source_image);
  core.send(&clone_msg);

  let ref msg = core.expect_message_with(|msg|
    msg["type"].as_str() == Some("restore") && msg["rate"].as_str().is_some());
  let id = msg["id"].as_str();

  core.set_mock_partclone(MockPartcloneState{ complete: 0.7865, rate: "9.00GB/min".to_owned() })
    .expect("!set_mock_partclone");

  let ref msg = core.expect_message_with(|msg| msg["rate"].as_str() == Some("9.00GB/min"));
  assert_eq!(msg["id"].as_str(), id);

  let cancel_msg = format!("type: cancel-restore\nid: {id}", id = id.unwrap());
  core.send(&cancel_msg);

  let ref msg = core.expect_message_with(|msg| msg["error"].as_str().is_some());
  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["error"].as_str(), Some("Cancelled"));
}
