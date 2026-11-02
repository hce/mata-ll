-- T17 (2026-10 fresh-eyes review): a clause's GADT refinement is local to
-- that clause. `describe :: Show a => G a -> a -> String` with the refining
-- clause FIRST was rejected as ambiguous ("nothing here determines the
-- type 'a', so no 'Show' instance can be chosen"): the clause's `a ~ Int`
-- was an ordinary binding in the function's substitution, narrowed its
-- type to `G Int -> Int -> String`, and left the next clause's `Show a`
-- over a variable that type no longer mentioned. Now the signature is
-- rigid for the whole clause and a constructor pattern's refinement is a
-- given of that clause alone (GHC's rule): the refining clause may come
-- first, in the middle, or twice under two constraints; its body may use a
-- method the signature does not give (`x + 1` under `Show a` alone is
-- well-typed under a ~ Int), a guard, a where-helper over the refined
-- parameter, a recursive call at the refined type, and a refinement that
-- reaches a parameter bound BEFORE the refining pattern or a nested
-- pattern after it; a structured index (`P :: E a -> E a -> E (a, a)`)
-- matched first leaves the function at its declared type.
data G a where
    GI :: G Int
    GAny :: G a

data H a where
    HI :: H Int
    HB :: H Bool
    HAny :: H a

describe :: Show a => G a -> a -> String
describe GI x = "int " <> show (x + 1)
describe GAny x = "any " <> show x

middle :: Show a => H a -> a -> String
middle HAny x = "any " <> show x
middle HI x = "int " <> show (x * 2)
middle HB x = "bool " <> show (not x)

both :: (Show a, Eq a) => H a -> a -> a -> String
both HI x y = "int " <> show (x + y)
both HB x y = "bool " <> show (x && y)
both HAny x y = "any " <> show (x == y)

guarded :: Show a => a -> G a -> String
guarded x GI | x == 0 = "zero"
             | x < 0 = "neg " <> show (negate x)
guarded x GI = "pos " <> show x
guarded x GAny = "any " <> show x

helper :: Show a => G a -> a -> String
helper GI x = "int " <> twice <> "/" <> show (bump 1)
  where
    twice = show (x * 2)
    bump y = y + x
helper GAny x = "any " <> show x

recur :: Show a => G a -> a -> String
recur GI x = if x > 0 then "int " <> recur GAny (x - 1) else "int zero"
recur GAny x = "any " <> show x

nested :: Show a => G a -> Maybe a -> String
nested GI (Just x) = "int " <> show (x + 1)
nested GAny (Just x) = "any " <> show x
nested _ Nothing = "none"

data E a where
    I :: Int -> E Int
    B :: Bool -> E Bool
    P :: E a -> E a -> E (a, a)

ev :: E a -> a
ev (P x y) = (ev x, ev y)
ev (I n) = n
ev (B b) = b

main :: IO ()
main = do
    putStrLn (describe GI 41)
    putStrLn (describe GAny True)
    putStrLn (describe GAny "s")
    putStrLn (middle HI 21)
    putStrLn (middle HB False)
    putStrLn (middle HAny "m")
    putStrLn (both HI 1 2)
    putStrLn (both HB True False)
    putStrLn (both HAny "a" "a")
    putStrLn (guarded 0 GI)
    putStrLn (guarded (-3) GI)
    putStrLn (guarded 7 GI)
    putStrLn (guarded "g" GAny)
    putStrLn (helper GI 5)
    putStrLn (helper GAny [1, 2 :: Int])
    putStrLn (recur GI 3)
    putStrLn (recur GAny ())
    putStrLn (nested GI (Just 1))
    putStrLn (nested GAny (Just "x"))
    putStrLn (nested GI Nothing)
    print (ev (P (I 1) (I 2)))
    print (ev (B True))
    print (ev (P (B False) (B True)))

-- expect: int 42
-- expect: any True
-- expect: any "s"
-- expect: int 42
-- expect: bool True
-- expect: any "m"
-- expect: int 3
-- expect: bool False
-- expect: any True
-- expect: zero
-- expect: neg 3
-- expect: pos 7
-- expect: any "g"
-- expect: int 10/6
-- expect: any [1,2]
-- expect: int any 2
-- expect: any ()
-- expect: int 2
-- expect: any "x"
-- expect: none
-- expect: (1,2)
-- expect: True
-- expect: (False,True)
