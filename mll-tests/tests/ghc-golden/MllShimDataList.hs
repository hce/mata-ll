{-# LANGUAGE GHC2021 #-}

-- Companion to MllShim for test cases that `import Data.List` bare:
-- mata-ll's Data.List exports a few names GHC's does not. The golden
-- generator adds `import MllShimDataList` next to a rewritten bare
-- `import Data.List` so those names resolve.
module MllShimDataList
  ( append
  ) where

-- mata-ll's Data.List append.
append :: [a] -> [a] -> [a]
append = (++)
