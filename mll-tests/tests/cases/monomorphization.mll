twice :: (a -> a) -> a -> a
twice f x = f (f x)

main :: IO ()
main = do
    assert (twice (+1) 5 == 7) "twice (+1) 5"
    assert (twice (<>"!") "hi" == "hi!!") "twice (<>!) hi"
    assert (twice (*2) 3 == 12) "twice (*2) 3"
