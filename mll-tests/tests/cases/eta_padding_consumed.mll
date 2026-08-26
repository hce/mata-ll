module Main where

-- G9: the N-ary convention pads a function's Lua parameter list to its
-- full arrow count and call sites pass outstanding arguments in the SAME
-- flat call — so every clause result must be APPLIED to the padding, and
-- local functions must be padded at all. Unpadded/unconsumed, Lua
-- silently discarded the extra arguments: `pick True f 1 2` returned `f`
-- unapplied (printed as a function value).

-- Multi-clause, two patterns, four arrows.
pick :: Bool -> (Int -> Int -> String) -> Int -> Int -> String
pick True h = h
pick False _ = \_ y -> show y

-- Guarded (single clause, guards route through the matrix emitter).
pickg :: Bool -> (Int -> Int -> String) -> Int -> Int -> String
pickg b h | b = h
          | otherwise = \_ y -> show (y + 1)

-- One pattern, three padding slots, lambda-tower bodies.
deep3 :: Bool -> Int -> Int -> Int -> String
deep3 True = \a -> \b c -> show (a + b + c)
deep3 False = \_ -> \b _ -> show b

-- A where-local with fewer patterns than arrows, called saturated.
viaWhere :: Bool -> (Int -> Int -> String) -> Int -> Int -> String
viaWhere b h = go b h
  where
    go True h2 = h2
    go False _ = \_ y -> show (y * 2)

-- Action-typed padding: the clause result is an IO function.
actEta :: Bool -> Int -> IO ()
actEta True = print
actEta False = \_ -> putStrLn "none"

-- A padded clause with a where group: emitting the local must not
-- clobber the enclosing clause's padding, and the local (`helper`, one
-- pattern, function result) is itself padded.
withWhere :: Bool -> (Int -> Int -> String) -> Int -> Int -> String
withWhere True h = helper h
  where helper k = k
withWhere False _ = \_ y -> show (y + 10)

-- A padded clause whose body is a nested case: the padding applies to
-- the CASE's result, never to the branches of the nested match.
pickc :: Bool -> Int -> (Int -> Int -> String) -> Int -> Int -> String
pickc True n h = case n of
  0 -> h
  _ -> \_ y -> show (y + 3)
pickc False _ _ = \x _ -> show x

-- A padded clause selected through a guard whose condition contains its
-- own match (emitted via a sub-generator, which must not inherit the
-- padding).
pickgc :: Int -> (Int -> Int -> String) -> Int -> Int -> String
pickgc n h
  | (case n of
       0 -> True
       _ -> False) = h
  | otherwise = \_ y -> show (y + 4)

main :: IO ()
main = do
  putStrLn (pick True (\a b -> show (a + b)) 1 2)
  putStrLn (pick False (\a b -> show (a + b)) 1 2)
  putStrLn (pickg True (\a b -> show (a - b)) 5 2)
  putStrLn (pickg False (\a b -> show (a - b)) 5 2)
  putStrLn (deep3 True 1 2 3)
  putStrLn (deep3 False 1 2 3)
  putStrLn (viaWhere True (\a b -> show (a + b)) 3 4)
  putStrLn (viaWhere False (\a b -> show (a + b)) 3 4)
  -- Partial application, applied later.
  let g = pick True (\a b -> show (a * b))
  putStrLn (g 3 4)
  actEta True 7
  actEta False 8
  putStrLn (withWhere True (\a b -> show (a + b)) 20 1)
  putStrLn (withWhere False (\a b -> show (a + b)) 20 1)
  putStrLn (pickc True 0 (\a b -> show (a + b)) 30 2)
  putStrLn (pickc True 9 (\a b -> show (a + b)) 30 2)
  putStrLn (pickc False 9 (\a b -> show (a + b)) 30 2)
  putStrLn (pickgc 0 (\a b -> show (a + b)) 40 3)
  putStrLn (pickgc 9 (\a b -> show (a + b)) 40 3)

-- expect: 3
-- expect: 2
-- expect: 3
-- expect: 3
-- expect: 6
-- expect: 2
-- expect: 7
-- expect: 8
-- expect: 12
-- expect: 7
-- expect: none
-- expect: 21
-- expect: 11
-- expect: 32
-- expect: 5
-- expect: 30
-- expect: 43
-- expect: 7
