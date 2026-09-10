-- Helper module: re-export through a qualified alias. `module RI` names
-- the import aliased RI; the unqualified import puts its entities in
-- scope, so the entry passes them on (Haskell 2010 section 5.2).
module ReexAlias (module RI) where

import ReexInner
import qualified ReexInner as RI
