module DiamondEast where

import DiamondShared

east :: Item -> Int
east i = unwrap i + 2
