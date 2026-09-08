-- A tower of user classes and instances: complex numbers and
-- polynomials with Num instances, a Semigroup/Monoid pair with mconcat
-- and foldMap, a Pretty class with a default method over instances for
-- base types, lists and pairs, and derived Ord/Enum on a sum type.
-- Leans on: user Num instances driven by literals, Semigroup/Monoid
-- instances on newtypes, instance contexts (Pretty a => Pretty [a]),
-- class default methods, showsPrec with negative operands, deriving
-- Ord across constructors with fields, foldMap/mconcat at user monoids.
data Complex = Complex Number Number
    deriving Eq

instance Show Complex where
    showsPrec d (Complex re im) = showParen (d > 6)
        (showsPrec 7 re . showString (if im < 0 then " - " else " + ") . showsPrec 7 (abs im) . showString "i")

instance Num Complex where
    (+) (Complex a b) (Complex c d) = Complex (a + c) (b + d)
    (-) (Complex a b) (Complex c d) = Complex (a - c) (b - d)
    (*) (Complex a b) (Complex c d) = Complex (a * c - b * d) (a * d + b * c)
    negate (Complex a b) = Complex (negate a) (negate b)
    abs (Complex a b) = Complex (sqrt (a * a + b * b)) 0
    signum (Complex a b) = Complex (signum a) (signum b)
    fromInteger n = Complex (fromInteger n) 0

magnitude :: Complex -> Number
magnitude (Complex a b) = sqrt (a * a + b * b)

-- Polynomials as coefficient lists, lowest degree first.
newtype Poly = Poly [Integer]
    deriving Eq

trim :: [Integer] -> [Integer]
trim = reverse . dropWhile (== 0) . reverse

instance Show Poly where
    show (Poly cs) = case trim cs of
        [] -> "0"
        coeffs -> joinTerms (reverse [term c i | (c, i) <- zip coeffs [0 :: Int ..], c /= 0])
      where
        term c 0 = show c
        term c 1 = show c <> "x"
        term c i = show c <> "x^" <> show i
        joinTerms [] = ""
        joinTerms [t] = t
        joinTerms (t : ts) = t <> " + " <> joinTerms ts

addCoeffs :: [Integer] -> [Integer] -> [Integer]
addCoeffs [] ys = ys
addCoeffs xs [] = xs
addCoeffs (x : xs) (y : ys) = (x + y) : addCoeffs xs ys

instance Num Poly where
    (+) (Poly a) (Poly b) = Poly (trim (addCoeffs a b))
    (-) (Poly a) (Poly b) = Poly (trim (addCoeffs a (map negate b)))
    (*) (Poly a) (Poly b) = Poly (trim [sum [a !! i * b !! (k - i) | i <- [0 .. k], i < length a, k - i < length b] | k <- [0 .. length a + length b - 2]])
    negate (Poly a) = Poly (map negate a)
    abs (Poly a) = Poly (map abs a)
    signum (Poly a) = Poly (take 1 (map signum (trim a)))
    fromInteger n = Poly (trim [n])

x :: Poly
x = Poly [0, 1]

evalPoly :: Poly -> Integer -> Integer
evalPoly (Poly cs) v = foldr (\c acc -> c + v * acc) 0 cs

derivative :: Poly -> Poly
derivative (Poly cs) = Poly (trim (zipWith (*) (drop 1 cs) [1 ..]))

newtype MaxInt = MaxInt Int
    deriving Show

instance Semigroup MaxInt where
    (<>) (MaxInt a) (MaxInt b) = MaxInt (max a b)

instance Monoid MaxInt where
    mempty = MaxInt 0

newtype Ordered = Ordered [Int]
    deriving Show

instance Semigroup Ordered where
    (<>) (Ordered a) (Ordered b) = Ordered (mergeInts a b)

instance Monoid Ordered where
    mempty = Ordered []

mergeInts :: [Int] -> [Int] -> [Int]
mergeInts [] ys = ys
mergeInts xs [] = xs
mergeInts (a : as) (b : bs)
    | a <= b = a : mergeInts as (b : bs)
    | otherwise = b : mergeInts (a : as) bs

class Pretty a where
    pretty :: a -> String
    prettyList :: [a] -> String
    prettyList items = "[" <> joinWith ", " (map pretty items) <> "]"

joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [s] = s
joinWith sep (s : ss) = s <> sep <> joinWith sep ss

instance Pretty Int where
    pretty n = "#" <> show n

instance Pretty Bool where
    pretty True = "yes"
    pretty False = "no"
    prettyList bs = mconcat (map (\b -> if b then "Y" else "N") bs)

instance Pretty a => Pretty [a] where
    pretty = prettyList

instance (Pretty a, Pretty b) => Pretty (a, b) where
    pretty (a, b) = "<" <> pretty a <> " | " <> pretty b <> ">"

instance Pretty a => Pretty (Maybe a) where
    pretty Nothing = "-"
    pretty (Just a) = pretty a

data Priority = Low | Medium Int | High String
    deriving (Show, Eq, Ord)

data Suit = Clubs | Diamonds | Hearts | Spades
    deriving (Show, Eq, Ord, Enum, Bounded)

main :: IO ()
main = do
    let z = Complex 1 2
        w = Complex 3 (-1)
    print (z + w, z - w, z * w, negate z)
    print (z * z * z, abs w, signum w, z + 1, 2 * w)
    print (Just (z * w), [z, w], magnitude (Complex 3 4))
    let p = x * x + 2 * x + 1
        q = x - 1
    print (p, q, p * q, p - p, derivative (p * q))
    print (map (evalPoly p) [0, 1, 2, -3], map (evalPoly (p * q)) [1, 2], evalPoly 7 100)
    print (p == (x + 1) * (x + 1), signum (negate p), abs (negate p))
    print (mconcat (map MaxInt [3, 9, 2]), mconcat ([] :: [MaxInt]), MaxInt 4 <> MaxInt 1)
    print (foldMap (\n -> MaxInt (n * n)) [1, 5, 2], foldMap (\n -> Ordered [n]) [4, 1, 3], mconcat [Ordered [1, 5], Ordered [2, 3], Ordered [0, 9]])
    putStrLn (pretty (3 :: Int))
    putStrLn (pretty [1, 2, 3 :: Int])
    putStrLn (pretty [True, False, True])
    putStrLn (pretty [(1 :: Int, True), (2, False)])
    putStrLn (pretty (Just [Just (1 :: Int), Nothing]))
    putStrLn (pretty ([] :: [Int]) <> pretty ([] :: [Bool]))
    print (Low < Medium 1, Medium 2 < Medium 10, Medium 100 < High "a", High "b" > High "a", maximum [Medium 3, Low, High "", Medium 7])
    print (compare (Medium 5) (Medium 5), [Low, High "x"] < [Low, Medium 0], max (High "a") (Medium 9))
    print ([minBound .. maxBound :: Suit], succ Clubs, pred Spades, [Clubs ..], fromEnum Hearts, toEnum 1 :: Suit)
    print (zip [Clubs ..] [Spades, Hearts ..], [Clubs, Hearts ..])
