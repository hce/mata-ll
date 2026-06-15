use std::io::{self, BufRead, Write};
use std::path::Path;

/// Accumulated REPL state: declarations collected so far.
struct ReplState {
    /// Top-level declarations accumulated so far (data types, functions, instances, etc.)
    decls: Vec<String>,
    /// Library search paths
    lib_paths: Vec<String>,
    /// Line index where the bare main slot starts (everything before is prelude/runtime)
    baseline_main_line: usize,
}

impl ReplState {
    fn new(lib_paths: Vec<String>) -> Self {
        // Compile a bare program to find where the main slot definition starts.
        // Everything before that line is prelude/runtime boilerplate.
        let baseline_source = "main :: IO ()\nmain = pure ()\n";
        let lib_refs: Vec<&Path> = lib_paths.iter().map(|p| Path::new(p.as_str())).collect();
        let baseline_main_line = match mllc::compile(baseline_source, Path::new("."), &lib_refs) {
            Ok(r) => find_main_slot_line(&r.lua_code),
            Err(_) => 0,
        };
        ReplState {
            decls: Vec::new(),
            lib_paths,
            baseline_main_line,
        }
    }

    fn clear(&mut self) {
        self.decls.clear();
    }

    /// Try to compile and run an expression, printing the result.
    /// Returns true if the input was consumed (even on error), false if empty.
    fn eval(&mut self, input: &str) -> bool {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Build a full program from accumulated decls + this input.
        // Heuristic: if the input looks like a top-level declaration, add it
        // to decls. Otherwise, wrap it as a main expression.
        let is_decl = is_declaration(trimmed);

        let source = if is_decl {
            // Try compiling with the new declaration added
            let mut all_decls = self.decls.clone();
            all_decls.push(trimmed.to_string());
            build_source(&all_decls, None)
        } else {
            // Treat as an expression to evaluate and print
            build_source(&self.decls, Some(trimmed))
        };

        // Compile
        let lib_refs: Vec<&Path> = self.lib_paths.iter().map(|p| Path::new(p.as_str())).collect();
        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn({
                let source = source.clone();
                let lib_refs: Vec<std::path::PathBuf> = lib_refs.iter().map(|p| p.to_path_buf()).collect();
                move || {
                    let lib_refs: Vec<&Path> = lib_refs.iter().map(|p| p.as_path()).collect();
                    mllc::compile(&source, Path::new("."), &lib_refs)
                }
            })
            .unwrap()
            .join();

        let compile_result = match result {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                eprintln!("{}", e);
                return true;
            }
            Err(e) => {
                eprintln!("compiler panicked: {:?}", e);
                return true;
            }
        };

        // Execute in a fresh Lua VM (we recompile everything each time)
        let lua = mlua::Lua::new();
        match lua.load(&compile_result.lua_code).set_name("repl").exec() {
            Ok(()) => {
                // If it was a declaration, persist it
                if is_decl {
                    self.decls.push(trimmed.to_string());
                }
            }
            Err(e) => {
                eprintln!("runtime error: {}", e);
            }
        }

        true
    }
}

/// Heuristic: does this input look like a top-level declaration?
fn is_declaration(s: &str) -> bool {
    // Type signatures: foo :: Type
    if s.contains(" :: ") && !s.starts_with('(') {
        return true;
    }
    // Data/newtype/class/instance/type definitions
    let first_word = s.split_whitespace().next().unwrap_or("");
    matches!(first_word, "data" | "newtype" | "class" | "instance" | "type" | "infixl" | "infixr" | "infix" | "import")
        // Function definition: starts with lowercase identifier followed by args/pattern or =
        || (first_word.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
            && (s.contains(" = ") || s.contains(" =\n")))
}

/// Build a complete .mll source from accumulated declarations and an optional expression.
fn build_source(decls: &[String], expr: Option<&str>) -> String {
    let mut source = String::new();

    for d in decls {
        source.push_str(d);
        source.push('\n');
        source.push('\n');
    }

    match expr {
        Some(e) => {
            // Wrap expression in main that shows and prints it
            source.push_str("main :: IO ()\n");
            source.push_str(&format!("main = putStrLn (show ({}))\n", e));
        }
        None => {
            // Need a main for compilation; if decls already define main, skip
            let has_main = decls.iter().any(|d| {
                d.starts_with("main ") || d.starts_with("main=") || d == "main"
                    || d.starts_with("main ::")
            });
            if !has_main {
                source.push_str("main :: IO ()\nmain = pure ()\n");
            }
        }
    }

    source
}

fn print_help() {
    println!("mata-ll REPL (debug)");
    println!();
    println!("Enter expressions to evaluate, or declarations to accumulate.");
    println!("Multi-line input: end a line with \\\\ to continue on the next line.");
    println!();
    println!("Commands:");
    println!("  /clear     Reset all state");
    println!("  /decls     Show accumulated declarations");
    println!("  /lua EXPR  Show compiled Lua for an expression");
    println!("  /source    Show the full source that would be compiled");
    println!("  /help      Show this help");
    println!("  /quit      Exit");
}

fn main() {
    let lib_paths: Vec<String> = std::env::args().skip(1).collect();

    print_help();
    println!();

    let mut state = ReplState::new(lib_paths);
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("mll> ");
        stdout.flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap() == 0 {
            break; // EOF
        }

        // Multi-line continuation: lines ending with \\
        while line.trim_end().ends_with('\\') {
            // Remove the trailing backslash
            let len = line.trim_end().len();
            line.truncate(len - 1);
            line.push('\n');

            print!("...> ");
            stdout.flush().unwrap();

            let mut cont = String::new();
            if stdin.lock().read_line(&mut cont).unwrap() == 0 {
                break;
            }
            line.push_str(&cont);
        }

        let trimmed = line.trim();

        // Meta commands
        if trimmed.starts_with('/') {
            let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
            match parts[0] {
                "/clear" => {
                    state.clear();
                    println!("State cleared.");
                }
                "/decls" => {
                    if state.decls.is_empty() {
                        println!("(no declarations)");
                    } else {
                        for (i, d) in state.decls.iter().enumerate() {
                            println!("[{}] {}", i, d);
                        }
                    }
                }
                "/lua" => {
                    let expr = parts.get(1).unwrap_or(&"").trim();
                    if expr.is_empty() {
                        // Show lua for current accumulated state
                        let source = build_source(&state.decls, None);
                        show_lua(&source, &state.lib_paths, state.baseline_main_line);
                    } else {
                        let source = build_source(&state.decls, Some(expr));
                        show_lua(&source, &state.lib_paths, state.baseline_main_line);
                    }
                }
                "/source" => {
                    let source = build_source(&state.decls, None);
                    println!("{}", source);
                }
                "/drop" => {
                    if state.decls.is_empty() {
                        println!("(no declarations to drop)");
                    } else {
                        let removed = state.decls.pop().unwrap();
                        println!("Dropped: {}", truncate(&removed, 60));
                    }
                }
                "/help" => print_help(),
                "/quit" | "/exit" | "/q" => break,
                _ => println!("Unknown command: {}", parts[0]),
            }
            continue;
        }

        state.eval(trimmed);
    }
}

/// Find the line index where the last __mll_fn[N] = definition starts,
/// just before the "-- Entry point" comment. This is where main lives.
fn find_main_slot_line(lua_code: &str) -> usize {
    let lines: Vec<&str> = lua_code.lines().collect();
    // Find "-- Entry point" and walk backwards to the slot definition
    let entry_idx = lines.iter().rposition(|l| l.starts_with("-- Entry point"));
    if let Some(idx) = entry_idx {
        // Walk backwards past blank lines to find the start of the main fn
        let mut i = idx.saturating_sub(1);
        while i > 0 && lines[i].trim().is_empty() {
            i -= 1;
        }
        // Now walk backwards to the start of this function (the __mll_fn[N] = line)
        while i > 0 && !lines[i].starts_with("__mll_fn[") && !lines[i].starts_with("local ") {
            i -= 1;
        }
        return i;
    }
    0
}

fn show_lua(source: &str, lib_paths: &[String], baseline_main_line: usize) {
    let lib_refs: Vec<&Path> = lib_paths.iter().map(|p| Path::new(p.as_str())).collect();
    match mllc::compile(source, Path::new("."), &lib_refs) {
        Ok(r) => {
            // Skip the runtime+prelude, show from where user code starts.
            // Also strip the entry point boilerplate at the end.
            let user_code: String = r.lua_code
                .lines()
                .skip(baseline_main_line)
                .take_while(|l| !l.starts_with("-- Entry point"))
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = user_code.trim();
            if trimmed.is_empty() {
                println!("(no user code generated)");
            } else {
                println!("{}", trimmed);
            }
        }
        Err(e) => eprintln!("{}", e),
    }
}

fn truncate(s: &str, max: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.len() > max {
        format!("{}...", &one_line[..max])
    } else {
        one_line
    }
}
