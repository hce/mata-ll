module Main where

-- T15 (2026-10 fresh-eyes review): a method's own context in the
-- monomorphizer's two worlds. Specialized: `describeWith·Bool` is a
-- constrained function whose context is the method's `Show b`, so its
-- copies past the specialization limit (twenty `b` types here) go
-- dictionary-passing with a Show dictionary instead of a type-erased
-- shared copy (which showed a constructor's TAG: `T:1` for `T:Red`).
-- Dictionary-passing: a polymorphically recursive `deep` reaches the
-- method through the Describe dictionary of `[a]`, `[[a]]`, …, whose
-- `describeWith` entry is the instance's dictionary form taking the
-- method's Show dictionary after the context's; a concrete-head instance
-- with a monomorphic sibling (`plain·Bool`) and a nullary sibling
-- (`tag·Bool`, a CAF) still has a dictionary form (the siblings are their
-- own); a nullary method with a context of its own (`emptyOf :: Monoid m
-- => f m`) is a closure over its dictionaries in a composed dictionary,
-- and a context instance (`Container (Wrap f)`) at a ground type is
-- composed, not tabled, so the context's dictionary is closed over
-- before the method's own arrive.

class Describe a where
    describeWith :: Show b => a -> b -> String
    plain :: a -> String
    tag :: a

instance Describe Bool where
    describeWith flag b = (if flag then "T:" else "F:") <> show b
    plain flag = if flag then "yes" else "no"
    tag = True

instance Describe a => Describe [a] where
    describeWith xs b = mconcat (map (\x -> describeWith x b) xs) <> "|" <> show b
    plain xs = mconcat (map plain xs)
    tag = [tag]

data Color = Red | Green deriving (Show)

deep :: (Describe a, Show a) => Int -> a -> String
deep 0 x = describeWith x x <> plain x <> describeWith x Red
deep n x = deep (n - 1) [x]

viaTag :: (Describe a, Show b) => a -> b -> String
viaTag x b = describeWith (tag `asTypeOf` x) b

class Container f where
    emptyOf :: Monoid m => f m
    toL :: f a -> [a]

newtype Box a = Box a

instance Container Box where
    emptyOf = Box mempty
    toL (Box a) = [a]

data Two a = Two a a

instance Container Two where
    emptyOf = Two mempty mempty
    toL (Two a b) = [a, b]

newtype Wrap f a = Wrap (f a)

instance Container f => Container (Wrap f) where
    emptyOf = Wrap emptyOf
    toL (Wrap x) = toL x

asF :: f a -> f b -> f a
asF a _ = a

sizes :: Container f => Int -> c -> f Int -> String
sizes 0 _ x = mconcat (toL (emptyOf `asF` x)) <> "|" <> show (length (toL x)) <> show (toL (emptyOf `asF` x) :: [[Int]])
sizes n y x = sizes (n - 1) [y] x

main :: IO ()
main = do
    putStrLn (describeWith True (1 :: Int))
    putStrLn (describeWith True "s")
    putStrLn (describeWith True [1 :: Int])
    putStrLn (describeWith True [True])
    putStrLn (describeWith True (Just (2 :: Int)))
    putStrLn (describeWith True (Just "m"))
    putStrLn (describeWith True (1 :: Int, 2 :: Int))
    putStrLn (describeWith True (1 :: Int, "p"))
    putStrLn (describeWith True ("q", True))
    putStrLn (describeWith True [[1 :: Int]])
    putStrLn (describeWith True [Just (3 :: Int)])
    putStrLn (describeWith True (Just [4 :: Int]))
    putStrLn (describeWith True Red)
    putStrLn (describeWith True [Green])
    putStrLn (describeWith True (Just Red))
    putStrLn (describeWith (tag :: Bool) (Red, Green))
    putStrLn (describeWith True [(1 :: Int, Red)])
    putStrLn (describeWith True (Just (Just Green)))
    putStrLn (describeWith True [[Red]])
    putStrLn (describeWith False (Just [Red]))
    putStrLn (viaTag False [Red, Green])
    putStrLn (viaTag [True] (Just Green))
    putStrLn (deep 0 True)
    putStrLn (deep 1 False)
    putStrLn (deep 3 True)
    putStrLn (mconcat (toL (emptyOf :: Box String)) <> "!")
    print (toL (emptyOf :: Two [Int]))
    print (toL (emptyOf :: Wrap Two [Bool]))
    putStrLn (sizes 0 "c" (Box (1 :: Int)))
    putStrLn (sizes 3 "c" (Two (1 :: Int) 2))
    putStrLn (sizes 2 "c" (Wrap (Two (1 :: Int) 2)))
    putStrLn (sizes 2 "c" (Wrap (Wrap (Box (1 :: Int)))))

-- expect: T:1
-- expect: T:"s"
-- expect: T:[1]
-- expect: T:[True]
-- expect: T:Just 2
-- expect: T:Just "m"
-- expect: T:(1,2)
-- expect: T:(1,"p")
-- expect: T:("q",True)
-- expect: T:[[1]]
-- expect: T:[Just 3]
-- expect: T:Just [4]
-- expect: T:Red
-- expect: T:[Green]
-- expect: T:Just Red
-- expect: T:(Red,Green)
-- expect: T:[(1,Red)]
-- expect: T:Just (Just Green)
-- expect: T:[[Red]]
-- expect: F:Just [Red]
-- expect: T:[Red,Green]
-- expect: T:Just Green|Just Green
-- expect: T:TrueyesT:Red
-- expect: F:[False]|[False]noF:Red|Red
-- expect: T:[[[True]]]|[[[True]]]|[[[True]]]|[[[True]]]yesT:Red|Red|Red|Red
-- expect: !
-- expect: [[],[]]
-- expect: [[],[]]
-- expect: |1[[]]
-- expect: |2[[],[]]
-- expect: |2[[],[]]
-- expect: |1[[]]
