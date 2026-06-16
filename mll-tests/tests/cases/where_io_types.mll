-- Test: where-clause functions with IO types should have resolved types
-- (regression: fresh type variables were not unified with inferred types)

run :: Integer -> IO ()
run n = printNum n
    where printNum x = putStrLn (show x)

compute :: Integer -> Integer
compute n = helper n
    where helper x = x * x + 1

greet :: String -> IO ()
greet name = sayHello name
    where sayHello s = putStrLn ("Hello, " <> s <> "!")

main :: IO ()
main = do
    run 42
    greet "world"
    putStrLn (show (compute 5))
-- expect: 42
-- expect: Hello, world!
-- expect: 26
