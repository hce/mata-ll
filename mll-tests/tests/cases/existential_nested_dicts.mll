-- A constrained-existential constructor pattern binds each captured
-- dictionary to ITS OWN field's hidden type. Every existential skolem is
-- named after its constructor's variable, so a nested existential pattern
-- (`B (A y) p q`, both over a variable `a`) contributes a skolem of the
-- same name; the monomorphizer used to pick the first skolem of that name
-- anywhere in the sub-patterns, handed B's Eq dictionary to y's type, and
-- `p == q` ran with no evidence (type-erased table identity: False for
-- two equal W values). The checker now records the skolems it mints on the
-- pattern node and the monomorphizer reads them. Shapes: the nested
-- existential before, after, between and twice beside the own fields; an
-- own field under a nested plain constructor (`Just x`, a tuple) and
-- under a nested existential's ordinary field; wildcard, as-pattern and
-- parenthesized own fields; both contexts used in one body; an
-- existential inside an existential's field; the Ord superclass; a case
-- alternative and a lambda; a dictionary-passing body (17 types).

data W = W Int
instance Eq W where
    (==) (W a) (W b) = (a `mod` 2) == (b `mod` 2)
instance Ord W where
    compare (W a) (W b) = compare (a `mod` 2) (b `mod` 2)
instance Show W where
    show (W a) = "W" <> show a

data A = forall a. Show a => A a
data B = forall a. Eq a => B A a a
data C = forall a. Eq a => C a a A
data M = forall a. Eq a => M a A a
data N = forall a. Eq a => N A A a a
data J = forall a. Eq a => J A (Maybe a) a
data P = forall a. Eq a => P (a, a) A
data G b = forall a. Show a => G a b
data H = forall a. Eq a => H (G a) a
data D = forall a. Eq a => D a a B
data O = forall a. Ord a => O A a a
data K1 = K1 deriving Show
data K2 = K2 deriving Show
data K3 = K3 deriving Show
data K4 = K4 deriving Show
data K5 = K5 deriving Show
data K6 = K6 deriving Show
data K7 = K7 deriving Show
data K8 = K8 deriving Show
data K9 = K9 deriving Show
data K10 = K10 deriving Show
data K11 = K11 deriving Show
data K12 = K12 deriving Show
data K13 = K13 deriving Show
data K14 = K14 deriving Show
data K15 = K15 deriving Show
data K16 = K16 deriving Show
data K17 = K17 deriving Show

before :: B -> Bool
before (B (A _) p q) = p == q

after :: C -> Bool
after (C p q (A _)) = p == q

between :: M -> Bool
between (M p (A _) q) = p == q

twice :: N -> Bool
twice (N (A _) (A _) p q) = p == q

underJust :: J -> Bool
underJust (J (A _) (Just x) p) = x == p
underJust (J (A _) Nothing _) = False

underTuple :: P -> Bool
underTuple (P (x, y) (A _)) = x == y

underNested :: H -> String
underNested (H (G x y) p) = show x <> show (y == p)

wild :: B -> String
wild (B (A y) _ q) = show y <> ":" <> show (q == q)

asPat :: B -> String
asPat (B (A y) p@_ q) = show y <> ":" <> show (p == q)

paren :: B -> String
paren (B (A y) (p) (q)) = show y <> ":" <> show (p == q)

both :: B -> String
both (B (A y) p q) = show y <> " " <> show (p == q)

nested :: D -> String
nested (D p1 p2 (B (A y) q r)) = show (p1 == p2) <> show y <> show (q == r)

ordBoth :: O -> String
ordBoth (O (A y) p q) = show (p < q, p == q, compare p q, max p q == q) <> show y

byCase :: B -> Bool
byCase b = case b of
    B (A _) p q -> p == q

byLambda :: B -> Bool
byLambda = \(B (A _) p q) -> p == q

viaDict :: Show b => b -> B -> String
viaDict t (B (A y) p q) = show t <> show y <> show (p == q)

main :: IO ()
main = do
    print (before (B (A (1 :: Int)) (W 2) (W 4)))
    print (before (B (A "s") (W 1) (W 2)))
    print (after (C (W 2) (W 4) (A "c")))
    print (after (C (W 2) (W 3) (A "c")))
    print (between (M (W 3) (A True) (W 5)))
    print (between (M (W 3) (A True) (W 6)))
    print (twice (N (A (1 :: Int)) (A "two") (W 2) (W 6)))
    print (twice (N (A (1 :: Int)) (A "two") (W 2) (W 7)))
    print (underJust (J (A ()) (Just (W 2)) (W 4)))
    print (underJust (J (A ()) (Just (W 2)) (W 5)))
    print (underJust (J (A ()) Nothing (W 4)))
    print (underTuple (P (W 1, W 3) (A "x")))
    print (underTuple (P (W 1, W 4) (A "x")))
    putStrLn (underNested (H (G "g" (W 2)) (W 4)))
    putStrLn (underNested (H (G "g" (W 2)) (W 5)))
    putStrLn (wild (B (A (1 :: Int)) (W 2) (W 4)))
    putStrLn (asPat (B (A (1 :: Int)) (W 2) (W 4)))
    putStrLn (asPat (B (A (1 :: Int)) (W 2) (W 5)))
    putStrLn (paren (B (A (1 :: Int)) (W 2) (W 4)))
    putStrLn (both (B (A "yy") (W 2) (W 4)))
    putStrLn (both (B (A "yy") (W 2) (W 5)))
    putStrLn (nested (D (W 2) (W 4) (B (A "q") (W 1) (W 3))))
    putStrLn (nested (D (W 2) (W 5) (B (A "q") (W 1) (W 4))))
    putStrLn (ordBoth (O (A (5 :: Int)) (W 1) (W 2)))
    putStrLn (ordBoth (O (A (5 :: Int)) (W 2) (W 1)))
    putStrLn (ordBoth (O (A (5 :: Int)) (W 3) (W 1)))
    print (byCase (B (A "c") (W 4) (W 6)))
    print (byCase (B (A "c") (W 4) (W 7)))
    print (byLambda (B (A "c") (W 4) (W 6)))
    print (byLambda (B (A "c") (W 4) (W 7)))
    putStrLn (viaDict K1 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K2 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K3 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K4 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K5 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K6 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K7 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K8 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K9 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K10 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K11 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K12 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K13 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K14 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K15 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K16 (B (A "z") (W 2) (W 4)))
    putStrLn (viaDict K17 (B (A "z") (W 2) (W 5)))
    putStrLn (viaDict (0 :: Int) (B (A "z") (W 2) (W 4)))
