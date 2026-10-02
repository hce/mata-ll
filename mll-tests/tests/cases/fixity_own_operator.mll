import FixityOuter

(|>) :: Int -> (Int -> Int) -> Int
x |> f = f x

minus :: Int -> Int -> Int
minus a b = a - b

main :: IO ()
main = do
    assert (outer == 9) "the imported module groups its own operator by its own declaration"
    assert (outerMixed == 18) "and its backtick operator likewise"
    assert (1 + 2 |> (* 10) == 21) "an own operator without a declaration is infixl 9"
    assert ((3 |> (+ 1)) + 1 == 5) "parenthesized use"
    assert (10 `minus` 3 `minus` 2 == 5) "an own backtick operator without a declaration is infixl 9"
    assert (2 * 10 `minus` 3 == 14) "and binds tighter than multiplication"
    print (outer, outerMixed)
    print (1 + 2 |> (* 10), (3 |> (+ 1)) + 1)
    print (10 `minus` 3 `minus` 2, 2 * 10 `minus` 3)
