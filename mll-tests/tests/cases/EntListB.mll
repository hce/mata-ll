module EntListB (Color(..), paint, Shade(..)) where

data Color = Red | Green
    deriving Show

data Shade = Dark | Light
    deriving Show

paint :: Color -> String
paint c = "painted " <> show c
