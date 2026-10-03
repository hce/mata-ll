infixl 6 <.>
(<.>) :: Int -> Int -> Int
a <.> b = a * 10 + b

infixl 6 <->
(<->) :: Int -> Int -> Int
a <-> b = a - b

(.-) :: Int -> Int -> Int
a .- b = a * 2 - b

(-.) :: Int -> Int -> Int
a -. b = a - b * 2

(.:) :: Int -> Int -> Int
a .: b = a * 3 + b

(-:) :: Int -> Int -> Int
a -: b = a * 4 + b

(%%) :: Int -> Int -> Int
a %% b = a * 5 + b

(#) :: Int -> Int -> Int
a # b = a * 6 + b

lt_gt :: Int -> Int -> Int
lt_gt a b = a + b

class Ring a where
    (<+>) :: a -> a -> a
    (<->>) :: a -> a -> a
    (<.>>) :: a -> a -> a

instance Ring Int where
    a <+> b = a + b
    a <->> b = a - b
    a <.>> b = a * b

instance Ring a => Ring [a] where
    xs <+> ys = zipWith (\a b -> a <+> b) xs ys
    xs <->> ys = zipWith (\a b -> a <->> b) xs ys
    xs <.>> ys = zipWith (\a b -> a <.>> b) xs ys

fold3 :: Ring a => Int -> a -> a
fold3 0 x = (x <+> x) <->> (x <.>> x)
fold3 n x = fold3 (n - 1) x <+> x

wrap :: (Ring a, Show a) => Int -> a -> String
wrap 0 x = show ((x <+> x) <->> (x <.>> x))
wrap n x = wrap (n - 1) [x, x <+> x]

main :: IO ()
main = do
    k <- pure (3 :: Int)
    print (k <.> 4, k <-> 4, k .- 4, k -. 4, k .: 4, k -: 4, k %% 4, k # 4, lt_gt k 4)
    print (foldr (\a b -> a <.> b) 0 [1, 2, k], foldr (\a b -> a <-> b) 0 [1, 2, k])
    print (zipWith (\a b -> a .- b) [k] [1], zipWith (\a b -> a -. b) [k] [1], map (.: 1) [k], map (-: 1) [k])
    print (1 <.> 2 <.> k, 9 <-> 2 <-> k, 9 <.> 2 <-> k)
    print (fold3 2 k, fold3 1 [k, 4])
    putStrLn (wrap 2 k)
