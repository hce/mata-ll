use std::io::{self, BufRead, Write};
use std::path::Path;

/// `mllc::compile` on the compiler's calibrated stack
/// (`mllc::with_compiler_stack`): every compile in this front-end must run
/// on such a thread, or input the mll CLI handles could overflow here.
fn compile_on_compiler_stack(
    source: &str,
    lib_paths: &[String],
) -> Result<mllc::CompileResult, mllc::CompileError> {
    mllc::with_compiler_stack(|| {
        let lib_refs: Vec<&Path> = lib_paths.iter().map(|p| Path::new(p.as_str())).collect();
        mllc::compile(source, Path::new("."), &lib_refs)
    })
}

/// Accumulated REPL state: declarations collected so far.
struct ReplState {
    /// Top-level declarations accumulated so far (data types, functions, instances, etc.)
    decls: Vec<String>,
    /// Library search paths
    lib_paths: Vec<String>,
}

impl ReplState {
    fn new(lib_paths: Vec<String>) -> Self {
        ReplState {
            decls: Vec::new(),
            lib_paths,
        }
    }

    fn clear(&mut self) {
        self.decls.clear();
    }

    /// Build the full program for one interpretation of the input.
    fn source_for(&self, input: &str, interp: Interp) -> String {
        match interp {
            Interp::Decl => {
                let mut all_decls = self.decls.clone();
                all_decls.push(input.to_string());
                build_source(&all_decls, None)
            }
            Interp::ExprShow => build_source(&self.decls, Some(input)),
            Interp::ExprRun => build_source_run(&self.decls, input),
        }
    }

    /// Compile and run the input, printing the result.
    /// Returns true if the input was consumed (even on error), false if empty.
    ///
    /// An input line is ambiguous: `double x = x * 2` is a declaration,
    /// `let x = 1 in x + 1` is an expression, and no syntactic test tells
    /// them apart reliably. So the compiler decides: try one interpretation,
    /// and on compile failure try the other; whichever compiles wins. The
    /// syntactic shape (`looks_like_declaration`) only picks which
    /// interpretation goes FIRST — and, when both fail, whose error is
    /// reported: the first interpretation's, because the shape test encodes
    /// the surface cues (leading keyword, `::`, an `=` binding) of what the
    /// user most plausibly meant, while the other interpretation's error is
    /// usually an artifact of the wrapping (`main = putStrLn (show (data
    /// Foo)))` and would mislead.
    fn eval(&mut self, input: &str) -> bool {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return false;
        }

        let decl_first = looks_like_declaration(trimmed);
        let mut first_err: Option<mllc::CompileError> = None;

        // ExprRun is the IO-action interpretation (`putStrLn "hi"` at the
        // prompt): the show-wrapper cannot type an action (no Show (IO a)),
        // so a third wrapping executes it and discards the result (F17;
        // GHCi additionally prints a Show-able result — not attempted
        // here). It comes after ExprShow so plain values keep printing.
        let order = if decl_first {
            [Interp::Decl, Interp::ExprShow, Interp::ExprRun]
        } else {
            [Interp::ExprShow, Interp::ExprRun, Interp::Decl]
        };
        for interp in order {
            let source = self.source_for(trimmed, interp);

            // Compile. Caught rather than resumed: a compiler panic must not
            // take the REPL session down with it. A panic is a compiler bug,
            // not a wrong interpretation — report it immediately rather than
            // masking it behind the fallback.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compile_on_compiler_stack(&source, &self.lib_paths)
            }));

            let compile_result = match result {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                    continue;
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
                    if matches!(interp, Interp::Decl) {
                        self.decls.push(trimmed.to_string());
                    }
                }
                Err(e) => {
                    eprintln!("runtime error: {}", e);
                }
            }
            return true;
        }

        // Both interpretations failed to compile: report the first (more
        // plausible) interpretation's error — see the method comment.
        eprintln!("{}", first_err.expect("both interpretations tried"));
        true
    }
}

/// One way to read a REPL line — see the order chosen in `eval`.
#[derive(Clone, Copy)]
enum Interp {
    /// A new top-level declaration.
    Decl,
    /// An expression, shown and printed.
    ExprShow,
    /// An IO action, executed for its effects (result discarded).
    ExprRun,
}

/// Syntactic shape test: does this input look like a top-level declaration?
/// Only a plausibility ranking — it picks which interpretation `eval` tries
/// first and whose error is reported; the compiler makes the actual call.
fn looks_like_declaration(s: &str) -> bool {
    // Type signatures: foo :: Type
    if s.contains(" :: ") && !s.starts_with('(') {
        return true;
    }
    // Data/newtype/class/instance/type definitions
    let first_word = s.split_whitespace().next().unwrap_or("");
    matches!(first_word, "data" | "newtype" | "class" | "instance" | "type" | "infixl" | "infixr" | "infix" | "import")
        // Expression-only leading keywords can never open a top-level
        // declaration, no matter what follows (`let x = 1 in x + 1`,
        // `if p then ... else ...`, `case e of ...`, `do ...`).
        || (!matches!(first_word, "let" | "if" | "then" | "else" | "case" | "of" | "do" | "in")
            // Function definition: starts with lowercase identifier followed by args/pattern or =
            && first_word.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
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

/// Build a source that RUNS the input as an IO action, discarding its
/// result: `main = (e) >> pure ()`.
fn build_source_run(decls: &[String], expr: &str) -> String {
    let mut source = String::new();
    for d in decls {
        source.push_str(d);
        source.push('\n');
        source.push('\n');
    }
    source.push_str("main :: IO ()\n");
    source.push_str(&format!("main = ({}) >> pure ()\n", expr));
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
    println!("  /drop      Remove the most recent declaration");
    println!("  /help      Show this help");
    println!("  /quit      Exit (also /exit, /q)");
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
        // A closed stdout (the REPL's output piped somewhere that went
        // away) is an exit condition, not a panic (F17).
        if stdout.flush().is_err() {
            break;
        }

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            // Non-UTF-8 (or otherwise failing) stdin: read_line reports
            // InvalidData and may have consumed an unspecified amount, so
            // the stream cannot be resumed reliably — end the session
            // cleanly instead of panicking (F17).
            Err(e) => {
                eprintln!("stdin error: {e}");
                break;
            }
        }

        // Multi-line continuation: lines ending with \\
        while line.trim_end().ends_with('\\') {
            // Remove the trailing backslash
            let len = line.trim_end().len();
            line.truncate(len - 1);
            line.push('\n');

            print!("...> ");
            if stdout.flush().is_err() {
                break;
            }

            let mut cont = String::new();
            match stdin.lock().read_line(&mut cont) {
                Ok(0) => break,
                Ok(_) => line.push_str(&cont),
                Err(e) => {
                    eprintln!("stdin error: {e}");
                    break;
                }
            }
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
                        show_lua(&source, &state.lib_paths);
                    } else {
                        let source = build_source(&state.decls, Some(expr));
                        show_lua(&source, &state.lib_paths);
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

fn show_lua(source: &str, lib_paths: &[String]) {
    match compile_on_compiler_stack(source, lib_paths) {
        Ok(r) => {
            // Show only the code compiled from the user's program: the
            // section boundaries published on CompileResult cut off the
            // runtime prelude before it and the entry-point/exports
            // boilerplate after it. Computed by the compiler where the file
            // is assembled — never rediscovered here by scanning the text.
            let end = r.entry_point_start.unwrap_or(r.lua_code.len());
            let user_code = &r.lua_code[r.user_code_start..end];
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

/// The first `max` CHARACTERS of `s` on one line, with `...` when cut. Cut
/// at a character boundary — a byte-index slice panicked on a declaration
/// with a multibyte character at the cut (`byte index N is not a char
/// boundary`), taking the session down on `/drop`.
fn truncate(s: &str, max: usize) -> String {
    let one_line = s.replace('\n', " ");
    match one_line.char_indices().nth(max) {
        Some((cut, _)) => format!("{}...", &one_line[..cut]),
        None => one_line,
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_cuts_at_a_character_boundary() {
        // 3 ASCII + a 2-byte character + more: cutting at 4 characters must
        // not slice inside the multibyte character.
        assert_eq!(truncate("abcé-rest", 4), "abcé...");
        assert_eq!(truncate("é", 4), "é");
        assert_eq!(truncate("line1\nline2", 20), "line1 line2");
        assert_eq!(truncate("ααααα", 3), "ααα...");
    }
}

#[cfg(test)]
mod repl_action_tests {
    use super::*;

    /// F17: an IO action at the prompt executes (the show-wrapper cannot
    /// type it); a plain value still prints via show; a declaration still
    /// accumulates. eval returning true = the input was consumed without
    /// panicking; the printed output goes to the test's stdout.
    #[test]
    fn eval_handles_values_actions_and_decls() {
        let mut st = ReplState::new(vec![]);
        assert!(st.eval("2 + 3"), "plain value evaluates");
        assert!(st.eval("putStrLn \"side effect\""), "IO action executes");
        assert!(
            st.eval("double :: Int -> Int\ndouble x = x * 2"),
            "declaration accumulates"
        );
        assert_eq!(st.decls.len(), 1, "declaration persisted");
        assert!(st.eval("double 21"), "uses the accumulated declaration");
        assert_eq!(st.decls.len(), 1, "expression did not accumulate");
    }

    #[test]
    fn run_wrapper_shape() {
        let src = build_source_run(&["x :: Int".into()], "putStrLn \"hi\"");
        assert!(src.contains("main = (putStrLn \"hi\") >> pure ()"), "{src}");
        assert!(src.starts_with("x :: Int\n"), "{src}");
    }
}
