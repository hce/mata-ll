module Data.IORef
    ( newIORef, readIORef, writeIORef
    , modifyIORef, modifyIORef'
    ) where

-- The IORef operations are compiler intrinsics living in the auto-imported
-- Prelude namespace (like the STArray family); this module re-exports them
-- under GHC's module name so `import Data.IORef (...)` compiles unchanged
-- on both sides of the oracle.
--
-- Laziness is GHC's: writeIORef doesn't force the stored value, modifyIORef
-- stores the unevaluated `f old` (the classic space-leak shape — prefer
-- modifyIORef' for counters and accumulators), and modifyIORef' forces the
-- new value to WHNF before the store. `instance Eq (IORef a)` is pointer
-- identity. The atomic*/mkWeakIORef family is absent: mata-ll's host has
-- no preemptive threading and no weak references (see HASKDIFF).
