-- Pattern bindings in let are RECURSIVE and LAZY (audit finding 20,
-- Haskell 2010 3.12): the pattern's variables are in scope for the
-- right-hand side itself, for sibling bindings, and the match happens on
-- first demand of a variable — never eagerly. Before the fix
-- `let (a, b) = (1, a) in b` failed with "Unbound variable a".

check :: (Show a, Eq a) => String -> a -> a -> IO ()
check name got want =
    if got == want
        then putStrLn ("ok " <> name)
        else error ("FAIL " <> name <> ": got " <> show got <> " want " <> show want)

selfRef :: Int
selfRef = let (a, b) = (1, a) in b

viaParam :: Int -> Int
viaParam n = let (a, b) = (n + 1, a * 2) in b

main :: IO ()
main = do
    -- Expression-form let ... in.
    check "self-ref" selfRef 1
    check "via-param" (viaParam 10) 22
    -- Do-block form.
    let (x, y) = (5 :: Int, x + 1)
    check "do-form" (x + y) 11
    -- The pattern variables are in scope for SIBLING bindings in the group
    -- (expression-form let, whose groups may mix pattern and named binds).
    check "sibling" (let (p, q) = (2 :: Int, 3 :: Int)
                         s = p + q
                     in s) 5
    -- Laziness: an unmatched pattern binding whose variables are never
    -- demanded must not be forced (GHC: `let (a, b) = undefined in 5` is 5).
    let (u, v) = error "must stay unevaluated"
    check "lazy-unforced" (42 :: Int) 42
