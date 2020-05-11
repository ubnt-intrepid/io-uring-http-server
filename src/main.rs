mod io_uring;
mod server;

use crate::server::Server;

fn main() -> anyhow::Result<()> {
    let addr = "127.0.0.1:8000";
    println!("Listening on {}", addr);
    let mut server = Server::bind(addr)?;
    server.run_loop()?;
    Ok(())
}
