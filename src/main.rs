#![deny(elided_lifetimes_in_paths)]

use env_logger::Env;
use std::env;
use std::net::TcpListener;
// use std::path::PathBuf;
use tokenapi::run;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    // `init` does call `set_logger`, so this is all we need to do.
    // We are falling back to printing all logs at info-level or above
    // if the RUST_LOG environment variable has not been set.
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    let registry_path = String::from(env::var("MAPPINGS").expect("You need to set MAPPINGS"));

    let listen_address = env::var("LISTEN").expect("You need to set LISTEN");
    let listener = TcpListener::bind(listen_address)
        .expect(&format!("Failed to bind to {:?}", env::var("LISTEN")));

    run(listener, registry_path)?.await
}
