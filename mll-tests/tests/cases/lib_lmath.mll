import LMath

-- approximate equality for floating point
approx :: Number -> Number -> Bool
approx a b = abs (a - b) < 0.0001

main :: IO ()
main = do
    -- constants
    assert (approx pi 3.14159265) "pi approx"
    assert (huge > 999999999999.0) "huge is large"

    -- abs is the Prelude's Num method (LMath once shadowed it with a
    -- Number-only binding, breaking `abs` at Int for every importer).
    assert (abs 5.0 == 5.0) "abs positive"
    assert (abs (-3 :: Int) == 3) "abs at Int after import LMath"
    assert (abs (-3.0) == 3.0) "abs negative"
    assert (abs 0.0 == 0.0) "abs zero"

    -- ceil / floor
    assert (ceil 2.3 == 3) "ceil 2.3"
    assert (ceil 2.0 == 2) "ceil exact"
    assert (ceil (-1.5) == -1) "ceil negative"
    assert (floor 2.7 == 2) "floor 2.7"
    assert (floor 2.0 == 2) "floor exact"
    assert (floor (-1.5) == -2) "floor negative"

    -- sqrt
    assert (approx (sqrt 4.0) 2.0) "sqrt 4"
    assert (approx (sqrt 9.0) 3.0) "sqrt 9"
    assert (approx (sqrt 2.0) 1.41421356) "sqrt 2"
    assert (sqrt 0.0 == 0.0) "sqrt 0"

    -- fmod
    assert (approx (fmod 10.0 3.0) 1.0) "fmod 10 3"
    assert (approx (fmod 7.5 2.5) 0.0) "fmod 7.5 2.5"

    -- trig: sin/cos at known values
    assert (approx (sin 0.0) 0.0) "sin 0"
    assert (approx (sin (pi / 2.0)) 1.0) "sin pi/2"
    assert (approx (cos 0.0) 1.0) "cos 0"
    assert (approx (cos pi) (-1.0)) "cos pi"

    -- sin^2 + cos^2 = 1
    let x = 1.234
    assert (approx (sin x * sin x + cos x * cos x) 1.0) "sin^2+cos^2=1"

    -- exp / log inverse
    assert (approx (log (exp 1.0)) 1.0) "log(exp(1))"
    assert (approx (exp (log 5.0)) 5.0) "exp(log(5))"
    assert (approx (exp 0.0) 1.0) "exp 0 = 1"

    -- atan2
    assert (approx (atan2 1.0 1.0) (pi / 4.0)) "atan2 1 1 = pi/4"
    assert (approx (atan2 0.0 1.0) 0.0) "atan2 0 1 = 0"

    -- frexp goes through the portable shim (__mll_frexp): stock 5.4/5.5
    -- compile math.frexp out, so the direct binding was a nil call there.
    let (m, e) = frexp 8.0
    assert (approx m 0.5) "frexp 8: mantissa 0.5"
    assert (e == 4) "frexp 8: exponent 4"
    let (m2, e2) = frexp (0.0 - 0.75)
    assert (approx m2 (0.0 - 0.75)) "frexp -0.75: mantissa -0.75"
    assert (e2 == 0) "frexp -0.75: exponent 0"
    let (m0, e0) = frexp 0.0
    assert (m0 == 0.0 && e0 == 0) "frexp 0: (0, 0)"

    -- logBase takes GHC's argument order: logBase base x.
    assert (approx (logBase 2.0 8.0) 3.0) "logBase 2 8 = 3"
    assert (approx (logBase 10.0 1000.0) 3.0) "logBase 10 1000 = 3"
