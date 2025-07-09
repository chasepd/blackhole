use tokio::net::{TcpListener, TcpStream};
use dashmap::DashMap;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;
use tokio::time::sleep as async_sleep;
use blackhole::{read_responses, select_ports, Args, run_server};
use clap::Parser;
use tokio::sync::broadcast;

type SharedMap<T> = Arc<DashMap<String, T>>;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let responses = read_responses("responses")?;
    let responses = Arc::new(responses);
    let ip_counter: SharedMap<u32> = Arc::new(DashMap::new());
    let last_request_time_by_ip: SharedMap<Instant> = Arc::new(DashMap::new());
    println!("Loaded {} fake responses.", responses.len());
    run_server(args, ip_counter, last_request_time_by_ip, responses, 5, false).await?;
    Ok(())
}
