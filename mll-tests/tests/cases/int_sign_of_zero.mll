-- An Int has no sign of zero (GHC's Int is an integer). On a doubles-only
-- host (LuaJIT) an Int is a double, and `negate 0`, `0 * (-5)`,
-- `0 `div` (-5)`, `ceiling (-0.5)` and `truncate (-0.5)` compute -0.0
-- (N9, M10): it printed `-0` through every show route (direct, structural,
-- derived, showsPrec, a Data.Map key, a range's first element) and
-- `show x == "0"` was False. The producers stay as they are; every
-- observer of the sign gives GHC's answer: the shows, `read . show`, and
-- the Int -> Number conversions (`1 / fromIntegral x` is Infinity, never
-- -Infinity). `==`, `compare`, `abs`, `signum`, `toInteger` and the
-- divisions are the controls. Lua 5.3+, whose Int is an integer, prints
-- the same lines.
import qualified Data.Map as M

data Box = forall a. (Num a, Show a) => Box a

viaDict :: Box -> String
viaDict (Box x) = show (negate 0 `asTypeOf` x)

data R = R { rx :: Int, ry :: Number } deriving (Show, Eq)

data P = P Int Int deriving (Show, Eq)

zero :: Int -> Int
zero x = x * 0

main :: IO ()
main = do
    let z = length ([] :: [Int])
        nz = negate z
        t = truncate (-0.5 :: Number) :: Int
        c = ceiling (-0.5 :: Number) :: Int
    print (nz, c, t, round (-0.4 :: Number) :: Int, z `div` (-5), z `quot` (-5), z `mod` (-5), z `rem` (-5))
    print (negate (zero 5), zero 5 * (-1), zero (-7), (-3) * z, z * (-3), 0 - z, nz + nz)
    print (show nz == "0", show c == "0", viaDict (Box (3 :: Int)))
    print [nz, 1, zero 7 * (-1), c, t]
    print (nz, "x")
    print (Just nz, Just (Just c), Left nz :: Either Int Bool)
    print (R { rx = nz, ry = fromIntegral nz }, P nz c)
    print (showsPrec 7 nz "", showsPrec 11 c "|", showsPrec 6 nz "")
    print (fromIntegral nz :: Number, 1 / fromIntegral t :: Number, 1 / fromIntegral nz :: Number, 1 / toEnum c :: Number)
    print (abs nz, signum nz, abs c, negate nz, nz == 0, compare nz 0, nz < 0, nz <= 0, max nz 0 == 0)
    print (minimum [nz, 0], maximum [0, c], min 0 nz, max c 0)
    print (show t <> "|" <> show nz, replicate nz "a", [nz .. 2], [c, 2 .. 6], take 3 [nz ..])
    print (toInteger nz, fromIntegral t :: Integer, fromIntegral nz :: Int, read (show nz) :: Int, read (show c) :: Int)
    print (M.toList (M.fromList [(nz, "a"), (1, "b")]), M.lookup 0 (M.fromList [(c, "a")]), M.keys (M.insert nz () (M.fromList [(2, ())])))
    print (even nz, odd c, nz ^ (2 :: Int), gcd nz 4, lcm nz 3)
    putStrLn (show nz)
