// #[macro_use] extern crate log;
// extern crate env_logger;
extern crate flate2;
extern crate uuid;
extern crate wait_timeout;
extern crate yaml_rust;
extern crate zmq;
mod coreutil;

use coreutil::CoreHandle;
use std::time::Duration;
use wait_timeout::ChildExt;

macro_rules! assert_partition {
    ($part:expr, $expected:expr) => {{
        assert_eq!($part["name"].as_str(), Some($expected.name));
        assert_eq!($part["size"].as_i64(), Some($expected.size));
        assert_eq!($part["fstype"].as_str(), $expected.fstype);
        assert_eq!($part["label"].as_str(), $expected.label);
        assert_eq!($part["mounted"].as_bool(), Some($expected.mounted));
        assert_eq!($part["uuid"].as_str(), $expected.uuid);
    }};
}

// Tests asserting from a client's perspective

#[test]
fn initial_status_message() {
    let core = CoreHandle::new().unwrap();
    assert_eq!(core.initial_message["type"].as_str(), Some("status"));
    assert_eq!(core.initial_message["status"].as_str(), Some("started"));

    let sda = &core.initial_message["sources"][0];
    assert_eq!(sda["name"].as_str(), Some("sda"));
    assert_eq!(sda["size"].as_i64(), Some(750_156_374_016));

    assert_partition!(
        &sda["parts"][0],
        PartitionExpectation {
            name: "sda1",
            size: 104_857_600,
            fstype: Some("ntfs"),
            label: Some("System Reserved"),
            mounted: false,
            uuid: Some("123-123-123"),
        }
    );
    assert_partition!(
        &sda["parts"][1],
        PartitionExpectation {
            name: "sda2",
            size: 536_766_054_400,
            fstype: Some("ntfs"),
            label: Some("SSD"),
            mounted: false,
            uuid: Some("234-234-234"),
        }
    );
    assert_partition!(
        &sda["parts"][2],
        PartitionExpectation {
            name: "sda3",
            size: 181_070_200_832,
            fstype: Some("ext4"),
            label: Some("Arch"),
            mounted: true,
            uuid: Some("345-345-345"),
        }
    );
    assert_partition!(
        &sda["parts"][3],
        PartitionExpectation {
            name: "sda4",
            size: 1024,
            fstype: None,
            label: None,
            mounted: false,
            uuid: None,
        }
    );
    assert_partition!(
        &sda["parts"][4],
        PartitionExpectation {
            name: "sda5",
            size: 32_212_254_720,
            fstype: None,
            label: None,
            mounted: false,
            uuid: None,
        }
    );

    let sdb = &core.initial_message["sources"][1];
    assert_eq!(sdb["name"].as_str(), Some("sdb"));
    assert_eq!(sdb["size"].as_i64(), Some(62_109_253_632));

    assert_partition!(
        &sdb["parts"][0],
        PartitionExpectation {
            name: "sdb1",
            size: 524_288_000,
            fstype: Some("ext2"),
            label: Some("boot"),
            mounted: false,
            uuid: Some("456-456-456"),
        }
    );
    assert_partition!(
        &sdb["parts"][1],
        PartitionExpectation {
            name: "sdb2",
            size: 2_147_483_648,
            fstype: Some("swap"),
            label: Some("swap"),
            mounted: false,
            uuid: Some("567-567-567"),
        }
    );
    assert_partition!(
        &sdb["parts"][2],
        PartitionExpectation {
            name: "sdb3",
            size: 59_436_433_408,
            fstype: Some("f2fs"),
            label: Some("main"),
            mounted: false,
            uuid: Some("678-678-678"),
        }
    );

    let compression_options = &core.initial_message["compression_options"];
    assert_eq!(compression_options[0].as_str(), Some("gz"));
}

#[test]
fn status_request() {
    let core = CoreHandle::new().unwrap();

    core.send("type: status-request");
    let message = core.expect_message_with(|msg| msg["type"].as_str() == Some("status"));
    assert_eq!(message["status"].as_str(), Some("running"));

    let sda = &core.initial_message["sources"][0];
    assert_eq!(sda["name"].as_str(), Some("sda"));
    assert_eq!(sda["size"].as_i64(), Some(750_156_374_016));

    assert_partition!(
        &sda["parts"][4],
        PartitionExpectation {
            name: "sda5",
            size: 32_212_254_720,
            fstype: None,
            label: None,
            mounted: false,
            uuid: None,
        }
    );
}

#[test]
fn kill_request() {
    let mut core = CoreHandle::new().unwrap();
    core.send("type: kill-request");

    let message = core.expect_message_with(|msg| msg["type"].as_str() == Some("status"));
    assert_eq!(message["status"].as_str(), Some("dying"));

    match core.process.wait_timeout(Duration::from_secs(2)).unwrap() {
        Some(status) => assert!(status.success()),
        None => assert!(false, "process did not stop"),
    }
}

#[derive(Debug, Clone, Copy)]
struct PartitionExpectation {
    name: &'static str,
    size: i64,
    fstype: Option<&'static str>,
    label: Option<&'static str>,
    mounted: bool,
    uuid: Option<&'static str>,
}
