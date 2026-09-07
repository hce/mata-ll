module Main where

-- `foldl` is lazy in its accumulator: each step suspends `f acc x`, in
-- GHC and in the compiled `[]` instance alike, so a step whose function
-- never looks at the accumulator never runs the earlier steps. The
-- type-erased runtime twin of `foldl` — reached from a generic copy,
-- which the dead-variable use of `lastOr` below used to keep alive —
-- applied `f` eagerly per element and raised on the `undefined`
-- element. The two twins now share one strictness.

firstOr :: Foldable t => t a -> Int
firstOr xs = foldr (\x _ -> 1) 0 xs

lastOr :: Foldable t => t a -> Int
lastOr xs = foldl (\_ x -> x `seq` 1) 0 xs

main :: IO ()
main = do
    print (length [undefined, []])
    print (foldl (\acc x -> x `seq` (1 :: Int)) 0 [undefined, []])
    print (foldr (\x acc -> x `seq` (1 :: Int)) 0 [[], undefined])
    print (lastOr [undefined, []])
    print (firstOr [[], undefined])
    print (lastOr [undefined, [], []])
    print (lastOr [undefined, [Just (1 :: Int)]])
    print (lastOr (Just (1 :: Int)))
    print (lastOr [1 :: Int])
    assert (lastOr [undefined, []] == 1) "foldl never demands the suspended first step"
    -- once demanded, the suspended chain runs the steps in order
    print (foldl (\acc x -> acc - x) 100 [1 :: Int, 2, 3])
    assert (foldl (\acc x -> acc - x) 100 [1 :: Int, 2, 3] == 94) "the accumulator chain folds left"
