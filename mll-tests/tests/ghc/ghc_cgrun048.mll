-- GHC cgrun048: Concatmap and list monad
-- Tests concatMap and list as a monad via >>=

main :: IO ()
main = do
    -- concatMap basics
    assert (concatMap (\x -> [x, x * 10]) [1, 2, 3] == [1, 10, 2, 20, 3, 30]) "concatMap"
    assert (concatMap (\_ -> ([] :: [Integer])) [1, 2, 3] == []) "concatMap empty"
    assert (concatMap (\x -> [x]) [1, 2, 3] == [1, 2, 3]) "concatMap singleton"

    -- List bind (>>=)
    let result = [1, 2, 3] >>= \x -> [x, x + 10]
    assert (result == [1, 11, 2, 12, 3, 13]) "list >>="

    -- Nested bind
    let pairs = [1, 2, 3] >>= \x -> [10, 20] >>= \y -> [x + y]
    assert (pairs == [11, 21, 12, 22, 13, 23]) "list nested >>="

    -- return for list
    assert ((return 42 :: [Integer]) == [42]) "list return"

    putStrLn "ok"
