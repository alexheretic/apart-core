extern crate zmq;

use inbound::Request;
use inbound::Request::*;
use clone;
use clone::{CloneJob, CloneStatus};
use restore::*;
use outbound::*;
use std::error::Error;
use std::thread;
use std::mem;
use std::collections::HashMap;
use std::fs;
use std::sync::mpsc::{Sender, Receiver, channel};
use std::io::{Result as IoResult};
use lsblk;

pub struct Server {
  socket: zmq::Socket,
  clones: HashMap<String, CloneJob>,
  restores: HashMap<String, RestoreJob>,
  io_receiver: Receiver<IoResult<String>>,
  io_master_sender: Sender<IoResult<String>>
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

    let (io_master_sender, io_receiver) = channel();
    let mut server = Server {
      socket,
      clones: HashMap::new(),
      restores: HashMap::new(),
      io_receiver,
      io_master_sender
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
            Some(DeleteImageRequest { file }) => {
              if clone::is_valid_image_name(&file) {
                let tx = self.io_master_sender.clone();
                thread::spawn(move|| if let Err(err) = tx.send(fs::remove_file(&file).map(|_| file)) {
                  debug!("Could not send, shutting down?: {}", err);
                });
              }
              else { warn!("Invalid image file for deletion: {}", file); }
            },
            _ => warn!("Unhandled inbound message:\n{}", msg),
          };
          true
        },
        Ok(Err(_)) => {
          warn!("Invalid string zmq message received, ignoring");
          true
        },
        // EAGAIN no message waiting / within timeout, EINTR inturrupted while waited
        Err(zmq::Error::EAGAIN) | Err(zmq::Error::EINTR) => false,
        Err(x) => {
          error!("Unexpected error calling server.socket.recv_string(0): {}", x);
          return Err(Box::new(x));
        }
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
        if let Ok(status) = job.try_recv() {
          self.zmq_send(&status.to_yaml())?;
          if let RestoreStatus::Finished {..} = status {
            finished_job_ids.push(id.to_owned());
          }
          did_work = true;
        }
      }
      for id in &finished_job_ids { // allow RestoreJob Drop to cleanup resources
        self.restores.remove(id);
      }

      if let Ok(result) = self.io_receiver.try_recv() {
        match result {
          Ok(file) => self.zmq_send(&deleted_clone_yaml(&file))?,
          Err(err) => error!("Deleting failed: {}", err)
        }
        did_work = true
      }

      if did_work { self.socket.set_rcvtimeo(0)?; }
      else { // go easy on cpu when there doesn't seem like much to do
        self.socket.set_rcvtimeo(10)?;
      }
    }
  }
}
