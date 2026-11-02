-- A nullary method's dictionary field holds the method's VALUE — a CAF
-- slot, a thunk until something forces it (`myMin = (myMin, myMin)` of the
-- tuple instance is built lazily) — not a function. The backend claimed
-- every dictionary read WHNF, so a callee fed by such a read had its
-- parameter judged always-cheap, bound it bare and returned the raw thunk
-- (`asTypeOf`), which the structural show then indexed as the tuple:
-- "attempt to perform arithmetic on a function value". Found writing the
-- dictionary-world cases for the Bounded tuple instances (T18).
module Main where

class MyBounded a where
    myMin :: a
    myMax :: a

instance MyBounded Int where
    myMin = 0
    myMax = 9

instance MyBounded Bool where
    myMin = False
    myMax = True

instance (MyBounded a, MyBounded b) => MyBounded (a, b) where
    myMin = (myMin, myMin)
    myMax = (myMax, myMax)

data Any = forall a. (MyBounded a, Show a) => Any a

showBoth :: Any -> String
showBoth (Any x) = show (x, myMin `asTypeOf` x, myMax `asTypeOf` x)

lowOf :: Any -> String
lowOf (Any x) = show (myMin `asTypeOf` x)

main :: IO ()
main = do
    print (myMax :: (Int, Bool))
    print (myMin :: (Bool, (Int, Int)))
    putStrLn (showBoth (Any (1 :: Int, True)))
    putStrLn (showBoth (Any (False, (2 :: Int, 3 :: Int))))
    putStrLn (lowOf (Any ((7 :: Int, False), True)))

-- expect: (9,True)
-- expect: (False,(0,0))
-- expect: ((1,True),(0,False),(9,True))
-- expect: ((False,(2,3)),(False,(0,0)),(True,(9,9)))
-- expect: ((0,False),False)
