-- Regression: application must respect the callee's real arity.
--
-- A let/where-bound curried lambda `\x -> \y -> e` used to compile to nested
-- one-parameter Lua functions while the application spine `f 1 2` was emitted
-- as one flat call `f(1, 2)` — invoking only the outer layer and silently
-- returning the inner closure instead of the result. The fix makes every
-- lambda emit one Lua function of its full type arity (matching top-level
-- functions and partial-application closures), adds the same discipline to
-- the `$` and `.` operator emissions, and curries function arguments handed
-- to the erased runtime generics (map/zipWith) whose result type variable is
-- instantiated to a function.

-- Top-level function whose body is a nested curried lambda (eta path).
mk2 :: Int -> Int -> Int -> Int
mk2 x = \y -> \z -> x * 100 + y * 10 + z

add :: Int -> Int -> Int
add a b = a + b

double :: Int -> Int
double x = x * 2

myid :: Int -> Int
myid x = x

-- where-bound curried lambda: direct, staged, and partial application.
whereApply :: Int
whereApply = f 1 2
  where f = \x -> \y -> x + y

whereStaged :: Int
whereStaged = (f 1) 2
  where f = \x -> \y -> x + y

wherePartial :: Int
wherePartial = add10 5
  where
    f = \x -> \y -> x + y
    add10 = f 10

main :: IO ()
main = do
    -- staged application of a let-bound curried lambda
    assert ((let addN = \n -> (\x -> x + n) in (addN 10) 5) == 15) "staged"
    -- curried application without parens
    assert ((let f = \x -> \y -> x + y in f 1 2) == 3) "flat"
    -- parenthesized first application
    assert ((let f = \x -> \y -> x + y in (f 1) 2) == 3) "paren-staged"
    -- name the partial application, then apply
    assert ((let f = \x -> \y -> x + y in let g = f 1 in g 2) == 3) "name-then-apply"
    -- partial application result used later
    assert ((let f = \x -> \y -> x + y in let add10 = f 10 in add10 5) == 15) "partial"
    -- partial application in higher-order position
    assert ((let f = \x -> \y -> x + y in map (f 10) [1, 2, 3]) == [11, 12, 13]) "map-partial"
    -- where-bound forms
    assert (whereApply == 3) "where-flat"
    assert (whereStaged == 3) "where-staged"
    assert (wherePartial == 15) "where-partial"
    -- three-argument curried lambda: full, staged, and partial-then-rest
    let f3 = \a -> \b -> \c -> a * 100 + b * 10 + c
    assert (f3 1 2 3 == 123) "f3 full"
    assert (((f3 1) 2) 3 == 123) "f3 staged"
    assert ((let g = f3 1 2 in g 3) == 123) "f3 partial-then-rest"
    -- top-level nested-lambda body (eta-expansion path)
    assert (mk2 1 2 3 == 123) "top-level nested lambda"
    assert ((mk2 1 2) 3 == 123) "top-level nested lambda staged"
    -- curried lambda passed to a compiled generic (foldr is specialized)
    assert (foldr (\x -> \acc -> x + acc) 0 [1, 2, 3] == 6) "foldr curried"
    -- curried lambda passed to the erased runtime map (adapter path):
    -- map applies ONE argument and must get a working 1-ary adder back
    let adds = map (\n -> \x -> x + n) [1, 5, 10]
    assert (map (\g -> g 42) adds == [43, 47, 52]) "map curried lambda"
    -- top-level 2-ary function passed to runtime map (adapter path)
    assert (map (\g -> g 5) (map add [1, 2]) == [6, 7]) "map top-level 2-ary"
    -- zipWith whose result type is a function (adapter path)
    let fs = zipWith (\a -> \b -> \c -> a + b + c) [1, 2] [10, 20]
    assert (map (\g -> g 100) fs == [111, 122]) "zipWith curried"
    -- composition applied to two arguments in one spine
    assert ((add . double) 3 4 == 10) "composition 2-arg"
    -- $ whose result is still a function
    assert ((add $ 1) 2 == 3) "dollar partial"
    -- direct cheap calls first, then a hidden thunked call site ($ / .):
    -- the call-site analysis must not judge the params always-cheap
    assert (add 1 2 == 3) "add direct"
    assert (double 3 == 6) "double direct"
    assert (myid 4 == 4) "myid direct"
    assert ((double . myid) (sum [1, 2, 3]) == 12) "composition thunked arg"
    assert ((double $ sum [1, 2, 3]) == 12) "dollar thunked arg"
    putStrLn "ok"
