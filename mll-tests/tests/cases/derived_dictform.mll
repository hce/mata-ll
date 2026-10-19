module Main where

-- T2 (2026-10 fresh-eyes review): a derived instance of a parameterized
-- type had no dictionary form. The deriving clause registered its
-- instance without a context, so the monomorphizer could not compose a
-- dictionary from it: past the specialization limit (the 17th element
-- type) every direct caller got the generic derived function, whose field
-- methods are the type-erased runtime ones (`Box Red` printed `Box 1`,
-- `Box ()` printed `Box Nothing`), and inside a dictionary-passing body
-- `show (Box x)` printed `Box (2)` for GHC's `Box W2!` while `compare
-- (Box x) (Box y)` crashed on a nil `compare`. A derived instance now
-- records GHC's derived context (`C a` for each parameter that occurs in
-- a field type, nothing for a phantom one — so `Tagged Meters` no longer
-- demands `Show Meters`) and takes the same dictionary-form path as a
-- written `instance Show a => Show (Box a)`. Shapes: 18 list depths and
-- 20 unrelated element types through the derived Show past the limit;
-- derived Eq and Ord past the limit; show/compare/max/min of a derived
-- type inside a dictionary-passing body, at an existential type and
-- under polymorphic recursion (`Deep (Box a)`); nested derived types,
-- a field under a list and a Maybe, a record, two parameters with one
-- phantom, a phantom-only parameter, a derived Bounded product and a
-- derived Enum/Bounded of a parameterized enumeration.

data Color = Red | Green deriving (Show, Eq, Ord)
data Box a = Box a deriving (Show, Eq, Ord, Bounded)
newtype W = W Int deriving (Show, Eq, Ord)
data Q = Q Int Int deriving (Show, Eq, Ord)
data Pair a b = Pair a deriving (Show, Eq, Ord)
data L a = L [a] (Maybe a) deriving (Show, Eq, Ord)
data R a = R { rx :: a, ry :: Int } deriving (Show, Eq, Ord)
data P a = P a a deriving (Show, Eq, Ord, Bounded)
data E a = EA | EB | EC deriving (Show, Eq, Ord, Enum, Bounded)
data Meters = Meters
data Tagged t = Tagged Int deriving (Show, Eq, Ord)
data Deep a = Deep (Box a) deriving (Show)

data V = V Int
instance Show V where
    show (V n) = "V" <> show n <> "!"
instance Eq V where
    (==) (V a) (V b) = (a `mod` 2) == (b `mod` 2)
instance Ord V where
    compare (V a) (V b) = compare (a `mod` 2) (b `mod` 2)

data Some = forall a. (Show a, Ord a) => Some a

viaBox :: (Show a, Ord a) => a -> a -> String
viaBox x y = show (Box x) <> " " <> show (compare (Box x) (Box y)) <> " " <> show (max (Box x) (Box y)) <> " " <> show (min (Box x) (Box y))

useSome :: Some -> String
useSome (Some a) = viaBox a a

showDeep :: Show a => Int -> Deep a -> String
showDeep 0 d = show d
showDeep n (Deep b) = showDeep (n - 1) (Deep (Box b))

eqBox :: Eq a => a -> a -> Bool
eqBox x y = Box (Box x) == Box (Box y)

ordBox :: Ord a => a -> a -> Ordering
ordBox x y = compare (Box (Box x)) (Box (Box y))

main :: IO ()
main = do
    print (Box Red)
    print (Box [Red])
    print (Box [[Red]])
    print (Box [[[Red]]])
    print (Box [[[[Red]]]])
    print (Box [[[[[Red]]]]])
    print (Box [[[[[[Red]]]]]])
    print (Box [[[[[[[Red]]]]]]])
    print (Box [[[[[[[[Red]]]]]]]])
    print (Box [[[[[[[[[Red]]]]]]]]])
    print (Box [[[[[[[[[[Red]]]]]]]]]])
    print (Box [[[[[[[[[[[Red]]]]]]]]]]])
    print (Box [[[[[[[[[[[[Red]]]]]]]]]]]])
    print (Box [[[[[[[[[[[[[Red]]]]]]]]]]]]])
    print (Box [[[[[[[[[[[[[[Red]]]]]]]]]]]]]])
    print (Box [[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]])
    print (Box [[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]])
    print (Box [[[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]]])
    print (Box ())
    print (Box LT)
    print (Box (Left 1 :: Either Int Int))
    print (Box (W 1))
    print (Box (Box Green))
    print (Box (1 :: Int))
    print (Box "s")
    print (Box "c")
    print (Box True)
    print (Box (Just Red))
    print (Box (Nothing :: Maybe Int))
    print (Box (1 :: Int, "x"))
    print (Box [1 :: Int, 2])
    print (Box (Q 1 2))
    print (Box (True, False))
    print (Box (Right "r" :: Either Int String))
    print (Box GT)
    print (Box (Box (W 3)))
    print (Box (Box ()))
    print (Box (-1 :: Int))
    print (Box [Red, Green])
    print (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box (Box Red))))))))))))))))))
    print (Box Red == Box Red, Box [Red] == Box [Green], Box [[Red]] == Box [[Red]], Box [[[Red]]] == Box [[[Green]]])
    print (Box [[[[Red]]]] == Box [[[[Red]]]], Box [[[[[Red]]]]] == Box [[[[[Green]]]]], Box [[[[[[Red]]]]]] == Box [[[[[[Red]]]]]], Box [[[[[[[Red]]]]]]] == Box [[[[[[[Green]]]]]]])
    print (Box [[[[[[[[Red]]]]]]]] == Box [[[[[[[[Red]]]]]]]], Box [[[[[[[[[Red]]]]]]]]] == Box [[[[[[[[[Green]]]]]]]]], Box [[[[[[[[[[Red]]]]]]]]]] == Box [[[[[[[[[[Red]]]]]]]]]], Box [[[[[[[[[[[Red]]]]]]]]]]] == Box [[[[[[[[[[[Green]]]]]]]]]]])
    print (Box [[[[[[[[[[[[Red]]]]]]]]]]]] == Box [[[[[[[[[[[[Red]]]]]]]]]]]], Box [[[[[[[[[[[[[Red]]]]]]]]]]]]] == Box [[[[[[[[[[[[[Green]]]]]]]]]]]]], Box [[[[[[[[[[[[[[Red]]]]]]]]]]]]]] == Box [[[[[[[[[[[[[[Red]]]]]]]]]]]]]], Box [[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]] == Box [[[[[[[[[[[[[[[Green]]]]]]]]]]]]]]])
    print (Box [[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]] == Box [[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]], Box [[[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]]] == Box [[[[[[[[[[[[[[[[[Green]]]]]]]]]]]]]]]]], Box (V 1) == Box (V 3), Box (V 1) /= Box (V 2))
    print (compare (Box Red) (Box Green), compare (Box [Red]) (Box [Green]), compare (Box [[Red]]) (Box [[Red]]), compare (Box [[[Red]]]) (Box [[[Green]]]))
    print (compare (Box [[[[Red]]]]) (Box [[[[Red]]]]), compare (Box [[[[[Red]]]]]) (Box [[[[[Green]]]]]), compare (Box [[[[[[Red]]]]]]) (Box [[[[[[Red]]]]]]), compare (Box [[[[[[[Red]]]]]]]) (Box [[[[[[[Green]]]]]]]))
    print (compare (Box [[[[[[[[Red]]]]]]]]) (Box [[[[[[[[Red]]]]]]]]), compare (Box [[[[[[[[[Red]]]]]]]]]) (Box [[[[[[[[[Green]]]]]]]]]), compare (Box [[[[[[[[[[Red]]]]]]]]]]) (Box [[[[[[[[[[Red]]]]]]]]]]), compare (Box [[[[[[[[[[[Red]]]]]]]]]]]) (Box [[[[[[[[[[[Green]]]]]]]]]]]))
    print (compare (Box [[[[[[[[[[[[Red]]]]]]]]]]]]) (Box [[[[[[[[[[[[Red]]]]]]]]]]]]), compare (Box [[[[[[[[[[[[[Red]]]]]]]]]]]]]) (Box [[[[[[[[[[[[[Green]]]]]]]]]]]]]), compare (Box [[[[[[[[[[[[[[Red]]]]]]]]]]]]]]) (Box [[[[[[[[[[[[[[Red]]]]]]]]]]]]]]), compare (Box [[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]) (Box [[[[[[[[[[[[[[[Green]]]]]]]]]]]]]]]))
    print (compare (Box [[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]]) (Box [[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]]), compare (Box [[[[[[[[[[[[[[[[[Red]]]]]]]]]]]]]]]]]) (Box [[[[[[[[[[[[[[[[[Green]]]]]]]]]]]]]]]]]), compare (Box (V 1)) (Box (V 4)), Box (V 2) < Box (V 3), Box (V 2) <= Box (V 4), max (Box (V 5)) (Box (V 6)), min (Box (V 5)) (Box (V 6)))
    putStrLn (viaBox (V 2) (V 4))
    putStrLn (viaBox (V 1) (V 4))
    putStrLn (viaBox (3 :: Int) 4)
    putStrLn (useSome (Some (V 5)))
    putStrLn (useSome (Some "c"))
    putStrLn (showDeep 0 (Deep (Box (3 :: Int))))
    putStrLn (showDeep 1 (Deep (Box (3 :: Int))))
    putStrLn (showDeep 2 (Deep (Box (3 :: Int))))
    putStrLn (showDeep 2 (Deep (Box (V 3))))
    print (eqBox Red Red, eqBox [Red] [Green], ordBox Red Green, ordBox [Green] [Red])
    print (L [Red, Green] (Just Red), L [Red] Nothing == L [Red] Nothing, compare (L [Red] Nothing) (L [Red] (Just Green)))
    print (R { rx = Box Red, ry = -1 }, R { rx = [Red], ry = 2 } == R { rx = [Red], ry = 2 }, compare (R { rx = Red, ry = 1 }) (R { rx = Red, ry = 0 }))
    print (minBound :: P Bool, maxBound :: P Bool, minBound :: P (E Int), maxBound :: P (Box Bool))
    print ([minBound .. maxBound] :: [E Meters])
    print (fromEnum (EB :: E Int), succ (EA :: E Int), (EB :: E Int) < EC, maxBound :: E Int, compare (EC :: E Meters) EA)
    print (Pair "x" :: Pair String Meters, (Pair "x" :: Pair String Meters) < Pair "y", compare (Pair 1 :: Pair Int Meters) (Pair 1))
    print (Tagged 3 :: Tagged Meters, Tagged 3 == (Tagged 3 :: Tagged Meters), compare (Tagged 3) (Tagged 4 :: Tagged Meters), Box (Tagged 1 :: Tagged Meters))
