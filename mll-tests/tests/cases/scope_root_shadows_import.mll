import ScopeLeak

step :: Int -> Int
step x = x * 10

main :: IO ()
main = do
    assert (viaLeak 1 == 3) "an imported module keeps its own step"
    assert (step 1 == 10) "the root's references mean the root's step"
    print (viaLeak 1, step 1)
