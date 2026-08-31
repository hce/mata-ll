use clap::{Parser, ValueEnum};
use std::path::Path;

/// Printed by -v/--version. clap prefixes the first line with the binary
/// name ("mll"), so the string starts mid-sentence. The commit is
/// `mllc::GIT_COMMIT` — the full hash mllc's build script captured, the
/// same value codegen stamps into every emitted module ("unknown" when
/// git or .git was unavailable at build time), so the binary and its
/// output can never disagree about provenance.
fn version_info() -> &'static str {
    // clap's version attribute wants &'static str (the crate's "string"
    // feature is off); the text is built once and lives for the process.
    static INFO: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INFO.get_or_init(|| {
        format!(
            "— the mata-ll compiler and runner\n\
             \n\
             MIT License: free and open-source software, provided \"as is\",\n\
             without warranty of any kind.\n\
             Copyright (c) 2026 Hans-Christian Esperer\n\
             \n\
             version:    {}\n\
             git commit: {}",
            env!("CARGO_PKG_VERSION"),
            mllc::GIT_COMMIT,
        )
    })
}

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
    version = version_info(),
    long_version = version_info(),
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

    // Run compilation on the compiler's calibrated stack (deeply nested
    // ASTs — e.g. list literals desugaring into cons chains — need one
    // inference frame per level; mllc::COMPILER_STACK_SIZE and
    // mllc::MAX_NESTING_DEPTH are sized together so input up to the depth
    // limit compiles or is cleanly diagnosed instead of overflowing). A
    // compiler panic (an ICE) resumes here and exits through the standard
    // panic path — the thread's hook has already printed the message.
    mllc::with_compiler_stack(move || run_compiler(cli));
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

    // Auto-add the in-tree lib/ and contrib/ directories when running from a
    // cargo build tree (target/{debug,release}/mll). The auto-add is gated on
    // that layout: an installed binary (e.g. ~/.cargo/bin/mll) would resolve
    // ../../lib to an unrelated user directory and silently extend the module
    // search path with it. Installed binaries rely on the embedded stdlib;
    // additional paths come from -L, which always takes precedence (searched
    // first, in the order given).
    let dev_root = std::env::current_exe().ok().and_then(|exe| {
        let exe_dir = exe.parent()?;
        let profile = exe_dir.file_name()?.to_str()?;
        if profile != "debug" && profile != "release" {
            return None;
        }
        let target = exe_dir.parent()?;
        if target.file_name()? != "target" {
            return None;
        }
        target.parent().map(|root| root.to_path_buf())
    });
    let auto_lib = dev_root.as_ref()
        .map(|root| root.join("lib"))
        .and_then(|p| p.canonicalize().ok());
    let auto_contrib = dev_root.as_ref()
        .map(|root| root.join("contrib"))
        .and_then(|p| p.canonicalize().ok());

    let mut lib_paths: Vec<&Path> = cli.lib_paths.iter()
        .map(|p| Path::new(p.as_str()))
        .collect();
    if let Some(ref auto) = auto_lib {
        lib_paths.push(auto.as_path());
    }
    if let Some(ref auto) = auto_contrib {
        lib_paths.push(auto.as_path());
    }

    let options = mllc::CompileOptions {
        embed_source,
        source_name: Some(filename.clone()),
        // The CLI keeps the environment-variable path for pass disabling.
        disable_opt_passes: None,
    };
    let result = match mllc::compile_with_options(&source, source_dir, &lib_paths, &options) {
        Ok(r) => r,
        Err(e) => {
            eprint!("{}", e);
            std::process::exit(1);
        }
    };

    // Non-fatal diagnostics (e.g. the module compiled, but with no `main` and
    // no `export` there is nothing in it to run or call). The compile itself
    // succeeded, so these go to stderr and the output is still produced.
    for warning in &result.warnings {
        eprintln!("Warning: {}", warning);
    }

    // Write .lua file if requested or if not running
    let mut written_lua: Option<String> = None;
    if cli.emit_lua || !cli.run {
        // --recompile rewrites the emitted .lua in place
        let out_filename = if cli.recompile {
            filename.clone()
        } else {
            let path = Path::new(filename);
            if path.extension().and_then(|e| e.to_str()) != Some("mll") {
                eprintln!(
                    "Error: cannot derive an output name for {}: the input \
                     file does not end in .mll, so writing the .lua output \
                     would overwrite it or land next to it with a surprising \
                     name.",
                    filename
                );
                eprintln!(
                    "note: to recompile a previously emitted .lua file in \
                     place, use --recompile; to run without writing a file, \
                     use --run."
                );
                std::process::exit(1);
            }
            path.with_extension("lua").display().to_string()
        };
        if let Err(e) = std::fs::write(&out_filename, &result.lua_code) {
            eprintln!("Error writing {}: {}", out_filename, e);
            std::process::exit(1);
        }
        written_lua = Some(out_filename.clone());
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
        // The chunk name labels every traceback line, and those line numbers
        // are positions in the GENERATED Lua — naming the chunk after the
        // .mll source made a traceback's `foo.mll:812` point into the wrong
        // file's coordinates. Name it the written .lua when one exists (a
        // real, openable location); otherwise say what the text is, so a
        // reader reaches for --emit-lua instead of the .mll line.
        let chunk_name = match &written_lua {
            Some(out) => out.clone(),
            None => format!("{} (generated Lua; --emit-lua writes it)", filename),
        };
        run_lua(&result.lua_code, filename, &chunk_name, &cli.prog_args);
    }
}

fn run_lua(code: &str, filename: &str, chunk_name: &str, prog_args: &[String]) {
    let lua = mlua::Lua::new();

    // Populate the Lua `arg` table the way `lua`/`luajit` do, so a program's
    // getArgs sees the forwarded arguments. arg[0] is the script name (the
    // .mll the user invoked, matching how lua sets it to the script it was
    // handed) and arg[1..] are the program arguments.
    let arg_table = lua.create_table().expect("create arg table");
    arg_table.set(0, filename).expect("set arg[0]");
    for (i, a) in prog_args.iter().enumerate() {
        arg_table.set(i as i64 + 1, a.as_str()).expect("set arg[i]");
    }
    lua.globals().set("arg", arg_table).expect("install arg table");

    match lua.load(code).set_name(chunk_name).exec() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            std::process::exit(1);
        }
    }
}
