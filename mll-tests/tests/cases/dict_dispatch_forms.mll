module Main where

-- T5/X1/X2/L14 (2026-10 fresh-eyes review): method dispatch in the
-- dictionary-passing fallback (a function past the specialization limit,
-- polymorphically recursive, or called at an existential type) is one
-- decision for every form a method takes, so a use whose class-variable
-- binding contains a type variable or an existential skolem always goes
-- through evidence. Each form is exercised on the specialized path (a
-- plain call), under polymorphic recursion over a dead variable, and at
-- an existential type: structural operators and named methods over a
-- polymorphic element (`[x] == [y]`, `(x, 1) == (y, 1)`, `Just x == Just
-- y`, `[x] < [y]`, `compare [x] [y]`, `compare (Just x) (Just y)`,
-- `compare (x, 0) (y, 0)`, `max`/`min`, nested shapes), first-class
-- operators (`zipWith (==)`, `foldr (+) 0`, a section), prefix negation
-- at a user Num type and through `instance Num a => Num (W a)`, numeric
-- literals at an existential type, an operator whose class variable sits
-- in the right operand (`2 *. x`), a `Monad m =>` body at IO and ST
-- (the builtin bind as a dictionary value), and a `Traversable t =>` body
-- at list and Maybe (`traverse`'s own Applicative dictionary).

data W = W Int deriving (Show)
instance Eq W where
    (==) (W a) (W b) = (a `mod` 2) == (b `mod` 2)
instance Ord W where
    compare (W a) (W b) = compare b a

data V = V Int Int deriving (Show, Eq)
instance Num V where
    (+) (V a b) (V c d) = V (a + c) (b + d)
    (-) (V a b) (V c d) = V (a - c) (b - d)
    (*) (V a b) (V c d) = V (a * c) (b * d)
    negate (V a b) = V (negate a) (negate b)
    abs (V a b) = V (abs a) (abs b)
    signum (V a b) = V (signum a) (signum b)
    fromInteger i = V (fromInteger i) (fromInteger i)

data Wrap a = Wrap a deriving (Show)
instance Num a => Num (Wrap a) where
    (+) (Wrap a) (Wrap b) = Wrap (a + b)
    (-) (Wrap a) (Wrap b) = Wrap (a - b)
    (*) (Wrap a) (Wrap b) = Wrap (a * b)
    negate (Wrap a) = Wrap (negate a)
    abs (Wrap a) = Wrap (abs a)
    signum (Wrap a) = Wrap (signum a)
    fromInteger i = Wrap (fromInteger i)

class Scale a where
    (*.) :: Int -> a -> a

data P = P Int Int deriving (Show)
instance Scale P where
    (*.) k (P a b) = P (k * a) (k * b)

eqL :: Eq a => Int -> b -> a -> a -> Bool
eqL 0 _ x y = [x] == [y]
eqL n z x y = eqL (n - 1) [z] x y

eqT :: Eq a => Int -> b -> a -> a -> Bool
eqT 0 _ x y = (x, 1 :: Int) == (y, 1)
eqT n z x y = eqT (n - 1) [z] x y

eqM :: Eq a => Int -> b -> a -> a -> Bool
eqM 0 _ x y = Just x == Just y
eqM n z x y = eqM (n - 1) [z] x y

neL :: Eq a => Int -> b -> a -> a -> Bool
neL 0 _ x y = [x, x] /= [y, x]
neL n z x y = neL (n - 1) [z] x y

eqNested :: Eq a => Int -> b -> a -> a -> Bool
eqNested 0 _ x y = [Just (x, [y])] == [Just (y, [x])]
eqNested n z x y = eqNested (n - 1) [z] x y

ltL :: Ord a => Int -> b -> a -> a -> Bool
ltL 0 _ x y = [x] < [y]
ltL n z x y = ltL (n - 1) [z] x y

geM :: Ord a => Int -> b -> a -> a -> Bool
geM 0 _ x y = Just x >= Nothing && Just x <= Just y
geM n z x y = geM (n - 1) [z] x y

cmpL :: Ord a => Int -> b -> a -> a -> Ordering
cmpL 0 _ x y = compare [x] [y]
cmpL n z x y = cmpL (n - 1) [z] x y

cmpM :: Ord a => Int -> b -> a -> a -> Ordering
cmpM 0 _ x y = compare (Just x) (Just y)
cmpM n z x y = cmpM (n - 1) [z] x y

cmpT :: Ord a => Int -> b -> a -> a -> Ordering
cmpT 0 _ x y = compare (x, 0 :: Int) (y, 0)
cmpT n z x y = cmpT (n - 1) [z] x y

cmpT2 :: Ord a => Int -> b -> a -> a -> Ordering
cmpT2 0 _ x y = compare (0 :: Int, x) (0, y)
cmpT2 n z x y = cmpT2 (n - 1) [z] x y

maxL :: Ord a => Int -> b -> a -> a -> ([a], [a])
maxL 0 _ x y = (max [x] [y], min [x] [y])
maxL n z x y = maxL (n - 1) [z] x y

cmpNested :: Ord a => Int -> b -> a -> a -> Ordering
cmpNested 0 _ x y = compare [(x, Just y)] [(x, Just x)]
cmpNested n z x y = cmpNested (n - 1) [z] x y

same :: Eq a => Int -> b -> [a] -> [a] -> [Bool]
same 0 _ xs ys = zipWith (==) xs ys
same n y xs ys = same (n - 1) [y] xs ys

ordered :: Ord a => Int -> b -> [a] -> [a] -> [Bool]
ordered 0 _ xs ys = zipWith (<) xs ys
ordered n y xs ys = ordered (n - 1) [y] xs ys

total :: Num a => Int -> b -> [a] -> a
total 0 _ xs = foldr (+) 0 xs
total n y xs = total (n - 1) [y] xs

sect :: Num a => Int -> b -> [a] -> [a]
sect 0 _ xs = map (+ 1) xs
sect n y xs = sect (n - 1) [y] xs

neg :: Num a => Int -> b -> a -> a
neg 0 _ x = -x
neg n y x = neg (n - 1) [y] x

negWrap :: (Num a, Show a) => Int -> a -> String
negWrap 0 x = show (-x)
negWrap n x = negWrap (n - 1) (Wrap x)

lit :: Num a => Int -> b -> a -> a
lit 0 _ x = x + 1 - 3 * 2
lit n y x = lit (n - 1) [y] x

twice :: Scale a => Int -> b -> a -> a
twice 0 _ x = 2 *. x
twice n y x = twice (n - 1) [y] x

twiceM :: Monad m => Int -> c -> m Int -> m Int
twiceM 0 _ m = m >>= \a -> m >>= \b -> return (a + b)
twiceM n y m = twiceM (n - 1) [y] m

fm :: Monad m => Int -> c -> m Int -> m Int
fm 0 _ m = fmap (+ 1) m
fm n y m = fm (n - 1) [y] m

lift2 :: Monad m => Int -> c -> m Int -> m Int
lift2 0 _ m = liftA2 (+) m (pure 10)
lift2 n y m = lift2 (n - 1) [y] m

seq2 :: Monad m => Int -> c -> m Int -> m Int
seq2 0 _ m = m >> m
seq2 n y m = seq2 (n - 1) [y] m

stCount :: Int
stCount = runST (do
    arr <- newSTArray 1 0
    r <- twiceM 20 () (modifySTArray arr 0 (+ 1) >> readSTArray arr 0)
    pure r)

tr :: Traversable t => Int -> c -> t Int -> Maybe (t Int)
tr 0 _ xs = traverse (\x -> if x > 0 then Just x else Nothing) xs
tr n y xs = tr (n - 1) [y] xs

trIO :: Traversable t => Int -> c -> t Int -> IO (t Int)
trIO 0 _ xs = traverse (\x -> print x >> return (x + 1)) xs
trIO n y xs = trIO (n - 1) [y] xs

mapMM :: (Traversable t, Monad m) => Int -> c -> (Int -> m Int) -> t Int -> m (t Int)
mapMM 0 _ f xs = mapM f xs
mapMM n y f xs = mapMM (n - 1) [y] f xs

sm :: Foldable t => Int -> c -> t Int -> Int
sm 0 _ xs = foldr (+) 0 xs
sm n y xs = sm (n - 1) [y] xs

data Pair = forall a. Ord a => Pair a a
data SomeNum = forall a. (Num a, Show a) => SomeNum a
data SomeFrac = forall a. (Fractional a, Show a) => SomeFrac a
data SomeS = forall a. (Scale a, Show a) => SomeS a
data SomeM = forall m. Monad m => SomeM (m Int) (m Int -> IO ())

exEq :: Pair -> Bool
exEq (Pair x y) = [x] == [y]

exEqT :: Pair -> Bool
exEqT (Pair x y) = (x, 1 :: Int) == (y, 1)

exLt :: Pair -> Bool
exLt (Pair x y) = [x] < [y]

exCmpM :: Pair -> Ordering
exCmpM (Pair x y) = compare (Just x) (Just y)

exCmpT :: Pair -> Ordering
exCmpT (Pair x y) = compare (x, [y]) (y, [x])

exMax :: Pair -> Bool
exMax (Pair x y) = max [x] [y] == [y] && min (Just x) (Just y) == Just x

exSame :: Pair -> [Bool]
exSame (Pair x y) = zipWith (==) [x, y] [y, y]

exTotal :: SomeNum -> String
exTotal (SomeNum x) = show (foldr (+) 0 [x, x])

exNeg :: SomeNum -> String
exNeg (SomeNum x) = show (-x)

exLit :: SomeNum -> String
exLit (SomeNum x) = show (x + 1)

exFrac :: SomeFrac -> String
exFrac (SomeFrac x) = show (x + 0.5)

exTwice :: SomeS -> String
exTwice (SomeS x) = show (3 *. x)

runSome :: SomeM -> IO ()
runSome (SomeM m k) = k (m >>= \a -> return (a * 2))

main :: IO ()
main = do
    print (eqL 0 () (W 2) (W 4), eqT 0 () (W 2) (W 4), eqM 0 () (W 2) (W 4), neL 0 () (W 1) (W 4))
    print (eqL 20 () (W 2) (W 4), eqT 20 () (W 2) (W 4), eqM 20 () (W 2) (W 4), neL 20 () (W 1) (W 4))
    print (eqNested 0 () (W 2) (W 4), eqNested 20 () (W 2) (W 4), eqNested 20 () (W 1) (W 4))
    print (ltL 0 () (W 2) (W 4), geM 0 () (W 2) (W 4), cmpL 0 () (W 2) (W 4), cmpM 0 () (W 2) (W 4))
    print (ltL 20 () (W 2) (W 4), geM 20 () (W 2) (W 4), cmpL 20 () (W 2) (W 4), cmpM 20 () (W 2) (W 4))
    print (cmpT 0 () (W 2) (W 4), cmpT2 0 () (W 2) (W 4), cmpT 20 () (W 2) (W 4), cmpT2 20 () (W 2) (W 4))
    print (maxL 0 () (W 2) (W 4), maxL 20 () (W 2) (W 4))
    print (cmpNested 0 () (W 2) (W 4), cmpNested 20 () (W 2) (W 4), cmpNested 20 () (W 4) (W 2))
    print (same 0 () [W 2, W 3] [W 4, W 4], same 20 () [W 2, W 3] [W 4, W 4])
    print (ordered 0 () [W 2, W 3] [W 4, W 1], ordered 20 () [W 2, W 3] [W 4, W 1])
    print (total 0 () [V 1 2, V 3 4], total 20 () [V 1 2, V 3 4], total 20 () [5 :: Integer, 6])
    print (sect 0 () [V 1 2], sect 20 () [V 1 2], sect 20 () [1 :: Integer])
    print (neg 0 () (V 1 2), neg 20 () (V 1 2), neg 20 () (5 :: Integer), neg 20 () (3 :: Int))
    putStrLn (negWrap 0 (5 :: Int) <> " " <> negWrap 2 (5 :: Int))
    print (lit 0 () (V 1 2), lit 20 () (V 1 2), lit 20 () (10 :: Integer), lit 20 () (10 :: Int))
    print (twice 0 () (P 1 2), twice 20 () (P 1 2))
    r1 <- twiceM 0 () (return 1)
    r2 <- twiceM 20 () (return 21)
    print (r1, r2, twiceM 20 () (Just 1), twiceM 20 () [1, 2])
    r3 <- fm 20 () (return 1)
    r4 <- lift2 20 () (return 2)
    r5 <- seq2 20 () (putStrLn "x" >> return 3)
    print (r3, r4, r5, fm 20 () (Just 1), lift2 20 () [1, 2], stCount)
    print (tr 0 () [1, 2], tr 20 () [1, 2, 3], tr 20 () [1, 0, 3], tr 20 () (Just 4), tr 20 () Nothing)
    r6 <- trIO 20 () [1, 2]
    r7 <- trIO 20 () (Just 9)
    print (r6, r7)
    r8 <- mapMM 20 () (\x -> return (x * 2)) [1, 2, 3]
    print (r8, mapMM 20 () (\x -> if x > 1 then Just x else Nothing) [1, 2], mapMM 20 () (\x -> [x, x + 1]) (Just 1))
    print (sm 0 () [5], sm 20 () [1, 2, 3], sm 20 () (Just 4))
    print (exEq (Pair (W 2) (W 4)), exEqT (Pair (W 2) (W 4)), exLt (Pair (W 2) (W 4)), exCmpM (Pair (W 2) (W 4)))
    print (exCmpT (Pair (W 2) (W 4)), exMax (Pair (W 2) (W 4)), exSame (Pair (W 2) (W 4)))
    putStrLn (exTotal (SomeNum (V 1 2)) <> " " <> exNeg (SomeNum (V 1 2)) <> " " <> exNeg (SomeNum (7 :: Integer)))
    putStrLn (exLit (SomeNum (5 :: Integer)) <> " " <> exLit (SomeNum (5 :: Int)) <> " " <> exFrac (SomeFrac (1.5 :: Number)))
    putStrLn (exTwice (SomeS (P 1 2)))
    runSome (SomeM (return 5) (\m -> m >>= print))
