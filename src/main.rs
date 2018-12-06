extern crate chrono;
extern crate env_logger;
extern crate json;
#[macro_use]
extern crate log;
extern crate regex;
extern crate uuid;
extern crate yaml_rust;
extern crate zmq;

mod asynchronous;
mod child;
mod clone;
mod compression;
mod inbound;
mod lsblk;
mod outbound;
mod partclone;
mod restore;
mod server;

use crate::server::Server;
use std::alloc::System;

#[global_allocator]
static GLOBAL: System = System;

fn main() {
    env_logger::init();

    match std::env::args().take(2).last() {
        Some(arg) => {
            if arg.starts_with("ipc://") {
                if let Err(err) = Server::start_at(&arg) {
                    error!("Core failed: {}", err);
                }
            } else {
                print_help();
            }
        }
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
