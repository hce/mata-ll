-- The Prelude's `data ExitValue = Normal | Err Int` must stay fully usable
-- from user code that does NOT shadow its constructors: constructing,
-- pattern-matching, and passing to `exit`'s argument type all resolve to the
-- Prelude's own tags. (Companion to constructor_shadowing.mll, which checks
-- that shadowing those names works; this checks that not shadowing them does.)

describeExit :: ExitValue -> String
describeExit Normal = "normal"
describeExit (Err n) = show n

-- `exit` itself calls out to a host-provided Lua global, so it cannot run
-- under the harness — but its ExitValue argument type must typecheck against
-- values built from the Prelude's constructors.
quitWith :: ExitValue -> IO ()
quitWith = exit

main :: IO ()
main = do
    assert (describeExit Normal == "normal") "Prelude Normal matches"
    assert (describeExit (Err 3) == "3") "Prelude Err matches and unwraps"
    putStrLn "ok"
