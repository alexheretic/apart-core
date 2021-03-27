#![allow(unused)]

extern crate flate2;
extern crate uuid;
extern crate yaml_rust;
extern crate zmq;

use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use yaml_rust::{Yaml, YamlLoader};

pub struct CoreHandle {
    pub process: Child,
    pub initial_message: Yaml,
    tmp_dir: TmpDir,
    pub socket: zmq::Socket,
}

impl Drop for CoreHandle {
    // clean up started binary
    fn drop(&mut self) {
        if let Ok(None) = self.process.try_wait() {
            println!("sending kill message to apart-core...");
            // try to send kill message, ignore errors
            if self.socket.send("type: kill-request", 0).is_ok() {
                let _ = self.socket.recv_string(0);
            };
        }

        match self.process.try_wait() {
            Err(err) => match err.raw_os_error() {
                Some(10) => {}                   // already dead
                _ => println!("ERROR: {}", err), // unknown error
            },
            // process won't end on it's own, so kill
            Ok(None) => {
                if let Err(err) = self.process.kill() {
                    println!("ERROR: {}", err)
                }
            }
            // process has ended
            Ok(Some(_)) => {}
        }
    }
}

fn expect_message_from(socket: &zmq::Socket) -> Yaml {
    let message_str = socket
        .recv_string(0)
        .expect("expected to receive server message within 1s")
        .unwrap();
    println!("Received:\n---\n{}\n---", message_str);
    YamlLoader::load_from_str(&message_str)
        .expect("invalid yaml")
        .remove(0)
}

struct TmpDir {
    dir: String,
}

impl TmpDir {
    fn new(uuid: &uuid::Uuid) -> TmpDir {
        let tmp_dir = format!("target/tmp-{}", uuid);
        fs::create_dir_all(&tmp_dir).unwrap();

        for file_path in fs::read_dir("tests/mockpcl")
            .unwrap()
            .filter(|el| el.is_ok())
            .map(|el| el.unwrap().path())
            .filter(|p| p.is_file() && p.file_name().is_some())
        {
            fs::copy(
                &file_path,
                format!(
                    "{}/{}",
                    &tmp_dir,
                    &file_path.file_name().unwrap().to_str().unwrap()
                ),
            )
            .expect("copy failed");
        }

        TmpDir {
            dir: fs::canonicalize(tmp_dir)
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap(),
        }
    }

    fn existing_path_of(&self, filename: &str) -> Result<PathBuf> {
        let file = self.dir.to_owned() + "/" + filename;
        let mut control_path = PathBuf::new();
        control_path.push(&file);
        if !control_path.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("{} not found", file),
            ));
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
    pub rate: String,
    pub error: bool,
}

impl MockPartcloneState {
    pub fn new() -> MockPartcloneState {
        MockPartcloneState {
            complete: 0.0,
            rate: "1.11GB/min".to_owned(),
            error: false,
        }
    }
    pub fn complete(&mut self, complete: f64) -> &mut MockPartcloneState {
        self.complete = complete;
        self
    }
    pub fn rate(&mut self, rate: &str) -> &mut MockPartcloneState {
        self.rate = rate.to_owned();
        self
    }
    pub fn error(&mut self, error: bool) -> &mut MockPartcloneState {
        self.error = error;
        self
    }
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

        // support running in workspace mode too
        let path = ["target/debug/apart-core", "../target/debug/apart-core"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .expect("`debug/apart-core` not found");

        let core = Command::new(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .arg(ipc_address)
            .env("RUST_LOG", "info")
            .env("APART_PARTCLONE_CMD", format!("{}/mockpcl", tmp_dir.dir))
            .env("APART_LSBLK_CMD", format!("{}/mocklsblk", tmp_dir.dir))
            .spawn()?;

        let message = expect_message_from(&socket);
        Ok(CoreHandle {
            process: core,
            socket,
            initial_message: message,
            tmp_dir,
        })
    }

    pub fn expect_message(&self) -> Yaml {
        expect_message_from(&self.socket)
    }

    pub fn expect_message_with<P>(&self, predicate: P) -> Yaml
    where
        P: Fn(&Yaml) -> bool,
    {
        let start = Instant::now();
        loop {
            let msg = self.expect_message();
            if predicate(&msg) {
                return msg;
            }
            // println!("Ignoring non-matching msg: {:?}", msg);
            assert!(
                Instant::now().duration_since(start) < Duration::from_secs(1),
                "expected message not received within 1 second"
            );
        }
    }

    pub fn send(&self, msg: &str) {
        self.socket.send(msg, 0).expect("sending to core failed");
    }

    pub fn set_mock_partclone(
        &self,
        variant: &str,
        &MockPartcloneState {
            complete,
            ref rate,
            error,
        }: &MockPartcloneState,
    ) -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(self.path_of(&format!(".control.mockpcl.{}", variant)))?;
        write!(
            file,
            "complete={:.2}\n\
             rate=\"{}\"\n\
             error={}",
            complete * 100.,
            rate,
            error
        )?;
        Ok(())
    }

    pub fn get_tmp_file_contents_bytes(&self, filename: &str) -> Result<Vec<u8>> {
        let mut contents = Vec::new();
        fs::File::open(self.tmp_dir.existing_path_of(filename)?)?.read_to_end(&mut contents)?;
        Ok(contents)
    }

    pub fn get_tmp_file_contents_utf8(&self, filename: &str) -> Result<String> {
        let mut contents = String::new();
        fs::File::open(self.tmp_dir.existing_path_of(filename)?)?.read_to_string(&mut contents)?;
        Ok(contents)
    }

    pub fn tmp_file_contents_is_1(&self, filename: &str) -> bool {
        if let Ok(contents) = self.get_tmp_file_contents_utf8(filename) {
            return contents.trim() == "1";
        }
        false
    }

    /// no error if absent
    pub fn path_of(&self, filename: &str) -> PathBuf {
        let file = self.tmp_dir.dir.to_owned() + "/" + filename;
        let mut path = PathBuf::new();
        path.push(&file);
        path
    }

    pub fn tmp_dir(&self) -> &str {
        &self.tmp_dir.dir
    }
}
