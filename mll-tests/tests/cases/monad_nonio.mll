-- Tests for non-IO monads: List and Maybe >>=/>> in various contexts

main :: IO ()
main = do
    -- List monad bind in let-bindings
    let xs = [1, 2, 3] >>= \x -> [x, x * 10]
    assert (xs == [1, 10, 2, 20, 3, 30]) "list >>= basic"

    -- List monad then
    let ys = [1, 2] >> [10, 20]
    assert (ys == [10, 20, 10, 20]) "list >> basic"

    -- Maybe monad bind
    let mj = Just 5 >>= \x -> Just (x + 1)
    assert (mj == Just 6) "maybe >>= Just"

    -- Maybe bind propagates Nothing
    let mn = (Nothing :: Maybe Integer) >>= \x -> Just (x + 1)
    assert (mn == Nothing) "maybe >>= Nothing"

    -- Maybe then
    let mt1 = Just 1 >> Just 2
    assert (mt1 == Just 2) "maybe >> Just Just"
    let mt2 = (Nothing :: Maybe Integer) >> Just 2
    assert (mt2 == (Nothing :: Maybe Integer)) "maybe >> Nothing Just"

    -- Chained list binds
    let zs = [1, 2] >>= \x -> [10, 20] >>= \y -> [x + y]
    assert (zs == [11, 21, 12, 22]) "list >>= chained"

    -- List bind with filter-like behavior
    let evens = [1, 2, 3, 4, 5, 6] >>= \x ->
            if x `mod` 2 == 0 then [x] else []
    assert (evens == [2, 4, 6]) "list >>= filter"

    -- Maybe bind chain
    let chain = Just 10 >>= \x ->
            if x > 5 then Just (x * 2) else Nothing
    assert (chain == Just 20) "maybe >>= chain success"

    let chain2 = Just 3 >>= \x ->
            if x > 5 then Just (x * 2) else Nothing
    assert (chain2 == Nothing) "maybe >>= chain fail"

    -- Non-IO bind used in a where clause
    let result = compute 3
    assert (result == [4, 30]) "list >>= in where"

    putStrLn "."

compute :: Integer -> [Integer]
compute n = xs
    where xs = [n] >>= \x -> [x + 1, x * 10]
