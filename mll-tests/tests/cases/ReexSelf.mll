-- Helper module: the self re-export idiom. `module ReexSelf` in its own
-- export list exports every own declaration.
module ReexSelf (module ReexSelf) where

selfA :: Int -> Int
selfA x = x + 10

selfB :: Int -> Int
selfB x = selfA x + 1
