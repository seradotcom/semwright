use clap::Parser;
use rmcp::ServiceExt;
use std::path::PathBuf;
#[derive(Parser)]#[command(version,about="Semwright MCP over stdio; broker policy is always authoritative")]
struct Args{#[arg(long,env="SEMWRIGHT_SOCKET")]socket:Option<PathBuf>,#[arg(long)]session_file:Option<PathBuf>}
#[tokio::main]async fn main(){if let Err(error)=run().await{eprintln!("semwright-mcp: {error}");std::process::exit(1);}}
async fn run()->Result<(),Box<dyn std::error::Error>>{let args=Args::parse();let socket=args.socket.map(Ok).unwrap_or_else(semwright_protocol::default_socket)?;let handler=semwright_mcp::Server::new(socket,args.session_file)?;let server=handler.serve(rmcp::transport::stdio()).await?;server.waiting().await?;Ok(())}
