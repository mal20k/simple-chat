use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Set the server port to listen on. Defaults to `8080`.
    #[arg(short)]
    port: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let port = if let Some(port) = cli.port {
        port
    } else {
        8080
    };
    let addr = format!("127.0.0.1:{port}");
    simple_chat::server::run(addr).await
}
