extern crate zmq;

use inbound::Request;
use inbound::Request::*;
use clone::{CloneJob, CloneStatus};
use restore::*;
use outbound::*;
use std::error::Error;
use std::thread;
use std::mem;
use std::collections::HashMap;
use std::time::Duration;
use lsblk;

pub struct Server {
  socket: zmq::Socket,
  clones: HashMap<String, CloneJob>,
  restores: HashMap<String, RestoreJob>
}

impl Drop for Server {
  fn drop(&mut self) {
    if self.zmq_send(&status_yaml("dying", Vec::new())).is_err() {
      warn!("Failed to send dying status message");
    }
  }
}

impl Server {
  /// Start up server using an input ipc address for communication with the client
  pub fn start_at(ipc_address: &str) -> Result<(), Box<Error>> {
    let socket = zmq::Context::new().socket(zmq::PAIR)?;
    socket.connect(ipc_address)?;
    socket.set_sndtimeo(1000)?; // block 1s on sends or error
    socket.set_rcvtimeo(0)?; // non-blocking recv (EAGAIN when nothing)
    socket.set_linger(0)?; // don't tolerate disconnection

    let mut server = Server {
      socket: socket,
      clones: HashMap::new(),
      restores: HashMap::new()
    };
    server.zmq_send(&status_yaml("started", lsblk::blockdevices()?))?;
    server.run()
  }

  fn zmq_send(&self, msg: &str) -> Result<(), Box<Error>> {
    match self.socket.send_str(msg, 0) {
      Err(x) => Err(Box::new(x)),
      Ok(x) => Ok(x)
    }
  }

  /// Start the event loop & run until a reason to stop
  fn run(&mut self) -> Result<(), Box<Error>> {
    loop {
      let mut did_work = match self.socket.recv_string(0) {
        Ok(Ok(msg)) => {
          match Request::parse(&msg) {
            None => (),
            Some(StatusRequest) => self.zmq_send(&status_yaml("running", lsblk::blockdevices()?))?,
            Some(KillRequest) => {
              info!("KillRequest received dying...");
              return Ok(())
            },
            Some(CloneRequest { source, destination, name }) => {
              match CloneJob::new(source, destination, name) {
                Ok(job) => {
                  info!("Starting new job: {}", job);
                  self.clones.insert(job.id().to_owned(), job);
                },
                Err(err) => error!("Clonejob creation failed: {}", err)
              }
            },
            Some(RestoreRequest { source, destination }) => {
              match RestoreJob::new(source, destination) {
                Ok(job) => {
                  info!("Starting new job: {}", job);
                  self.restores.insert(job.id().to_owned(), job);
                },
                Err(err) => error!("RestoreJob creation failed: {}", err)
              }
            },
            Some(CancelCloneRequest { id }) => if let Some(job) = self.clones.remove(&id) {
              let cancelled_msg = job.fail_status("Cancelled").to_yaml();
              mem::drop(job); // ensure actually cancelled before messaging
              self.zmq_send(&cancelled_msg)?;
            },
            Some(CancelRestoreRequest { id }) => if let Some(job) = self.restores.remove(&id) {
              let cancelled_msg = job.fail_status("Cancelled").to_yaml();
              mem::drop(job); // ensure actually cancelled before messaging
              self.zmq_send(&cancelled_msg)?;
            },
          };
          true
        },
        Ok(Err(_)) => {
          warn!("Invalid string zmq message received, ignoring");
          true
        },
        Err(zmq::Error::EAGAIN) => false,
        Err(x) => return Err(Box::new(x))
      };

      let mut finished_job_ids = Vec::new();
      for (id, job) in &self.clones {
        match job.try_recv() {
          Ok(status) => {
            self.zmq_send(&status.to_yaml())?;
            if let CloneStatus::Finished {..} = status {
              finished_job_ids.push(id.to_owned());
            }
            did_work = true;
          }
          _ => ()
        }
      }
      for id in &finished_job_ids { // allow CloneJob Drop to cleanup resources
        self.clones.remove(id);
      }

      let mut finished_job_ids = Vec::new();
      for (id, job) in &self.restores {
        match job.try_recv() {
          Ok(status) => {
            self.zmq_send(&status.to_yaml())?;
            if let RestoreStatus::Finished {..} = status {
              finished_job_ids.push(id.to_owned());
            }
            did_work = true;
          }
          _ => ()
        }
      }
      for id in &finished_job_ids { // allow CloneJob Drop to cleanup resources
        self.clones.remove(id);
      }

      if !did_work { // go easy on cpu when there doesn't seem like much to do
        // TODO wait for 10ms on incoming messages instead
        thread::sleep(Duration::from_millis(10));
      }
    }
  }
}
