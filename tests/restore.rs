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

static MOCK_IMAGE_CONTENTS: &str = "mock-partition-/dev/sda5-data";

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

  assert_eq!(core.get_tmp_file_contents_utf8(".latest.o.mockpcl.dd.txt").expect("!last -o"),
    "/dev/abc123");
  assert!(!core.tmp_file_contents_is_1(".latest.r.mockpcl.dd.txt"), "partclone.dd invoked with -r");

  core.set_mock_partclone("dd", MockPartcloneState::new().complete(0.5634).rate("0.01GB/min"))
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(0.5634));
  let expected_estimated_finished_time = UTC::now() + mock_duration;

  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["rate"].as_str(), Some("0.01GB/min"));
  assert_eq!(msg["syncing"].as_bool(), Some(false));

  let estimated_finish = msg["estimated_finish"].as_str().expect("missing estimated_finish");
  let estimated_finish_time: DateTime<UTC> = estimated_finish.parse().expect("!parse estimated_finish");

  let finish_time_diff = estimated_finish_time.signed_duration_since(expected_estimated_finished_time);
  if abs(finish_time_diff) > OldDuration::seconds(1) {
    assert_eq!(estimated_finish_time, expected_estimated_finished_time, "expected within a second");
  }
  assert_eq!(msg["start"].as_str(), start);
  assert_eq!(msg["finish"].as_str(), None);

  core.set_mock_partclone("dd", MockPartcloneState::new().complete(1.0).rate("12.23GB/min"))
    .expect("!set_mock_partclone");

  // should get some message just before finish notifying syncing status
  core.expect_message_with(|msg|
    msg["complete"].as_f64() != Some(1.0) && msg["syncing"].as_bool() == Some(true));

  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
  assert_eq!(msg["id"].as_str(), id);
  // assert_eq!(msg["rate"].as_str(), Some("12.23GB/min"));
  assert_eq!(msg["start"].as_str(), start);
  assert!(msg["finish"].as_str().is_some(), "missing restore.finish");

  let partclone_stdin = core.get_tmp_file_contents_utf8(".latest.stdin.mockpcl.dd.txt")
    .expect("!.latest.stdin.mockpcl.dd.txt");
  assert_eq!(partclone_stdin, MOCK_IMAGE_CONTENTS);

  assert!(core.tmp_file_contents_is_1(".latest.finished.mockpcl.dd.txt"), "partclone didn't finish");
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

  core.set_mock_partclone("f2fs", MockPartcloneState::new().complete(1.0).rate("1.23GB/min"))
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
  assert_eq!(msg["source"].as_str(), Some(source_image.as_ref()));
  assert_eq!(msg["destination"].as_str(), Some("/dev/abc122"));
  assert_eq!(core.get_tmp_file_contents_utf8(".latest.o.mockpcl.f2fs.txt").expect("!last -o"),
    "/dev/abc122");
  assert!(core.tmp_file_contents_is_1(".latest.r.mockpcl.f2fs.txt"),
    "partclone.f2fs not invoke with -r");

  let partclone_stdin = core.get_tmp_file_contents_utf8(".latest.stdin.mockpcl.f2fs.txt")
    .expect("!.latest.stdin.mockpcl.f2fs.txt");
  assert_eq!(partclone_stdin, MOCK_IMAGE_CONTENTS);

  assert!(core.tmp_file_contents_is_1(".latest.finished.mockpcl.f2fs.txt"), "partclone didn't finish");
}

#[test]
fn restore_using_partclone_fstype_variant_ext2() {
  let core = CoreHandle::new().unwrap();

  let source_image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.ext2.gz");
  let clone_msg = format!("type: restore\n\
                          source: {source}\n\
                          destination: /dev/abc124", source = source_image);
  core.send(&clone_msg);

  core.set_mock_partclone("ext2", MockPartcloneState::new().complete(1.0).rate("1.23GB/min"))
    .expect("!set_mock_partclone");
  let ref msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
  assert_eq!(msg["source"].as_str(), Some(source_image.as_ref()));
  assert_eq!(msg["destination"].as_str(), Some("/dev/abc124"));
  assert_eq!(core.get_tmp_file_contents_utf8(".latest.o.mockpcl.ext2.txt").expect("!last -o"),
    "/dev/abc124");
  assert!(core.tmp_file_contents_is_1(".latest.r.mockpcl.ext2.txt"),
    "partclone.f2fs not invoke with -r");

  let partclone_stdin = core.get_tmp_file_contents_utf8(".latest.stdin.mockpcl.ext2.txt")
    .expect("!.latest.stdin.mockpcl.ext2.txt");
  assert_eq!(partclone_stdin, MOCK_IMAGE_CONTENTS);
  assert!(core.tmp_file_contents_is_1(".latest.finished.mockpcl.ext2.txt"), "partclone didn't finish");
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

  core.set_mock_partclone("dd", MockPartcloneState::new().complete(0.7865).rate("9.00GB/min"))
    .expect("!set_mock_partclone");

  let ref msg = core.expect_message_with(|msg| msg["rate"].as_str() == Some("9.00GB/min"));
  assert_eq!(msg["id"].as_str(), id);

  let cancel_msg = format!("type: cancel-restore\nid: {id}", id = id.unwrap());
  core.send(&cancel_msg);

  let ref msg = core.expect_message_with(|msg| msg["error"].as_str().is_some());
  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["error"].as_str(), Some("Cancelled"));

  assert!(!core.tmp_file_contents_is_1(".latest.finished.mockpcl.dd.txt"), "partclone not cancelled");
}

#[test]
fn restore_error() {
  let core = CoreHandle::new().unwrap();

  let source_image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.dd.gz");
  let clone_msg = format!("type: restore\n\
                          source: {source}\n\
                          destination: /dev/abc124", source = source_image);
  core.send(&clone_msg);

  let ref msg = core.expect_message_with(|msg|
    msg["type"].as_str() == Some("restore") && msg["rate"].as_str().is_some());
  let id = msg["id"].as_str();

  core.set_mock_partclone("dd", MockPartcloneState::new().complete(0.7865).rate("9.00GB/min").error(true))
    .expect("!set_mock_partclone");

  let ref msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("restore-failed"));
  assert_eq!(msg["id"].as_str(), id);
  assert_eq!(msg["error"].as_str(), Some("Failed"));

  assert!(!core.tmp_file_contents_is_1(".latest.finished.mockpcl.dd.txt"), "partclone not cancelled");
}
