-- length/sum/product/maximum/minimum are STRICT left folds (GHC parity:
-- base's length/sum/product are foldl', its list maximum/minimum are
-- strict). Over the lazy `foldl` they built an O(n) chain of pending
-- `f acc x` thunks that overflowed the Lua stack on an UNFUSED list of
-- 2.4e5 elements (Lua 5.4) / ~3e4 (LuaJIT) — GHC completes them. `reverse`
-- defeats the list-pipeline fusion, so these folds run the library
-- definitions; 3e5 elements is past both thresholds.
--
-- foldl' is a Foldable class method: the list instance is a direct loop,
-- Maybe/Either are one-step, and an instance that omits it (Tree below)
-- takes the class default — GHC's continuation-passing definition over
-- foldr, the first default body a BUILTIN class carries.

data Tree a = Leaf | Node (Tree a) a (Tree a)

instance Foldable Tree where
    foldr _ z Leaf = z
    foldr f z (Node l x r) = foldr f (f x (foldr f z r)) l
    foldl _ z Leaf = z
    foldl f z (Node l x r) = foldl f (f (foldl f z l) x) r

fromList :: [a] -> Tree a
fromList [] = Leaf
fromList (x:xs) = Node Leaf x (fromList xs)

big :: Int -> [Int]
big n = reverse [1 .. n]

main :: IO ()
main = do
    let xs = big 300000
    print (length xs)
    print (sum xs)
    print (product (map (\x -> if x > 0 then 1 else 2) xs))
    print (maximum xs, minimum xs)
    print (foldl' (\acc x -> acc + x) 0 (big 300000))
    -- Maybe / Either instances, and Integer (bignum) accumulators.
    print (foldl' (+) 0 (Just 5), length (Just "x"), sum (Right 3 :: Either String Int))
    print (sum (Nothing :: Maybe Int), product (Left "e" :: Either String Int))
    print (sum (map toInteger (big 1000)) * 1000000000000)
    -- The class default (Tree has no foldl' of its own): strict, in order.
    let t = fromList [1 .. 50000 :: Int]
    print (sum t, length t, maximum t, minimum t)
    print (foldl' (flip (:)) [] (fromList [1, 2, 3 :: Int]))
    print (foldl' (\acc x -> acc * 10 + x) 0 (fromList [1, 2, 3 :: Int]))
    -- foldl itself stays lazy in the accumulator (GHC parity).
    print (foldl (\acc x -> if x == 0 then 0 else acc) (error "lazy acc") [1, 0 :: Int])
    -- foldl' forces the accumulator at every step: a bottom accumulator
    -- that is never selected still raises.
    r <- try (foldl' (\acc x -> if x == 0 then 0 else acc) (error "strict acc") [1, 0 :: Int] `seq` pure ())
    case r of
        Left _   -> putStrLn "foldl' forces the accumulator"
        Right () -> error "foldl' must force the seeded bottom"
