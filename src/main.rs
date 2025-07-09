use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use dashmap::DashMap;
use clap::Parser;
use rand::seq::SliceRandom;
use rand::Rng;
use std::sync::Arc;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use anyhow::Result;
use tokio::time::sleep as async_sleep;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Minimum port to listen on
    #[arg(short = 'm', long)]
    min_port: Option<u16>,
    /// Maximum port to listen on
    #[arg(short = 'M', long)]
    max_port: Option<u16>,
    /// List of ports to listen on
    #[arg(short = 'l', long)]
    port_list: Vec<u16>,
}

type SharedMap<T> = Arc<DashMap<String, T>>;

// Helper to select 10 ports
fn select_ports(args: &Args) -> Vec<u16> {
    if !args.port_list.is_empty() {
        let mut rng = rand::thread_rng();
        let mut ports = args.port_list.clone();
        ports.shuffle(&mut rng);
        ports.into_iter().take(10).collect()
    } else if let (Some(min), Some(max)) = (args.min_port, args.max_port) {
        let mut rng = rand::thread_rng();
        let mut ports = Vec::new();
        while ports.len() < 10 {
            let port = rng.gen_range(min..=max);
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
        ports
    } else {
        panic!("Invalid port specification; must provide either port list or min/max");
    }
}

async fn handle_client(
    mut stream: TcpStream,
    peer_ip: String,
    ip_counter: SharedMap<u32>,
    last_request_time_by_ip: SharedMap<Instant>,
    responses: Arc<Vec<String>>,
) -> Result<()> {
    // Rate-based delay logic
    let now = Instant::now();
    let last_time = last_request_time_by_ip.get(&peer_ip).map(|t| *t);
    let mut delay = ip_counter.get(&peer_ip).map(|v| *v).unwrap_or(0);
    if let Some(last) = last_time {
        let elapsed = now.duration_since(last).as_secs();
        let decay = (elapsed / 20) as u32;
        if decay > 0 {
            let mut entry = ip_counter.entry(peer_ip.clone()).or_insert(0);
            *entry = entry.saturating_sub(decay);
            delay = *entry;
        }
    }
    last_request_time_by_ip.insert(peer_ip.clone(), now);
    let mut entry = ip_counter.entry(peer_ip.clone()).or_insert(0);
    *entry = (*entry + 1).min(10);
    delay = *entry;
    if delay > 0 {
        async_sleep(Duration::from_secs(delay as u64)).await;
    }
    // Send a random response
    let resp = {
        let responses = &*responses;
        let idx = rand::thread_rng().gen_range(0..responses.len());
        responses[idx].clone()
    };
    tokio::io::AsyncWriteExt::write_all(&mut stream, resp.as_bytes()).await?;
    Ok(())
}

async fn run_server(
    args: Args,
    ip_counter: SharedMap<u32>,
    last_request_time_by_ip: SharedMap<Instant>,
    responses: Arc<Vec<String>>,
) -> Result<()> {
    loop {
        let ports = select_ports(&args);
        println!("Listening on ports: {:?}", ports);
        let mut listeners = Vec::new();
        for port in &ports {
            match TcpListener::bind(("0.0.0.0", *port)).await {
                Ok(listener) => listeners.push(listener),
                Err(e) => println!("Failed to bind port {}: {}", port, e),
            }
        }
        let ip_counter = ip_counter.clone();
        let last_request_time_by_ip = last_request_time_by_ip.clone();
        let responses = responses.clone();
        let server_task = tokio::spawn(async move {
            let mut tasks = Vec::new();
            for listener in listeners {
                let ip_counter = ip_counter.clone();
                let last_request_time_by_ip = last_request_time_by_ip.clone();
                let responses = responses.clone();
                let task = tokio::spawn(async move {
                    loop {
                        match listener.accept().await {
                            Ok((stream, addr)) => {
                                let peer_ip = addr.ip().to_string();
                                let ip_counter = ip_counter.clone();
                                let last_request_time_by_ip = last_request_time_by_ip.clone();
                                let responses = responses.clone();
                                tokio::spawn(async move {
                                    let _ = handle_client(stream, peer_ip, ip_counter, last_request_time_by_ip, responses).await;
                                });
                            }
                            Err(_) => break, // Listener closed
                        }
                    }
                });
                tasks.push(task);
            }
            // Wait for 5 seconds, then drop listeners
            async_sleep(Duration::from_secs(5)).await;
            // Listeners will be dropped here, closing sockets
        });
        server_task.await?;
    }
}

fn read_responses<P: AsRef<Path>>(dir: P) -> Result<Vec<String>> {
    let mut responses = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let content = fs::read_to_string(&path)?;
            responses.push(content);
        }
    }
    Ok(responses)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let responses = read_responses("responses")?;
    let responses = Arc::new(responses);
    let ip_counter: SharedMap<u32> = Arc::new(DashMap::new());
    let last_request_time_by_ip: SharedMap<Instant> = Arc::new(DashMap::new());
    println!("Loaded {} fake responses.", responses.len());
    run_server(args, ip_counter, last_request_time_by_ip, responses).await?;
    Ok(())
}
