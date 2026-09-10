-- Helper module for the module re-export tests: a plain module (header
-- but no export list, so everything is exported) whose names travel
-- through the Reex* re-exporters.
module ReexInner where

innerFn :: Int -> Int
innerFn x = x + 1

innerTag :: String
innerTag = "inner"

data InnerT = MkInnerT Int
    deriving (Show, Eq)
