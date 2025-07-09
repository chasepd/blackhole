use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;
use rand::seq::SliceRandom;
use rand::Rng;
use clap::Parser;
use std::sync::Arc;
use std::collections::HashSet;
use dashmap::DashMap;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep as async_sleep;
use tokio::sync::broadcast;
use tokio::sync::oneshot;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short = 'm', long)]
    pub min_port: Option<u16>,
    #[arg(short = 'M', long)]
    pub max_port: Option<u16>,
    #[arg(short = 'l', long, value_delimiter = ',')]
    pub port_list: Vec<u16>,
}

pub type SharedMap<T> = Arc<DashMap<String, T>>;

pub fn read_responses<P: AsRef<Path>>(dir: P) -> Result<Vec<Arc<str>>> {
    let mut responses = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let content = fs::read_to_string(&path)?;
            responses.push(Arc::from(content.into_boxed_str()));
        }
    }
    if responses.is_empty() {
        return Err(anyhow!("No response files found in the directory."));
    }
    Ok(responses)
}

pub fn select_ports(args: &Args) -> Vec<u16> {
    if !args.port_list.is_empty() {
        let mut rng = rand::thread_rng();
        let mut ports = args.port_list.clone();
        ports.shuffle(&mut rng);
        ports.into_iter().take(10).collect()
    } else if let (Some(min), Some(max)) = (args.min_port, args.max_port) {
        if min > max {
            panic!("min_port must be less than or equal to max_port");
        }
        let mut rng = rand::thread_rng();
        let mut ports = Vec::new();
        let mut used_ports = HashSet::new();
        while used_ports.len() < 10 {
            let port = rng.gen_range(min..=max);
            if used_ports.insert(port) {
                ports.push(port);
            }
        }
        ports
    } else {
        panic!("Invalid port specification; must provide either port list or min/max");
    }
}

pub async fn handle_client(
    mut stream: TcpStream,
    peer_ip: String,
    ip_counter: SharedMap<u32>,
    last_request_time_by_ip: SharedMap<Instant>,
    responses: Arc<Vec<Arc<str>>>,
) -> Result<()> {
    // Rate-based delay logic
    let now = Instant::now();
    let last_time = last_request_time_by_ip.get(&peer_ip).map(|t| *t);
    if let Some(last) = last_time {
        let elapsed = now.duration_since(last).as_secs();
        let decay = (elapsed / 20) as u32;
        if decay > 0 {
            let mut entry = ip_counter.entry(peer_ip.clone()).or_insert(0);
            *entry = entry.saturating_sub(decay);
        }
    }
    last_request_time_by_ip.insert(peer_ip.clone(), now);
    let mut entry = ip_counter.entry(peer_ip.clone()).or_insert(0);
    let delay = *entry;
    *entry = (*entry + 1).min(10);
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

pub async fn run_server(
    args: Args,
    ip_counter: SharedMap<u32>,
    last_request_time_by_ip: SharedMap<Instant>,
    responses: Arc<Vec<Arc<str>>>,
    rotation_secs: u64,
    test_mode: bool,
    shutdown_rx: Option<oneshot::Receiver<()>>,
) -> Result<()> {
    println!("run_server function called!");
    loop {
        println!("Starting server loop...");
        let ports = select_ports(&args);
        println!("Attempting to bind to ports: {:?}", ports);
        let mut listeners = Vec::new();
        for port in &ports {
            match TcpListener::bind(("0.0.0.0", *port)).await {
                Ok(listener) => {
                    println!("Successfully bound to port {}", port);
                    listeners.push(listener)
                },
                Err(e) => println!("Failed to bind port {}: {}", port, e),
            }
        }
        if listeners.is_empty() {
            println!("No ports were successfully bound!");
            return Err(anyhow!("Failed to bind to any ports"));
        }
        println!("Server is now listening on {} ports", listeners.len());
        let ip_counter = ip_counter.clone();
        let last_request_time_by_ip = last_request_time_by_ip.clone();
        let responses = responses.clone();
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let mut tasks = Vec::new();
        for listener in listeners {
            let ip_counter = ip_counter.clone();
            let last_request_time_by_ip = last_request_time_by_ip.clone();
            let responses = responses.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();
            let task = tokio::spawn(async move {
                println!("Starting accept loop for listener");
                loop {
                    tokio::select! {
                        biased;
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                        res = listener.accept() => {
                            match res {
                                Ok((stream, addr)) => {
                                    println!("Connection accepted from {}", addr.ip());
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
                    }
                }
            });
            tasks.push(task);
        }
        if test_mode {
            if let Some(rx) = shutdown_rx {
                tokio::select! {
                    _ = async_sleep(Duration::from_secs(rotation_secs)) => {
                        let _ = shutdown_tx.send(());
                    }
                    _ = rx => {
                        println!("Test signaled shutdown");
                        let _ = shutdown_tx.send(());
                    }
                }
            } else {
                async_sleep(Duration::from_secs(rotation_secs)).await;
                let _ = shutdown_tx.send(());
            }
        } else {
            tokio::select! {
                _ = async_sleep(Duration::from_secs(rotation_secs)) => {
                    let _ = shutdown_tx.send(());
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("Ctrl+C received, shutting down.");
                    let _ = shutdown_tx.send(());
                    for task in tasks {
                        let _ = task.await;
                    }
                    break;
                }
            }
        }
        for task in tasks {
            let _ = task.await;
        }
        if test_mode {
            break;
        }
    }
    Ok(())
} 