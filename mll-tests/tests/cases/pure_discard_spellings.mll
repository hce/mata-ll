-- A discarded `pure x` / `return x` statement in every spelling: the plain
-- application, the `$` form, and parenthesised forms, in `do` statements
-- and on the left of `>>`. Each is effect-free and yields nothing
-- observable, so the emitter drops it; the `return $ e` spelling once
-- reached the statement emitter as a bare payload (the Lua statement `5`),
-- which failed to LOAD. The payloads are kept lazy: a discarded `return ⊥`
-- must not raise (GHC: `return undefined >> act` runs `act`).

count :: Int -> IO Int
count n = do
    return $ n + 1
    pure $ (n * 2)
    (return (n - 1))
    return $ (error "never demanded" :: Int)
    pure $ n

main :: IO ()
main = do
    return $ (5 :: Int)
    pure $ "unused"
    (pure ())
    return (error "discarded bottom" :: Int)
    putStrLn "after discards"
    n <- count 20
    print n
    (return $ ()) >> putStrLn "after >> discard"
    (pure $ (1 :: Int)) >> putStrLn "after parenthesised >> discard"
    r <- return $ (n + 1)
    print r
    let act = return $ (7 :: Int) :: IO Int
    v <- act
    print v
