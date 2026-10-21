-- A callee's strictness row applies to a call only when the call saturates
-- the row. A row says what the callee forces when it RUNS; a partial
-- application builds a closure and forces nothing, so `f x = seq (g x) 1`
-- is lazy in x for a two-parameter g — GHC prints 1 for `f (error …)` —
-- and the row used to be zipped with whatever arguments were supplied,
-- making f strict in x and raising. Shapes: a partial application under
-- seq, under $!, bound by let and where, stored in a data field, passed to
-- a function that never calls it, returned from an eta-padded function, of
-- a where-local and a let-bound function, of a specialized constrained
-- function, of a class method, through dictionary passing from an
-- existential, through a point-free composition; and the saturated twin of
-- each (its force stays), an over-saturated call, a partial application
-- whose argument another use demands. Found by the 2026-10 fresh-eyes
-- review (L8).
-- expect: 1
-- expect: 4
-- expect: 5
-- expect: 6
-- expect: 7
-- expect: 8
-- expect: 9
-- expect: 10
-- expect: 11
-- expect: 12
-- expect: 13
-- expect: 14
-- expect: 15
-- expect: 16
-- expect: [2,3,17,9]
-- expect: [21,22,23,24,25,26,27]
-- expect: [28,29,30,31,33]
-- expect: 32

module Main where

g :: Int -> Int -> Int
g a b = a + b

f :: Int -> Int
f x = seq (g x) 1

h :: Int -> Int
h x = case g x of
    k -> k 1 `seq` 2

apply1 :: (Int -> Int) -> Int
apply1 k = k 5

p :: Int -> Int
p x = apply1 (g x) `seq` 3

q :: Int -> Int
q x = let k = g x in 4

bang :: Int -> Int
bang x = const 5 $! g x

whereBound :: Int -> Int
whereBound x = k `seq` 6
  where
    k = g x

data Box = Box (Int -> Int)

stored :: Int -> Int
stored x = case Box (g x) of
    Box _ -> 7

never :: (Int -> Int) -> Int
never _ = 8

passed :: Int -> Int
passed x = never (g x)

ret :: Int -> Int -> Int
ret x = g x

returned :: Int -> Int
returned x = seq (ret x) 9

whereLocal :: Int -> Int
whereLocal x = seq (go x) 10
  where
    go a b = a * b

whereCapture :: Int -> Int -> Int
whereCapture x y = seq (go y) 11
  where
    go a b = a * b + x

letLocal :: Int -> Int
letLocal x = let go a b = a - b in seq (go x) 12

gn :: Num a => a -> a -> a
gn a b = a + b

spec :: Int -> Int
spec x = seq (gn x) 13

class Comb a where
    comb :: a -> a -> a

instance Comb Int where
    comb a b = a * 10 + b

method :: Int -> Int
method x = seq (comb x) 14

data AnyN = forall a. (Num a, Show a) => AnyN a

dictCall :: AnyN -> Int
dictCall (AnyN v) = seq (gn v) 15

pf :: Int -> Int -> Int
pf = g . succ

pointFree :: Int -> Int
pointFree x = seq (pf x) 16

app2 :: (a -> b) -> a -> b
app2 k x = x `seq` k x

over :: Int -> Int
over x = seq (app2 g x 1) 17

both :: Int -> Int
both x = never (g x) + x

satSeq :: Int -> Int
satSeq x = seq (g x 1) 21

satBang :: Int -> Int
satBang x = const 22 $! g x 1

satWhere :: Int -> Int
satWhere x = k `seq` 23
  where
    k = g x 1

satLet :: Int -> Int
satLet x = let k = g x 1 in k `seq` 24

satStored :: Int -> Int
satStored x = case Box (g x) of
    Box k -> k 1 `seq` 25

satPassed :: Int -> Int
satPassed x = apply1 (g x) `seq` 26

satReturned :: Int -> Int
satReturned x = seq (ret x 1) 27

satWhereLocal :: Int -> Int
satWhereLocal x = seq (go x 1) 28
  where
    go a b = a * b

satLetLocal :: Int -> Int
satLetLocal x = let go a b = a - b in seq (go x 1) 29

satSpec :: Int -> Int
satSpec x = seq (gn x 1) 30

satMethod :: Int -> Int
satMethod x = seq (comb x 1) 31

satDict :: AnyN -> Int
satDict (AnyN v) = seq (gn v v) 32

satPointFree :: Int -> Int
satPointFree x = seq (pf x 1) 33

main :: IO ()
main = do
    print (f (error "boom"))
    print (q (error "boom"))
    print (bang (error "boom"))
    print (whereBound (error "boom"))
    print (stored (error "boom"))
    print (passed (error "boom"))
    print (returned (error "boom"))
    print (whereLocal (error "boom"))
    print (whereCapture 1 (error "boom"))
    print (letLocal (error "boom"))
    print (spec (error "boom"))
    print (method (error "boom"))
    print (dictCall (AnyN (error "boom" :: Int)))
    print (pointFree (error "boom"))
    print (map (\k -> k 1) [h, p, over, both])
    print (map (\k -> k 1) [satSeq, satBang, satWhere, satLet, satStored, satPassed, satReturned])
    print (map (\k -> k 1) [satWhereLocal, satLetLocal, satSpec, satMethod, satPointFree])
    print (satDict (AnyN (1 :: Int)))
