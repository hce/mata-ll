module Main where

-- T15 (2026-10 fresh-eyes review): a class method's own context is part
-- of the method's type. A use emits it as a wanted at the instantiated
-- type (checked against the instances, discharged by an enclosing
-- context, satisfied through a superclass), an instance body and a
-- default body may assume it of the method's own variables, and the
-- context travels through a constrained existential. The context used
-- to be discarded at the class declaration, so every body that used it
-- was refused with "the signature does not require". Shapes: `Show b`,
-- `Monoid m` (result position only, with a default body), `(Eq b, Show
-- b)`, `Ord b` whose body uses `==` and `<`, a default body inherited by
-- two instances, a use under `Show b` in the caller's context, and a
-- use inside an existential unpack.

class Describe a where
    describeWith :: Show b => a -> b -> String
    same :: Ord b => a -> b -> b -> String
    same _ x y = if x == y then "same" else (if x < y then "less" else "more")

instance Describe Bool where
    describeWith flag b = (if flag then "T:" else "F:") <> show b

instance Describe Int where
    describeWith n b = show n <> "/" <> show b
    same n x y = show n <> (if x == y then "=" else "/")

class Container f where
    toL :: f a -> [a]
    cfold :: Monoid m => (a -> m) -> f a -> m
    showAll :: Show a => f a -> [String]
    showAll c = map show (toL c)
    def :: Monoid m => f a -> m
    def _ = mempty

newtype Box a = Box a

instance Container Box where
    toL (Box a) = [a]
    cfold f (Box a) = f a

data Two a = Two a a

instance Container Two where
    toL (Two a b) = [a, b]
    cfold f (Two a b) = f a <> f b
    showAll (Two a b) = [show b, show a]

class Tag a where
    tagged :: (Eq b, Show b) => a -> b -> b -> String

instance Tag Int where
    tagged n x y = show n <> (if x == y then "=" else "/") <> show x <> show y

viaCtx :: (Describe a, Show b) => a -> b -> String
viaCtx a b = describeWith a b <> "!"

twice :: Show b => b -> String
twice b = describeWith True b <> describeWith False b

data Some = forall a. Describe a => Some a

unpack :: Some -> String
unpack (Some x) = describeWith x (3 :: Int) <> " " <> describeWith x "q"

main :: IO ()
main = do
    putStrLn (describeWith True (3 :: Int))
    putStrLn (describeWith (7 :: Int) [True, False])
    putStrLn (same True (1 :: Int) 1)
    putStrLn (same True "a" "b")
    putStrLn (same (2 :: Int) "b" "a")
    print (cfold (\x -> [x, x]) (Box (1 :: Int)))
    print (showAll (Box True))
    print (showAll (Two (1 :: Int) 2))
    print (cfold (\x -> [x]) (Two "a" "b"))
    putStrLn (cfold show (Two (1 :: Int) 2))
    putStrLn (def (Box (1 :: Int)) <> "end")
    print (def (Two "c" "d") :: [Int])
    putStrLn (tagged (1 :: Int) "a" "a")
    putStrLn (tagged (2 :: Int) [1 :: Int] [2])
    putStrLn (viaCtx True (3 :: Int))
    putStrLn (twice [True, False])
    putStrLn (twice "s")
    putStrLn (unpack (Some True))
    putStrLn (unpack (Some (7 :: Int)))
    mapM_ (putStrLn . unpack) [Some False, Some (1 :: Int)]

-- expect: T:3
-- expect: 7/[True,False]
-- expect: same
-- expect: less
-- expect: 2/
-- expect: [1,1]
-- expect: ["True"]
-- expect: ["2","1"]
-- expect: ["a","b"]
-- expect: 12
-- expect: end
-- expect: []
-- expect: 1="a""a"
-- expect: 2/[1][2]
-- expect: T:3!
-- expect: T:[True,False]F:[True,False]
-- expect: T:"s"F:"s"
-- expect: T:3 T:"q"
-- expect: 7/3 7/"q"
-- expect: F:3 F:"q"
-- expect: 1/3 1/"q"
