module EntListA (T(..), U, mkU, unU, V(V1), mkV2, showV, C(..), R, rx, mkR, D, useD) where

data T = T1 Int | T2
    deriving Show

data U = MkU Int

data V = V1 Int | V2 Int
    deriving Show

data R = R { rx :: Int, ry :: Int }

class C a where
    cm :: a -> String

instance C Int where
    cm n = "C " <> show n

class D a where
    dm :: a -> Int

instance D Int where
    dm n = n * 100

mkU :: Int -> U
mkU = MkU

unU :: U -> Int
unU (MkU n) = n

mkV2 :: Int -> V
mkV2 = V2

showV :: V -> String
showV (V1 n) = "V1 " <> show n
showV (V2 n) = "V2 " <> show n

mkR :: Int -> Int -> R
mkR = R

useD :: Int -> Int
useD n = dm n
