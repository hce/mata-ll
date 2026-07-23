-- Middle module: imports only leafA from DiamondLeaf, so leafB is merged in
-- transitively but is not part of DiamondMid's interface.
module DiamondMid (midFn) where

import DiamondLeaf (leafA)

midFn :: Int -> Int
midFn x = leafA (leafA x)
