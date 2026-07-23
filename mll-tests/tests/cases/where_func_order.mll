-- Regression test: where-clause function ordering
-- Functions defined in where blocks must be available to value bindings
-- that appear before them in source order.
-- (Bug: generated Lua emitted value bindings before function definitions)

-- Value bindings reference a later-defined where function
mulLimb :: Int -> Int -> Int
mulLimb a o = direct + 38 * wrap
  where
    direct = helper a o 0 o
    wrap   = helper a o (o + 1) 15
    helper _ _ j jmax
      | j > jmax  = 0
      | otherwise = a + helper a o (j + 1) jmax

-- Multiple value bindings referencing the same where function
splitCompute :: Int -> Int
splitCompute x = low + high
  where
    low  = scale x 1
    high = scale x 100
    scale a b = a * b + 1

-- Where function with pattern matching and guards
classify :: Int -> String
classify n = label
  where
    label = descr (categorize n)
    categorize x
      | x < 0     = 0
      | x == 0    = 1
      | otherwise = 2
    descr 0 = "negative"
    descr 1 = "zero"
    descr _ = "positive"

-- Recursive where function referenced by a value binding
sumRange :: Int -> Int
sumRange n = result
  where
    result = go 0 n
    go acc 0 = acc
    go acc k = go (acc + k) (k - 1)

main :: IO ()
main = do
    assert (mulLimb 5 3 == 2300) "mulLimb value-before-func"
    assert (splitCompute 10 == 1012) "splitCompute multi-value"
    assert (classify (-3) == "negative") "classify negative"
    assert (classify 0 == "zero") "classify zero"
    assert (classify 5 == "positive") "classify positive"
    assert (sumRange 10 == 55) "sumRange recursive where"
    putStrLn "All where-clause ordering tests passed!"
