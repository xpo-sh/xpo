use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::{sleep, Instant};

pub async fn wait_for_port(port: u16, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    let addr = format!("127.0.0.1:{}", port);
    eprintln!("  Waiting for localhost:{}...", port);
    loop {
        match TcpStream::connect(&addr).await {
            Ok(_) => {
                eprintln!("  localhost:{} is ready", port);
                return Ok(());
            }
            Err(_) => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "upstream localhost:{} not reachable after {}s",
                        port,
                        timeout.as_secs()
                    )
                    .into());
                }
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[tokio::test]
    async fn wait_for_port_immediate_success() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let result = wait_for_port(port, Duration::from_secs(2)).await;
        assert!(result.is_ok());
        drop(listener);
    }

    #[tokio::test]
    async fn wait_for_port_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let result = wait_for_port(port, Duration::from_secs(1)).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not reachable"));
    }

    #[tokio::test]
    async fn wait_for_port_delayed_start() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        tokio::spawn(async move {
            sleep(Duration::from_millis(800)).await;
            let _listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
            sleep(Duration::from_secs(5)).await;
        });

        let result = wait_for_port(port, Duration::from_secs(3)).await;
        assert!(result.is_ok());
    }
}
