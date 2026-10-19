-- A GADT constructor pattern refines the scrutinee's rigid index for its
-- branch wherever it is matched, not only as a top-level clause pattern:
-- in a case alternative over a parameter of type `E a` (the branch sees
-- `a ~ Int`, so `n :: Int` is the result `a`), in a nested pattern under
-- an existential (`Box (I n)` proves the hidden `a` is Int for that
-- clause), and in a case inside the branch of an existential match. The
-- equality holds in that branch alone: the other alternatives still see
-- every constructor, and the function's own type stays `E a -> a`.
data E a where
    I :: Int -> E Int
    B :: Bool -> E Bool
    P :: E a -> E a -> E (a, a)

data Box where
    Box :: E a -> Box

ev :: E a -> a
ev e = case e of
    I n -> n
    B b -> b
    P x y -> (ev x, ev y)

twice :: E a -> (a, a)
twice e = (ev e, case e of { I n -> n + n; B b -> b && b; P x y -> (ev x, ev y) })

describe :: Box -> String
describe (Box (I n)) = "int " <> show n
describe (Box e) = case e of
    B b -> "bool " <> show b
    _ -> "pair"

sumBoxes :: [Box] -> Int
sumBoxes [] = 0
sumBoxes (Box (I n) : rest) = n + sumBoxes rest
sumBoxes (Box (P x y) : rest) = case (x, y) of
    (I a, I b) -> a + b + sumBoxes rest
    _ -> sumBoxes rest
sumBoxes (_ : rest) = sumBoxes rest

main :: IO ()
main = do
    print (ev (I 3))
    print (ev (B True))
    print (ev (P (B False) (B True)))
    print (twice (I 4))
    print (twice (B True))
    putStrLn (describe (Box (I 1)))
    putStrLn (describe (Box (B False)))
    putStrLn (describe (Box (P (I 1) (I 2))))
    print (sumBoxes [Box (I 1), Box (B True), Box (P (I 10) (I 20)), Box (P (B True) (B False)), Box (I 5)])

-- expect: 3
-- expect: True
-- expect: (False,True)
-- expect: (4,8)
-- expect: (True,True)
-- expect: int 1
-- expect: bool False
-- expect: pair
-- expect: 36
