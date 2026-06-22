-- Passing an mata-ll callback OUT to a Lua host function (the fold pattern).
--
-- Many Lua host APIs take a callback plus an initial state, call the callback
-- once per item, and thread its return value as the next state -- exactly like
-- a fold. A SQL driver's row iterator is the classic example:
--
--     final = db.fold(query, function(row, state) ... end, initial)
--
-- mata-ll declares such a host function as an FFI binding whose callback
-- argument is an mata-ll function. The compiler wraps the callback so the Lua
-- host can call it with positional arguments, marshals values across the
-- boundary, and -- for effectful callbacks -- runs the returned action.
--
-- The threaded state is a polymorphic type variable (`acc` below), so it passes
-- through the host opaquely: any mata-ll value, including tuples and ADTs,
-- round-trips intact. The type checker enforces that the state is one shared
-- type variable across the callback's accumulator, the callback's result, the
-- initial-state argument, and the return type.
--
-- This example is compile-only: running it needs a Lua host that provides a
-- `db.fold(query, cb, init)` global (rows are modelled as plain integers here).

-- Pure fold: the callback computes the next state with no side effects.
foldRows :: String -> (Integer -> acc -> acc) -> acc -> LuaPure "db.fold" acc

-- Effectful fold: the callback may perform I/O per row. It returns `LuaIO s acc`;
-- the compiler runs that action and threads the resulting state.
foldRowsIO :: String -> (Integer -> acc -> LuaIO s acc) -> acc -> LuaIO "db.fold" acc

-- Sum a column.
total :: Integer
total = foldRows "SELECT n FROM t" (\n acc -> acc + n) 0

-- Accumulate sum and count together. The (Integer, Integer) state is opaque to
-- the Lua host and handed back to us unchanged on every row -- no marshalling,
-- so the tuple is never flattened.
summary :: (Integer, Integer)
summary = foldRows "SELECT n FROM t"
    (\n acc -> case acc of (s, c) -> (s + n, c + 1))
    (0, 0)

-- An effectful fold step that logs each row as it is processed.
logStep :: Integer -> Integer -> LuaIO s Integer
logStep n acc = do
    liftIO (putStrLn ("row: " <> show n))
    pure (acc + n)

main :: IO ()
main = do
    putStrLn ("total = " <> show total)
    case summary of
        (s, c) -> putStrLn ("sum = " <> show s <> ", count = " <> show c)
    logged <- foldRowsIO "SELECT n FROM t" logStep 0
    putStrLn ("logged total = " <> show logged)
