extern crate zmq;

use inbound::Request;
use inbound::Request::*;
use clone::{CloneJob, JobStatus};
use outbound::*;
use std::error::Error;
use std::thread;
use std::collections::HashMap;
use std::time::Duration;
use lsblk;

pub struct Server {
  socket: zmq::Socket,
  jobs: HashMap<String, CloneJob>
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
      jobs: HashMap::new()
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
                  self.jobs.insert(job.id().to_owned(), job);
                },
                Err(err) => error!("Clonejob creation failed: {}", err)
              }
            },
            Some(CancelCloneRequest { ref id }) => if let Some(job) = self.jobs.remove(id) {
              self.zmq_send(&job.fail_status("Cancelled").to_yaml())?;
            }
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
      for (id, job) in &self.jobs {
        match job.rx.try_recv() {
          Ok(status) => {
            self.zmq_send(&status.to_yaml())?;
            if let JobStatus::Finished {..} = status {
              finished_job_ids.push(id.to_owned());
            }
            did_work = true;
          }
          _ => ()
        }
      }

      for id in &finished_job_ids { // allow CloneJob Drop to cleanup resources
        self.jobs.remove(id);
      }

      if !did_work { // go easy on cpu when there doesn't seem like much to do
        thread::sleep(Duration::from_millis(10));
      }
    }
  }
}
