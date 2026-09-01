-- String building: render 20000 ints and join them with mconcat; the
-- twin collects parts into a table and joins once with table.concat —
-- the speed-of-light way to build a big string in Lua. The mconcat is
-- chunked (200 x 100) because its recursion depth is the element
-- count and LuaJIT's fixed C stack overflows near 20000 frames (PUC
-- Lua takes the flat version). Only the length is printed, so the
-- comparison is byte-count-exact without dumping 100KB to stdout.
module Main where

import LString (strLen)

chunk :: Int -> String
chunk c = mconcat (map (\i -> show i <> ",") [c * 100 + 1 .. c * 100 + 100])

main :: IO ()
main = print (strLen (mconcat (map chunk [0 .. 199])))
