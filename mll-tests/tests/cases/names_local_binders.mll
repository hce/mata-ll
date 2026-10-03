import Data.IORef

step :: Int -> Int -> Int
step n' n_prime = if n' > 100 then n_prime else step (n' + 100) (n_prime + n')

primes :: Int -> Int
primes y = y' + y_prime * 10 + y'' * 100 + y_prime_prime * 1000
  where
    y' = y + 1
    y_prime = y + 2
    y'' = y + 3
    y_prime_prime = y + 4

keywords :: Int -> Int -> Int -> Int -> Int
keywords end end_ function function_ = end * 1000 + end_ * 100 + function * 10 + function_

underscores :: Int -> Int -> Int -> Int
underscores _x _usr_x usr_x = _x * 100 + _usr_x * 10 + usr_x

flipAll :: Bool -> Bool -> [Bool] -> [Bool]
flipAll not_ error_ xs = if not_ && error_ then map not xs else xs

runtimeNames :: Int -> Int -> Int -> Int -> Int
runtimeNames hashmap_insert show_Int foldl_prime string_mconcat_prim =
    hashmap_insert + show_Int * 2 + foldl_prime * 3 + string_mconcat_prim * 4
        + (if show (hashmap_insert + show_Int) == "3" then 100 else 0) + foldl' (+) 0 [foldl_prime, 1]

luaGlobals :: Int -> Int -> Int -> Int -> [Int] -> Int
luaGlobals pairs select string math table = pairs + select * 2 + string * 3 + math * 4 + sum table

moreGlobals :: Int -> Int -> Int -> Int -> Int
moreGlobals next rawequal tostring setmetatable = next - rawequal + tostring * setmetatable

builtins :: Int -> Int -> Int -> Int -> Int -> Int
builtins map head filter show pure = map + head * 2 + filter * 3 + show * 4 + pure * 5

renamed :: Int -> Int -> Int -> Int -> Int
renamed bsLength hmInsert runST main = bsLength - hmInsert + runST * main

instanceNames :: Int -> Int -> Int -> [Int] -> Bool
instanceNames eq_Int ord_lt__Int add_Integer negate_Int =
    eq_Int == ord_lt__Int || add_Integer < sum negate_Int || negate eq_Int == 0

label :: (Int -> String) -> Int -> String
label show_Int n = show n <> show_Int n

bump :: Int -> String
bump show_Int = show (show_Int + 1)

lifted :: Int -> Maybe Int
lifted fmap_Maybe = fmap (+ fmap_Maybe) (Just fmap_Maybe)

applyNot :: (Bool -> Bool) -> Bool -> [Bool]
applyNot not b = [not b, not (not b)]

render :: ([Int] -> String) -> [Int] -> String
render show xs = show xs <> "!"

positions :: Maybe Int -> [Int] -> Int
positions m xs = case m of
    Just select ->
        let next = select + 1
            table = next * 2
        in sum [pairs + table | pairs <- xs, let string = pairs, string > 0] + rawequal next
    Nothing -> 0
  where
    rawequal tostring = tostring * 3

whole :: [Int] -> Int
whole string@(math : _) = math + length string
whole [] = 0

backticks :: (Int -> Int -> Int) -> (Int -> Int -> Int) -> Int -> Int
backticks div mod n = (n `div` 2) * 10 + (n `mod` 2)

halves :: Int -> (Int, Int)
halves n = (n `div` 2, n `mod` 2)

aire :: Int -> Int -> Int
aire côté largeur = côté * largeur

main :: IO ()
main = do
    k <- pure (3 :: Int)
    print (step 1 2, primes k)
    print ((\end end_ -> end - end_) (10 :: Int) k)
    print (keywords 1 2 k 4, underscores 1 2 k)
    print (flipAll True True [True, False], flipAll True False [True])
    print (runtimeNames 1 2 k 4)
    print (luaGlobals 1 2 k 4 [5, 6], moreGlobals 9 2 k 4)
    print (builtins 1 2 k 4 5, renamed 9 2 k 4)
    print (instanceNames 1 2 k [4], instanceNames 1 1 k [4])
    putStrLn (label (\_ -> "x") 5)
    putStrLn (bump k)
    print (lifted k)
    print (applyNot (\_ -> True) False)
    putStrLn (render (\_ -> "l") [k])
    print (positions (Just k) [1, 2, 0], positions Nothing [1])
    print (whole [k, 4], whole [])
    print (backticks (+) (-) 7, halves 7, backticks (\a _ -> a) (\_ b -> b) k)
    print (aire k 4)
    error_ <- newIORef (0 :: Int)
    modifyIORef error_ (+ k)
    pairs <- readIORef error_
    let print_ = pairs * 2
    print (pairs, print_)
