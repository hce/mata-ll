module Main where

-- G4: the WIDE route into dictionary passing. `describe` is used at more
-- distinct concrete types than SPEC_LIMIT (16): the 17th specialization
-- trips the cap, every already-generated specialization is purged, call
-- sites already rewritten to purged names are reverted, and the whole
-- program routes through the dictionary form. Everything below must
-- stay byte-identical to GHC through that mid-compilation switch.

describe :: Show a => a -> String
describe x = "v=" <> show x

main :: IO ()
main = do
  putStrLn (describe (1 :: Int))
  putStrLn (describe True)
  putStrLn (describe "s")
  putStrLn (describe (Just (2 :: Int)))
  putStrLn (describe [3 :: Int])
  putStrLn (describe (4 :: Int, True))
  putStrLn (describe (Just True))
  putStrLn (describe [True, False])
  putStrLn (describe (Just [5 :: Int]))
  putStrLn (describe [[6 :: Int]])
  putStrLn (describe (Just (Just (7 :: Int))))
  putStrLn (describe (True, "t"))
  putStrLn (describe [Just (8 :: Int)])
  putStrLn (describe (Just "u"))
  putStrLn (describe ("v", [9 :: Int]))
  putStrLn (describe [(10 :: Int, False)])
  putStrLn (describe (Just (11 :: Int), [12 :: Int]))
  putStrLn (describe [[[13 :: Int]]])
  putStrLn (describe ((), Just ()))

-- expect: v=1
-- expect: v=True
-- expect: v="s"
-- expect: v=Just 2
-- expect: v=[3]
-- expect: v=(4,True)
-- expect: v=Just True
-- expect: v=[True,False]
-- expect: v=Just [5]
-- expect: v=[[6]]
-- expect: v=Just (Just 7)
-- expect: v=(True,"t")
-- expect: v=[Just 8]
-- expect: v=Just "u"
-- expect: v=("v",[9])
-- expect: v=[(10,False)]
-- expect: v=(Just 11,[12])
-- expect: v=[[[13]]]
-- expect: v=((),Just ())
