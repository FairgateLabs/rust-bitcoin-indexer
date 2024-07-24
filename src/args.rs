use clap::{command, Parser};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]

pub struct Args {
    /// Database Bitcoin indexer file path
    #[arg(long, short = 'd')]
    pub db_file_path: Option<String>,

    /// Bitcoin node rpc url
    #[arg(long, short = 'n')]
    pub node_rpc_url: Option<String>,

    /// Bitcoin height to start indexing from
    #[arg(long, short = 'c')]
    pub checkpoint_height: Option<u32>,
}
