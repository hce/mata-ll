-- Higher-order functions, closures, and let expressions

data Color = Red | Green | Blue

colorName :: Color -> String
colorName c = case c of
    Red   -> "red"
    Green -> "green"
    Blue  -> "blue"

applyTwice :: (Integer -> Integer) -> Integer -> Integer
applyTwice f x = f (f x)

main :: IO ()
main = do
    assert (colorName Red == "red") "colorName Red"
    assert (colorName Green == "green") "colorName Green"
    assert (colorName Blue == "blue") "colorName Blue"
    let inc = \x -> x + 1
    assert (applyTwice inc 5 == 7) "applyTwice inc 5"
    assert (applyTwice (\x -> x * 2) 3 == 12) "applyTwice double 3"
    let result = let a = 10
                     b = 20
                 in a + b
    assert (result == 30) "let binding"

    -- flip with partial application: param name 'f' must not resolve
    -- to the top-level function 'f' in the fn_table
    let f = flip take [1, 2, 3, 4, 5]
    assert (f 3 == [1, 2, 3]) "flip take partial app"
    assert (flip take [10, 20, 30] 2 == [10, 20]) "flip take inline"
