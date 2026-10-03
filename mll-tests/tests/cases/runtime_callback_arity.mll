module Main where
import Data.IORef
import qualified Data.Map as M

twiceF :: (Int -> Int) -> Int -> Int
twiceF g x = g (g x)

addThen :: Int -> (Int -> Int) -> Int -> Int
addThen n g x = g x + n

addN :: Int -> Int -> Int
addN n x = x + n

bumpWith :: IORef (Int -> Int) -> ((Int -> Int) -> Int -> Int) -> IO ()
bumpWith = modifyIORef

bumpStrict :: ((Int -> Int) -> Int -> Int) -> IORef (Int -> Int) -> IO ()
bumpStrict h r = modifyIORef' r h

adders :: [Int -> Int]
adders = map addN [1, 2, 3]

scaled :: [Int] -> [Int -> Int]
scaled = map (\n x -> x * n)

applyAll :: Int -> [Int -> Int] -> [Int]
applyAll v fs = map (\f -> f v) fs

viaRef :: IORef (Int -> Int) -> Int -> IO Int
viaRef r v = do
    k <- readIORef r
    pure (k v)

main :: IO ()
main = do
    r <- newIORef (\x -> x + (1 :: Int))
    modifyIORef r (\h -> \x -> h x * 2)
    viaRef r 10 >>= print
    modifyIORef r (\h x -> h (h x))
    viaRef r 10 >>= print
    modifyIORef' r twiceF
    viaRef r 0 >>= print
    writeIORef r (\x -> x - 1)
    modifyIORef r (addThen 100)
    viaRef r 10 >>= print
    modifyIORef r $ addThen 1000
    viaRef r 10 >>= print
    modifyIORef' r $ \h x -> h x + h x
    viaRef r 1 >>= print
    flip modifyIORef twiceF r
    viaRef r 0 >>= print
    mapM_ (modifyIORef' r) [twiceF, addThen 7]
    viaRef r 0 >>= print
    writeIORef r id
    bumpWith r (\g x -> g x * 3)
    viaRef r 5 >>= print
    bumpStrict (addThen 2) r
    viaRef r 5 >>= print
    sequence_ [modifyIORef r twiceF, modifyIORef' r (\h -> \x -> h x - 1)]
    viaRef r 5 >>= print
    print (applyAll 10 adders)
    print (applyAll 10 (scaled [1, 2, 3]))
    print (applyAll 10 (fmap addN [4, 5]))
    print (applyAll 10 ((\n x -> x - n) <$> [1, 2]))
    print (applyAll 10 (flip map [1, 2, 3] addN))
    print (applyAll 10 ((map $ addN) [7]))
    print (applyAll 2 (zipWith (\a b x -> a * x + b) [1, 2] [3, 4]))
    print (applyAll 2 (zipWith addThen [1, 2] [(* 10), (+ 1)]))
    print (map (\f -> f 1 2) (map (\n a b -> a + b + n) [5, 6 :: Int]))
    print (applyAll 3 ((map . addThen) 4 [(* 2), (+ 1)]))
    print (foldr (\x k acc -> k (acc + x)) id [1, 2, 3 :: Int] 100)
    print (foldl (\k x acc -> k (acc * 10 + x)) id [1, 2, 3 :: Int] 0)
    print (foldl' (\k x acc -> k (acc - x)) id [1, 2, 3 :: Int] 0)
    print (foldr (\x k acc -> k (acc + x)) id (Just (5 :: Int)) 100)
    let m = M.insertWith (\new old x -> new (old x)) "k" (+ 1) (M.fromList [("k", (* (2 :: Int)))])
    print (fmap (\f -> f 10) (M.lookup "k" m))
    print (fmap (\f -> f 10) (M.lookup "k" (M.adjust twiceF "k" m)))
    print (M.foldrWithKey (\_ f g x -> f (g x)) id m 1)
