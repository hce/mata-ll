-- A call site hidden inside a record update's field expression (Q62).
-- `pick` is also called visibly with a cheap literal; if the hidden site is
-- invisible to call-site analysis, `y` is judged always-cheap, the callee
-- skips forcing it, and the thunked `fib 15` argument is read as a raw
-- thunk table (arithmetic on a table value). The multi-clause definition
-- keeps `pick` out of both the fold splice and the inliner, and `y` is
-- demanded on only one clause, so the argument takes the lazy protocol.
data Rec = Rec { count :: Int, label :: String }

fib :: Int -> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

pick :: Int -> Bool -> Int
pick y True = y + 1
pick y False = 0

bump :: Rec -> Rec
bump r = r { count = pick (fib 15) True }

main :: IO ()
main = do
  print (pick 1 True)
  print (count (bump (Rec 0 "x")))
