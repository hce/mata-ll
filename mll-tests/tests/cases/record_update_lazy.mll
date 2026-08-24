-- Record update fields are LAZY, exactly like constructor fields:
-- `r { a = e }` forces `r` (to copy it) but suspends `e` — GHC builds
-- the new record with the field bound to the unevaluated thunk, so
-- updating a field to bottom and reading a DIFFERENT field is fine.
-- Regression: codegen emitted update fields eagerly (expr_ast), so
-- `p { pa = error "boom" }` raised at construction, and the demand
-- analyses claimed update-field demands, entry-forcing arguments GHC
-- never touches (see bump below).

data P = P { pa :: Int, pb :: Int, pc :: String }

-- demand facet: pa is never read, so x must not be forced —
-- `bump (error "boomX") p` is total under GHC
bump :: Int -> P -> Int
bump x p = pb (p { pa = x + 1 })

main :: IO ()
main = do
    let p = P { pa = 1, pb = 2, pc = "keep" }
    -- bottom field never read
    let q = p { pa = error "boom" }
    print (pb q)
    putStrLn (pc q)
    -- overwriting the bottom field makes the record fully readable again
    let r = q { pa = 3 }
    print (pa r)
    print (pb r)
    -- chained update overwrites the bottom before any read
    let s = p { pa = error "boom2" } { pa = 5 }
    print (pa s)
    -- multi-field update: one bottom field, the sibling still reads
    let t = p { pa = error "boom3", pb = 42 }
    print (pb t)
    print (bump (error "boomX") p)
