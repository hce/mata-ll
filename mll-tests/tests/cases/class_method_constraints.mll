-- Every use of a builtin class method emits that class's constraint, so
-- the ambiguity, missing-context and skolem-evidence checks see Enum,
-- Bounded, Functor, Applicative and Monad uses exactly as they see Show
-- or Num ones (the rejections are pinned in compile_errors.rs). This case
-- pins what stays accepted and runs as under GHC: the constrained
-- definitions used at Int, Bool, Maybe, lists and IO; Haskell 2010
-- defaulting with Enum/Bounded in the constraint set (`succ 1` is Integer);
-- the Enum Bool instance; an existential whose context carries Enum; a
-- where-bound helper under a `Monad m =>` signature binding on the outer
-- monad; do-notation at Maybe in a pure let; the IO generics without
-- signatures in a where-helper; traverse at Maybe; a user Applicative.
module Main where

import Control.Monad (forM_, when)

data Some = forall a. (Show a, Enum a) => Some a

data Id a = Id a deriving Show

instance Functor Id where
    fmap f (Id a) = Id (f a)

instance Applicative Id where
    pure = Id
    (<*>) (Id f) (Id a) = Id (f a)

instance Monad Id where
    (>>=) (Id a) f = f a

next :: Enum a => a -> a
next x = succ x

top :: Bounded a => a
top = maxBound

twice :: Monad m => m Int -> m Int
twice x = go
  where
    go = x >>= \v -> return (v + 1)

thrice :: Monad m => m Int -> m Int
thrice x = do
    v <- x
    return (v + 2)

each :: Functor f => f Int -> f Int
each = fmap (+ 1)

lift :: Applicative f => Int -> f Int
lift n = pure (n * 2)

bump :: Some -> String
bump (Some x) = show (succ x)

main :: IO ()
main = do
    print (next (3 :: Int))
    print (next False)
    print (top :: Bool)
    print (succ 1)
    print (pred 10 + 1)
    print (toEnum 65 :: Int)
    print ([minBound .. maxBound] :: [Bool])
    print (fromEnum True)
    print (pred True)
    print [False ..]
    print [True .. False]
    print [False, True ..]
    print (map fromEnum [LT ..])
    putStrLn (bump (Some (3 :: Int)))
    twice (pure 1) >>= print
    print (twice (Just 1))
    print (twice [1, 2])
    thrice (return 1) >>= print
    print (thrice (Just 1))
    print (each [1, 2])
    print (each (Just 1))
    r <- each (pure 41)
    print r
    print (lift 2 :: Maybe Int)
    l <- lift 3
    print l
    let m = do
            a <- Just 1
            b <- Just 2
            return (a + b)
    print m
    let act = fmap (+ 1) (pure 41)
    v <- act
    print v
    let y = return 3
    w <- y
    print w
    print (traverse (\x -> if x > 0 then Just x else Nothing) [1, 2, 3])
    print (traverse (\x -> if x > 0 then Just x else Nothing) [1, 0, 3])
    print (pure 1 :: Id Int)
    print (Id 1 >>= \x -> return (x + 1))
    print (fmap (+ 1) (Id 41))
    mapM_ print [1, 2]
    forM_ [3] print
    helper 5
  where
    helper n = do
        print n
        when (n > 0) (putStrLn "pos")
        return ()

-- expect: 4
-- expect: True
-- expect: True
-- expect: 2
-- expect: 10
-- expect: 65
-- expect: [False,True]
-- expect: 1
-- expect: False
-- expect: [False,True]
-- expect: []
-- expect: [False,True]
-- expect: [0,1,2]
-- expect: 4
-- expect: 2
-- expect: Just 2
-- expect: [2,3]
-- expect: 3
-- expect: Just 3
-- expect: [2,3]
-- expect: Just 2
-- expect: 42
-- expect: Just 4
-- expect: 6
-- expect: Just 3
-- expect: 42
-- expect: 3
-- expect: Just [1,2,3]
-- expect: Nothing
-- expect: Id 1
-- expect: Id 2
-- expect: Id 42
-- expect: 1
-- expect: 2
-- expect: 3
-- expect: 5
-- expect: pos
