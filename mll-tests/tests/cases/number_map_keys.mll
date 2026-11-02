-- A Number map key reads back as the double it was stored as. Lua 5.3+
-- keeps an integral float table key as an integer, so a key enumerated
-- by keys/toList carried the integer subtype: `negate` of the key 0.0
-- lost the sign of zero and `k * k` wrapped at 2^63. (A `-0.0` key is the
-- separately pinned negative_zero_keys divergence and stays out of here.)
import qualified Data.Map as M

main :: IO ()
main = do
    let m = M.fromList [(0.0, "z"), (3037000500.0, "big"), (2.5, "half")] :: M.Map Number String
    print (M.keys m)
    print (map negate (M.keys m))
    print (map (\k -> 1.0 / negate k) (M.keys m))
    print (map (\k -> k * k) (M.keys m))
    print (M.toList m)
    let m2 = M.insert 4611686018427387904 () M.empty :: M.Map Number ()
    print (map (\(k, _) -> (k + k, k * k)) (M.toList m2))
    print (M.lookup 3037000500 m, M.member 0 m, M.size m)
    print (M.keys (M.delete 2.5 m))
    print (sum (M.keys m))
    print (M.foldrWithKey (\k _ acc -> k : acc) [] m)
    print (M.keys (M.insert 7 "seven" (M.delete 0 m)))
