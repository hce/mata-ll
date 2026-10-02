module ScopeCfgA where

data Config = CA Int

showA :: Config -> String
showA (CA n) = "A" <> show n

defaultA :: Config
defaultA = CA 1
