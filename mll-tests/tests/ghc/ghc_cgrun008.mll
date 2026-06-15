-- GHC cgrun008: List comprehension with filtering

main :: IO ()
main = putStrLn (show (length comp_list))
  where
    given_list = [1, 2, 3, 4, 5, 6, 7, 8, 9]
    comp_list = [(elem1, elem2) | elem1 <- given_list, elem2 <- given_list, elem1 >= 4, elem2 < 3]
