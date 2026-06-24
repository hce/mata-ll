-- Middle module: imports only leafA from DiamondLeaf, so leafB is merged in
-- transitively but is not part of DiamondMid's interface.
module DiamondMid (midFn) where

import DiamondLeaf (leafA)

midFn :: Integer -> Integer
midFn x = leafA (leafA x)
