extern crate chrono;
extern crate env_logger;
extern crate flate2;
#[macro_use]
extern crate log;
extern crate uuid;
extern crate yaml_rust;
extern crate zmq;
mod coreutil;

use chrono::prelude::*;
use chrono::Duration as OldDuration;
use coreutil::*;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// Tests asserting from a client's perspective performing a partition clone

#[test]
fn do_clone_job() {
    let _ = env_logger::try_init();

    let core = CoreHandle::new().unwrap();
    // default estimated remaining duration in mock partclone
    let mock_duration = OldDuration::minutes(3) + OldDuration::seconds(2);

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sda5\n\
         destination: {destination}\n\
         name: do_clone_job",
        destination = core.tmp_dir()
    );

    let send_time = Utc::now();
    core.send(&clone_msg);
    let expected_filename = format!(
        "do_clone_job-{}.apt.dd.gz",
        Local::now().format("%Y-%m-%dT%H%M")
    );

    let msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("clone"));
    let id = msg["id"].as_str();
    let start = msg["start"].as_str();
    // ensure we get a timely initial message not waiting for partclone rate/estimated remaining
    assert!(Utc::now().signed_duration_since(send_time) <= OldDuration::milliseconds(500));

    let msg = core.expect_message_with(|msg| {
        msg["type"].as_str() == Some("clone") && msg["rate"].as_str().is_some()
    });

    assert_eq!(msg["id"].as_str(), id);
    assert_eq!(msg["start"].as_str(), start);
    assert_eq!(msg["complete"].as_f64(), Some(0.0));
    assert_eq!(msg["finish"].as_str(), None);
    assert_eq!(msg["source"].as_str(), Some("/dev/sda5"));
    assert_eq!(msg["source_uuid"].as_str(), None);
    assert_eq!(
        msg["destination"].as_str(),
        Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref())
    );
    assert_eq!(
        core.get_tmp_file_contents_utf8(".latest.s.mockpcl.dd.txt")
            .expect("!last source"),
        "/dev/sda5"
    );
    assert!(
        !core.tmp_file_contents_is_1(".latest.c.mockpcl.dd.txt"),
        "partclone.dd invoked with '-c'"
    );

    assert!(!core
        .path_of(&format!("{}/{}", core.tmp_dir(), expected_filename))
        .exists());

    core.set_mock_partclone(
        "dd",
        MockPartcloneState::new()
            .complete(0.5634)
            .rate("0.01GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(0.5634));
    let expected_estimated_finished_time = Utc::now() + mock_duration;

    assert_eq!(msg["id"].as_str(), id);
    assert_eq!(msg["rate"].as_str(), Some("0.01GB/min"));

    let estimated_finish = msg["estimated_finish"]
        .as_str()
        .expect("missing estimated_finish");
    let estimated_finish_time: DateTime<Utc> =
        estimated_finish.parse().expect("!parse estimated_finish");

    let finish_time_diff =
        estimated_finish_time.signed_duration_since(expected_estimated_finished_time);
    if abs(finish_time_diff) > OldDuration::seconds(1) {
        assert_eq!(
            estimated_finish_time, expected_estimated_finished_time,
            "expected within a second"
        );
    }
    assert_eq!(msg["start"].as_str(), start);
    assert_eq!(msg["finish"].as_str(), None);

    assert!(!core
        .path_of(&format!("{}/{}", core.tmp_dir(), expected_filename))
        .exists());

    core.set_mock_partclone(
        "dd",
        MockPartcloneState::new().complete(1.0).rate("12.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
    assert_eq!(msg["id"].as_str(), id);
    assert_eq!(msg["start"].as_str(), start);
    assert!(msg["finish"].as_str().is_some(), "missing clone.finish");
    assert!(
        msg["image_size"].as_i64().is_some(),
        "missing clone.image_size"
    );

    let output = core
        .get_tmp_file_contents_bytes(&expected_filename)
        .expect("!read $tmp_dir/do_clone_job.apt.gz");
    assert_eq!(
        decompress_gz(&output).expect("!decompress"),
        "mock-partition-/dev/sda5-data"
    );

    assert!(
        core.tmp_file_contents_is_1(".latest.finished.mockpcl.dd.txt"),
        "partclone didn't finish"
    );
}

fn abs(duration: OldDuration) -> OldDuration {
    if duration < OldDuration::zero() {
        return duration * -1;
    }
    duration
}

use flate2::read::GzDecoder;
use std::io::{Read, Result};

fn decompress_gz(zipped: &[u8]) -> Result<String> {
    let mut d = GzDecoder::new(zipped);
    let mut s = String::new();
    d.read_to_string(&mut s)?;
    Ok(s)
}

#[test]
fn clone_using_partclone_fstype_variant_f2fs() {
    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sdb3\n\
         destination: {destination}\n\
         name: f2fs_job",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);
    let expected_filename = format!(
        "f2fs_job-{}.apt.f2fs.gz",
        Local::now().format("%Y-%m-%dT%H%M")
    );
    core.set_mock_partclone(
        "f2fs",
        MockPartcloneState::new().complete(1.0).rate("1.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
    assert_eq!(msg["source"].as_str(), Some("/dev/sdb3"));
    assert_eq!(
        msg["destination"].as_str(),
        Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref())
    );
    assert_eq!(
        core.get_tmp_file_contents_utf8(".latest.s.mockpcl.f2fs.txt")
            .expect("!last source"),
        "/dev/sdb3"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.c.mockpcl.f2fs.txt"),
        "partclone.f2fs not invoked with '-c'"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.finished.mockpcl.f2fs.txt"),
        "partclone didn't finish"
    );
}

#[test]
fn clone_using_partclone_fstype_variant_ext2() {
    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sdb1\n\
         destination: {destination}\n\
         name: ext2_job",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);
    let expected_filename = format!(
        "ext2_job-{}.apt.ext2.gz",
        Local::now().format("%Y-%m-%dT%H%M")
    );

    core.set_mock_partclone(
        "ext2",
        MockPartcloneState::new().complete(1.0).rate("1.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
    assert_eq!(msg["source"].as_str(), Some("/dev/sdb1"));
    assert_eq!(
        msg["destination"].as_str(),
        Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref())
    );
    assert_eq!(
        core.get_tmp_file_contents_utf8(".latest.s.mockpcl.ext2.txt")
            .expect("!last source"),
        "/dev/sdb1"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.c.mockpcl.ext2.txt"),
        "partclone.ext2 not invoked with '-c'"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.finished.mockpcl.ext2.txt"),
        "partclone didn't finish"
    );
}

#[test]
fn clone_and_compress_with_zstd() {
    let _ = env_logger::try_init();

    if Command::new("zstdmt")
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .is_err()
    {
        warn!("Can't test zstd as `zstdmt` is not installed on this system");
        return;
    }

    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sdb1\n\
         destination: {destination}\n\
         name: zst_job\n\
         compression: zst",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);
    let expected_filename = format!(
        "zst_job-{}.apt.ext2.zst",
        Local::now().format("%Y-%m-%dT%H%M")
    );

    core.set_mock_partclone(
        "ext2",
        MockPartcloneState::new().complete(1.0).rate("1.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
    assert_eq!(msg["source"].as_str(), Some("/dev/sdb1"));
    assert_eq!(
        msg["destination"].as_str(),
        Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref())
    );
    assert_eq!(
        core.get_tmp_file_contents_utf8(".latest.s.mockpcl.ext2.txt")
            .expect("!last source"),
        "/dev/sdb1"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.c.mockpcl.ext2.txt"),
        "partclone.ext2 not invoked with '-c'"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.finished.mockpcl.ext2.txt"),
        "partclone didn't finish"
    );
}

#[test]
fn clone_and_compress_with_lz4() {
    let _ = env_logger::try_init();

    if Command::new("lz4")
        .arg("--version")
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        warn!("Can't test lz4 as `lz4` is not installed on this system");
        return;
    }

    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sdb1\n\
         destination: {destination}\n\
         name: lz4_job\n\
         compression: lz4",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);
    let expected_filename = format!(
        "lz4_job-{}.apt.ext2.lz4",
        Local::now().format("%Y-%m-%dT%H%M")
    );

    core.set_mock_partclone(
        "ext2",
        MockPartcloneState::new().complete(1.0).rate("1.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
    assert_eq!(msg["source"].as_str(), Some("/dev/sdb1"));
    assert_eq!(
        msg["destination"].as_str(),
        Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref())
    );
    assert_eq!(
        core.get_tmp_file_contents_utf8(".latest.s.mockpcl.ext2.txt")
            .expect("!last source"),
        "/dev/sdb1"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.c.mockpcl.ext2.txt"),
        "partclone.ext2 not invoked with '-c'"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.finished.mockpcl.ext2.txt"),
        "partclone didn't finish"
    );
}

#[test]
fn clone_uncompressed_uuid_info() {
    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sdb1\n\
         destination: {destination}\n\
         name: no_z_job\n\
         compression: uncompressed",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);
    let expected_filename = format!(
        "no_z_job-{}.apt.ext2.uncompressed",
        Local::now().format("%Y-%m-%dT%H%M")
    );

    core.set_mock_partclone(
        "ext2",
        MockPartcloneState::new().complete(1.0).rate("1.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(1.0));
    assert_eq!(msg["source"].as_str(), Some("/dev/sdb1"));
    assert_eq!(msg["source_uuid"].as_str(), Some("456-456-456"));
    assert_eq!(
        msg["destination"].as_str(),
        Some(format!("{}/{}", core.tmp_dir(), expected_filename).as_ref())
    );
    assert_eq!(
        core.get_tmp_file_contents_utf8(".latest.s.mockpcl.ext2.txt")
            .expect("!last source"),
        "/dev/sdb1"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.c.mockpcl.ext2.txt"),
        "partclone.ext2 not invoked with '-c'"
    );
    assert!(
        core.tmp_file_contents_is_1(".latest.finished.mockpcl.ext2.txt"),
        "partclone didn't finish"
    );
}

#[test]
fn handle_partclone_rate_output() {
    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sdb1\n\
         destination: {destination}\n\
         name: weird_rate_job",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);
    core.set_mock_partclone(
        "ext2",
        MockPartcloneState::new().complete(0.5).rate("1.23GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() == Some(0.5));
    assert_eq!(msg["rate"].as_str(), Some("1.23GB/min"));
    // Partclone can output the rate with and without the 'Rate: ' prefix
    core.set_mock_partclone(
        "ext2",
        MockPartcloneState::new()
            .complete(0.6)
            .rate("Rate: 2.34GB/min"),
    )
    .expect("!set_mock_partclone");
    let msg = core.expect_message_with(|msg| msg["complete"].as_f64() != Some(0.5));
    assert_eq!(msg["rate"].as_str(), Some("2.34GB/min"));
}

#[test]
fn cancel_clone_job() {
    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sda5\n\
         destination: {destination}\n\
         name: cancel_clone_job",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);

    let msg = core.expect_message_with(|msg| {
        msg["type"].as_str() == Some("clone") && msg["rate"].as_str().is_some()
    });
    let id = msg["id"].as_str();
    let destination = msg["destination"].as_str().unwrap();

    core.set_mock_partclone(
        "dd",
        MockPartcloneState::new()
            .complete(0.7865)
            .rate("9.00GB/min"),
    )
    .expect("!set_mock_partclone");

    let msg = core.expect_message_with(|msg| msg["rate"].as_str() == Some("9.00GB/min"));
    assert_eq!(msg["id"].as_str(), id);

    let inprogress_path = format!("{}.inprogress", destination);
    assert!(Path::new(&inprogress_path).exists());

    let cancel_msg = format!("type: cancel-clone\nid: {id}", id = id.unwrap());
    core.send(&cancel_msg);

    let msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("clone-failed"));
    assert_eq!(msg["id"].as_str(), id);
    assert_eq!(msg["error"].as_str(), Some("Cancelled"));

    assert!(!Path::new(&destination).exists());

    let start = Instant::now();
    loop {
        assert!(
            Instant::now().duration_since(start) < Duration::from_secs(1),
            "*.inprogress file not deleted"
        );
        if !Path::new(&inprogress_path).exists() {
            break;
        }
    }

    assert!(
        !core.tmp_file_contents_is_1(".latest.finished.mockpcl.dd.txt"),
        "partclone not killed"
    );
}

#[test]
fn clone_job_error() {
    let core = CoreHandle::new().unwrap();

    let clone_msg = format!(
        "type: clone\n\
         source: /dev/sda5\n\
         destination: {destination}\n\
         name: clone_job_error",
        destination = core.tmp_dir()
    );
    core.send(&clone_msg);

    let msg = core.expect_message_with(|msg| {
        msg["type"].as_str() == Some("clone") && msg["rate"].as_str().is_some()
    });
    let id = msg["id"].as_str();
    let destination = msg["destination"].as_str().unwrap();

    core.set_mock_partclone(
        "dd",
        MockPartcloneState::new()
            .complete(0.7865)
            .rate("9.00GB/min")
            .error(true),
    )
    .expect("!set_mock_partclone");

    let msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("clone-failed"));
    assert_eq!(msg["id"].as_str(), id);
    assert_eq!(msg["error"].as_str(), Some("Failed"));

    let inprogress_path = format!("{}.inprogress", destination);
    let start = Instant::now();
    loop {
        assert!(
            Instant::now().duration_since(start) < Duration::from_secs(1),
            "*.inprogress file not deleted"
        );
        if !Path::new(&inprogress_path).exists() {
            break;
        }
    }
    assert!(
        !Path::new(&destination).exists(),
        "Image file exists after error"
    );
    assert!(
        !core.tmp_file_contents_is_1(".latest.finished.mockpcl.dd.txt"),
        "partclone not killed"
    );
}

#[test]
fn delete_image() {
    let core = CoreHandle::new().unwrap();
    let image = format!("{}/{}", core.tmp_dir(), "mockimg-2017-04-20T1500.apt.dd.gz");
    assert!(Path::new(&image).exists()); // test setup sanity

    let clone_msg = format!(
        "type: delete-clone\n\
         file: {}",
        image
    );
    core.send(&clone_msg);
    let msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("deleted-clone"));
    assert_eq!(msg["file"].as_str().unwrap(), image);
    assert!(!Path::new(&image).exists(), "image not actually deleted");
}

#[test]
fn delete_image_not_found() {
    let core = CoreHandle::new().unwrap();
    let image = format!(
        "{}/{}",
        core.tmp_dir(),
        "not-here-2017-04-20T1500.apt.dd.gz"
    );
    assert!(!Path::new(&image).exists()); // test setup sanity

    let clone_msg = format!(
        "type: delete-clone\n\
         file: {}",
        image
    );
    core.send(&clone_msg);
    let msg = core.expect_message_with(|msg| msg["type"].as_str() == Some("delete-clone-failed"));
    assert_eq!(msg["file"].as_str().unwrap(), image);
    assert_eq!(msg["error"].as_str(), Some("No such file"));
}
