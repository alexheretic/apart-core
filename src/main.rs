#[macro_use] extern crate log;
extern crate json;
extern crate zmq;
extern crate yaml_rust;
extern crate wait_timeout;
extern crate env_logger;
extern crate regex;
extern crate chrono;
extern crate uuid;

mod server;
mod inbound;
mod outbound;
mod partclone;
mod clone;
mod restore;
mod lsblk;
mod child;
mod compression;

use server::Server;

fn main() {
    env_logger::init().unwrap();

    match std::env::args().take(2).last() {
        Some(arg) => {
            if arg.starts_with("ipc://") {
                if let Err(err) = Server::start_at(&arg) {
                    error!("Core failed: {}", err);
                }
            } else {
                print_help();
            }
        },
        _ => print_help(),
    }
}

fn print_help() {
    println!(
        "{}\n{}\n\n{}\n{}",
        "Apart-core",
        "  usage: apart-core IPC_ADDRESS",
        "  ENV VAR 'APART_PARTCLONE_CMD': override the partclone command location",
        "  ENV VAR 'APART_LSBLK_CMD': override the lsblk command location"
    );
    std::process::exit(1);
}
