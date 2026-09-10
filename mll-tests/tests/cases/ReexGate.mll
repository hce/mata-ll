-- Helper module: an export list WITHOUT a re-export entry. Importers see
-- gateFn but not ReexInner's names (Haskell 2010 section 5.2: an export
-- list gates transitive names unless re-exported).
module ReexGate (gateFn) where

import ReexInner

gateFn :: Int -> Int
gateFn x = innerFn x
