-- Tests for return/pure resolution in ST blocks with various result types
-- Exercises the monomorphizer's return_type_methods resolution

main :: IO ()
main = do
    -- ST returning Int
    let r1 = runST (do
            arr <- newSTArray 1 0
            writeSTArray arr 0 42
            readSTArray arr 0)
    assert (r1 == 42) "ST return Int"

    -- ST returning tuple
    let r2 = runST (do
            arr <- newSTArray 2 0
            writeSTArray arr 0 10
            writeSTArray arr 1 20
            a <- readSTArray arr 0
            b <- readSTArray arr 1
            pure (a, b))
    assert (r2 == (10, 20)) "ST return tuple"

    -- ST returning list (the case that triggered the original bug)
    let r3 = runST (do
            arr <- newSTArray 3 0
            writeSTArray arr 0 1
            writeSTArray arr 1 2
            writeSTArray arr 2 3
            a <- readSTArray arr 0
            b <- readSTArray arr 1
            c <- readSTArray arr 2
            pure [a, b, c])
    assert (r3 == [1, 2, 3]) "ST return list"

    -- ST returning Maybe
    let r4 = runST (do
            arr <- newSTArray 1 0
            writeSTArray arr 0 5
            v <- readSTArray arr 0
            pure (Just v))
    assert (r4 == Just 5) "ST return Maybe"

    -- ST with conditional return
    let r5 = runST (do
            arr <- newSTArray 1 0
            writeSTArray arr 0 100
            v <- readSTArray arr 0
            if v > 50
                then pure "big"
                else pure "small")
    assert (r5 == "big") "ST conditional return"

    -- Nested ST: return from inner computation used in outer
    let r6 = runST (do
            arr <- newSTArray 2 0
            writeSTArray arr 0 3
            writeSTArray arr 1 4
            a <- readSTArray arr 0
            b <- readSTArray arr 1
            pure (a * a + b * b))
    assert (r6 == 25) "ST return expression"

    putStrLn "."
