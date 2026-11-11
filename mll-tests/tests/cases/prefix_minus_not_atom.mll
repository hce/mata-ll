-- Pins Haskell 2010's reading of `-`: it is the prefix negation operator
-- of the infix grammar, never part of a literal and never an argument
-- atom. So `x -3` and `f -1` are the subtractions `x - 3` and `f - 1`,
-- `f (-3)` passes a negative number, `(- 3)` is negation rather than a
-- section, and a continuation line that starts with `- 3` continues the
-- infix expression above it instead of applying it to `(-3)` (F7). The
-- other operator continuations (`+`, `*`, `.`, `$`, `>>=`, a backtick
-- operator) are pinned beside it.

f :: Int -> Int
f x = x * 2

g :: Int -> Int -> Int
g a b = a * 10 + b

h :: Int -> Int
h (-1) = 100
h n = n

k :: Int -> Int
k n = case n of
  -1 -> 7
  -2 -> 8
  _ -> 0

main :: IO ()
main = do
  let x = 10 :: Int
  print (f (-3))
  print (x - 3)
  print (x -3)
  print (x- 3)
  print ((- 3) :: Int)
  print ((subtract 3) x)
  print (negate x)
  print [-1, -2, -3 :: Int]
  print (negate 7 -1)
  print (g 1 (-2))
  print (h (-1), h 5)
  print (k (-1), k (-2), k 3)
  print (- 3 + 4 :: Int)
  print (-3 * 4 :: Int)
  print (- x * 2)
  print (-x `div` 2)
  print (-3 `mod` 2 :: Int)
  print ((-1) ^ (2 :: Int) :: Int, -1 ^ (2 :: Int) :: Int)
  print (-2.5 * 2 :: Number)
  let y = x
        - 3
  print y
  let y2 = x
         -3
  print y2
  let y3 = 2.5 :: Number
  let y4 = y3
         - 1.5e0 * 2
  print y4
  let z = x
        + 3
        * 2
  print z
  let w = f
          $ x
  print w
  let v = f
        . f
        $ x
  print v
  let u = x
        `div` 3
  print u
  let t = [1, 2, 3 :: Int]
          >>= \i -> [i, i]
  print t
  print (g x 0
           -1)
  print (sum [x, x]
           - x)
