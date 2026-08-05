import LString (strLen)

-- A thunked CAF referenced by a function emitted BEFORE the CAF's own
-- binding. The forward-declaration layout must not claim the slot is
-- concrete: the reference needs its __force, or an `if` condition reads
-- the raw thunk table — always truthy — and takes the wrong branch for a
-- False CAF (silently, on every host).
check :: Int -> Bool
check i = if flag then i > 10 else i > 0

-- False, and not constant-foldable (the FFI call keeps it a real thunk).
flag :: Bool
flag = strLen "ab" == 3

-- A non-Bool thunked CAF forward-referenced in a strict position.
scaled :: Int -> Int
scaled i = i * base

base :: Int
base = strLen "abcd" * 10

main :: IO ()
main = do
    assert (check 5) "forward-referenced False CAF must be forced, not read as a truthy thunk"
    assert (not (check 11 == False && check 5 == False)) "check dispatches on the forced value"
    assert (scaled 3 == 120) "forward-referenced Int CAF forces to its value"
    putStrLn "ok"
