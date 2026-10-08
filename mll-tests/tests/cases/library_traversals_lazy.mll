import Data.List

newtype Identity a = Identity a

runIdentity :: Identity a -> a
runIdentity (Identity x) = x

instance Functor Identity where
    fmap f (Identity x) = Identity (f x)

instance Applicative Identity where
    pure x = Identity x
    liftA2 f (Identity x) (Identity y) = Identity (f x y)

instance Monad Identity where
    (>>=) (Identity x) k = k x

newtype State s a = State (s -> (a, s))

runState :: State s a -> s -> (a, s)
runState (State f) s = f s

instance Functor (State s) where
    fmap f (State g) = State (\s -> case g s of (a, s2) -> (f a, s2))

instance Applicative (State s) where
    pure x = State (\s -> (x, s))
    liftA2 f (State g) (State h) = State (\s -> case g s of
        (a, s2) -> case h s2 of
            (b, s3) -> (f a b, s3))

instance Monad (State s) where
    (>>=) (State g) k = State (\s -> case g s of (a, s2) -> runState (k a) s2)

tick :: Int -> State [Int] Int
tick x = State (\s -> (x * 2, x : s))

data Pair a = Pair a a
    deriving (Show)

instance Functor Pair where
    fmap f (Pair a b) = Pair (f a) (f b)

instance Foldable Pair where
    foldr f z (Pair a b) = f a (f b z)
    foldl f z (Pair a b) = f (f z a) b

instance Traversable Pair where
    traverse f (Pair a b) = liftA2 (\x y -> Pair x y) (f a) (f b)

step :: Int -> IO () -> IO ()
step n a = do
    r <- try a
    case r of
        Left _ -> putStrLn (show n <> " raised")
        Right _ -> pure ()

boom :: [Int]
boom = error "b"

main :: IO ()
main = do
    step 1 (print (any (\x -> x > 3) [1 :: Int ..], all (\x -> x < 3) [1 :: Int ..], elem 5 [1 :: Int ..], notElem 5 [1 :: Int ..]))
    step 2 (print (or (map (\x -> x > 3) [1 :: Int ..]), and (map (\x -> x < 3) [1 :: Int ..])))
    step 3 (print (elem 1 (1 : boom), any even (1 : 2 : boom), and [True, False, error "b"]))
    step 4 (print (elem 0 (1 : boom)))
    step 5 (print (take 5 (concat (repeat [1 :: Int, 2])), take 3 (concatMap (\x -> [x, x]) [1 :: Int ..])))
    step 6 (print (take 1 (concat ([1 :: Int] : error "b"))))
    step 7 (print (take 1 (concat ([] : error "b" :: [[Int]]))))
    step 8 (print (take 3 [x | x <- [1 :: Int ..], even x], take 3 (foldMap (\x -> [x]) [1 :: Int ..]), take 4 (mconcat (repeat [1 :: Int, 2]))))
    step 9 (print (concat [[1 :: Int], [], [2, 3], []], concatMap (\x -> replicate x x) [0 :: Int, 1, 2, 3]))
    step 10 (putStrLn (foldMap show [1 :: Int, 2, 3]))
    step 11 (print (mapM_ (\x -> if x > 3 then Nothing else Just x) [1 :: Int ..], mapM (\x -> if x > 3 then Nothing else Just x) [1 :: Int ..]))
    step 12 (print (mapM (\x -> if x > 3 then Left x else Right x) [1 :: Int ..], sequence [Right 1, Left "a", Left "b" :: Either String Int]))
    step 13 (print (mapM (\x -> [x, x * 10]) [1 :: Int, 2], sequence [Just 1, Just (2 :: Int)]))
    step 14 (do
        xs <- mapM (\x -> print x >> pure (x * 2)) [1 :: Int, 2, 3]
        print xs
        ys <- mapM (\x -> pure (if x == 2 then error "b" else x)) [1 :: Int, 2, 3]
        print (length ys)
        sequence_ [putStrLn "a", putStrLn "b"]
        mapM_ print [7 :: Int, 8])
    step 15 (do
        _ <- mapM (\x -> if x == 2 then error "stop" else print x) [1 :: Int, 2, 3]
        pure ())
    step 16 (print (mapM (\x -> Just x) (1 : boom)))
    step 17 (print (take 3 (fst (unzip (zip [1 :: Int ..] [1 :: Int ..]))), take 3 (snd (unzip (zip [1 :: Int ..] [2 :: Int ..])))))
    step 18 (print (length (fst (unzip [error "b" :: (Int, Int)]))))
    step 19 (print (unzip [(1 :: Int, "a"), (2, "b")], unzip3 [(1 :: Int, "a", True), (2, "b", False)]))
    step 20 (print (case unzip (error "b" :: [(Int, Int)]) of (_, _) -> 1 :: Int))
    step 21 (print (take 3 (scanr (\x _ -> x) 0 [1 :: Int ..]), scanr (+) 0 [1 :: Int, 2, 3], scanr1 (+) [1 :: Int, 2, 3], take 3 (scanr1 (\x _ -> x) [1 :: Int ..])))
    step 22 (print (scanr (+) 0 ([] :: [Int]), scanr1 (+) ([] :: [Int]), length (scanr (\_ _ -> error "b") 0 [1 :: Int, 2])))
    step 23 (print (take 3 (fst (partition even [1 :: Int ..])), take 3 (snd (partition even [1 :: Int ..])), partition even [1 :: Int .. 10]))
    step 24 (print (case partition even boom of (_, _) -> 1 :: Int))
    step 25 (print (case partition (\_ -> error "b") [1 :: Int] of (_, _) -> 1 :: Int))
    step 26 (print (case partition even (1 : boom) of (_, _) -> 1 :: Int))
    step 27 (print (take 1 (snd (partition even (1 : boom))), length (fst (partition even [])) ))
    step 28 (print (null (fst (partition even [1, 3, error "b"]))))
    print (take 3 (runIdentity (traverse (\x -> Identity (x + 1)) [1 :: Int ..])))
    print (take 3 (runIdentity (mapM (\x -> Identity x) [1 :: Int ..])))
    print (runState (mapM tick [1, 2, 3]) [])
    print (runState (mapM_ tick [1, 2, 3]) [])
    print (runState (traverse tick [4, 5]) [0])
    print (runIdentity (sequence [Identity (1 :: Int), Identity 2]))
    print (mapM (\x -> Just x) (Pair 1 (2 :: Int)), any even (Pair 1 (3 :: Int)), elem 3 (Pair 1 (3 :: Int)), concat (Pair [1 :: Int] [2]))
    print (traverse (\x -> [x, x + 10]) (Pair 1 (2 :: Int)))
    print (and (Just True), or (Right False :: Either Int Bool), elem 3 (Just (3 :: Int)), concat (Just [1 :: Int, 2]), foldMap (\x -> [x]) (Just (1 :: Int)))
    mapM_ print (Just (5 :: Int))
    r <- traverse (\x -> pure (x + 1)) (Right 4 :: Either String Int)
    print r
