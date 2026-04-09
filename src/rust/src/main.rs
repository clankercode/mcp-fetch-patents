//! Binary entry point for patent-mcp-server.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "patent-mcp-server", about = "MCP server for fetching and caching patents by ID")]
struct Args {
    /// Local cache directory (default: ~/.local/share/patent-cache/patents)
    #[arg(long)]
    cache_dir: Option<String>,

    /// Log level: debug, info, warn, error
    #[arg(long, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Canonicalize a patent ID and print JSON to stdout
    Canonicalize {
        /// Patent ID to canonicalize (e.g. US7654321, EP1234567)
        id: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Configure tracing to stderr (stdout is MCP JSON-RPC transport)
    let filter = args.log_level.parse::<tracing_subscriber::filter::LevelFilter>()
        .unwrap_or(tracing_subscriber::filter::LevelFilter::INFO);
    tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_writer(std::io::stderr)
        .init();

    let mut config = patent_mcp::config::load_config()?;
    if let Some(dir) = args.cache_dir {
        config.cache_local_dir = std::path::PathBuf::from(dir);
    }

    match args.command {
        Some(Command::Canonicalize { id }) => {
            let result = patent_mcp::id_canon::canonicalize(&id);
            println!("{}", serde_json::to_string(&result)?);
            Ok(())
        }
        None => patent_mcp::server::run_server(config).await,
    }
}
