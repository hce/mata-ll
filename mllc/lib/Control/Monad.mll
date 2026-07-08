module Control.Monad
    ( mapM_, forM_, when, unless, guard
    , void, join
    , sequence_
    ) where

-- mapM_ and when are already in Prelude, re-exported here

forM_ :: [a] -> (a -> IO ()) -> IO ()
forM_ xs f = mapM_ f xs

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
