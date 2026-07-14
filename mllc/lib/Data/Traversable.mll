module Data.Traversable
    ( traverse, sequenceA
    , mapM, sequence
    , fmap
    ) where

-- traverse is a Traversable class method and sequenceA is defined over
-- it in the auto-imported Prelude; mapM/sequence are the Prelude's
-- list-specific monadic traversals (GHC generalizes them to any
-- Traversable; see HASKDIFF.md). This module only re-exports, so
-- GHC-style `import Data.Traversable (...)` selections work.
