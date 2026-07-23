export foo :: Int
foo = 123

export bar :: Int -> Int
bar = (+ foo)

export run :: IO ()
run = main

main :: IO ()
main = print $ bar 123
