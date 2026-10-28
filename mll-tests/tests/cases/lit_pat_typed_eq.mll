-- An integer literal pattern matches exactly when GHC's definition
-- matches (Haskell 2010 §3.17.2: the scrutinee `== fromInteger k` with
-- the Eq and Num of the scrutinee's type), at every type: a user Num
-- type with a hand-written Eq that is not structural (M compares mod 3)
-- or a derived one (V, the newtype N whose fromInteger scales), nested
-- under a constructor, in a tuple, as a negative literal, in a where/let
-- local function, a lambda, a case, a list-comprehension generator; a
-- Number scrutinee against integer and fractional literals; a big
-- Integer literal; and a literal pattern whose Eq and Num come from an
-- existential constructor's captured dictionaries — in a field of that
-- very pattern, in a case on the unpacked value, and through a
-- dictionary-passing function applied to it. A literal pattern never
-- matched on purpose at a user type before (it compared the value as if
-- it were a bignum).
module Main where

data M = M Int deriving (Show)
instance Eq M where
    (==) (M a) (M b) = (a `mod` 3) == (b `mod` 3)
instance Num M where
    (+) (M a) (M c) = M (a + c)
    (-) (M a) (M c) = M (a - c)
    (*) (M a) (M c) = M (a * c)
    negate (M a) = M (negate a)
    abs (M a) = M (abs a)
    signum (M a) = M (signum a)
    fromInteger i = M (fromInteger i)

data V = V Int deriving (Show, Eq)
instance Num V where
    (+) (V a) (V c) = V (a + c)
    (-) (V a) (V c) = V (a - c)
    (*) (V a) (V c) = V (a * c)
    negate (V a) = V (negate a)
    abs (V a) = V (abs a)
    signum (V a) = V (signum a)
    fromInteger i = V (fromInteger i)

newtype N = N Int deriving (Eq, Show)
instance Num N where
    (+) (N a) (N c) = N (a + c)
    (-) (N a) (N c) = N (a - c)
    (*) (N a) (N c) = N (a * c)
    negate (N a) = N (negate a)
    abs (N a) = N (abs a)
    signum (N a) = N (signum a)
    fromInteger i = N (fromInteger i * 10)

data Box = forall a. (Num a, Eq a, Show a) => Box a

direct :: M -> String
direct 0 = "zero"
direct 1 = "one"
direct _ = "other"

viaCase :: V -> String
viaCase v = case v of
    1 -> "v-one"
    (-2) -> "v-minus-two"
    _ -> "v-other"

nestedJust :: Maybe M -> String
nestedJust (Just 0) = "just-zero"
nestedJust (Just _) = "just-other"
nestedJust Nothing = "nothing"

tup :: (M, N) -> String
tup (1, 20) = "one-twenty"
tup (1, _) = "one-any"
tup (_, 30) = "any-thirty"
tup _ = "neither"

viaGuard :: M -> String
viaGuard m
    | m == 2 = "guard-two"
    | otherwise = "guard-other"

negLit :: N -> String
negLit (-2) = "neg-two"
negLit _ = "not-neg-two"

localFn :: M -> M -> String
localFn a b = go a <> "/" <> let h 1 = "h-one"; h _ = "h-other" in h b
  where
    go 0 = "go-zero"
    go _ = "go-other"

lam :: M -> String
lam = \x -> case x of
    0 -> "lam-zero"
    _ -> "lam-other"

comp :: [M] -> [String]
comp xs = ["hit" | 0 <- xs]

numPat :: Number -> String
numPat 0 = "zero"
numPat 0.5 = "half"
numPat 3 = "three"
numPat _ = "other"

big :: Integer -> String
big 100000000000000000000 = "big"
big (-100000000000000000000) = "minus-big"
big 7 = "seven"
big _ = "not-big"

poly :: (Num a, Eq a) => a -> String
poly 0 = "p-zero"
poly 1 = "p-one"
poly _ = "p-other"

inField :: Box -> String
inField (Box 0) = "box-zero"
inField (Box x) = "box-" <> show x

inCase :: Box -> String
inCase (Box x) = case x of
    2 -> "two"
    _ -> "not-two"

viaPoly :: Box -> String
viaPoly (Box x) = poly x

main :: IO ()
main = do
    putStrLn (direct (M 3))
    putStrLn (direct (M 4))
    putStrLn (direct (M 5))
    putStrLn (viaCase (V 1))
    putStrLn (viaCase (V (-2)))
    putStrLn (viaCase (V 7))
    putStrLn (nestedJust (Just (M 3)))
    putStrLn (nestedJust (Just (M 4)))
    putStrLn (nestedJust Nothing)
    putStrLn (tup (M 4, N 200))
    putStrLn (tup (M 1, N 21))
    putStrLn (tup (M 2, N 300))
    putStrLn (tup (M 2, N 31))
    putStrLn (viaGuard (M 5))
    putStrLn (viaGuard (M 6))
    putStrLn (negLit (N (-20)))
    putStrLn (negLit (N (-2)))
    putStrLn (localFn (M 6) (M 7))
    putStrLn (localFn (M 7) (M 6))
    putStrLn (lam (M 9))
    putStrLn (lam (M 10))
    print (comp [M 0, M 1, M 3, M 6, M 7])
    putStrLn (numPat 0)
    putStrLn (numPat 0.5)
    putStrLn (numPat 3.0)
    putStrLn (numPat 2)
    putStrLn (big (10 ^ 20))
    putStrLn (big (negate (10 ^ 20)))
    putStrLn (big 7)
    putStrLn (big 8)
    putStrLn (poly (M 6))
    putStrLn (poly (V 0))
    putStrLn (poly (N 10))
    putStrLn (poly (0 :: Int))
    putStrLn (poly (1 :: Integer))
    putStrLn (poly (0 :: Number))
    putStrLn (inField (Box (0 :: Int)))
    putStrLn (inField (Box (2.5 :: Number)))
    putStrLn (inField (Box (M 3)))
    putStrLn (inField (Box (M 4)))
    putStrLn (inCase (Box (2 :: Int)))
    putStrLn (inCase (Box (M 5)))
    putStrLn (inCase (Box (M 6)))
    putStrLn (viaPoly (Box (1 :: Integer)))
    putStrLn (viaPoly (Box (7 :: Int)))
    putStrLn (viaPoly (Box (M 6)))
    putStrLn (viaPoly (Box (M 7)))
    putStrLn (viaPoly (Box (N 10)))
