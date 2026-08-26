module Main where

-- First-class sections of ($) and (.): the OpFunc fallback used to emit
-- the operator verbatim into Lua ("($) (+) 1 2" was a Lua syntax error;
-- "." is Lua index syntax). Both now build real function values that
-- follow the flat N-ary call protocol, with the operators' laziness:
-- ($) passes its argument raw, (.) suspends the inner application, and
-- first-class (&&)/(||) keep their short-circuit through Lua's and/or.

main :: IO ()
main = do
  print (($) (+) 1 2)
  print (($) ($) (+) 1 2)
  print (zipWith ($) [(+ 1), (* 2)] [10, 20])
  print (map ((.) (+ 1) (* 2)) [3])
  print (zipWith (&&) [True, False] [True, True])
  print (zipWith (||) [True, False] [False, False])
  -- Laziness: the argument of a first-class ($) reaches the callee
  -- unforced; a first-class composition never runs its inner function
  -- when the outer ignores its argument; short-circuit skips bombs.
  print (zipWith ($) [\_ -> (7 :: Int)] [undefined] !! 0)
  print (((.) (\_ -> (8 :: Int)) (\x -> undefined)) (1 :: Int))
  print (zipWith (&&) [False] [undefined])
  print (zipWith (||) [True] [undefined])

-- expect: 3
-- expect: 3
-- expect: [11,40]
-- expect: [7]
-- expect: [True,False]
-- expect: [True,False]
-- expect: 7
-- expect: 8
-- expect: [False]
-- expect: [True]
