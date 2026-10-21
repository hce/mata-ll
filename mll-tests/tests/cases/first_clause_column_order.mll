-- A multi-clause function forces only the columns GHC's left-to-right
-- match would force: the first clause is tried on every call, but it
-- tries its columns left to right and is abandoned at the first column
-- that fails, so a column to the right of a failing one is never forced.
-- Every column the first clause scrutinized used to be forced at entry —
-- `f (x : _) (y : _) = x + y; f _ _ = 0` raised on `f [] (error …)` where
-- GHC prints 0 — and the Prelude's zip3, zipWith3, (<*>) and liftA2 at
-- Maybe and Either had the same shape. Now an entry force stands only on
-- the first clause's columns up to and including its first column that
-- can fail (a tuple or record column to the left cannot fail, so the
-- force behind it stays), the demand row still proves a column every
-- body forces, and every other scrutinized column is forced inside the
-- clause condition in column order. Shapes: the four of the report, three
-- columns with the refutable column first, middle and last and every pair
-- of them, a literal column, a variable column scrutinized only by the
-- second clause, a where-local group, a body-strict pair, a tuple column
-- left of a cons, as- and nested patterns on the right, guards, an
-- instance method, an IO body, the Prelude functions, a partial
-- application. Found by the 2026-10 fresh-eyes review (L7).
-- expect: 0
-- expect: 3
-- expect: 2
-- expect: (1,2)
-- expect: -1
-- expect: (3,-2)
-- expect: (0,7)
-- expect: (0,8)
-- expect: (0,9)
-- expect: (0,0,3)
-- expect: (0,0,4)
-- expect: (0,0,5)
-- expect: (0,0,0,6)
-- expect: (1,2,0)
-- expect: (0,3)
-- expect: (2,3)
-- expect: (6,0)
-- expect: (0,5)
-- expect: (0,6,0)
-- expect: (-1,3,2)
-- expect: (0,3)
-- expect: io fallback
-- expect: 3
-- expect: 0
-- expect: 0
-- expect: [(1,2,3)]
-- expect: []
-- expect: [6]
-- expect: Nothing
-- expect: Left "e"
-- expect: 0

f :: [Int] -> [Int] -> Int
f (x : _) (y : _) = x + y
f _ _ = 0

h :: Int -> Int -> Int
h 0 0 = 1
h _ _ = 2

g :: Maybe Int -> [Int] -> Int
g (Just a) (b : _) = a + b
g Nothing _ = -1
g _ [] = -2

t1 :: [Int] -> [Int] -> [Int] -> Int
t1 (x : _) _ _ = x
t1 _ _ _ = 0

t2 :: [Int] -> [Int] -> [Int] -> Int
t2 _ (y : _) _ = y
t2 _ _ _ = 0

t3 :: [Int] -> [Int] -> [Int] -> Int
t3 _ _ (z : _) = z
t3 _ _ _ = 0

u1 :: [Int] -> [Int] -> [Int] -> Int
u1 (x : _) (y : _) _ = x + y
u1 _ _ _ = 0

u2 :: [Int] -> [Int] -> [Int] -> Int
u2 (x : _) _ (z : _) = x + z
u2 _ _ _ = 0

u3 :: [Int] -> [Int] -> [Int] -> Int
u3 _ (y : _) (z : _) = y + z
u3 _ _ _ = 0

u4 :: [Int] -> [Int] -> [Int] -> Int
u4 (x : _) (y : _) (z : _) = x + y + z
u4 _ _ _ = 0

v :: [Int] -> [Int] -> Int
v (x : _) _ = x
v [] (y : _) = y
v [] [] = 0

w :: [Int] -> [Int] -> Int
w a b = go a b
  where
    go (x : _) (y : _) = x + y
    go _ _ = 0

s :: [Int] -> [Int] -> Int
s (x : _) (y : _) = x + y
s xs ys = length xs + length ys

tp :: (Int, Int) -> [Int] -> Int
tp (a, b) (y : _) = a + b + y
tp _ _ = 0

n1 :: [Int] -> [Int] -> Int
n1 (x : _) ys@(y : _) = x + y + length ys
n1 _ _ = 0

n2 :: Maybe Int -> Maybe (Int, Int) -> Int
n2 (Just a) (Just (b, c)) = a + b + c
n2 _ _ = 0

gd :: [Int] -> [Int] -> Int
gd (x : _) (y : _)
    | x > 0 = x + y
    | otherwise = y
gd _ _ = -1

data T = A | B Int

class Cmb a where
    cmb :: a -> a -> Int

instance Cmb T where
    cmb (B x) (B y) = x + y
    cmb _ _ = 0

io :: [Int] -> [Int] -> IO ()
io (x : _) (y : _) = print (x + y)
io _ _ = putStrLn "io fallback"

main :: IO ()
main = do
    print (f [] (error "f col 2 forced"))
    print (f [1] [2])
    print (h 1 (error "h col 2 forced"))
    print (h 0 0, h 0 1)
    print (g Nothing (error "g col 2 forced"))
    print (g (Just 1) [2], g (Just 1) [])
    print (t1 [] (error "t1 2") (error "t1 3"), t1 [7] (error "t1 2") (error "t1 3"))
    print (t2 (error "t2 1") [] (error "t2 3"), t2 (error "t2 1") [8] (error "t2 3"))
    print (t3 (error "t3 1") (error "t3 2") [], t3 (error "t3 1") (error "t3 2") [9])
    print (u1 [] (error "u1 2") (error "u1 3"), u1 [1] [] (error "u1 3"), u1 [1] [2] (error "u1 3"))
    print (u2 [] (error "u2 2") (error "u2 3"), u2 [1] (error "u2 2") [], u2 [1] (error "u2 2") [3])
    print (u3 (error "u3 1") [] (error "u3 3"), u3 (error "u3 1") [2] [], u3 (error "u3 1") [2] [3])
    print (u4 [] (error "u4 2") (error "u4 3"), u4 [1] [] (error "u4 3"), u4 [1] [2] [], u4 [1] [2] [3])
    print (v [1] (error "v col 2 forced"), v [] [2], v [] [])
    print (w [] (error "w col 2 forced"), w [1] [2])
    print (s [] [1, 2], s [1] [2])
    print (tp (1, 2) [3], tp (1, 2) [])
    print (n1 [] (error "n1 col 2 forced"), n1 [1] [2, 3])
    print (n2 Nothing (error "n2 col 2 forced"), n2 (Just 1) (Just (2, 3)), n2 (Just 1) Nothing)
    print (gd [] (error "gd col 2 forced"), gd [1] [2], gd [0] [2])
    print (cmb A (error "cmb col 2 forced"), cmb (B 1) (B 2))
    io [] (error "io col 2 forced")
    io [1] [2]
    print (length (zip3 [] [] (error "zip3 col 3 forced" :: [Int])))
    print (length (zip3 [1 :: Int] [] (error "zip3 col 3 forced" :: [Int])))
    print (zip3 [1 :: Int] [2 :: Int] [3 :: Int])
    print (zipWith3 (\a b c -> a + b + c) [] (error "zipWith3 col 3 forced") (error "zipWith3 col 4 forced" :: [Int]))
    print (zipWith3 (\a b c -> a + b + c) [1] [2] [3 :: Int])
    print ((Nothing :: Maybe (Int -> Int)) <*> error "ap_Maybe col 2 forced")
    print ((Left "e" :: Either String (Int -> Int)) <*> error "ap_Either col 2 forced")
    let p = f []
    print (p (error "partial col 2 forced"))
