-- A Number is an IEEE double on every host. An integer-syntax literal at
-- Number denotes GHC's `fromInteger` at Double — the nearest double,
-- folded in double arithmetic, `-0` a negative zero — and on Lua 5.3+ it
-- never carries the integer subtype, whose arithmetic wraps at 2^63
-- (`fact 25` printed 7.034535277573964e18, `(2 :: Number) ^ 64` printed
-- 0.0, `-0 :: Number` printed 0.0 on every host). Pins the literal and
-- the folder, `read`, `fromIntegral`, the dictionary-routed literal of an
-- existential, a literal pattern, `^`, the empty `sum`/`product`, and the
-- sign of zero through `negate`, `abs`, `signum`, `min` and `/`.
data Box = forall a. (Num a, Show a, Eq a) => Box a

fact :: Number -> Number
fact n = if n <= 1 then 1 else n * fact (n - 1)

isBig :: Number -> Bool
isBig 9007199254740993 = True
isBig _ = False

viaDict :: Box -> String
viaDict (Box x) = show (x * 2 + 0, negate 0 `asTypeOf` x, x == 0)

main :: IO ()
main = do
    print (fact 25)
    print ((2 :: Number) ^ (64 :: Int))
    print (-0 :: Number)
    print ((9007199254740993 :: Number) == 9007199254740992)
    print (negate (0 :: Number), 1 / negate (0 :: Number))
    print ((4611686018427387904 :: Number) + 4611686018427387904)
    print ((3 :: Number) / 2, (7 :: Number) * 1000000000000)
    print (10000000000000000000000000000000 :: Number)
    print (fromIntegral (3037000500 :: Int) * fromIntegral (3037000500 :: Int) :: Number)
    print (fromIntegral (9007199254740993 :: Int) :: Number)
    print (sum [] :: Number, product [] :: Number, sum [1, 2] :: Number)
    print (read "5" :: Number, read "-0" :: Number, read "9007199254740993" :: Number)
    print (isBig 9007199254740992, isBig 1)
    print (viaDict (Box (3 :: Number)), viaDict (Box (3 :: Integer)))
    print (map (\x -> x * x) [3037000500 :: Number, -0])
    print (min 0 (-0 :: Number), abs (-0 :: Number), signum (-0 :: Number))
    print (toEnum 3 :: Number, fromEnum (2 :: Number))
    print ([1 .. 3] :: [Number])
    print (truncate (-2.5 :: Number) :: Int, round (2 :: Number) :: Int)
    print (show (0 :: Number), 0 == (-0 :: Number))
