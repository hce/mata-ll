import qualified ScopeMid as Q
import qualified ScopeLeft as L
import qualified ScopeRight as R

main :: IO ()
main = do
    assert (Q.helper 1 == 2) "the aliased module's own helper"
    assert (Q.viaInner 1 == 1001) "the helper of its import stays behind it"
    assert (L.helper 1 == 2) "one alias names one module"
    assert (R.helper 1 == 100) "the other alias names the other"
    assert (L.pick 1 == 8) "one name at one type"
    assert (R.pick "a" == "a!") "the same name at another type"
    print (Q.helper 1, Q.viaInner 1, L.helper 1, R.helper 1, L.pick 1)
    putStrLn (R.pick "a")
