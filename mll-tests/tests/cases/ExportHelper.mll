-- Helper module for testing module export control
module ExportHelper (publicFn, PublicType(..)) where

data PublicType = PubA | PubB Int
    deriving (Show, Eq)

data PrivateType = PrivX

publicFn :: Int -> Int
publicFn n = n + privateFn n

privateFn :: Int -> Int
privateFn n = n * 2
