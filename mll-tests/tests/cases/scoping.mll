-- Comprehensive scoping tests

-- Top-level let-in value bindings
topLetVal :: Int
topLetVal = let x = 1 in let y = 2 in x + y

topShadow :: Int
topShadow = let x = 1 in let x = 2 in x

-- Where clause scoping
addSquares :: Int -> Int -> Int
addSquares a b = sa + sb
    where sa = a * a
          sb = b * b

-- Where with local function
collatz :: Int -> Int
collatz n = go n 0
    where go 1 steps = steps
          go n steps
              | n `mod` 2 == 0 = go (n `div` 2) (steps + 1)
              | otherwise = go (3 * n + 1) (steps + 1)

-- Mutual recursion
isEven' :: Int -> Bool
isEven' 0 = True
isEven' n = isOdd' (n - 1)

isOdd' :: Int -> Bool
isOdd' 0 = False
isOdd' n = isEven' (n - 1)

-- Closure over free variables
makeAdder :: Int -> Int -> Int
makeAdder n = \x -> x + n

-- Higher-order with closures
applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)


main :: IO ()
main = do
    -- Top-level let-in values
    assert (topLetVal == 3) "top-level let-in"
    assert (topShadow == 2) "top-level shadow"

    -- Inline let-in
    assert ((let x = 10 in x + 5) == 15) "inline let"
    assert ((let x = 1 in let y = 2 in x + y) == 3) "nested let"
    assert ((let x = 1 in let x = 2 in x) == 2) "let shadow"

    -- Where
    assert (addSquares 3 4 == 25) "where squares"
    assert (collatz 6 == 8) "collatz 6"
    assert (collatz 1 == 0) "collatz 1"

    -- Mutual recursion
    assert (isEven' 4 == True) "mutual even"
    assert (isOdd' 3 == True) "mutual odd"
    assert (isEven' 5 == False) "mutual not even"

    -- Closures
    let add5 = makeAdder 5
    assert (add5 10 == 15) "closure let-bound"
    assert (add5 0 == 5) "closure reuse"
    assert (makeAdder 5 10 == 15) "closure direct"
    assert (applyTwice (makeAdder 3) 0 == 6) "closure higher-order"

    -- Do-block shadowing
    let x = 1
    assert (x == 1) "do shadow before"
    let x = 2
    assert (x == 2) "do shadow after"
    assert ((let w = 10 in w + 1) == 11) "nested let in assert"
