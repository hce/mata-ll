module Control.Monad
    ( mapM, mapM_, forM, forM_, when, unless, guard
    , void, join
    , sequence, sequence_
    ) where

-- mapM, mapM_, sequence, when are already in Prelude, re-exported here

-- forM_ = flip mapM_: result-discarding traversal with args swapped, so the
-- action can be written as a trailing lambda block.
forM_ :: (Foldable t, Monad m) => t a -> (a -> m b) -> m ()
forM_ xs f = mapM_ f xs

-- forM = flip mapM: result-collecting traversal with args swapped, so the
-- action can be written as a trailing lambda block.
forM :: (Traversable t, Monad m) => t a -> (a -> m b) -> m (t b)
forM xs f = mapM f xs

unless :: Applicative f => Bool -> f () -> f ()
unless cond action = if cond then pure () else action

-- DEVIATION (see HASKDIFF.md "Control.Monad is narrower than GHC's"):
-- GHC's guard is `Alternative f => Bool -> f ()`; mata-ll has no
-- Alternative class, so guard is fixed at the list instance. For the
-- Maybe equivalent write `if c then Just () else Nothing`.
guard :: Bool -> [()]
guard True = [()]
guard False = []

-- GHC's void is `Functor f => f a -> f ()`; mata-ll's is Monad-constrained
-- (slightly narrower — see HASKDIFF.md), polymorphic over any Monad.
void :: Monad m => m a -> m ()
void action = action >> pure ()

join :: Monad m => m (m a) -> m a
join x = x >>= \inner -> inner

sequence_ :: (Foldable t, Monad m) => t (m a) -> m ()
sequence_ t = foldr (\x k -> x >> k) (pure ()) t
