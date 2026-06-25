-- Regression test: a `let` binding inside a do-block in one clause of a
-- multi-clause function must get its own `local`, not leak as a shared global.
--
-- The codegen tracked declared locals in a set that accumulated ACROSS
-- clauses. Clause 0 below binds `r` via `<-` (declaring a local r); when
-- clause `n` then binds `r` via `let`, the name was already "declared", so it
-- was emitted as a bare assignment to a shared global instead of a local. The
-- returned closures then captured the same global r, so a later call clobbered
-- an earlier closure's value. (In the BASIC example this corrupted the FOR
-- loop-frame stack across nested loops.) Fixed by scoping the local set per
-- clause, since each clause is an independent Lua branch.

ioId :: Integer -> IO Integer
ioId n = return n

slow :: Integer -> Integer
slow n = n + 0

make :: Integer -> IO (Integer -> Integer)
make 0 = do
    r <- ioId 0
    return (\q -> q + r)
make n = do
    let r = slow n
    return (\q -> q + r)

main :: IO ()
main = do
    f1 <- make 5
    f2 <- make 7
    assert (f1 0 == 5) "first closure keeps its own r (no cross-clause global leak)"
    assert (f2 0 == 7) "second closure keeps its own r"
    putStrLn "clause_local_scope ok"
