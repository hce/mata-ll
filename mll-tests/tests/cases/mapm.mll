-- Result-collecting monadic traversal: mapM / sequence (Prelude) and forM
-- (Control.Monad). Generic over any Monad; exercised here in IO and Maybe.

import Control.Monad (forM)

safe :: Int -> Maybe Int
safe n = if n < 0 then Nothing else Just (n * 2)

main :: IO ()
main = do
  -- mapM in IO collects results in order.
  rs <- mapM (\x -> return (x * x)) [1, 2, 3, 4]
  assert (rs == [1, 4, 9, 16]) "mapM in IO"
  -- sequence in IO.
  ys <- sequence [return 1, return 2, return 3]
  assert (ys == [1, 2, 3]) "sequence in IO"
  -- forM is flip mapM.
  zs <- forM [10, 20] (\x -> return (x + 1))
  assert (zs == [11, 21]) "forM in IO"
  -- Pure monad (Maybe): success collects, failure short-circuits.
  assert (mapM safe [1, 2, 3] == Just [2, 4, 6]) "mapM in Maybe (ok)"
  assert (mapM safe [1, -5, 3] == Nothing) "mapM in Maybe (fail)"
  assert (sequence [Just 1, Just 2, Just 3] == Just [1, 2, 3]) "sequence in Maybe (ok)"
  assert (sequence [Just 1, Nothing, Just 3] == Nothing) "sequence in Maybe (fail)"
