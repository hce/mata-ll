-- A20: Data.Set, and the ordered enumeration Data.Map/Set gained from
-- A16+A17 — structural elements/keys included.
import qualified Data.Set as S
import qualified Data.Map as M

main :: IO ()
main = do
    let s = S.fromList [3, 1, 2, 1]
    assert (S.size s == 3) "set dedups"
    assert (S.member 2 s) "member"
    assert (not (S.member 9 s)) "not member"
    print (S.toList s)
    print (S.toList (S.union s (S.fromList [0, 2])))
    print (S.toList (S.intersection s (S.fromList [2, 3, 9])))
    print (S.toList (S.difference s (S.fromList [2])))
    print (S.toList (S.filter even s))
    -- structural elements: ascending Ord order out
    let t = S.fromList [(2, 1), (1, 9), (1, 2)]
    print (S.toList t)
    assert (S.member (1, 9) t) "tuple member"
    -- Data.Map additions
    let m = M.insertWith (+) 1 10 (M.fromList [(1, 5), (2, 7)])
    assert (M.findWithDefault 0 1 m == 15) "insertWith combines"
    assert (M.findWithDefault 0 9 m == 0) "findWithDefault default"
    print (M.toList (M.adjust (\v -> v * 2) 2 m))
    print (M.elems m)
    putStrLn "ok"

-- expect: [1,2,3]
-- expect: [0,1,2,3]
-- expect: [2,3]
-- expect: [1,3]
-- expect: [2]
-- expect: [(1,2),(1,9),(2,1)]
-- expect: [(1,15),(2,14)]
-- expect: [15,7]
-- expect: ok
