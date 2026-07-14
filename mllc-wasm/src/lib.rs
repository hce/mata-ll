use wasm_bindgen::prelude::*;
use std::path::Path;

#[wasm_bindgen]
pub fn compile_mll(source: &str) -> String {
    match mllc::compile(source, Path::new("."), &[]) {
        Ok(result) => {
            if result.warnings.is_empty() {
                result.lua_code
            } else {
                // Surface non-fatal diagnostics (e.g. "no main and no export:
                // nothing to run or call") the same way errors are shown —
                // as a comment block ahead of the generated Lua.
                let rendered: Vec<String> = result.warnings.iter()
                    .map(|w| format!("-- Warning:\n-- {}", format!("{}", w).replace('\n', "\n-- ")))
                    .collect();
                format!("{}\n\n{}", rendered.join("\n"), result.lua_code)
            }
        }
        Err(e) => format!("-- Error:\n-- {}", format!("{}", e).replace('\n', "\n-- ")),
    }
}
