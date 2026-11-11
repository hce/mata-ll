-- Pins Haskell 2010 §3.14's two `let` forms inside a do block: `let decls`
-- alone is the let statement, whose names scope over the rest of the
-- block, and `let decls in e` is an expression statement — the
-- let-expression — whatever position it holds (first, middle or last,
-- with `in` on the bindings' line or on a deeper line) and wherever the
-- do block lives (a top-level action, a where-bound one, a nested do,
-- a non-IO monad). The names of a `let … in e` statement are not in
-- scope after it (F8).

counter :: Int -> IO Int
counter n = do
  let m = n * 2 in print m
  let m = n + 1
  print m
  let r = m * 3
      s = r + 1
    in print (r, s)
  pure m

maybeChain :: Int -> Maybe Int
maybeChain n = do
  let a = n + 1 in Just a
  b <- Just (n * 2)
  let c = b + 1
  let d = c
   in pure (d + 1)

helper :: Int -> IO ()
helper n = go n
  where
    go k = do
      let twice = k * 2 in print twice
      let half = k `div` 2
      print half
      let label = "k=" <> show k in putStrLn label

main :: IO ()
main = do
  let x = 1 :: Int in print x
  print (2 :: Int)
  m <- counter 5
  print m
  print (maybeChain 3)
  helper 9
  let outer = 7 :: Int
  r <- do
    let inner = outer + 1 in pure (inner * 2)
  print r
  let x = 100 :: Int
  print x
  let y = x
   in print (y + 1)
