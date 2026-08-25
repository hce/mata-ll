-- Ord Bool (GHC parity: False < True) — it was missing entirely, while
-- Eq Bool existed.  All seven Ord methods route to the new runtime
-- helpers.

main :: IO ()
main = do
    print (False < True)
    print (True < False)
    print (True <= True)
    print (compare True False)
    print (compare False False)
    print (compare False True)
    print (max False True)
    print (min False True)
