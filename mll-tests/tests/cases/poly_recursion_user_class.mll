-- Polymorphic recursion through USER-class methods (audit finding 9).
-- The recursion changes the constrained type each step (a -> [a] -> [[a]] …),
-- so no static specialization can cover it: the function takes a dictionary,
-- and the dictionary for `[a]` must be CONSTRUCTED at the recursive call from
-- the dictionary for `a` via the parameterized instance (GHC's dictionary
-- elaboration). Before the fix the same element dictionary was passed
-- through unchanged and the initial dictionary referenced a nonexistent
-- global, so even depth-0 crashed with "attempt to call a nil value
-- (field 'csize')". Built-in Show dodged this via its shape-generic runtime
-- method; user classes have no such fallback.

class CSize a where
    csize :: a -> Integer

instance CSize Integer where
    csize _ = 1

instance CSize a => CSize [a] where
    csize [] = 0
    csize (x:xs) = csize x + csize xs

instance CSize a => CSize (Maybe a) where
    csize Nothing  = 0
    csize (Just x) = csize x

-- Recursion at [a]: depth 0 uses the dictionary directly.
poly :: CSize a => Integer -> a -> Integer
poly 0 x = csize x
poly n x = poly (n - 1) [x]

-- Recursion that GROWS the structure: 2^n leaves, so the answer depends on
-- the constructed [a] dictionary genuinely recursing per level.
grow :: CSize a => Integer -> a -> Integer
grow 0 x = csize x
grow n x = grow (n - 1) [x, x]

-- Recursion through a user instance on Maybe.
wrap :: CSize a => Integer -> a -> Integer
wrap 0 x = csize x
wrap n x = wrap (n - 1) (Just x)

check :: String -> Integer -> Integer -> IO ()
check name got want =
    if got == want
        then putStrLn ("ok " <> name)
        else error ("FAIL " <> name <> ": got " <> show got <> " want " <> show want)

main :: IO ()
main = do
    -- The element argument carries the user constraint `CSize a`; with
    -- polymorphic numeric literals its type is `(CSize a, Num a) => a`, which
    -- GHC (and mata-ll) cannot default — `CSize` is not a standard class — so
    -- the literal is annotated to pin `a = Integer`.
    check "poly-depth0" (poly 0 (7 :: Integer)) 1
    check "poly-depth3" (poly 3 (7 :: Integer)) 1
    check "grow-depth0" (grow 0 (5 :: Integer)) 1
    check "grow-depth1" (grow 1 (5 :: Integer)) 2
    check "grow-depth4" (grow 4 (5 :: Integer)) 16
    check "wrap-depth3" (wrap 3 (9 :: Integer)) 1
    -- Built-in Show polymorphic recursion keeps working alongside.
    -- show [[1]] == "[[1]]", 5 characters
    check "show-still-works" (lengthS (showdeep 2 (1 :: Integer))) 5

lengthS :: String -> Integer
lengthS s = sLen s

sLen :: String -> LuaPure "string.len" Integer

showdeep :: Show a => Integer -> a -> String
showdeep 0 x = show x
showdeep n x = showdeep (n - 1) [x]
