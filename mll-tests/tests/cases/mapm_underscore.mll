-- Result-discarding monadic traversals: mapM_ (Prelude), forM_ and
-- sequence_ (Control.Monad). Generic over any Monad — exercised both in IO
-- STATEMENT position (this is what regressed when they were first
-- generalized: the result-only `m ()` was not pinned by the do-context)
-- and in a pure monad (Maybe), where short-circuiting is observable.

import Control.Monad (forM_, sequence_)

safe :: Int -> Maybe Int
safe n = if n < 0 then Nothing else Just (n * 2)

main :: IO ()
main = do
  -- Bare statement position in an IO do-block.
  mapM_ putStrLn ["mapM_ a", "mapM_ b"]
  forM_ [1, 2, 3] (\n -> putStrLn ("forM_ " <> show n))
  sequence_ [putStrLn "sequence_ 1", putStrLn "sequence_ 2"]
  -- Pure monad (Maybe): success yields Just (), failure short-circuits.
  assert (mapM_ safe [1, 2, 3] == Just ()) "mapM_ in Maybe (ok)"
  assert (mapM_ safe [1, -5, 3] == Nothing) "mapM_ in Maybe (fail)"
  assert (sequence_ [Just 1, Just 2] == Just ()) "sequence_ in Maybe (ok)"
  assert (sequence_ [Just 1, Nothing] == Nothing) "sequence_ in Maybe (fail)"
