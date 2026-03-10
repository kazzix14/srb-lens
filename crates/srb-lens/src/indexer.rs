use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::builder;
use crate::model::Project;
use crate::parser::{autogen, cfg_text, parse_tree, symbol_table};

const CACHE_DIR: &str = ".srb-lens";
const CFG_FILE: &str = "cfg.txt";
const SYMBOLS_FILE: &str = "symbols.json";
const AUTOGEN_FILE: &str = "autogen.txt";
const METHOD_LOCS_FILE: &str = "method-locs.json";

/// Sorbet 実行コマンドの設定
///
/// 例:
/// - `SrbCommand::default()` → `srb tc ...`
/// - `SrbCommand::new("bundle exec srb")` → `bundle exec srb tc ...`
/// - `SrbCommand::new("docker compose exec app bundle exec srb")` → `docker compose exec app bundle exec srb tc ...`
#[derive(Debug, Clone)]
pub struct SrbCommand {
    program: String,
    prefix_args: Vec<String>,
}

impl Default for SrbCommand {
    fn default() -> Self {
        Self {
            program: "srb".to_string(),
            prefix_args: Vec::new(),
        }
    }
}

impl SrbCommand {
    /// コマンド文字列からパース。空白区切りで最初がプログラム、残りがプレフィックス引数。
    ///
    /// `"bundle exec srb"` → program=`bundle`, prefix_args=`["exec", "srb"]`
    pub fn new(command: &str) -> Self {
        let parts: Vec<&str> = command.split_whitespace().collect();
        match parts.as_slice() {
            [] => Self::default(),
            [program] => Self {
                program: program.to_string(),
                prefix_args: Vec::new(),
            },
            [program, rest @ ..] => Self {
                program: program.to_string(),
                prefix_args: rest.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    fn build_command(&self, project_root: &Path, extra_args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.current_dir(project_root);
        cmd.args(&self.prefix_args);
        cmd.arg("tc");
        cmd.args(extra_args);
        cmd.arg("--no-error-count");
        cmd.env("SRB_SKIP_GEM_RBIS", "1");
        cmd.stderr(std::process::Stdio::null());
        cmd
    }
}

/// キャッシュディレクトリのパスを返す
pub fn cache_dir(project_root: &Path) -> PathBuf {
    project_root.join(CACHE_DIR)
}

/// キャッシュが存在するか
pub fn cache_exists(project_root: &Path) -> bool {
    let dir = cache_dir(project_root);
    dir.join(CFG_FILE).exists() && dir.join(SYMBOLS_FILE).exists()
}

/// Sorbet を実行して .srb-lens/ にキャッシュを保存
pub fn index(project_root: &Path, srb_command: &SrbCommand) -> Result<Project, IndexError> {
    let dir = cache_dir(project_root);
    fs::create_dir_all(&dir)?;

    let cfg_output = run_sorbet(project_root, srb_command, &["--print=cfg-text"])?;
    fs::write(dir.join(CFG_FILE), &cfg_output)?;

    let symbols_output = run_sorbet(project_root, srb_command, &["--print=symbol-table-json"])?;
    fs::write(dir.join(SYMBOLS_FILE), &symbols_output)?;

    let autogen_output = run_sorbet(
        project_root,
        srb_command,
        &["--print=autogen", "--stop-after=namer"],
    )?;
    fs::write(dir.join(AUTOGEN_FILE), &autogen_output)?;

    let parse_tree_output = run_sorbet(
        project_root,
        srb_command,
        &["--print=parse-tree-json-with-locs", "--stop-after=parser"],
    )?;
    let method_locs = parse_tree::parse(&parse_tree_output)?;
    let method_locs_json =
        serde_json::to_string(&method_locs).map_err(|e| IndexError::SorbetOutput(e.to_string()))?;
    fs::write(dir.join(METHOD_LOCS_FILE), &method_locs_json)?;

    load_from_cache(project_root)
}

/// キャッシュから Project をロード
pub fn load_from_cache(project_root: &Path) -> Result<Project, IndexError> {
    let dir = cache_dir(project_root);

    let cfg_input = fs::read_to_string(dir.join(CFG_FILE))
        .map_err(|e| IndexError::CacheRead(CFG_FILE.to_string(), e))?;
    let symbols_input = fs::read_to_string(dir.join(SYMBOLS_FILE))
        .map_err(|e| IndexError::CacheRead(SYMBOLS_FILE.to_string(), e))?;

    let autogen_input = fs::read_to_string(dir.join(AUTOGEN_FILE)).ok();

    let cfg_methods = cfg_text::parse(&cfg_input)?;
    let symbol_tree = symbol_table::parse(&symbols_input)?;
    let autogen_files = match autogen_input {
        Some(input) => autogen::parse(&input)?,
        None => Vec::new(),
    };

    let mut project = builder::build(cfg_methods, symbol_tree, autogen_files);

    let method_locs_input = fs::read_to_string(dir.join(METHOD_LOCS_FILE)).ok();
    let method_locs: Vec<parse_tree::MethodLoc> = match method_locs_input {
        Some(input) => serde_json::from_str(&input).unwrap_or_default(),
        None => Vec::new(),
    };

    if method_locs.is_empty() {
        project.resolve_source_locations(project_root);
    } else {
        project.resolve_source_locations_from_locs(&method_locs);
    }

    Ok(project)
}

/// キャッシュが古いか（.rb ファイルの最新 mtime がキャッシュより新しいか）
pub fn cache_stale(project_root: &Path) -> bool {
    let dir = cache_dir(project_root);
    let cache_mtime = match fs::metadata(dir.join(CFG_FILE)).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };

    max_rb_mtime(project_root)
        .map(|rb_mtime| rb_mtime > cache_mtime)
        .unwrap_or(false)
}

/// project_root 配下の .rb ファイルの最新 mtime を返す
fn max_rb_mtime(project_root: &Path) -> Option<SystemTime> {
    const SKIP_DIRS: &[&str] = &["vendor", "node_modules", "tmp", "log", ".git", ".srb-lens", "sorbet"];

    fn walk(dir: &Path, max: &mut Option<SystemTime>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                walk(&path, max);
            } else if path.extension().is_some_and(|ext| ext == "rb") {
                if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                    *max = Some(match *max {
                        Some(cur) if cur >= mtime => cur,
                        _ => mtime,
                    });
                }
            }
        }
    }

    let mut max = None;
    walk(project_root, &mut max);
    max
}

/// キャッシュがあり新鮮ならロード、なければ/古ければ index を実行
pub fn load_or_index(
    project_root: &Path,
    srb_command: &SrbCommand,
) -> Result<Project, IndexError> {
    if cache_exists(project_root) && !cache_stale(project_root) {
        load_from_cache(project_root)
    } else {
        index(project_root, srb_command)
    }
}

fn run_sorbet(
    project_root: &Path,
    srb_command: &SrbCommand,
    extra_args: &[&str],
) -> Result<String, IndexError> {
    let mut cmd = srb_command.build_command(project_root, extra_args);
    let output = cmd.output().map_err(IndexError::SorbetExec)?;

    let stdout =
        String::from_utf8(output.stdout).map_err(|e| IndexError::SorbetOutput(e.to_string()))?;

    if stdout.is_empty() && !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IndexError::SorbetOutput(format!(
            "sorbet exited with {} and empty stdout. stderr: {stderr}",
            output.status
        )));
    }

    Ok(stdout)
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("cannot read cache file '{0}': {1}")]
    CacheRead(String, io::Error),
    #[error("failed to execute sorbet: {0}")]
    SorbetExec(io::Error),
    #[error("sorbet output error: {0}")]
    SorbetOutput(String),
    #[error("cfg-text parse error: {0}")]
    CfgParse(#[from] cfg_text::CfgParseError),
    #[error("symbol-table parse error: {0}")]
    SymbolParse(#[from] symbol_table::SymbolTableParseError),
    #[error("autogen parse error: {0}")]
    AutogenParse(#[from] autogen::AutogenParseError),
    #[error("parse-tree error: {0}")]
    ParseTree(#[from] parse_tree::ParseTreeError),
}
