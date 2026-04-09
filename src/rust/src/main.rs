//! Binary entry point for patent-mcp-server.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "patent-mcp-server",
    about = "MCP server for fetching and caching patents by ID"
)]
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
    /// Run the NL search planner and print JSON to stdout
    Plan {
        /// Natural language description
        description: String,
        /// Optional date cutoff (ISO YYYY-MM-DD)
        #[arg(long)]
        date_cutoff: Option<String>,
    },
    /// Score and rank patent hits from JSON on stdin, print JSON to stdout
    Rank {
        /// JSON object: {"hits_by_query": {...}, "concepts": [...], "date_cutoff": "..."}
        #[arg(long, default_value = "-")]
        input: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Configure tracing to stderr (stdout is MCP JSON-RPC transport)
    let filter = args
        .log_level
        .parse::<tracing_subscriber::filter::LevelFilter>()
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
        Some(Command::Plan {
            description,
            date_cutoff,
        }) => {
            let planner = patent_mcp::planner::NaturalLanguagePlanner;
            let intent = planner.plan(&description, date_cutoff.as_deref(), None);
            println!("{}", serde_json::to_string(&intent)?);
            Ok(())
        }
        Some(Command::Rank { input }) => {
            let json_str = if input == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            } else {
                input
            };
            let data: serde_json::Value = serde_json::from_str(&json_str)?;
            // Parse hits_by_query, concepts, date_cutoff from JSON
            let mut hits_by_query = std::collections::HashMap::new();
            if let Some(hbq) = data.get("hits_by_query").and_then(|v| v.as_object()) {
                for (query, hits_val) in hbq {
                    let hits: Vec<patent_mcp::ranking::PatentHit> =
                        serde_json::from_value(hits_val.clone()).unwrap_or_default();
                    hits_by_query.insert(query.clone(), hits);
                }
            }
            let concepts: Vec<String> = data
                .get("concepts")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let date_cutoff: Option<String> = data
                .get("date_cutoff")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let intent = patent_mcp::planner::SearchIntent {
                raw_description: String::new(),
                concepts,
                synonyms: std::collections::HashMap::new(),
                exclusions: vec![],
                date_cutoff,
                jurisdictions: vec![],
                query_variants: vec![],
                rationale: String::new(),
            };
            let ranker = patent_mcp::ranking::SearchRanker;
            let scored = ranker.rank(&hits_by_query, &intent);
            println!("{}", serde_json::to_string(&scored)?);
            Ok(())
        }
        None => patent_mcp::server::run_server(config).await,
    }
}
