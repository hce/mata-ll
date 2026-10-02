module ScopeCfgB where

data Config = CB String

showB :: Config -> String
showB (CB s) = "B" <> s

defaultB :: Config
defaultB = CB "x"
