use clap::Parser;
use tokio::runtime::Builder;

use ilearned::application::MemoryService;
use ilearned::config::{Config, EmbeddingConfig, FileConfig};
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
    // Config files load first: embedding flags fall back through
    // env, then the merged (local-over-global) file layer.
    let files = FileConfig::load_files().unwrap_or_else(|e| {
        eprintln!("{}", render_error(&e, cli.json));
        std::process::exit(exit_code(&e));
    });
    let file_embedding = files.clone().and_then(|f| f.embedding);
    let embedding = EmbeddingConfig::from_parts_with_files(
        cli.embed_endpoint.clone(),
        cli.embed_model.clone(),
        cli.embed_api_key.clone(),
        cli.embed_dims,
        cli.embed_timeout_secs,
        file_embedding,
    )
    .or_else(EmbeddingConfig::from_env);
    // `mcp` defaults to a project-local store when no explicit db is given:
    // ./.ilearned/ilearned.db (parent dirs are created by open_db).
    // A `db` set in a config file also counts as explicit.
    let db_from_file = files.as_ref().and_then(|f| f.db.clone());
    let db = cli.db.clone().or_else(|| {
        if matches!(&cli.command, Commands::Mcp)
            && std::env::var("ILEARNED_DB").is_err()
            && db_from_file.is_none()
        {
            Some(std::path::PathBuf::from("./.ilearned/ilearned.db"))
        } else {
            None
        }
    });
    let cfg = Config::load_with_files(
        db,
        cli.bind,
        cli.active_days,
        cli.forget_days,
        cli.retention_days,
        embedding,
        files,
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
        Commands::Mcp => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            let rt = Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async {
                if let Err(e) =
                    ilearned::surfaces::http::mcp::serve_stdio(std::sync::Arc::new(svc)).await
                {
                    eprintln!("{}", render_error(&e, cli.json));
                    std::process::exit(exit_code(&e));
                }
            });
        }
        cmd @ (Commands::Add(_)
        | Commands::Search(_)
        | Commands::Modify(_)
        | Commands::Delete(_)
        | Commands::Promote(_)
        | Commands::Downgrade(_)
        | Commands::Topic(_)
        | Commands::Clear(_)) => {
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
        Commands::Export(a) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            let out = svc.export(a.topic.as_deref(), a.deep).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            let mut body = String::new();
            for e in &out {
                match serde_json::to_string(e) {
                    Ok(line) => {
                        body.push_str(&line);
                        body.push('\n');
                    }
                    Err(e) => {
                        eprintln!(
                            "{}",
                            render_error(&ilearned::AppError::Internal(e.to_string()), cli.json)
                        );
                        std::process::exit(4);
                    }
                }
            }
            match &a.file {
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        if !parent.as_os_str().is_empty() {
                            let mk = std::fs::create_dir_all(parent);
                            if let Err(e) = mk {
                                eprintln!(
                                    "{}",
                                    render_error(
                                        &ilearned::AppError::Internal(e.to_string()),
                                        cli.json
                                    )
                                );
                                std::process::exit(4);
                            }
                        }
                    }
                    if let Err(e) = std::fs::write(path, &body) {
                        eprintln!(
                            "{}",
                            render_error(&ilearned::AppError::Internal(e.to_string()), cli.json)
                        );
                        std::process::exit(4);
                    }
                    let msg = if cli.json {
                        serde_json::json!({"exported": out.len()}).to_string()
                    } else {
                        format!("exported {} experience(s)", out.len())
                    };
                    println!("{msg}");
                }
                None => print!("{body}"),
            }
            std::process::exit(0);
        }
        Commands::Import(a) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            let text = match &a.file {
                Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
                    eprintln!(
                        "{}",
                        render_error(&ilearned::AppError::Storage(e.to_string()), cli.json)
                    );
                    std::process::exit(4);
                }),
                None => {
                    use std::io::Read as _;
                    let mut buf = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                        eprintln!(
                            "{}",
                            render_error(&ilearned::AppError::Internal(e.to_string()), cli.json)
                        );
                        std::process::exit(4);
                    }
                    buf
                }
            };
            let summary = svc.import_jsonl(&text, a.merge).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, cli.json));
                std::process::exit(exit_code(&e));
            });
            for err in &summary.errors {
                eprintln!(
                    "{}",
                    render_error(
                        &ilearned::AppError::InvalidInput(format!(
                            "line {}: {}",
                            err.line, err.message
                        )),
                        cli.json
                    )
                );
            }
            let msg = if cli.json {
                serde_json::json!({
                    "new": summary.new,
                    "updated": summary.updated,
                    "errors": summary.errors.len(),
                })
                .to_string()
            } else {
                format!(
                    "imported {} new, {} updated, {} error(s)",
                    summary.new,
                    summary.updated,
                    summary.errors.len()
                )
            };
            println!("{msg}");
            std::process::exit(if summary.errors.is_empty() { 0 } else { 2 });
        }
    }
}
