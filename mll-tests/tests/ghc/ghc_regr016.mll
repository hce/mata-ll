-- ghc_regr016: List of functions: map application

-- Apply each function in a list to a value
applyAll :: [a -> b] -> a -> [b]
applyAll [] _     = []
applyAll (f:fs) x = f x : applyAll fs x

-- Apply a value to each function in a list
applyEach :: a -> [a -> b] -> [b]
applyEach _ []     = []
applyEach x (f:fs) = f x : applyEach x fs

-- Compose a list of functions (right to left)
composeAll :: [a -> a] -> a -> a
composeAll []     x = x
composeAll (f:fs) x = f (composeAll fs x)

-- Pipeline: apply list of functions in sequence
pipeline :: [a -> a] -> a -> a
pipeline []     x = x
pipeline (f:fs) x = pipeline fs (f x)

-- Higher-order: create a list of adders
adders :: [Integer] -> [Integer -> Integer]
adders ns = map (\n -> \x -> x + n) ns

-- Create a list of multipliers
multipliers :: [Integer] -> [Integer -> Integer]
multipliers ns = map (\n -> \x -> x * n) ns

main :: IO ()
main = do
    -- applyAll: all functions to one value
    let fns = [(\x -> x + 1), (\x -> x * 2), (\x -> x - 3)] :: [Integer -> Integer]
    assert (applyAll fns 10 == [11, 20, 7]) "applyAll"
    assert (applyAll ([] :: [Integer -> Integer]) 5 == []) "applyAll empty"

    -- applyEach: same as applyAll but arg order flipped
    assert (applyEach 10 fns == [11, 20, 7]) "applyEach"

    -- composeAll (f1 . f2 . f3) x = f1(f2(f3(x)))
    let triple = [\x -> x + 1, \x -> x * 2, \x -> x - 3] :: [Integer -> Integer]
    -- composeAll [+1, *2, -3] 10 = (+1)((* 2)((- 3)(10))) = (+1)((* 2) 7) = (+1) 14 = 15
    assert (composeAll triple 10 == 15) "composeAll"

    -- pipeline [+1, *2, -3] 10 = ((*2)((+1) 10) - 3) = ((+1) 10 = 11, *2 = 22, -3 = 19)
    assert (pipeline triple 10 == 19) "pipeline"

    -- adders
    let adds = adders [1, 5, 10, 100]
    assert (applyAll adds 0 == [1, 5, 10, 100]) "adders at 0"
    assert (applyAll adds 42 == [43, 47, 52, 142]) "adders at 42"

    -- multipliers
    let muls = multipliers [2, 3, 5]
    assert (applyAll muls 4 == [8, 12, 20]) "multipliers at 4"

    -- map over list of functions (meta)
    let doubled = map (\f -> \x -> f (f x)) fns
    assert (applyAll doubled 5 == [7, 20, (-1)]) "doubled fns"

    putStrLn "ok"
