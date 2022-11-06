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

pub(crate) mod include {
    pub(crate) use log::{debug, error, info, trace, warn};
}

use crate::{include::*, server::Server};
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
        "Apart-core\
        \n  usage: apart-core IPC_ADDRESS\n\
        \n  ENV VAR 'APART_PARTCLONE_CMD': override the partclone command location\
        \n  ENV VAR 'APART_LSBLK_CMD': override the lsblk command location"
    );
    std::process::exit(1);
}
