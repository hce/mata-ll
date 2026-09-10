-- Helper module: the umbrella idiom. `module ReexInner` passes the whole
-- imported module on to importers; outerPrivate stays private.
module ReexUmbrella (module ReexInner, outerFn) where

import ReexInner

outerFn :: Int -> Int
outerFn x = outerPrivate (innerFn x)

outerPrivate :: Int -> Int
outerPrivate x = x * 2
