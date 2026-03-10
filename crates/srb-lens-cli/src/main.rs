use std::io::{self, Read as _};
use std::path::PathBuf;
use std::{fs, process};

use clap::{Parser, Subcommand};
use srb_lens::builder;
use srb_lens::indexer::{self, SrbCommand};
use srb_lens::parser::{autogen, cfg_text, symbol_table};

#[derive(Parser)]
#[command(name = "srb-lens", about = "Sorbet CFG analyzer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run Sorbet and build index cache in .srb-lens/
    Index {
        /// Project root directory (default: current dir)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Command to run Sorbet (e.g. "bundle exec srb", "docker compose exec app srb")
        #[arg(long, default_value = "srb")]
        srb_command: String,
    },
    /// Query method/class information
    Query {
        /// "Foo#bar" (instance), "Foo.bar" (class), or "Foo" (all methods)
        query: String,

        /// Project root for cache lookup (default: current dir)
        #[arg(short, long)]
        dir: Option<PathBuf>,

        /// Force re-index before querying
        #[arg(long)]
        index: bool,

        /// Command to run Sorbet (used with --index)
        #[arg(long, default_value = "srb")]
        srb_command: String,

        /// Path to cfg-text file (skip cache, read from file)
        #[arg(long)]
        cfg: Option<String>,

        /// Path to symbol-table-json file
        #[arg(long)]
        symbols: Option<String>,

        /// Path to autogen file
        #[arg(long)]
        autogen: Option<String>,

        /// List matching methods only (no details)
        #[arg(long)]
        list: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read cfg-text from stdin and query (for piping)
    Pipe {
        /// "Foo#bar", "Foo.bar", or "Foo"
        query: String,

        /// List matching methods only
        #[arg(long)]
        list: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Index { dir, srb_command }) => cmd_index(dir, &srb_command),
        Some(Commands::Query {
            query,
            dir,
            index,
            srb_command,
            cfg,
            symbols,
            autogen,
            list,
            json,
        }) => cmd_query(&query, dir, index, &srb_command, cfg, symbols, autogen, list, json),
        Some(Commands::Pipe { query, list, json }) => cmd_pipe(&query, list, json),
        None => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            if indexer::cache_exists(&root) {
                let project = indexer::load_from_cache(&root).unwrap_or_else(|e| {
                    eprintln!("error: {e}");
                    process::exit(1);
                });
                for method in &project.methods {
                    println!("{}", method.fqn);
                }
            } else {
                eprintln!("No cache found. Run `srb-lens index` first, or use `srb-lens query` / `srb-lens pipe`.");
                process::exit(1);
            }
        }
    }
}

fn cmd_index(dir: Option<PathBuf>, srb_command: &str) {
    let root =
        dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let cmd = SrbCommand::new(srb_command);
    eprintln!("Indexing {} ...", root.display());

    match indexer::index(&root, &cmd) {
        Ok(project) => {
            eprintln!(
                "Done. {} classes, {} methods indexed.",
                project.classes.len(),
                project.methods.len()
            );
            eprintln!("Cache saved to {}/", indexer::cache_dir(&root).display());
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_query(
    query: &str,
    dir: Option<PathBuf>,
    force_index: bool,
    srb_command: &str,
    cfg: Option<String>,
    symbols: Option<String>,
    autogen_path: Option<String>,
    list: bool,
    json: bool,
) {
    let root =
        dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let project = if cfg.is_some() || symbols.is_some() {
        build_from_files(cfg, symbols, autogen_path)
    } else {
        let cmd = SrbCommand::new(srb_command);

        if force_index {
            eprintln!("Indexing {} ...", root.display());
            indexer::index(&root, &cmd)
        } else {
            indexer::load_or_index(&root, &cmd)
        }
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        })
    };

    show_results(&project, query, list, json, Some(&root));
}

fn cmd_pipe(query: &str, list: bool, json: bool) {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
        eprintln!("error: cannot read stdin: {e}");
        process::exit(1);
    });

    let cfg_methods = cfg_text::parse(&buf).unwrap_or_else(|e| {
        eprintln!("error: cfg-text parse failed: {e}");
        process::exit(1);
    });

    let empty_tree = symbol_table::parse(
        r#"{"id":0,"name":{"kind":"CONSTANT","name":"<root>"},"kind":"CLASS_OR_MODULE"}"#,
    )
    .unwrap();

    let project = builder::build(cfg_methods, empty_tree, Vec::new());
    show_results(&project, query, list, json, None);
}

fn build_from_files(
    cfg: Option<String>,
    symbols: Option<String>,
    autogen_path: Option<String>,
) -> srb_lens::model::Project {
    let cfg_input = match cfg {
        Some(path) => fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("error: cannot read '{path}': {e}");
            process::exit(1);
        }),
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                eprintln!("error: cannot read stdin: {e}");
                process::exit(1);
            });
            buf
        }
    };

    let cfg_methods = cfg_text::parse(&cfg_input).unwrap_or_else(|e| {
        eprintln!("error: cfg-text parse failed: {e}");
        process::exit(1);
    });

    let symbol_tree = match symbols {
        Some(path) => {
            let json = fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("error: cannot read '{path}': {e}");
                process::exit(1);
            });
            symbol_table::parse(&json).unwrap_or_else(|e| {
                eprintln!("error: symbol-table parse failed: {e}");
                process::exit(1);
            })
        }
        None => symbol_table::parse(
            r#"{"id":0,"name":{"kind":"CONSTANT","name":"<root>"},"kind":"CLASS_OR_MODULE"}"#,
        )
        .unwrap(),
    };

    let autogen_files = match autogen_path {
        Some(path) => {
            let input = fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("error: cannot read '{path}': {e}");
                process::exit(1);
            });
            autogen::parse(&input).unwrap_or_else(|e| {
                eprintln!("error: autogen parse failed: {e}");
                process::exit(1);
            })
        }
        None => Vec::new(),
    };

    builder::build(cfg_methods, symbol_tree, autogen_files)
}

fn show_results(
    project: &srb_lens::model::Project,
    query: &str,
    list: bool,
    json: bool,
    root: Option<&std::path::Path>,
) {
    let methods = project.find_methods(query);
    if methods.is_empty() {
        eprintln!("no methods found matching '{query}'");
        process::exit(1);
    }

    if list {
        for m in &methods {
            println!("{}", m.fqn);
        }
        return;
    }

    for m in &methods {
        if json {
            print_method_json(m, project);
        } else {
            print_method_text(m, project, root);
        }
    }
}

fn print_method_text(
    m: &srb_lens::model::MethodInfo,
    project: &srb_lens::model::Project,
    root: Option<&std::path::Path>,
) {
    println!("== {} ==", m.fqn);

    if let Some(class) = project.classes.get(&m.fqn.class_fqn) {
        if let Some(super_class) = &class.super_class {
            println!("  class: {} < {super_class}", class.fqn);
        } else {
            println!("  class: {}", class.fqn);
        }
        if !class.mixins.is_empty() {
            println!("  mixins: {}", class.mixins.join(", "));
        }
        if let Some(path) = &class.file_path {
            if let Some(line) = class.line {
                println!("  defined: {path}:{line}");
            } else {
                println!("  defined: {path}");
            }
        }
    }

    if let Some(path) = &m.file_path {
        if let Some(line) = m.line {
            println!("  source: {path}:{line}");
        }
    }

    if !m.arguments.is_empty() {
        println!("  args:");
        for arg in &m.arguments {
            let opt = if arg.is_optional { " (optional)" } else { "" };
            println!("    {}: {}{opt}", arg.name, arg.ty);
        }
    }

    if let Some(ret) = &m.return_type {
        println!("  returns: {ret}");
    }

    if !m.ivars.is_empty() {
        println!("  ivars:");
        for ivar in &m.ivars {
            println!("    {}: {}", ivar.name, ivar.ty);
        }
    }

    if !m.calls.is_empty() {
        println!("  calls:");
        for call in &m.calls {
            if call.conditions.is_empty() {
                println!(
                    "    {}.{}() -> {}",
                    call.receiver_type, call.method_name, call.return_type
                );
            } else {
                let conds: Vec<String> = call
                    .conditions
                    .iter()
                    .map(|c| {
                        let branch = if c.is_true { "true" } else { "false" };
                        format!("{} = {}", c.call, branch)
                    })
                    .collect();
                println!(
                    "    {}.{}() -> {}  when: {}",
                    call.receiver_type, call.method_name, call.return_type, conds.join(", ")
                );
            }
        }
    }

    if !m.rescues.is_empty() {
        println!("  rescues: {}", m.rescues.join(", "));
    }

    if m.uses_block {
        println!("  uses_block: true");
    }

    if !m.basic_blocks.is_empty() {
        println!("  cfg:");
        for bb in &m.basic_blocks {
            match &bb.terminator {
                srb_lens::model::Terminator::Goto(target) => {
                    println!("    bb{} -> bb{}", bb.id, target);
                }
                srb_lens::model::Terminator::Branch {
                    condition,
                    true_bb,
                    false_bb,
                } => {
                    println!(
                        "    bb{} -[{}]-> true:bb{} / false:bb{}",
                        bb.id, condition, true_bb, false_bb
                    );
                }
                srb_lens::model::Terminator::BlockCall { true_bb, false_bb } => {
                    println!(
                        "    bb{} -[block]-> true:bb{} / false:bb{}",
                        bb.id, true_bb, false_bb
                    );
                }
                srb_lens::model::Terminator::Return => {
                    println!("    bb{} -> return", bb.id);
                }
            }
        }
    }

    // ソースコード表示
    if let (Some(root), Some(path), Some(start_line)) = (root, &m.file_path, m.line) {
        let full_path = root.join(path);
        if let Ok(content) = fs::read_to_string(&full_path) {
            let lines: Vec<&str> = content.lines().collect();
            if start_line > 0 && start_line <= lines.len() {
                let def_line = lines[start_line - 1];
                let indent = def_line.len() - def_line.trim_start().len();
                println!("  source:");
                for line in &lines[start_line - 1..] {
                    println!("    {line}");
                    // def と同じインデントの end で終了
                    let trimmed = line.trim_start();
                    if trimmed == "end" && (line.len() - trimmed.len()) == indent {
                        break;
                    }
                }
            }
        }
    }

    println!();
}

fn print_method_json(m: &srb_lens::model::MethodInfo, project: &srb_lens::model::Project) {
    let class_info = project.classes.get(&m.fqn.class_fqn);

    let args: Vec<String> = m
        .arguments
        .iter()
        .map(|a| {
            format!(
                r#"{{"name":"{}","type":"{}","optional":{}}}"#,
                a.name, a.ty, a.is_optional
            )
        })
        .collect();

    let calls: Vec<String> = m
        .calls
        .iter()
        .map(|c| {
            let conds: Vec<String> = c
                .conditions
                .iter()
                .map(|cond| {
                    format!(
                        r#"{{"call":"{}","is_true":{}}}"#,
                        cond.call, cond.is_true
                    )
                })
                .collect();
            format!(
                r#"{{"receiver":"{}","method":"{}","return_type":"{}","bb":{},"conditions":[{}]}}"#,
                c.receiver_type, c.method_name, c.return_type, c.bb_id, conds.join(",")
            )
        })
        .collect();

    let ivars: Vec<String> = m
        .ivars
        .iter()
        .map(|i| format!(r#"{{"name":"{}","type":"{}"}}"#, i.name, i.ty))
        .collect();

    let super_class = class_info
        .and_then(|c| c.super_class.as_ref())
        .map(|s| format!(r#""{s}""#))
        .unwrap_or_else(|| "null".to_string());

    let file_path = class_info
        .and_then(|c| c.file_path.as_ref())
        .map(|s| format!(r#""{s}""#))
        .unwrap_or_else(|| "null".to_string());

    let ret = m
        .return_type
        .as_ref()
        .map(|t| format!(r#""{t}""#))
        .unwrap_or_else(|| "null".to_string());

    let bbs: Vec<String> = m
        .basic_blocks
        .iter()
        .map(|bb| {
            use srb_lens::model::Terminator;
            match &bb.terminator {
                Terminator::Goto(target) => {
                    format!(r#"{{"id":{},"type":"goto","target":{}}}"#, bb.id, target)
                }
                Terminator::Branch {
                    condition,
                    true_bb,
                    false_bb,
                } => {
                    format!(
                        r#"{{"id":{},"type":"branch","condition":"{}","true_bb":{},"false_bb":{}}}"#,
                        bb.id, condition, true_bb, false_bb
                    )
                }
                Terminator::BlockCall { true_bb, false_bb } => {
                    format!(
                        r#"{{"id":{},"type":"block_call","true_bb":{},"false_bb":{}}}"#,
                        bb.id, true_bb, false_bb
                    )
                }
                Terminator::Return => {
                    format!(r#"{{"id":{},"type":"return"}}"#, bb.id)
                }
            }
        })
        .collect();

    println!(
        r#"{{"method":"{}","class":"{}","super_class":{},"file":{},"args":[{}],"returns":{},"calls":[{}],"ivars":[{}],"rescues":{:?},"uses_block":{},"basic_blocks":[{}]}}"#,
        m.fqn,
        m.fqn.class_fqn,
        super_class,
        file_path,
        args.join(","),
        ret,
        calls.join(","),
        ivars.join(","),
        m.rescues,
        m.uses_block,
        bbs.join(","),
    );
}
