use clap::Parser;

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
        Commands::Serve(_) => {
            eprintln!(
                "{}",
                render_error(
                    &ilearned::AppError::InvalidInput("serve mode arrives in Task 6".to_string()),
                    cli.json
                )
            );
            std::process::exit(2);
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
