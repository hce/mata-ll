module EntFieldA (mkP, pname, rename, Person(..), Opts(..), defaults) where

import Prelude hiding (error, either)

data P = P { name :: String, age :: Int }
    deriving Show

data Person = Person { fullName :: String, tag :: Int }
    deriving (Show, Eq)

data Opts = Opts { error :: Int, either :: Int }
    deriving Show

mkP :: String -> P
mkP s = P { name = s, age = 1 }

pname :: P -> String
pname p = name p

rename :: String -> P -> P
rename s p = p { name = s, age = age p + 1 }

defaults :: Opts
defaults = Opts { error = 1, either = 2 }
