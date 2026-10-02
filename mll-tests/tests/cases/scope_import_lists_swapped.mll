import ScopeLeft hiding (helper, pick)
import ScopeRight (helper, pick)

main :: IO ()
main = do
    assert (helper 5 == 500) "the listed module's helper"
    assert (pick "a" == "a!") "the listed module's pick"
    assert (leftF 5 == 6) "the other module still uses its own helper"
    print (helper 5, leftF 5)
    putStrLn (pick "a")
