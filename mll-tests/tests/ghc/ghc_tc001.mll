-- GHC tc001: Polymorphic identity and const
-- Tests basic polymorphism

myId :: a -> a
myId x = x

myConst :: a -> b -> a
myConst x _ = x

main :: IO ()
main = do
    assert (myId 42 == 42) "id int"
    assert (myId "hello" == "hello") "id string"
    assert (myId True == True) "id bool"
    assert (myId (Just 5) == Just 5) "id maybe"

    assert (myConst 1 "ignored" == 1) "const"
    assert (myConst "kept" 999 == "kept") "const string"

    assert (flip (-) 3 10 == 7) "flip"

    putStrLn "ok"
