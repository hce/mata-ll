-- Direct-perform IO self-loop conversion (opt pass 7), dispatch shape: a
-- single-clause body whose `case` compiles to a dispatch IIFE under the
-- forwarding runner, with all three branch kinds the pass must handle —
-- a pure-suspension terminal (`pure acc`, the payload-protecting
-- closure), a bare self call at action position (the `1` branch), and an
-- effectful action closure whose tail recurses (the default branch). The
-- printed step trace pins the per-iteration effect order; the exact sum
-- pins the simultaneous update; the depth (bounded here) is covered by
-- performloop_deep.

machine :: Int -> Int -> IO Int
machine n acc =
  case n of
    0 -> pure acc
    1 -> machine 0 acc
    _ -> do
      putStrLn ("step " <> show n)
      machine (n - 1) (acc + n)

main :: IO ()
main = do
  r <- machine 5 0
  putStrLn (show r)
  assert (r == 14) "case-dispatch direct-perform loop"
