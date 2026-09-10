-- Helper module: `module Q` re-exporting a QUALIFIED-ONLY import. H2010
-- defines a `module X` entry as the entities in scope both unqualified
-- and as X.e; a qualified-only import puts nothing in scope unqualified,
-- so the entry exports nothing (the compiler warns).
module ReexQual (module Q, qualFn) where

import qualified ReexInner as Q

qualFn :: Int -> Int
qualFn x = Q.innerFn x
