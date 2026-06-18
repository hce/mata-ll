-- Regression test: type aliases must be transparent
-- type aliases should be fully expanded before data type registration,
-- so data constructors using aliases unify with their expansions.
-- (Bug: data types were registered before aliases, so GF stayed opaque)

type Pair = (Integer, Integer)
type IntList = [Integer]
type GF = [Integer]

-- Type alias in a data constructor
data Point = Point GF GF

-- Type alias in function signatures
addPair :: Pair -> Pair -> Pair
addPair (a1, a2) (b1, b2) = (a1 + b1, a2 + b2)

-- Mixing alias and expansion in signatures
fromList :: [Integer] -> IntList
fromList xs = xs

-- Data constructor with alias, used with expansion
mkPoint :: [Integer] -> [Integer] -> Point
mkPoint x y = Point x y

-- Pattern match on data with alias fields, return expansion
getX :: Point -> [Integer]
getX (Point x _) = x

-- Chain: alias -> data -> function -> expansion
processGF :: GF -> Integer
processGF xs = case xs of
    (a:_) -> a
    _     -> 0

wrapAndUnwrap :: [Integer] -> Integer
wrapAndUnwrap xs =
    let p = mkPoint xs [0]
    in processGF (getX p)

main :: IO ()
main = do
    assert (addPair (1, 2) (3, 4) == (4, 6)) "Pair alias"
    assert (fromList [1, 2, 3] == [1, 2, 3]) "IntList alias"
    assert (getX (Point [10, 20] [30]) == [10, 20]) "GF in data constructor"
    assert (wrapAndUnwrap [42, 0, 0] == 42) "alias chain through data"
    assert (processGF [7, 8, 9] == 7) "GF as function param"
    putStrLn "All type alias tests passed!"
