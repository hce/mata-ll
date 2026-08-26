module Main where

-- G7: the generic original of `pick` contains a first-class reference to
-- dict-passing `gg` at a polymorphic type — uncompilable where emitted,
-- but DEAD once the only real use specializes at Int. The diagnosis is
-- deferred to DCE reachability, so this GHC-legal program compiles (it
-- used to be rejected with "cannot pass 'gg' as a function value").

gg :: Show a => Int -> a -> String
gg 0 x = show x
gg n x = gg (n - 1) [x]

pick :: Show a => Bool -> (Int -> a -> String) -> Int -> a -> String
pick True h = h
pick False _ = gg

main :: IO ()
main = do
  putStrLn (pick True gg 0 (3 :: Int))
  putStrLn (pick False gg 1 (4 :: Int))

-- expect: 3
-- expect: [4]
