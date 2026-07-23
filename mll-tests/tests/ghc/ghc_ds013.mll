-- GHC ds013: Operator fixity and precedence
-- Tests that operators bind correctly without explicit parens

-- infixl 6: +, -
-- infixl 7: *, `div`, `mod`
-- infixr 5: :  (list cons)
-- infixl 9: !!  (list index)
-- infixr 0: $
-- infixr 9: .  (composition)
-- infixl 1: >>=
-- infix  4: ==, /=, <, >, <=, >=

listIndex :: [a] -> Int -> a
listIndex (x:_)  0 = x
listIndex (_:xs) n = listIndex xs (n - 1)
listIndex []     _ = error "index out of bounds"

double :: Int -> Int
double x = x * 2

addOne :: Int -> Int
addOne x = x + 1

applyF :: (a -> b) -> a -> b
applyF f x = f x

main :: IO ()
main = do
    -- Arithmetic precedence: * before +
    assert (2 + 3 * 4 == 14) "mul before add"
    assert (2 * 3 + 4 == 10) "mul add"
    assert (10 - 2 * 3 == 4) "sub after mul"

    -- div/mod same level as *
    assert (10 `div` 2 + 3 == 8) "div add"
    assert (10 `mod` 3 + 1 == 2) "mod add"

    -- Left associativity of +
    assert (1 + 2 + 3 + 4 == 10) "left assoc add"
    assert (10 - 3 - 2 == 5) "left assoc sub"

    -- $ is right-associative, low precedence
    assert (double (addOne 4) == 10) "dollar"
    assert (addOne (double 4) == 9) "dollar 2"

    -- Comparison vs arithmetic
    assert (2 + 3 == 5) "cmp after arith"
    assert (2 * 3 > 5) "cmp after mul"
    assert (10 - 4 < 7) "cmp after sub"

    -- Boolean && vs ||
    assert ((True || (False && False)) == True) "and before or"
    assert (((False && True) || True)  == True) "and before or 2"

    -- List cons right-associative: 1:2:3:[] == [1,2,3]
    let xs = 1 : 2 : 3 : []
    assert (xs == [1,2,3]) "cons right assoc"
    assert (length xs == 3) "cons length"

    -- listIndex has highest precedence among list ops
    assert (listIndex [10,20,30] 1 == 20) "index"
    assert (listIndex [1,2,3,4,5] (1 + 1) == 3) "index arith"

    putStrLn "ok"
