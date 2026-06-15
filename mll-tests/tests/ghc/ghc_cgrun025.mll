-- GHC cgrun025: Let expressions and where clauses
-- Tests scoping of let and where bindings

hypotenuse :: Number -> Number -> Number
hypotenuse a b = sqrt (a * a + b * b)

circleArea :: Number -> Number
circleArea r = pi_ * r * r
  where
    pi_ = 3.14159265

main :: IO ()
main = do
    -- let in do block
    let x = 3
    let y = 4
    let result = x * x + y * y
    assert (result == 25) "let in do"

    -- where clause
    let hyp = hypotenuse 3.0 4.0
    assert (hyp == 5.0) "hypotenuse"

    -- let in do block with dependencies
    let a = 10
    let b = a + 5
    let c = a + b
    assert (c == 25) "let deps"

    -- Where with multiple bindings
    assert (circleArea 1.0 > 3.14) "circle > 3.14"
    assert (circleArea 1.0 < 3.15) "circle < 3.15"

    putStrLn "ok"
