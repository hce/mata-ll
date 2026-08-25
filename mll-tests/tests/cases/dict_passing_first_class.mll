-- Uses of dictionary-passing functions in every saturation shape (Q60).
-- `deep`'s polymorphic recursion drives `gshow` and `gsel` past the
-- specialization limit, so all three functions are compiled with runtime
-- dictionary passing. Each use shape below must call the dictionary-form
-- function saturated: a first-class reference and a partial application
-- eta-expand into a dictionary-building lambda, and an over-applied call
-- (the body returns a lambda) applies the saturated call's result — an
-- unsaturated direct call would pass a value where a dictionary slot is
-- expected, or silently drop arguments.
gshow :: Show a => a -> String
gshow x = show x

gsel :: Show a => a -> String -> String
gsel x = \p -> gshow x <> p

deep :: Show a => Int -> a -> String
deep n x = if n <= 0 then gsel x "." else gshow [x] <> deep (n - 1) [x]

main :: IO ()
main = do
  putStrLn (deep 2 (1 :: Int))
  putStrLn (mconcat (map gshow [10, 20, 30 :: Int]))
  putStrLn (mconcat (map (gsel True) ["a", "b"]))
  putStrLn (gsel (7 :: Int) "!")
