-- Direct-perform IO self-recursion, dispatch shape: a single-clause body
-- whose `case` flattens to statement-level branches, with all three
-- terminal kinds — a pure-suspension terminal (`pure acc`, the
-- payload-protecting closure), a bare self call at action position (the
-- `1` branch), and an effectful branch whose tail recurses (the default
-- branch). Historically the shape was a dispatch IIFE under the forwarding
-- runner that the retired performloop pass had to unpick; today each
-- saturated self tail is a bare `return self(…)` (tailloop loops it). The
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
