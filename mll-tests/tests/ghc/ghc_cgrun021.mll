-- GHC cgrun021: Higher-order functions
-- Tests function composition, application, and passing functions

apply :: (Integer -> Integer) -> Integer -> Integer
apply f x = f x

compose :: (Integer -> Integer) -> (Integer -> Integer) -> Integer -> Integer
compose f g x = f (g x)

twice :: (Integer -> Integer) -> Integer -> Integer
twice f = compose f f

thrice :: (Integer -> Integer) -> Integer -> Integer
thrice f = compose f (compose f f)

main :: IO ()
main = do
    assert (apply (+1) 5 == 6) "apply"
    assert (compose (*2) (+3) 4 == 14) "compose"
    assert (twice (+1) 0 == 2) "twice"
    assert (thrice (+1) 0 == 3) "thrice"
    assert (twice (*2) 3 == 12) "twice mul"
    assert (thrice (*2) 1 == 8) "thrice mul"

    -- flip
    assert (flip (-)  10 3 == -7) "flip sub"
    assert (flip const 1 2 == 2) "flip const"

    -- id and const
    assert (id 42 == 42) "id"
    assert (const 5 99 == 5) "const"

    putStrLn "ok"
