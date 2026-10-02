import qualified FixityQual as Q

(<+>) :: Int -> Int -> Int
a <+> b = a - b

plus :: Int -> Int -> Int
plus a b = a - b

main :: IO ()
main = do
    assert (Q.qualTotal == 17) "the module's own uses group by its declaration"
    assert (10 <+> 3 <+> 2 == 5) "a qualified-only import lends no fixity to the bare spelling"
    assert (10 `plus` 3 `plus` 2 == 5) "nor to a bare backtick operator"
    assert (Q.plus 10 (Q.plus 3 2) == 24) "the qualified function is the imported one"
    print (Q.qualTotal, 10 <+> 3 <+> 2, 10 `plus` 3 `plus` 2, Q.plus 10 (Q.plus 3 2))
