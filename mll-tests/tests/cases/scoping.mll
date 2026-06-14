-- Comprehensive scoping tests

-- Let-in expressions (tested inline in main)

-- Where clause scoping
addSquares :: Integer -> Integer -> Integer
addSquares a b = sa + sb
    where sa = a * a
          sb = b * b

-- Where with local function
collatz :: Integer -> Integer
collatz n = go n 0
    where go 1 steps = steps
          go n steps
              | n `mod` 2 == 0 = go (n `div` 2) (steps + 1)
              | otherwise = go (3 * n + 1) (steps + 1)

-- Mutual recursion
isEven' :: Integer -> Bool
isEven' 0 = True
isEven' n = isOdd' (n - 1)

isOdd' :: Integer -> Bool
isOdd' 0 = False
isOdd' n = isEven' (n - 1)

-- Closure over free variables
makeAdder :: Integer -> Integer -> Integer
makeAdder n = \x -> x + n

-- Higher-order with closures
applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)


main :: IO ()
main = do
    -- Let-in (inline)
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

    -- Closures (via direct call)
    assert (makeAdder 5 10 == 15) "closure direct"

    -- Do-block shadowing
    let x = 1
    assert (x == 1) "do shadow before"
    let x = 2
    assert (x == 2) "do shadow after"
    assert ((let w = 10 in w + 1) == 11) "nested let in assert"
