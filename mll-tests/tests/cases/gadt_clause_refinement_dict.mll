-- T17 (2026-10 fresh-eyes review), the dictionary-passing world: a
-- constrained GADT function whose refining clause comes first, reached
-- past the specialization cap (`describe` at more than sixteen `a` types
-- through GAny) and through an existential (`Some` packs a `G a` with its
-- `Show a`, so `unpack` calls `describe` at a hidden type with the packed
-- dictionary). In the dictionary form the GI clause's `show (x + 1)` is
-- solved at Int (the refinement `a ~ Int` is that clause's given), the
-- GAny clause's `show x` goes through the Show dictionary, and the function
-- keeps its declared type `G a -> a -> String` so the dictionary convention
-- lands on `a`; two constraints (`both`) take two dictionaries the same way.
data G a where
    GI :: G Int
    GAny :: G a

describe :: Show a => G a -> a -> String
describe GI x = "int " <> show (x + 1)
describe GAny x = "any " <> show x

both :: (Show a, Eq a) => G a -> a -> a -> String
both GI x y = "int " <> show (x + y)
both GAny x y = "any " <> show (x == y)

data Some = forall a. Show a => Some (G a) a

unpack :: Some -> String
unpack (Some g x) = describe g x

data SomeEq = forall a. (Show a, Eq a) => SomeEq (G a) a a

unpackEq :: SomeEq -> String
unpackEq (SomeEq g x y) = both g x y

main :: IO ()
main = do
    putStrLn (describe GI 41)
    putStrLn (describe GAny True)
    putStrLn (describe GAny "s")
    putStrLn (describe GAny (Just (2 :: Int)))
    putStrLn (describe GAny [3 :: Int])
    putStrLn (describe GAny (4 :: Int, True))
    putStrLn (describe GAny (Just True))
    putStrLn (describe GAny [True, False])
    putStrLn (describe GAny (Just [5 :: Int]))
    putStrLn (describe GAny [[6 :: Int]])
    putStrLn (describe GAny (Just (Just (7 :: Int))))
    putStrLn (describe GAny (True, "t"))
    putStrLn (describe GAny [Just (8 :: Int)])
    putStrLn (describe GAny (Just "u"))
    putStrLn (describe GAny ("v", [9 :: Int]))
    putStrLn (describe GAny [(10 :: Int, False)])
    putStrLn (describe GAny (Just (11 :: Int), [12 :: Int]))
    putStrLn (describe GAny [[[13 :: Int]]])
    putStrLn (describe GAny ((), Just ()))
    putStrLn (describe GAny (14 :: Int))
    putStrLn (unpack (Some GI 1))
    putStrLn (unpack (Some GAny "hidden"))
    putStrLn (unpack (Some GAny (Just False)))
    putStrLn (unpackEq (SomeEq GI 2 3))
    putStrLn (unpackEq (SomeEq GAny "p" "p"))
    putStrLn (unpackEq (SomeEq GAny [1 :: Int] [2]))

-- expect: int 42
-- expect: any True
-- expect: any "s"
-- expect: any Just 2
-- expect: any [3]
-- expect: any (4,True)
-- expect: any Just True
-- expect: any [True,False]
-- expect: any Just [5]
-- expect: any [[6]]
-- expect: any Just (Just 7)
-- expect: any (True,"t")
-- expect: any [Just 8]
-- expect: any Just "u"
-- expect: any ("v",[9])
-- expect: any [(10,False)]
-- expect: any (Just 11,[12])
-- expect: any [[[13]]]
-- expect: any ((),Just ())
-- expect: any 14
-- expect: int 2
-- expect: any "hidden"
-- expect: any Just False
-- expect: int 5
-- expect: any True
-- expect: any False
