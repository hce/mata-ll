module Main where

-- T12 (2026-10 fresh-eyes review): the dictionary-form copy of an
-- instance method was rewritten with a scope-blind walk, so a clause
-- binder named like a method of the class (`describe (Box label other)`
-- under a class with a `label` method) became a dictionary access of
-- that method — "attempt to index a function value" once `deep` forced
-- the dictionary form. Every binder site named after a method: clause
-- pattern, where-binding parameter, let, lambda, case alternative,
-- where-bound value; checked through the dictionary form (`deep`) and
-- the specialized form (the direct calls).

data Box a = Box a a

class Describe a where
    describe :: a -> String
    label :: a -> String
    tag :: a -> String

instance Describe Int where
    describe n = show n
    label _ = "int"
    tag _ = "i"

instance Describe a => Describe (Box a) where
    describe (Box label other) = inner label <> "|" <> viaLet <> "|" <> viaLam <> "|" <> viaCase <> "|" <> viaWhere
      where
        inner tag = describe tag
        viaLet = let label = describe other in label
        viaLam = (\tag -> describe tag) other
        viaCase = case other of
            label -> describe label
        viaWhere = describe label
    label _ = "box"
    tag _ = "b"

deep :: Describe a => Int -> a -> String
deep 0 x = describe x
deep n x = deep (n - 1) (Box x x)

main :: IO ()
main = do
    putStrLn (deep 1 (5 :: Int))
    putStrLn (deep 2 (5 :: Int))
    putStrLn (describe (Box (1 :: Int) 2))
    putStrLn (label (Box (1 :: Int) 2))
    putStrLn (tag (7 :: Int))

-- expect: 5|5|5|5|5
-- expect: 5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5|5
-- expect: 1|2|2|2|1
-- expect: box
-- expect: i
