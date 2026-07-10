-- Result-collecting traversals used in RETURN position: the monad variable
-- is pinned only by the enclosing function's declared return type, not by a
-- `<-` bind, an `==` comparison, or an inline annotation. This was broken
-- until monomorphization learned to match `m a` against the sugared IO/List
-- types when computing the specialization substitution.

import Control.Monad (forM)

safe :: Integer -> Maybe Integer
safe n = if n < 0 then Nothing else Just (n * 2)

collectMaybe :: [Integer] -> Maybe [Integer]
collectMaybe xs = mapM safe xs

seqMaybe :: [Maybe Integer] -> Maybe [Integer]
seqMaybe xs = sequence xs

forMaybe :: [Integer] -> Maybe [Integer]
forMaybe xs = forM xs safe

collectIO :: [Integer] -> IO [Integer]
collectIO xs = mapM (\x -> return (x * x)) xs

seqIO :: IO [Integer]
seqIO = sequence [return 1, return 2, return 3]

main :: IO ()
main = do
  assert (collectMaybe [1, 2, 3] == Just [2, 4, 6]) "mapM return position (Maybe ok)"
  assert (collectMaybe [1, -5] == Nothing) "mapM return position (Maybe fail)"
  assert (seqMaybe [Just 1, Just 2] == Just [1, 2]) "sequence return position (Maybe ok)"
  assert (seqMaybe [Just 1, Nothing] == Nothing) "sequence return position (Maybe fail)"
  assert (forMaybe [10, 20] == Just [20, 40]) "forM return position (Maybe)"
  rs <- collectIO [1, 2, 3]
  assert (rs == [1, 4, 9]) "mapM return position (IO)"
  ys <- seqIO
  assert (ys == [1, 2, 3]) "sequence return position (IO)"
