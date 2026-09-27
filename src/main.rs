use clap::Parser;
use tokio::runtime::Builder;

use ilearned::application::MemoryService;
use ilearned::config::Config;
use ilearned::domain::lifecycle::LifecycleConfig;
use ilearned::storage::SqliteRepo;
use ilearned::surfaces::cli::{exit_code, render_error, run_cli, Cli, Commands};

fn build_service(db: &std::path::Path) -> Result<MemoryService<SqliteRepo>, ilearned::AppError> {
    let repo = SqliteRepo::open(db)?;
    Ok(MemoryService::new(repo, LifecycleConfig::default()))
}

fn main() {
    let cli = Cli::parse();
    let cfg = Config::load(cli.db.clone(), cli.bind, None).unwrap_or_else(|e| {
        eprintln!("{}", render_error(&e, cli.json));
        std::process::exit(exit_code(&e));
    });
    match &cli.command {
        Commands::Serve(a) => {
            let svc = build_service(&cfg.db_path).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            let bind = a.bind.or(cli.bind).unwrap_or(cfg.bind);
            let rt = Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async {
                if let Err(e) = ilearned::surfaces::http::serve(svc, bind).await {
                    eprintln!("{}", render_error(&e, cli.json));
                    std::process::exit(exit_code(&e));
                }
            });
        }
        cmd => {
            let svc = build_service(&cfg.db_path).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            match run_cli(&svc, cmd, cli.json) {
                Ok(out) => {
                    println!("{out}");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("{}", render_error(&e, cli.json));
                    std::process::exit(exit_code(&e));
                }
            }
        }
    }
}
