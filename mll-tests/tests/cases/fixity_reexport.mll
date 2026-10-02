import FixityUmbrella
import FixityByName

main :: IO ()
main = do
    assert (10 -. 3 -. 2 == 9) "a re-exported operator arrives with its defining module's fixity"
    assert (byName == 9) "the re-exporting module sees it too"
    assert (2 * 10 -. 3 == 17) "infixr 6 binds looser than multiplication"
    assert ((1 ~=~ 1) == True) "the module re-export passes every operator on"
    print (10 -. 3 -. 2, byName, 2 * 10 -. 3, 1 ~=~ 1)
