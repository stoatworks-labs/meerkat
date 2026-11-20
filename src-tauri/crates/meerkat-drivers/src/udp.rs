//! Reading several UDP sockets at once until a deadline — every broadcast
//! driver sends from one socket per interface and hears answers on any of
//! them (or on a shared listener bound to the protocol's reply port).

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

pub struct Inbox {
    rx: mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>,
    tasks: Vec<JoinHandle<()>>,
}

impl Inbox {
    pub fn new(socks: &[Arc<UdpSocket>]) -> Inbox {
        let (tx, rx) = mpsc::unbounded_channel();
        let tasks = socks
            .iter()
            .map(|s| {
                let s = s.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 9000];
                    loop {
                        match s.recv_from(&mut buf).await {
                            Ok((n, from)) => {
                                if tx.send((buf[..n].to_vec(), from)).is_err() {
                                    break;
                                }
                            }
                            // ICMP port-unreachable surfaces as an error on some
                            // platforms; it says nothing about other senders.
                            Err(_) => tokio::task::yield_now().await,
                        }
                    }
                })
            })
            .collect();
        Inbox { rx, tasks }
    }

    /// The next datagram, or None once `deadline` passes.
    pub async fn next_until(&mut self, deadline: Instant) -> Option<(Vec<u8>, SocketAddr)> {
        tokio::time::timeout_at(deadline, self.rx.recv()).await.ok().flatten()
    }
}

impl Drop for Inbox {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}
