-- Explicit `;` between the bindings of a `let` group (Haskell 2010's
-- separator, usable alongside layout): `let a = 1; b = 2 in …` was a parse
-- error "Expected 'in', found ';'".

semi :: Int
semi = let a = 1; b = 2 in a + b

semi3 :: Int -> Int
semi3 n = let x = n * 2; y = x + 1; z = y * y in z - x

mixed :: Int
mixed =
    let a = 1; b = 2
        c = 3
    in a + b + c

main :: IO ()
main = do
    print semi
    print (semi3 4)
    print mixed
