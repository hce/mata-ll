-- C4: a NaN key. Data.Map's `compare nan _ = GT` and `nan == nan = False`
-- make a NaN key one that lands (the insert grows the map) and that no
-- lookup, member, delete or later insert ever matches; the key still
-- shows as NaN in keys/toList. mata-ll's HashMap backs Data.Map with a
-- Lua table, which refuses NaN as an index — the runtime boxes each NaN
-- key under a fresh sentinel instead.
--
-- Only shapes where GHC's answer does not depend on tree history are
-- probed: one NaN key per map when ordinary keys are looked up
-- afterwards (a second NaN can rotate above an ordinary key and hide it
-- from GHC's own lookups), and every NaN inserted after the ordinary
-- keys (a NaN compares GT both ways, so it enumerates last).
import qualified Data.Map as M
import qualified Data.Set as S

main :: IO ()
main = do
    let nan = 0.0 / 0.0 :: Number
    let m1 = M.fromList [(1.5, 2 :: Int)]
    print (M.lookup nan m1)
    print (M.member nan m1)
    let m2 = M.insert nan 1 m1
    print (M.size m2)
    print (M.lookup nan m2)
    print (M.member nan m2)
    print (M.lookup 1.5 m2)
    print (M.keys m2)
    print (M.toList m2)
    print (M.size (M.delete nan m2))
    print (M.toList (M.delete 1.5 m2))
    -- a second NaN insert adds a second entry
    let m3 = M.insert nan 3 m2
    print (M.size m3)
    print (M.toList m3)
    print (M.size (M.delete nan m3))
    -- fromList with NaN keys after the ordinary ones
    let m4 = M.fromList [(2.5, 1 :: Int), (nan, 2), (nan, 3)]
    print (M.size m4)
    print (M.toList m4)
    -- a structural key carrying a NaN
    let s0 = M.insert (1.5, 1 :: Int) (8 :: Int) M.empty
    let s = M.insert (nan, 1 :: Int) 7 s0
    print (M.size s)
    print (M.lookup (nan, 1 :: Int) s)
    print (M.member (nan, 1 :: Int) s)
    print (M.lookup (1.5, 1 :: Int) s)
    print (M.toList s)
    print (M.size (M.delete (nan, 1 :: Int) s))
    -- Data.Set
    let t = S.insert nan (S.fromList [1.5])
    print (S.size t)
    print (S.member nan t)
    print (S.member 1.5 t)
    print (S.toList t)
    print (S.size (S.delete nan t))
    print (S.toList (S.insert nan t))
    assert (M.size m3 == 3) "two NaN inserts are two entries"
    putStrLn "ok"
