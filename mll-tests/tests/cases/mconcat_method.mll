-- mconcat is a Monoid class method with GHC's default
-- (foldr mappend mempty), exactly as in base. The String instance
-- overrides it with the linear table.concat builder
-- (string_mconcat_prim), so this pins that the override is
-- byte-identical to GHC: flat and mapped spines, the empty list at
-- both instances, and the list instance still on the class default.

module Main where

halves :: [String] -> String
halves xs = mconcat xs <> "|" <> mconcat (map (<> ".") xs)

main :: IO ()
main = do
    putStrLn (mconcat ["con", "cat", "enation"])
    putStrLn (mconcat ([] :: [String]))
    putStrLn (halves ["a", "bc", "def"])
    print (mconcat [[1, 2], [], [3, 4, 5 :: Int]])
    print (mconcat ([] :: [[Int]]))
    putStrLn (mconcat (map show [1 .. 20 :: Int]))

-- expect: concatenation
-- expect:
-- expect: abcdef|a.bc.def.
-- expect: [1,2,3,4,5]
-- expect: []
-- expect: 1234567891011121314151617181920
