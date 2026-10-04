use clap::Parser;
use std::path::Path;
use tokio::runtime::Builder;

use ilearned::application::MemoryService;
use ilearned::config::{Config, EmbeddingConfig, FileConfig, ResolvedConfig};
use ilearned::embedding::openai::OpenAiEmbeddingProvider;
use ilearned::storage::SqliteRepo;
use ilearned::surfaces::cli::{
    commands::ConfigCommands, exit_code, render_error, run_cli, Cli, Commands,
};

fn run_config_command(
    config_file: Option<&Path>,
    command: &ConfigCommands,
) -> Result<String, ilearned::AppError> {
    match command {
        ConfigCommands::Show => {
            let (file, paths) = FileConfig::load_files_with_sources(config_file)?;
            let config = ResolvedConfig::from_file(file)?;
            let config_files = paths
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            serde_json::to_string_pretty(&serde_json::json!({
                "config": config,
                "config_files": config_files,
            }))
            .map_err(|e| ilearned::AppError::Internal(e.to_string()))
        }
        ConfigCommands::Init(args) => {
            let path = if args.global {
                FileConfig::global_path()
            } else {
                FileConfig::local_path()
            };
            let db_path = if args.global {
                FileConfig::global_data_db_path().ok_or_else(|| {
                    ilearned::AppError::InvalidInput(
                        "cannot determine global data directory: set XDG_DATA_HOME or HOME"
                            .to_string(),
                    )
                })?
            } else {
                FileConfig::local_db_fallback()
            };
            FileConfig::init(&path, &db_path.to_string_lossy(), args.force)?;
            Ok(path.display().to_string())
        }
    }
}

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
    let json = cli.command.json();
    if let Commands::Config(args) = &cli.command {
        match run_config_command(cli.config_file.as_deref(), &args.command) {
            Ok(out) => println!("{out}"),
            Err(e) => {
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            }
        }
        return;
    }
    // The selected CLI config file overlays the default global/local files;
    // environment variables then override file values per setting.
    let files =
        FileConfig::load_files_with_override(cli.config_file.as_deref()).unwrap_or_else(|e| {
            eprintln!("{}", render_error(&e, json));
            std::process::exit(exit_code(&e));
        });
    let file_embedding = files.clone().and_then(|f| f.embedding);
    let embedding =
        EmbeddingConfig::from_parts_with_files(None, None, None, None, None, file_embedding)
            .unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            });
    // Database resolution lives in `Config::load_with_files`: an explicit
    // `--config-file` overlay, `ILEARNED_DB`, or global/local file `db`
    // wins; otherwise an existing `./.ilearned/ilearned.db` is used, then
    // an existing `$XDG_DATA_HOME/ilearned/ilearned.db`
    // (`~/.local/share/ilearned/ilearned.db` fallback), else startup fails
    // with "db path is not configured".
    let bind_override = match &cli.command {
        Commands::Serve(a) => a.bind,
        _ => None,
    };
    let cfg = Config::load_with_files(None, bind_override, None, None, None, embedding, files)
        .unwrap_or_else(|e| {
            eprintln!("{}", render_error(&e, json));
            std::process::exit(exit_code(&e));
        });
    match &cli.command {
        Commands::Serve(a) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            });
            let bind = a.bind.unwrap_or(cfg.bind);
            let rt = Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async {
                if let Err(e) = ilearned::surfaces::http::serve(svc, bind).await {
                    eprintln!("{}", render_error(&e, json));
                    std::process::exit(exit_code(&e));
                }
            });
        }
        Commands::Mcp(_) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            });
            let rt = Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async {
                if let Err(e) =
                    ilearned::surfaces::http::mcp::serve_stdio(std::sync::Arc::new(svc)).await
                {
                    eprintln!("{}", render_error(&e, json));
                    std::process::exit(exit_code(&e));
                }
            });
        }
        cmd @ (Commands::Add(_)
        | Commands::Search(_)
        | Commands::Update(_)
        | Commands::Delete(_)
        | Commands::Promote(_)
        | Commands::Demote(_)
        | Commands::Topic(_)
        | Commands::Clear(_)
        | Commands::Embedding(_)) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            });
            match run_cli(&svc, cmd, json) {
                Ok(out) => {
                    println!("{out}");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("{}", render_error(&e, json));
                    std::process::exit(exit_code(&e));
                }
            }
        }
        Commands::Export(a) => {
            let svc = build_service(&cfg).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            });
            let out = svc.export(a.topic.as_deref(), a.deep).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
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
                            render_error(&ilearned::AppError::Internal(e.to_string()), json)
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
                                        json
                                    )
                                );
                                std::process::exit(4);
                            }
                        }
                    }
                    if let Err(e) = std::fs::write(path, &body) {
                        eprintln!(
                            "{}",
                            render_error(&ilearned::AppError::Internal(e.to_string()), json)
                        );
                        std::process::exit(4);
                    }
                    let msg = if json {
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
                eprintln!("{}", render_error(&e, json));
                std::process::exit(exit_code(&e));
            });
            let text = match &a.file {
                Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
                    eprintln!(
                        "{}",
                        render_error(&ilearned::AppError::Storage(e.to_string()), json)
                    );
                    std::process::exit(4);
                }),
                None => {
                    use std::io::Read as _;
                    let mut buf = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                        eprintln!(
                            "{}",
                            render_error(&ilearned::AppError::Internal(e.to_string()), json)
                        );
                        std::process::exit(4);
                    }
                    buf
                }
            };
            let summary = svc.import_jsonl(&text, a.merge).unwrap_or_else(|e| {
                eprintln!("{}", render_error(&e, json));
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
                        json
                    )
                );
            }
            let msg = if json {
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
        Commands::Config(_) => unreachable!("config commands are handled before service setup"),
    }
}
