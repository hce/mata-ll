export foo :: Integer
foo = 123

export bar :: Integer -> Integer
bar = (+ foo)

export run :: IO ()
run = main

main :: IO ()
main = print $ bar 123
