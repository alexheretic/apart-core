extern crate flate2;
extern crate uuid;
extern crate zmq;
extern crate yaml_rust;
extern crate wait_timeout;

use yaml_rust::{YamlLoader,Yaml};
use std::process::{Command, Child};
use std::io::{ErrorKind, Error, Result};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;
use std::fs;
use std::path::{PathBuf, Path};
use std::process::Stdio;
use std::io::{Read, Write};
use flate2::read::GzDecoder;

pub fn decompress(zipped: &[u8]) -> Result<String> {
  let mut d = GzDecoder::new(zipped)?;
  let mut s = String::new();
  d.read_to_string(&mut s)?;
  Ok(s)
}

pub struct CoreHandle {
  pub process: Child,
  pub initial_message: Yaml,
  tmp_dir: TmpDir,
  pub socket: zmq::Socket
}

impl Drop for CoreHandle {
  // clean up started binary
  fn drop(&mut self) {
    match self.process.wait_timeout(Duration::from_secs(0)) {
      Ok(None) => {
        println!("sending kill message to apart-core...");
        // try to send kill message, ignore errors
        match self.socket.send_str("type: kill-request", 0) {
          Ok(_) => match self.socket.recv_string(0) { _ => () },
          _ => ()
        };
      }
      _ => ()
    }

    match self.process.wait_timeout(Duration::from_secs(0)) {
      Err(err) => match err.raw_os_error() {
        Some(10) => return, // already dead
        _ => println!("ERROR: {}", err) // unknown error
      },
      // process won't end on it's own, so kill
      Ok(None) => if let Err(err) = self.process.kill() {
        println!("ERROR: {}", err)
      },
      // process has ended
      Ok(Some(_)) => return
    }
  }
}

fn expect_message_from(socket: &zmq::Socket) -> Yaml {
  let message_str = socket.recv_string(0).expect("expected to receive server message within 1s").unwrap();
  YamlLoader::load_from_str(&message_str).expect("invalid yaml").remove(0)
}

struct TmpDir {
  dir: String
}

impl TmpDir {
  fn new(uuid: &uuid::Uuid) -> TmpDir {
    let tmp_dir = format!("target/tmp-{}", uuid);
    fs::create_dir_all(&tmp_dir).unwrap();

    for file_path in fs::read_dir("tests/mockpcl").unwrap()
      .filter(|el| el.is_ok())
      .map(|el| el.unwrap().path())
      .filter(|p| p.is_file() && p.file_name().is_some()) {
        fs::copy(&file_path, format!("{}/{}", &tmp_dir, &file_path.file_name().unwrap().to_str().unwrap()))
          .expect("copy failed");
    }

    TmpDir { dir: fs::canonicalize(tmp_dir).unwrap().into_os_string().into_string().unwrap() }
  }

  fn path_of(&self, filename: &str) -> Result<PathBuf> {
    let file = self.dir.to_owned() + "/" + filename;
    let mut control_path = PathBuf::new();
    control_path.push(&file);
    if !control_path.exists() {
      return Err(Error::new(ErrorKind::NotFound, format!("{} not found", file)));
    }
    Ok(control_path)
  }
}

impl Drop for TmpDir {
  fn drop(&mut self) {
    // remove tmp directory
    fs::remove_dir_all(&self.dir).unwrap();
  }
}

pub struct MockPartcloneState {
  pub complete: f64,
  pub rate: String
}

impl CoreHandle {
  pub fn new() -> Result<CoreHandle> {
    let uuid = uuid::Uuid::new_v4();
    let ipc_address = format!("ipc:///tmp/apart-{}.ipc", uuid);
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::PAIR)?;
    socket.bind(&ipc_address)?;

    socket.set_sndtimeo(1000)?;
    socket.set_rcvtimeo(1000)?;
    socket.set_linger(0)?;

    let tmp_dir = TmpDir::new(&uuid);

    let core = Command::new("target/debug/apart-core")
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::inherit())
      .arg(ipc_address.to_string())
      .env("RUST_LOG", "info")
      .env("PARTCLONE_CMD", format!("{}/mockpcl", tmp_dir.dir))
      .env("LSBLK_CMD", format!("{}/mocklsblk", tmp_dir.dir))
      .spawn()?;

    let message = expect_message_from(&socket);
    Ok(CoreHandle {
      process: core,
      socket: socket,
      initial_message: message,
      tmp_dir: tmp_dir
    })
  }

  pub fn expect_message(&self) -> Yaml {
    expect_message_from(&self.socket)
  }


  pub fn expect_message_with<P>(&self, predicate: P) -> Yaml where P: Fn(&Yaml) -> bool {
    let start = Instant::now();
    loop {
      let msg = self.expect_message();
      if predicate(&msg) {
        return msg;
      }
      assert!(Instant::now().duration_since(start) < Duration::from_secs(1),
        "expected message not received within 1 second");
    }
  }

  pub fn send(&self, msg: &str) {
    self.socket.send_str(msg, 0).expect("sending to core failed");
  }

  pub fn set_mock_partclone(&self, MockPartcloneState { complete, rate }: MockPartcloneState) -> Result<()> {
    let mut file = fs::OpenOptions::new()
      .write(true)
      .open(self.tmp_dir.path_of(".control.mockpcl.dd")?)?;
    write!(file, "complete={:.2}\nrate={}", complete * 100., rate)?;
    Ok(())
  }

  pub fn get_tmp_file_contents_bytes(&self, filename: &str) -> Result<Vec<u8>> {
    let mut contents = Vec::new();
    fs::File::open(self.tmp_dir.path_of(filename)?)?.read_to_end(&mut contents)?;
    Ok(contents)
  }

  pub fn get_tmp_file_contents_utf8(&self, filename: &str) -> Result<String> {
    let mut contents = String::new();
    fs::File::open(self.tmp_dir.path_of(filename)?)?.read_to_string(&mut contents)?;
    Ok(contents)
  }

  pub fn get_mock_partclone_last_source_of(&self, variant: &str) -> Result<String> {
    Ok(self.get_tmp_file_contents_utf8(&format!(".latest.s.mockpcl.{}.txt", variant))?.trim().to_owned())
  }

  /// no error if absent
  pub fn path_of(&self, filename: &str) -> Result<PathBuf> {
    let file = self.tmp_dir.dir.to_owned() + "/" + filename;
    let mut path = PathBuf::new();
    path.push(&file);
    Ok(path)
  }

  pub fn tmp_dir(&self) -> &str {
    &self.tmp_dir.dir
  }
}
