-- Helper module for testing module export control
module ExportHelper (publicFn, PublicType(..)) where

data PublicType = PubA | PubB Integer
    deriving (Show, Eq)

data PrivateType = PrivX

publicFn :: Integer -> Integer
publicFn n = n + privateFn n

privateFn :: Integer -> Integer
privateFn n = n * 2
