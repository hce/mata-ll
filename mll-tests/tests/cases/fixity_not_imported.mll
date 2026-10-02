import FixityPrivate
import FixityListed (listedTotal)
import FixityHidden hiding ((<+>), plus)

(<+>) :: Int -> Int -> Int
a <+> b = a - b

plus :: Int -> Int -> Int
plus a b = a - b

main :: IO ()
main = do
    assert (privateTotal == 17) "the private operator keeps its fixity at home"
    assert (listedTotal == 34) "so does one the import list leaves out"
    assert (hiddenTotal == 57) "and one the import hides"
    assert (10 <+> 3 <+> 2 == 5) "an operator that is not imported lends no fixity to the importer's own"
    assert (10 <+> 3 * 2 == 14) "the importer's own operator is infixl 9"
    assert (10 `plus` 3 `plus` 2 == 5) "the same for a backtick operator"
    assert (10 `plus` 3 * 2 == 14) "which is infixl 9 as well"
    print (privateTotal, listedTotal, hiddenTotal)
    print (10 <+> 3 <+> 2, 10 <+> 3 * 2)
    print (10 `plus` 3 `plus` 2, 10 `plus` 3 * 2)
