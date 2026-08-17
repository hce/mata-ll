-- Test: a small function whose body is `f . g` or `f $ x` keeps the
-- laziness of the real `.`/`$` emitters when the CALL-SITE INLINER
-- substitutes its arguments. The inliner's `.` arm built `f(g(_x))`
-- directly — no suspension of `g _x` for a non-strict `f` — so
-- `(compose ignore add1) (error "boom")` forced the bottom that `ignore`
-- never demands, where `(ignore . add1) (error "boom")` written out (and
-- GHC) return 0.

ignore :: Int -> Int
ignore _ = 0

add1 :: Int -> Int
add1 x = x + 1

compose :: (Int -> Int) -> (Int -> Int) -> Int -> Int
compose f g = f . g

apply :: (Int -> Int) -> Int -> Int
apply f x = f $ x

twice :: (Int -> Int) -> Int -> Int
twice f = f . f

-- A cheap body that is a lambda whose OWN body is a `case`: the inliner
-- substitutes the parameter through the lambda, so the case must see it.
mkPick :: Int -> (Bool -> Int)
mkPick x = \b -> case b of
    True -> x
    False -> 0

mkLet :: Int -> (Int -> Int)
mkLet x = \y -> let z = x + y in z * 2

main :: IO ()
main = do
    assert ((ignore . add1) (error "written out") == 0) "written-out composition is non-strict"
    assert ((compose ignore add1) (error "boom") == 0) "inlined compose, partial then applied"
    assert (compose ignore add1 (error "boom2") == 0) "inlined compose, saturated"
    assert (apply ignore (error "boom3") == 0) "inlined $"
    assert (compose add1 add1 40 == 42) "inlined compose computes"
    assert (twice add1 40 == 42) "inlined self-composition computes"
    assert (twice ignore (error "boom4") == 0) "inlined self-composition is non-strict"
    -- The two-argument (unsaturated) call is the one the inliner substitutes:
    -- it yields the composed function as a value.
    let h = compose ignore add1
    assert (h (error "boom5") == 0) "inlined compose as a value is non-strict"
    let k = compose add1 add1
    assert (k 40 == 42) "inlined compose as a value computes"
    let t = twice ignore
    assert (t (error "boom6") == 0) "inlined self-composition as a value is non-strict"
    let p = mkPick 7
    assert (p True == 7 && p False == 0) "substituted parameter reaches a case inside the inlined lambda"
    let l = mkLet 20
    assert (l 1 == 42) "substituted parameter reaches a let inside the inlined lambda"
