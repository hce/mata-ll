module EntFieldB (Rec(..), total, bump, showRec) where

import Prelude hiding (either)

data Rec = Rec { either :: Int, other :: Int }
    deriving Show

total :: Rec -> Int
total r = either r + other r

bump :: Rec -> Rec
bump r = r { either = either r * 10 }

showRec :: Rec -> String
showRec r = show r
