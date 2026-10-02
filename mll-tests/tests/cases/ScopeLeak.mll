module ScopeLeak where

import ScopeInner

viaLeak :: Int -> Int
viaLeak x = twice x
