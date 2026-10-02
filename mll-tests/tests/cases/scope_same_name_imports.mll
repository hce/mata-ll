import ScopeLeft
import ScopeRight

main :: IO ()
main = do
    assert (leftF 5 == 6) "ScopeLeft's helper inside ScopeLeft"
    assert (rightG 5 == 500) "ScopeRight's helper inside ScopeRight"
    print (leftF 5, rightG 5)
