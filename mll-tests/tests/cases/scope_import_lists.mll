import ScopeLeft (helper, pick)
import ScopeRight hiding (helper, pick)

main :: IO ()
main = do
    assert (helper 5 == 6) "the listed module's helper"
    assert (pick 1 == 8) "the listed module's pick"
    assert (rightG 5 == 500) "the other module still uses its own helper"
    print (helper 5, pick 1, rightG 5)
