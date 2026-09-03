-- Integer literal patterns are Num-polymorphic; the TIR pattern now
-- carries its checked type, and a pattern resolved to machine Int
-- compares with native `==` instead of the type-directed __mll_lit_eq.
-- Pins: Int (incl. negative literals and nested positions over lazy
-- fields, which must compare the FORCED value), Integer (stays
-- type-directed — big magnitudes must still match beyond double
-- precision), Double matched by integer literals, and defaulted
-- (signature-free) literals.
module Main where

intMatch :: Int -> String
intMatch 0 = "zero"
intMatch (-3) = "minus three"
intMatch 7 = "seven"
intMatch _ = "other"

integerMatch :: Integer -> Int
integerMatch 12345678901234567890123 = 1
integerMatch (-12345678901234567890123) = 2
integerMatch 5 = 3
integerMatch _ = 4

doubleMatch :: Number -> Int
doubleMatch 1 = 10
doubleMatch 2.5 = 20
doubleMatch _ = 0

nested :: (Int, Int) -> String
nested (0, 1) = "zero one"
nested (0, _) = "zero something"
nested _ = "no"

consHead :: [Int] -> String
consHead (0 : _) = "starts zero"
consHead (_ : _) = "starts other"
consHead [] = "empty"

justLit :: Maybe Int -> String
justLit (Just 5) = "five"
justLit (Just _) = "some"
justLit Nothing = "none"

main :: IO ()
main = do
    mapM_ (putStrLn . intMatch) [0, -3, 7, 12]
    print (map integerMatch [12345678901234567890123, -12345678901234567890123, 5, 6])
    print (map doubleMatch [1, 2.5, 3.25])
    putStrLn (defaulted 3)
    putStrLn (defaulted 4)
    -- Lazy tuple fields: the literal condition must force before it
    -- compares (a thunk in the field would otherwise mismatch).
    let p = (2 + (-2), 3 - 2)
    putStrLn (nested p)
    putStrLn (nested (0, 9))
    putStrLn (consHead (map (+ 0) [0, 4]))
    putStrLn (consHead [9])
    putStrLn (justLit (Just (2 + 3)))
    putStrLn (justLit (Just 1))
    putStrLn (justLit Nothing)
  where
    -- No signature: the literal pattern defaults (Integer), so it must
    -- keep the type-directed compare.
    defaulted 3 = "three"
    defaulted _ = "not three"
