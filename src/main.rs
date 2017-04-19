#[macro_use]
extern crate log;
extern crate json;
extern crate zmq;
extern crate yaml_rust;
extern crate wait_timeout;
extern crate env_logger;
extern crate regex;
extern crate chrono;
mod server;
mod inbound;
mod outbound;
mod clone;
mod lsblk;

use server::Server;

fn main() {
  env_logger::init().unwrap();

  match std::env::args().take(2).last() {
    Some(arg) => {
      if arg.starts_with("ipc://") {
        Server::start_at(&arg).expect("Server failed");
      }
      else {
        print_help();
      }
    }
    _ => print_help()
  }
}

fn print_help() {
  println!("{}\n{}\n\n{}\n{}",
    "Apart-core",
    "  usage: apart-core IPC_ADDRESS",
    "  ENV VAR 'PARTCLONE_CMD': override the partclone command location",
    "  ENV VAR 'LSBLK_CMD': override the lsblk command location");
  std::process::exit(1);
}
