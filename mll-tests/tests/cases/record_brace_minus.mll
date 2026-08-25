-- A record construction/update ends in a brace: a `-` after `}` is
-- SUBTRACTION, not a negative-literal argument (F8 — the record value was
-- applied to -1, a bogus "Cannot unify ... a -> b").
module Main where

data V = V { vx :: Int }

upd :: V -> Int
upd r = vx r { vx = 3 } - 1

main :: IO ()
main = print (upd (V { vx = 9 }))

-- expect: 2
