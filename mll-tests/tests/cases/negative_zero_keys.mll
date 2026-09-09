-- -0.0 as a map key. GHC's Ord/Eq on Double equate -0.0 and 0.0
-- (compare (-0.0) 0.0 is EQ), so Data.Map and Data.Set treat them as ONE
-- key — in scalar position and inside a structural key alike. The
-- structural-key encoder used to spell them differently ("n-0" vs "n0"),
-- splitting one GHC key in two; it now encodes by numeric value.
--
-- One measured residual (pinned in divergent/, see DIVERGENCES.md): GHC's
-- insert stores the NEW key's spelling, so a map whose last write at zero
-- was -0.0 shows the key as -0.0; mata-ll's scalar store indexes the Lua
-- table with the key itself, and Lua normalizes -0.0, so the key shows as
-- 0.0 (structural keys store the written key and match GHC).
import qualified Data.Map as M
import qualified Data.Set as S

main :: IO ()
main = do
    let nz = -0.0 :: Number
    -- scalar keys
    let m1 = M.fromList [(0.0 :: Number, "a"), (nz, "b")]
    print (M.size m1)
    print (M.lookup 0.0 m1)
    print (M.lookup nz m1)
    print (M.toList m1)
    print (M.toList (M.fromList [(nz, "a"), (0.0 :: Number, "b")]))
    let m2 = M.insert nz "c" m1
    print (M.size m2)
    print (M.lookup 0.0 m2)
    -- structural (tuple) keys, both insertion orders
    let s1 = M.fromList [((0.0 :: Number, 1 :: Int), "a"), ((nz, 1), "b")]
    print (M.size s1)
    print (M.lookup (0.0, 1) s1)
    print (M.lookup (nz, 1) s1)
    print (M.toList s1)
    let s2 = M.fromList [((nz, 1 :: Int), "a"), ((0.0 :: Number, 1), "b")]
    print (M.size s2)
    print (M.lookup (nz, 1) s2)
    print (M.member (0.0, 1) s2)
    print (M.size (M.delete (0.0, 1) s2))
    print (M.toList s2)
    -- list and Maybe keys
    let l1 = M.fromList [([0.0 :: Number], True), ([nz], False)]
    print (M.size l1)
    print (M.lookup [0.0] l1)
    let j1 = M.fromList [(Just (0.0 :: Number), 1 :: Int), (Just nz, 2)]
    print (M.size j1)
    print (M.lookup (Just 0.0) j1)
    -- sets
    let t1 = S.fromList [0.0 :: Number, nz]
    print (S.size t1)
    print (S.member 0.0 t1)
    print (S.member nz t1)
    let t2 = S.fromList [(0.0 :: Number, 1 :: Int), (nz, 1)]
    print (S.size t2)
    print (S.member (nz, 1) t2)
