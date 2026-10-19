-- Pattern guards (Haskell 2010 §3.13) on a function that matches a GADT:
-- the guard lowering re-matches the clause patterns in case expressions,
-- so the GADT refinement each clause pattern introduces (`a ~ Int` under
-- `I n`) has to hold inside those cases and across the join points the
-- lowering binds. Regression: the join points were inferred, not checked
-- against the clause's result type, and a case over the rigid index was
-- a type error ("Cannot match 'a' with 'Bool'"). Shapes: a pattern guard
-- next to boolean guards and `otherwise` in one GADT clause, a where
-- binding used from the pattern guard, a pattern guard whose matched
-- value is itself a GADT (inside an existential), a case alternative
-- with a pattern guard over a GADT scrutinee, and a GADT pair constructor
-- so a refinement reaches a tuple result type.
data E a where
    I :: Int -> E Int
    B :: Bool -> E Bool
    P :: E a -> E a -> E (a, a)

data Box where
    Box :: E a -> Box

ev :: E a -> a
ev (I n) | Just m <- Just n, m > 0 = m
ev (I n) = negate n
ev (B b) = b
ev (P x y) = (ev x, ev y)

mixed :: E a -> a
mixed (I n)
    | n > 100 = 100
    | Just m <- half n, m > 0 = m
    | n < 0 = 0
    | otherwise = n
mixed (B b) | b = True
            | Just c <- Just (not b) = c
mixed (P x y) = (mixed x, mixed y)

half :: Int -> Maybe Int
half n = if even n then Just (n `div` 2) else Nothing

withWhere :: E a -> a
withWhere (I n)
    | Just m <- bump n = m
    | otherwise = n
  where
    bump k = if k > 0 then Just (k + offset) else Nothing
    offset = 10
withWhere (B b) = b
withWhere (P x y) = (withWhere x, withWhere y)

guardedByGadt :: Box -> Int -> String
guardedByGadt bx n
    | Box (I k) <- bx, k > n = "int above " <> show k
    | Box (B b) <- bx = "bool " <> show b
    | Box (I k) <- bx = "int " <> show k
    | otherwise = "pair"

caseAlt :: E a -> a
caseAlt e = case e of
    I n | Just m <- Just (n * 2), m > 10 -> m
        | otherwise -> n
    B b -> not b
    P x y -> (caseAlt x, caseAlt y)

main :: IO ()
main = do
    print (ev (I 3))
    print (ev (I (-3)))
    print (ev (B True))
    print (ev (P (I 1) (I (-2))))
    print (mixed (I 200))
    print (mixed (I 8))
    print (mixed (I (-5)))
    print (mixed (I 7))
    print (mixed (B True))
    print (mixed (B False))
    print (mixed (P (B False) (B True)))
    print (withWhere (I 5))
    print (withWhere (I (-5)))
    print (withWhere (B False))
    putStrLn (guardedByGadt (Box (I 9)) 3)
    putStrLn (guardedByGadt (Box (B True)) 3)
    putStrLn (guardedByGadt (Box (I 1)) 3)
    putStrLn (guardedByGadt (Box (P (I 1) (I 2))) 3)
    print (caseAlt (I 7))
    print (caseAlt (I 2))
    print (caseAlt (B True))
    print (caseAlt (P (I 9) (I 2)))

-- expect: 3
-- expect: 3
-- expect: True
-- expect: (1,2)
-- expect: 100
-- expect: 4
-- expect: 0
-- expect: 7
-- expect: True
-- expect: True
-- expect: (True,True)
-- expect: 15
-- expect: -5
-- expect: False
-- expect: int above 9
-- expect: bool True
-- expect: int 1
-- expect: pair
-- expect: 14
-- expect: 2
-- expect: False
-- expect: (18,2)
