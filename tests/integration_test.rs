use std::fs;
use std::net::{TcpStream as StdTcpStream, TcpListener as StdTcpListener};
use std::io::Read;
use std::thread;
use std::time::Duration;
use blackhole::{read_responses, Args};

#[tokio::test]
async fn test_server_response() {
    // Setup: create a temp responses dir and file
    let _ = fs::create_dir("test_responses");
    fs::write("test_responses/test.txt", "test response").unwrap();

    // Find a free port
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener); // Release the port for the async server

    // Prepare Args
    let args = Args {
        min_port: None,
        max_port: None,
        port_list: vec![port],
    };
    let responses = read_responses("test_responses").unwrap();
    let responses = std::sync::Arc::new(responses);
    let ip_counter = std::sync::Arc::new(dashmap::DashMap::new());
    let last_request_time_by_ip = std::sync::Arc::new(dashmap::DashMap::new());

    // Start server in background
    println!("About to spawn server...");
    let server = tokio::spawn(blackhole::run_server(
        args,
        ip_counter.clone(),
        last_request_time_by_ip.clone(),
        responses.clone(),
        1,      // rotation_secs: 1 second for fast test
        true,   // test_mode: disables ctrl_c and only runs one rotation
    ));
    println!("Server spawned, waiting for it to start...");
    thread::sleep(Duration::from_millis(500)); // Give the server time to start
    println!("Now trying to connect...");

    // Try to connect for up to 2 seconds
    let mut last_err = None;
    for attempt in 0..20 {
        println!("Connection attempt {}", attempt + 1);
        match StdTcpStream::connect(format!("127.0.0.1:{}", port)) {
            Ok(mut stream) => {
                println!("Connected successfully!");
                let mut buf = String::new();
                stream.read_to_string(&mut buf).unwrap();
                assert_eq!(buf, "test response");
                // Cleanup
                let _ = fs::remove_file("test_responses/test.txt");
                let _ = fs::remove_dir("test_responses");
                // Keep the server alive until the test completes
                let _ = server.await;
                return;
            }
            Err(e) => {
                println!("Connection attempt {} failed: {:?}", attempt + 1, e);
                last_err = Some(e);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    // If we get here, the connection failed
    let _ = server.await; // Still await the server to avoid runtime shutdown issues
    panic!("Could not connect to server on port {}: {:?}", port, last_err);
} 