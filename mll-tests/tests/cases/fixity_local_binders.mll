infixr 5 `minus`
minus :: Int -> Int -> Int
minus a b = a - b

infixr 5 <->
(<->) :: Int -> Int -> Int
a <-> b = a - b

viaParam :: (Int -> Int -> Int) -> Int
viaParam minus = 10 `minus` 3 `minus` 2

viaWhere :: Int -> Int
viaWhere x = x `minus` 3 `minus` 2
  where
    minus a b = a - b

viaLet :: Int -> Int
viaLet x =
    let minus a b = a - b
    in x `minus` 3 `minus` 2

viaLambda :: Int -> Int
viaLambda x = (\minus -> x `minus` 3 `minus` 2) (-)

viaCase :: Maybe (Int -> Int -> Int) -> Int
viaCase m = case m of
    Just minus -> 10 `minus` 3 `minus` 2
    Nothing -> 10 `minus` 3 `minus` 2

viaDo :: IO Int
viaDo = do
    minus <- pure (-)
    pure (10 `minus` 3 `minus` 2)

viaSection :: (Int -> Int -> Int) -> Int
viaSection minus = (10 `minus` 3 `minus`) 2

viaPreludeName :: (Int -> [Int] -> [Int]) -> [Int]
viaPreludeName elem = 2 `elem` [1] ++ [3]

bump :: Int -> [Int] -> [Int]
bump n xs = map (+ n) xs

outside :: Int
outside = 10 `minus` 3 `minus` 2

main :: IO ()
main = do
    assert (outside == 9) "the top-level function groups by its declaration"
    assert (10 <-> 3 <-> 2 == 9) "and so does the top-level operator"
    assert (viaParam (-) == 5) "a parameter of that name is infixl 9"
    assert (viaWhere 10 == 5) "a where-bound function of that name is infixl 9"
    assert (viaLet 10 == 5) "a let-bound function of that name is infixl 9"
    assert (viaLambda 10 == 5) "a lambda parameter of that name is infixl 9"
    assert (viaCase (Just (-)) == 5) "a case-bound variable of that name is infixl 9"
    assert (viaCase Nothing == 9) "the other alternative sees the top-level function"
    r <- viaDo
    assert (r == 5) "a do-bound variable of that name is infixl 9"
    assert (viaSection (-) == 5) "a section over a local binder groups its operand by infixl 9"
    assert (viaPreludeName bump == [3, 3]) "a binder named like a Prelude function does not take its fixity"
    print (outside, 10 <-> 3 <-> 2, viaParam (-), viaWhere 10, viaLet 10, viaLambda 10)
    print (viaCase (Just (-)), viaCase Nothing, r, viaSection (-), viaPreludeName bump)
