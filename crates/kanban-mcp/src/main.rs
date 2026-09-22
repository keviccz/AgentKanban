use std::io;

fn main() {
    if let Err(error) = run() {
        eprintln!("AgentKanban MCP: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if args == ["--version"] {
            println!("agentkanban-mcp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        if args == ["--help"] || args == ["-h"] {
            println!("AgentKanban MCP {}\n\nUsage: agentkanban-mcp\n\nLocal stdio MCP server. Data: %LOCALAPPDATA%\\AgentKanban\\agentkanban.sqlite3\nOverride the data directory with AGENTKANBAN_DATA_DIR.", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        return Err("unknown argument; use --help".into());
    }
    let db = kanban_core::Database::open_default()?;
    agentkanban_mcp::serve(db, io::stdin().lock(), io::stdout().lock())?;
    Ok(())
}
