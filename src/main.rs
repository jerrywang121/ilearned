use clap::Parser;
use tokio::runtime::Builder;

use ilearned::application::MemoryService;
use ilearned::config::{Config, EmbeddingConfig};
use ilearned::embedding::openai::OpenAiEmbeddingProvider;
use ilearned::storage::SqliteRepo;
use ilearned::surfaces::cli::{exit_code, render_error, run_cli, Cli, Commands};

fn build_service(cfg: &Config) -> Result<MemoryService<SqliteRepo>, ilearned::AppError> {
    let repo = SqliteRepo::open(&cfg.db_path)?;
    let svc = MemoryService::new(repo, cfg.lifecycle.clone());
    match cfg.embedding.clone() {
        Some(ec) => Ok(svc.with_embedding_provider(OpenAiEmbeddingProvider::new(&ec))),
        None => Ok(svc),
    }
}

fn main() {
    let cli = Cli::parse();
    let embedding = EmbeddingConfig::from_parts(
        cli.embed_endpoint.clone(),
        cli.embed_model.clone(),
        cli.embed_api_key.clone(),
        cli.embed_dims,
        cli.embed_timeout_secs,
    )
    .or_else(EmbeddingConfig::from_env);
    let cfg = Config::load(
        cli.db.clone(),
        cli.bind,
        cli.active_days,
        cli.forget_days,
        cli.retention_days,
        embedding,
    )
    .unwrap_or_else(|e| {
        eprintln!("{}", render_error(&e, cli.json));
        std::process::exit(exit_code(&e));
    });
    match &cli.command {
        Commands::Serve(a) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
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
            let svc = build_service(&cfg).unwrap_or_else(|e| {
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
