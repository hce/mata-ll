-- A clause's eta padding is applied by the clause's result alone. A
-- function with fewer patterns than arrows and a function-valued body is
-- emitted with padding parameters its clause result is applied to (the
-- N-ary convention). The padding travelled as emitter state the first
-- match emitted inside the clause consumed, and the clause's where group
-- is emitted before its result: a where VALUE whose right-hand side is a
-- guarded `case` applied the padding to every branch of that case —
-- `f (a, b) = \x -> x + c where c = case a of n | n > 0 -> n * b |
-- otherwise -> b` raised "attempt to call a number value" where GHC prints
-- 16, in a destructuring single clause and a multi-clause function alike.
-- Shapes: the report's three probes; a where value whose RHS is a guarded
-- case, a plain case, an if, a lambda with a guarded case, a call of a
-- where-local function with guards, a function-valued case applied in the
-- body, a case used lazily, an action, under padding of one and two
-- parameters, in a destructuring single clause, a multi-clause function,
-- a guarded clause (the guard reading the value too), a where-local group
-- that is itself padded, an IO body; a nested case in the clause body;
-- the same shapes without padding. The demand analysis's padded view of
-- a function padded only its plain clauses, so a guarded clause beside a
-- plain one (guardedUse) was one pattern short in the analysis — a
-- debug-build assertion; it now pads every clause and applies each guard
-- result to the padding, as codegen does. Found while fixing the
-- where-group emission.
-- expect: 16
-- expect: 13
-- expect: 18
-- expect: 10
-- expect: 10
-- expect: 18
-- expect: 16
-- expect: 13
-- expect: 16
-- expect: 13
-- expect: 23
-- expect: 7
-- expect: 60
-- expect: 36
-- expect: 206
-- expect: 203
-- expect: 16
-- expect: 13
-- expect: 10
-- expect: 5
-- expect: 23
-- expect: 15
-- expect: 5
-- expect: 18
-- expect: 10
-- expect: 10
-- expect: 80
-- expect: 12
-- expect: 10
-- expect: 26
-- expect: 23
-- expect: 10
-- expect: 14
-- expect: 9
-- expect: 18
-- expect: 10
-- expect: 2
-- expect: 13
-- expect: zero
-- expect: 13
-- expect: 16
-- expect: 18
-- expect: 10
-- expect: 18
-- expect: 10

tupEtaCase :: (Int, Int) -> Int -> Int
tupEtaCase (a, b) = \x -> x + c
  where
    c = case a of
          n | n > 0 -> n * b
            | otherwise -> b

multiEtaCase :: Maybe Int -> Int -> Int
multiEtaCase (Just a) = \x -> x + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0
multiEtaCase Nothing = \x -> x

plainEtaCase :: Int -> Int -> Int
plainEtaCase a = \x -> x + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0

tupPlainCase :: (Int, Int) -> Int -> Int
tupPlainCase (a, b) = \x -> x + c
  where
    c = case a of
          0 -> b
          n -> n * b

tupIf :: (Int, Int) -> Int -> Int
tupIf (a, b) = \x -> x + c
  where
    c = if a > 0 then a * b else b

tupLam :: (Int, Int) -> Int -> Int
tupLam (a, b) = \x -> g x + c
  where
    g = \y -> case y of
                m | m > 5 -> m * a
                  | otherwise -> m
    c = b

tupFnVal :: (Int, Int) -> Int -> Int -> Int
tupFnVal (a, b) = \x y -> h x + h y
  where
    h = case a of
          n | n > 0 -> (* n)
            | otherwise -> (+ b)

tupTwo :: (Int, Int) -> Int -> Int -> Int
tupTwo (a, b) = \x y -> x * y + c
  where
    c = case a of
          n | n > 0 -> n * b
            | otherwise -> b

tupLocalFn :: (Int, Int) -> Int -> Int
tupLocalFn (a, b) = \x -> x + c
  where
    c = scale a
    scale n | n > 0 = n * b
            | otherwise = b

tupLazy :: (Int, Int) -> Int -> Int
tupLazy (a, b) = \x -> if x > 0 then x else x + c
  where
    c = case a of
          n | n > 0 -> n * b
            | otherwise -> b

multiTwo :: Maybe Int -> Int -> Int -> Int
multiTwo (Just a) = \x y -> x + y + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0
multiTwo Nothing = \x y -> x - y

guardedEta :: Int -> Int -> Int
guardedEta a
  | a > 100 = \x -> x
  | otherwise = \x -> x + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0

guardedUse :: Maybe Int -> Int -> Int
guardedUse (Just a)
  | c > 5 = \x -> x * c
  | otherwise = \x -> x + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0
guardedUse Nothing = \x -> x

localPadded :: (Int, Int) -> Int -> Int
localPadded (a, b) = \x -> go (Just a) x + go Nothing x
  where
    go (Just n) = \y -> y + d
      where
        d = case n of
              m | m > 0 -> m * b
                | otherwise -> b
    go Nothing = \y -> y

bodyCase :: Maybe Int -> Int -> Int
bodyCase (Just a) = case a of
  0 -> \x -> x
  n -> \x -> x + n
bodyCase Nothing = \x -> x - 1

ioEta :: Maybe Int -> Int -> IO ()
ioEta (Just a) = \x -> print (x + c)
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0
ioEta Nothing = \x -> print x

actEta :: (Int, Int) -> Int -> IO ()
actEta (a, b) = \x -> act >> print (x + b)
  where
    act = case a of
            n | n > 0 -> print n
              | otherwise -> putStrLn "zero"

tupNoPad :: (Int, Int) -> Int -> Int
tupNoPad (a, b) x = x + c
  where
    c = case a of
          n | n > 0 -> n * b
            | otherwise -> b

multiNoPad :: Maybe Int -> Int -> Int
multiNoPad (Just a) x = x + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0
multiNoPad Nothing x = x

guardedNoPad :: Int -> Int -> Int
guardedNoPad a x
  | a > 100 = x
  | otherwise = x + c
  where
    c = case a of
          n | n > 0 -> n * 2
            | otherwise -> 0

main :: IO ()
main = do
  print (tupEtaCase (2, 3) 10)
  print (tupEtaCase (0, 3) 10)
  print (multiEtaCase (Just 4) 10)
  print (multiEtaCase (Just (-4)) 10)
  print (multiEtaCase Nothing 10)
  print (plainEtaCase 4 10)
  print (tupPlainCase (2, 3) 10)
  print (tupPlainCase (0, 3) 10)
  print (tupIf (2, 3) 10)
  print (tupIf (0, 3) 10)
  print (tupLam (2, 3) 10)
  print (tupLam (2, 3) 4)
  print (tupFnVal (2, 3) 10 20)
  print (tupFnVal (0, 3) 10 20)
  print (tupTwo (2, 3) 10 20)
  print (tupTwo (0, 3) 10 20)
  print (tupLocalFn (2, 3) 10)
  print (tupLocalFn (0, 3) 10)
  print (tupLazy (2, 3) 10)
  print (tupLazy (2, 3) (-1))
  print (multiTwo (Just 4) 10 5)
  print (multiTwo (Just 0) 10 5)
  print (multiTwo Nothing 10 5)
  print (guardedEta 4 10)
  print (guardedEta 200 10)
  print (guardedEta (-1) 10)
  print (guardedUse (Just 4) 10)
  print (guardedUse (Just 1) 10)
  print (guardedUse Nothing 10)
  print (localPadded (2, 3) 10)
  print (localPadded (0, 3) 10)
  print (bodyCase (Just 0) 10)
  print (bodyCase (Just 4) 10)
  print (bodyCase Nothing 10)
  ioEta (Just 4) 10
  ioEta Nothing 10
  actEta (2, 3) 10
  actEta (0, 3) 10
  print (tupNoPad (2, 3) 10)
  print (multiNoPad (Just 4) 10)
  print (multiNoPad Nothing 10)
  print (guardedNoPad 4 10)
  print (guardedNoPad 200 10)
