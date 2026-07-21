# mata-ll BASIC

A small Microsoft-style BASIC interpreter, written in mata-ll. It doubles as a
worked example of a multi-module mata-ll program: a byte-level lexer, a
precedence-climbing parser, an immutable interpreter state threaded through
`IO`, and both a file runner and an interactive REPL.

## Running

```sh
# Run a .bas program:
mll -r basic.mll demo.bas

# Start the REPL (numbered lines build a program; RUN, LIST, NEW, BYE):
mll -r basic.mll
```

`demo.bas` (factorials, Fibonacci, GOSUB), `arrays.bas` (arrays, string and
math builtins), and `dice.bas` (RND) are included.

## Modules

| Module       | Role                                                       |
|--------------|------------------------------------------------------------|
| `Tokens.mll` | The token type.                                            |
| `Lexer.mll`  | `String -> [Token]`, scanning raw bytes via `string.*`.    |
| `Syntax.mll` | The expression and statement AST.                          |
| `Parser.mll` | `[Token] -> [Stmt]`; expressions by precedence climbing.   |
| `Value.mll`  | Runtime values (number / string) and their coercions.     |
| `Interp.mll` | Program state, expression evaluation, statement execution. |
| `Util.mll`   | Small string helpers.                                      |
| `basic.mll`  | Entry point: file loader and REPL.                         |

## Language supported

- Statements: `LET` (and bare assignment), `PRINT` (with `;` / `,` and zoning),
  `INPUT`, `IF…THEN…ELSE` (and `IF…THEN <line>`), `FOR…TO…STEP` / `NEXT`,
  `GOTO`, `GOSUB` / `RETURN`, `DIM`, `REM`, `END`, `STOP`, and `:` to put
  several statements on one line.
- Variables: numeric (`A`, `B1`) and string (`A$`); arrays via `DIM`, with
  auto-dimensioning to 10 on first use.
- Operators: `+ - * / ^`, `MOD`, the comparisons, and `AND` / `OR` / `NOT`
  (treated logically), with the usual precedence.
- Builtins: `LEN`, `LEFT$`, `RIGHT$`, `MID$`, `CHR$`, `ASC`, `STR$`, `VAL`,
  `ABS`, `INT`, `SGN`, `SQR`, `SIN`, `COS`, `TAN`, `ATN`, `RND`.

`RND(x)` returns a fresh number in `[0,1)` from Lua's `math.random`; because
that is effectful, expression evaluation runs in `IO`.

## Known limitations

This is a teaching example, not a complete BASIC. Notable gaps:

- **`AND` / `OR` / `NOT` are logical, not bitwise** — they treat any non-zero
  value as true and yield 0 / -1.
- **String literals have no escape sequences.** A `"` always ends the string,
  so a quote cannot appear inside one.
- **`NEXT` ignores its variable name.** It always advances the innermost open
  `FOR` loop, so `NEXT J, I` and mismatched `NEXT` are not checked.
- **One numeric type.** Everything is a single number type (Lua's); there are
  no `%` / `!` / `#` type suffixes, and array indices are floored.
- **Arrays are unchecked.** They auto-dimension to bound 10 on first use, and
  an out-of-range or undeclared element reads as `0` / `""` rather than
  raising an error.
- **REPL immediate statements run in a throwaway state**, so variables do not
  persist between un-numbered lines; use a numbered program and `RUN`.
- **`INPUT` splits on commas without quote handling.**
- **Not implemented:** `WHILE` / `WEND`, `DATA` / `READ` / `RESTORE`,
  `DEF FN`, `ON … GOTO` / `GOSUB`, and `PRINT TAB()` / `SPC()`.
