-- An FFI-produced Number can carry Lua's INTEGER subtype (tonumber "2"
-- returns integer 2 on Lua 5.3+, and math-library results re-integerize)
-- while the same value written in source is the float 2.0. They are ==,
-- one value of one type, so a map must treat them as ONE key: the
-- structural encoder now encodes by numeric value (an integral float
-- encodes as the equal integer), where it used to split them ("i2" vs
-- "n2"). The scalar store always got this right from Lua's own key
-- normalization; both paths are probed. Not GHC-twinnable (the subtype
-- does not exist there), so this case self-asserts.
import qualified Data.Map as M

main :: IO ()
main = do
    let xi = ffi_tonumber_float "2"     -- integer-subtype 2
    let xf = 2.0 :: Number              -- float 2.0
    -- structural (tuple) keys
    let s1 = M.fromList [((xi, 1 :: Int), "a"), ((xf, 1), "b")]
    print (M.size s1)
    print (M.lookup (xf, 1) s1)
    print (M.lookup (xi, 1) s1)
    let s2 = M.insert (xf, 1 :: Int) "c" (M.fromList [((xi, 1), "a")])
    print (M.size s2)
    print (M.lookup (xi, 1) s2)
    print (M.size (M.delete (xi, 1) s2))
    -- list and Maybe keys
    print (M.size (M.fromList [([xi], True), ([xf], False)]))
    print (M.size (M.fromList [(Just xi, 1 :: Int), (Just xf, 2)]))
    -- scalar keys (Lua's own normalization; must agree with the above)
    let m1 = M.fromList [(xi, "a"), (xf, "b")]
    print (M.size m1)
    print (M.lookup xf m1)
    print (M.lookup xi m1)

-- expect: 1
-- expect: Just "b"
-- expect: Just "b"
-- expect: 1
-- expect: Just "c"
-- expect: 0
-- expect: 1
-- expect: 1
-- expect: 1
-- expect: Just "b"
-- expect: Just "b"
