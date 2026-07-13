use clap::{Parser, ValueEnum};
use std::path::Path;

/// Printed by -v/--version. clap prefixes the first line with the binary
/// name ("mll"), so the string starts mid-sentence. GIT_COMMIT is set by
/// build.rs at build time ("unknown" when git or .git is unavailable).
const VERSION_INFO: &str = concat!(
    "— the mata-ll compiler and runner\n",
    "\n",
    "MIT License: free and open-source software, provided \"as is\",\n",
    "without warranty of any kind.\n",
    "Copyright (c) 2026 Hans-Christian Esperer\n",
    "\n",
    "version:    ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "git commit: ",
    env!("GIT_COMMIT"),
);

/// How to embed the original .mll source into the emitted .lua.
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum EmbedSourceArg {
    /// Inside a clearly delimited Lua comment block
    Comments,
    /// As an exported module string variable named __SOURCE_CODE
    Var,
    /// Embed nothing (with --recompile: strip an existing embedding)
    None,
}

#[derive(Parser)]
#[command(
    name = "mll",
    about = "mll compiler and runner",
    // clap's built-in version flag uses uppercase -V; we want lowercase -v,
    // so the built-in flag is disabled and replaced by the manual `version`
    // field below. ArgAction::Version exits before required-argument
    // checking, so `mll -v` works without a source file.
    disable_version_flag = true,
    version = VERSION_INFO,
    long_version = VERSION_INFO,
)]
struct Cli {
    /// The .mll source file to compile (with --recompile: a previously
    /// emitted .lua file with embedded source)
    file: String,

    /// Run the compiled code immediately (don't write .lua file)
    #[arg(short, long)]
    run: bool,

    /// Write the compiled .lua file (default when not using --run)
    #[arg(short, long)]
    emit_lua: bool,

    /// Embed the original source into the emitted .lua so the file can be
    /// recompiled later without the .mll (see --recompile)
    #[arg(long, value_enum, value_name = "MODE")]
    embed_source: Option<EmbedSourceArg>,

    /// Treat FILE as previously emitted Lua carrying its source (see
    /// --embed-source): extract the source, recompile it, and rewrite FILE
    /// in place. The file's embedding mode is kept unless --embed-source
    /// overrides it.
    #[arg(long)]
    recompile: bool,

    /// Additional library search paths
    #[arg(short = 'L', long = "lib")]
    lib_paths: Vec<String>,

    /// Print license, version and build information, then exit
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    /// Arguments forwarded to the running program (readable via getArgs).
    /// Everything after the script name is collected here, so place mll's
    /// own flags before the script: `mll -r basic.mll game.bas`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    prog_args: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    // Run compilation on a thread with a large stack to handle deeply
    // nested ASTs (e.g. 256-element list literals desugar into 256
    // nested cons applications, each requiring a stack frame during
    // type inference).
    let builder = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024); // 64 MB stack
    let handler = builder.spawn(move || {
        run_compiler(cli);
    }).expect("Failed to spawn compiler thread");

    if let Err(e) = handler.join() {
        eprintln!("Compiler panicked: {:?}", e);
        std::process::exit(1);
    }
}

fn run_compiler(cli: Cli) {
    let filename = &cli.file;

    let file_text = match std::fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // --recompile: FILE is previously emitted Lua; the source to compile is
    // the embedded block, and (unless overridden) the embedding is preserved
    // so the rewritten file stays recompilable.
    let (source, detected_embed) = if cli.recompile {
        match mllc::embed::extract_source(&file_text) {
            Ok((source, mode)) => (source, Some(mode)),
            Err(e) => {
                eprintln!("Error recompiling {}: {}", filename, e);
                std::process::exit(1);
            }
        }
    } else {
        (file_text, None)
    };

    let embed_source = match cli.embed_source {
        Some(EmbedSourceArg::Comments) => Some(mllc::EmbedMode::Comments),
        Some(EmbedSourceArg::Var) => Some(mllc::EmbedMode::Var),
        Some(EmbedSourceArg::None) => None,
        None => detected_embed,
    };

    let source_dir = Path::new(filename).parent().unwrap_or(Path::new("."));

    // Auto-add lib/ directory relative to the compiler executable
    let exe_dir = std::env::current_exe().ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let auto_lib = exe_dir.as_ref()
        .map(|d| d.join("../../lib"))
        .and_then(|p| p.canonicalize().ok());

    let mut lib_paths: Vec<&Path> = cli.lib_paths.iter()
        .map(|p| Path::new(p.as_str()))
        .collect();
    if let Some(ref auto) = auto_lib {
        lib_paths.push(auto.as_path());
    }

    let options = mllc::CompileOptions {
        embed_source,
        source_name: Some(filename.clone()),
    };
    let result = match mllc::compile_with_options(&source, source_dir, &lib_paths, &options) {
        Ok(r) => r,
        Err(e) => {
            eprint!("{}", e);
            std::process::exit(1);
        }
    };

    // Write .lua file if requested or if not running
    if cli.emit_lua || !cli.run {
        // --recompile rewrites the emitted .lua in place
        let out_filename = if cli.recompile {
            filename.clone()
        } else {
            filename.replace(".mll", ".lua")
        };
        if let Err(e) = std::fs::write(&out_filename, &result.lua_code) {
            eprintln!("Error writing {}: {}", out_filename, e);
            std::process::exit(1);
        }
        if !cli.run {
            if cli.recompile {
                println!("Recompiled {} from its embedded source", filename);
            } else {
                println!("Compiled {} -> {}", filename, out_filename);
            }
        }
    }

    // Run with mlua if requested
    if cli.run {
        run_lua(&result.lua_code, filename, &cli.prog_args);
    }
}

fn run_lua(code: &str, filename: &str, prog_args: &[String]) {
    let lua = mlua::Lua::new();

    // Populate the Lua `arg` table the way `lua`/`luajit` do, so a program's
    // getArgs sees the forwarded arguments. arg[0] is the script name and
    // arg[1..] are the program arguments.
    let arg_table = lua.create_table().expect("create arg table");
    arg_table.set(0, filename).expect("set arg[0]");
    for (i, a) in prog_args.iter().enumerate() {
        arg_table.set(i as i64 + 1, a.as_str()).expect("set arg[i]");
    }
    lua.globals().set("arg", arg_table).expect("install arg table");

    match lua.load(code).set_name(filename).exec() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            std::process::exit(1);
        }
    }
}
