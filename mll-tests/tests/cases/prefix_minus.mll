-- Prefix-minus grouping, GHC-verified (the golden is GHC's own output).
-- Prefix minus has the fixity of binary '-' (infixl 6): its operand is
-- everything binding TIGHTER than 6, so `- a * b` is `negate (a * b)` and
-- ``- a `div` b`` is `negate (a div b)` — observable with div: for a = 7,
-- b = 2 that is -3, where `(negate a) `div` b` would be -4. Left of an
-- infixl 6 operator the negation takes only the tight operand:
-- `- a + b` is `negate a + b`.

a :: Integer
a = 7

b :: Integer
b = 2

main :: IO ()
main = do
  print (- a * b)
  print (- a * b == negate (a * b))
  print (- a `div` b)
  print (- a `div` b == negate (a `div` b))
  print (- a + b)
  print (- a + b == negate a + b)
  print (- a - b)
  print (- a * b + a)
  print (a + (- b))
  print (a == - b)
  print ((* (- 2)) a)
  print ((+ 1) (- a))
  print (map (\x -> - x * 3) [1, 2])
  print ((-) a b)
  putStrLn "prefix_minus: done"
