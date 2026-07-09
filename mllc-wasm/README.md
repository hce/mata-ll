# mllc-wasm

WebAssembly bindings for [`mllc`](https://crates.io/crates/mllc), the compiler
for **mata-ll** — a typed subset of Haskell that compiles to a single Lua file.

Built with `wasm-pack` (`--target web`), it exposes one entry point that
compiles mata-ll source to Lua entirely in the browser (the standard library is
bundled into the compiler, so no server or filesystem is needed):

```js
import init, { compile_mll } from "./pkg/mllc_wasm.js";
await init();
const lua = compile_mll('main :: IO ()\nmain = putStrLn "hi"\n');
```

This powers the client-side playground at <https://matall.org>.

- Source: <https://github.com/hce/mata-ll>

Licensed under the MIT License.
