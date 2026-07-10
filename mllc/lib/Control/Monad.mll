module Control.Monad
    ( mapM, mapM_, forM, forM_, when, unless, guard
    , void, join
    , sequence, sequence_
    ) where

-- mapM, mapM_, sequence, when are already in Prelude, re-exported here

forM_ :: [a] -> (a -> IO ()) -> IO ()
forM_ xs f = mapM_ f xs

-- forM = flip mapM: result-collecting traversal with args swapped, so the
-- action can be written as a trailing lambda block.
forM :: Monad m => [a] -> (a -> m b) -> m [b]
forM xs f = mapM f xs

unless :: Applicative f => Bool -> f () -> f ()
unless cond action = if cond then pure () else action

guard :: Bool -> [()]
guard True = [()]
guard False = []

void :: IO a -> IO ()
void action = action >> pure ()

join :: Maybe (Maybe a) -> Maybe a
join Nothing = Nothing
join (Just x) = x

sequence_ :: [IO ()] -> IO ()
sequence_ [] = pure ()
sequence_ (x:xs) = x >> sequence_ xs
