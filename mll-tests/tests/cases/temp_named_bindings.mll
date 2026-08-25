-- User bindings spelled like the emitter's own temporaries (_s, _cg,
-- _r, _arg0, …).  sanitize_name now mangles single-leading-underscore
-- names into the disjoint "_usr" namespace, so an emitted temporary can
-- never collide with a user binding by construction; this case pins
-- the shapes that put both in one scope (case scrutinee temps, guarded
-- cases, argument temps).

pick :: Maybe Int -> Int -> Int
pick _s d = case _s of
    Just v -> v + d
    Nothing -> d

guarded :: Int -> Int
guarded _cg
    | _cg > 10 = _cg * 2
    | otherwise = _cg + 100

usesInCase :: Int -> Int
usesInCase _cg = case _cg + 1 of
    v | v > 10 -> v
      | otherwise -> _cg

sums :: Int -> Int -> Int
sums _arg0 _r = _arg0 + _r * 10

main :: IO ()
main = do
    print (pick (Just 5) 1)
    print (pick Nothing 7)
    print (guarded 20)
    print (guarded 3)
    print (usesInCase 3)
    print (usesInCase 100)
    print (sums 2 3)
