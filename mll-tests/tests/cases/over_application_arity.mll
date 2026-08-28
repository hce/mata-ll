-- Regression: a call site must respect the CALLEE's arity, not the arrow
-- count of the type at the use.
--
-- Every function is emitted as one Lua function whose parameter list is its
-- DECLARED type's arrow count (clause patterns plus eta padding), and every
-- call site passes all outstanding arguments in one flat call. The two agree
-- because monomorphization gives each instantiation its own copy — except for
-- the shared prelude builtins (`const`, `id`, `flip`) and the erased runtime
-- generics (`head`, `map`, `foldr`), which have one copy for every type. At an
-- instantiation that turns a result type variable into a FUNCTION, such a use
-- carries more arguments than the callee has parameters, and Lua silently
-- discards the excess: `const inc "x" 3` returned `inc` unapplied, and the
-- reported shape `cannot = const inc` printed a function value.
--
-- The fix is threefold and each part is exercised below: mono specializes a
-- builtin whose instantiation widens its arity, codegen splits a call to a
-- fixed-arity callee into a saturating call plus an application of its result,
-- and a first-class reference to such a function is eta-expanded to the arity
-- its type promises.

inc :: Int -> Int
inc a = a + 1

add :: Int -> Int -> Int
add a b = a + b

myConst :: a -> b -> a
myConst x _ = x

applyC :: (a -> b -> a) -> a -> b -> a
applyC f x y = f x y

applyToTen :: (Int -> Int) -> Int
applyToTen f = f 10

-- The reported shape: a point-free definition whose eta padding is applied to
-- a partial application of a builtin.
pointFree :: String -> Int -> Int
pointFree = const inc

-- The same through a bare (unapplied) builtin reference.
pointFreeBare :: (Int -> Int) -> String -> Int -> Int
pointFreeBare = const

main :: IO ()
main = do
    -- the reported bug
    assert (pointFree "x" 1 == 2) "point-free const"
    assert (pointFreeBare inc "x" 2 == 3) "point-free bare const"
    -- saturated over-application of a shared builtin
    assert (const inc "x" 3 == 4) "const"
    assert (id inc 4 == 5) "id"
    assert (flip const "x" inc 5 == 6) "flip"
    assert (const add "x" 1 2 == 3) "const two surplus args"
    -- the erased runtime generics: their result is a function
    assert (head [inc, inc] 6 == 7) "head"
    assert (head (take 1 [inc]) 7 == 8) "take"
    assert (foldr const inc [] 8 == 9) "foldr"
    assert ((([inc] !! 0) $ 9) == 10) "index"
    assert (([const] !! 0) inc "x" 9 == 10) "builtin from a list"
    assert ((zipWith const [inc] ["x"] !! 0) 9 == 10) "zipWith"
    assert ((filter (\_ -> True) [inc] !! 0) 9 == 10) "filter"
    assert (flip id 9 inc == 10) "flip id"
    -- the operator emissions, infix and as applied first-class values (both
    -- reach the emission that knows the operands' static types)
    assert ((id . id) inc 10 == 11) "compose"
    assert ((id $ id) inc 11 == 12) "dollar"
    assert ((($) const inc "x") 12 == 13) "first-class dollar"
    assert (((.) id id) inc 13 == 14) "first-class compose"
    assert (((.) applyToTen id) inc == 11) "first-class compose, widened inner"
    -- a lambda whose eta padding lands on a builtin
    assert ((\_ -> const) 0 inc "x" 12 == 13) "lambda eta padding"
    -- first-class references handed to a higher-order consumer
    assert (applyC const inc "x" 13 == 14) "higher-order const"
    assert (((map id [inc] !! 0) $ 14) == 15) "map adapter"
    assert (map (\g -> g 15) (map id [inc]) == [16]) "map adapter, mapped again"
    -- a user-defined polymorphic function takes the ordinary specialization
    -- path; it is here so the two paths are asserted side by side
    assert (myConst inc "x" 16 == 17) "user-defined"
    assert (applyC myConst inc "x" 17 == 18) "higher-order user-defined"
    -- the surplus arguments must not make the callee stricter: `const` never
    -- forces its second argument, split call or not
    assert (const inc undefined 18 == 19) "unused argument stays unforced"
    putStrLn "ok"
