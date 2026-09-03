-- A newtype is transparent at runtime: `N e` IS `e`, so demanding `N e`
-- to WHNF demands `e` (GHC: `N ⊥` is ⊥, newtype patterns are irrefutable).
-- Saturated constructor applications are erased in the IR and the
-- FIRST-CLASS constructor is a forcing identity, so a newtype-typed result
-- is never a raw thunk where the runtime's WHNF-return invariant expects a
-- value. Both shapes below once crashed on "arithmetic on a table value":
-- the head of `map N (map f xs)` was a thunk inside a thunk, and a Num
-- instance's `negate (N a) = N (negate a)` returned a raw thunk to a
-- dictionary-passing caller.

newtype N = N Int deriving (Show, Eq)
newtype Rad = MkRad Number deriving (Show, Eq)

unN :: N -> Int
unN (N a) = a

addOne :: Int -> Int
addOne x = x + 1

apply :: (Int -> N) -> Int -> N
apply f x = f x

rewrap :: N -> N
rewrap (N a) = N a

toRad :: Number -> Rad
toRad d = MkRad (d * 3.141592653589793 / 180)

unRad :: Rad -> Number
unRad (MkRad r) = r

instance Num N where
    (+) (N a) (N b) = N (a + b)
    (-) (N a) (N b) = N (a - b)
    (*) (N a) (N b) = N (a * b)
    negate (N a) = N (negate a)
    abs (N a) = N (abs a)
    signum (N a) = N (signum a)
    fromInteger i = N (fromInteger i)

-- A constrained function: at one type it is specialized; the dictionary
-- form is exercised by dict_builtin_operators.mll past the limit.
arith :: Num a => a -> a -> a
arith x y = (x + y) * 2 - negate y + abs (x - y) * signum x + fromInteger 1

main :: IO ()
main = do
    -- First-class constructor over lazy elements.
    let ys = map N (map addOne [1, 2, 3])
    case ys of
        (N a : _) -> print (a * 2)
        [] -> putStrLn "empty"
    print (map unN ys)
    print (unN (apply N (addOne 41)) + 0)
    print (map unN (map (rewrap . N) [10, 20]))
    -- Laziness: a suspended payload is not forced by building or passing
    -- the wrapper, only by demanding it.
    let lazyN = N (error "must not be forced" :: Int)
    putStrLn "lazyN built"
    print (length [lazyN, N 1])
    print (case lazyN of N _ -> "irrefutable")
    print (fst (unN (N 7), lazyN))
    -- `seq` on a newtype forces the payload (GHC parity).
    r <- try (N (error "seq forces the payload" :: Int) `seq` pure ())
    case r of
        Right () -> error "seq on a newtype must force the payload"
        Left _   -> putStrLn "seq forces the newtype payload"
    -- Constructor named differently from its type.
    print (map unRad (map toRad [0, 180]))
    print (unRad (MkRad 2.5) * 2)
    print (arith (N 1) (N 2))
    print (arith (3 :: Int) 4)
