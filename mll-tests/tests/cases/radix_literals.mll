-- lua-compat-skip: luajit
-- Radix literals (Haskell 2010 hex/octal, GHC2021 binary) and
-- NumericUnderscores separators.  Regression: the lexer knew only
-- decimal, so `0xFF` lexed as the APPLICATION `0 xFF` and surfaced as
-- a baffling type error; `1_000_000` split at the underscore.  A
-- wrong-base digit (`0o18`) is a loud lexer error now, pinned in
-- compile_errors.rs.  Skipped on LuaJIT only for the maxBound probe
-- (an Int past 2^53 is a double there); the Integer probes are exact
-- bignums on every target.

main :: IO ()
main = do
    print (0xFF :: Int)
    print (0xff + 0x01 :: Int)
    print (0o17 :: Int)
    print (0b1011 :: Int)
    print (1_000_000 :: Int)
    print (0xFF_FF :: Int)
    print (1_000.5 :: Number)
    print (2.5e1_0 :: Number)
    -- maxBound :: Int, spelled in hex
    print (0x7FFFFFFFFFFFFFFF :: Int)
    -- past maxBound: an Integer literal, re-based to decimal exactly
    print 0x10000000000000000
    print 0b10000000000000000000000000000000000000000000000000000000000000000
    -- underscores stay legal at the Integer size too
    print 36_893_488_147_419_103_232
