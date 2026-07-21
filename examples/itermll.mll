-- This example demonstrates how mata-ll converts iterators into
-- lazy lists. You will see that while iterating through this
-- list, mata-ll will call into Lua repeatedly to fetch new values.
--
-- By convention, called Lua iterators must be pure. This one
-- isn't, for demonstration purposes. It will print each time
-- it yields a value. So the Lua print and mata-ll putStrLn
-- statements should interleave. If the list is strictly
-- evaluated, the statements would not interleave.

myIter              :: LuaIterator "my_iter" [Integer]
myIter'             :: LuaIterator "my_iter" [Integer]
runawayLoopIterator :: LuaIterator "runaway_loop_iterator" [Integer]

export run :: IO ()
run = flip mapM_ myIter $ \item ->
        putStrLn $ "Streaming from Lua to mata-ll, we got: " <> show item

export runStrict :: IO ()
runStrict = let myIter'' = length myIter' `seq` myIter' in
        flip mapM_ myIter'' $ \item ->
            putStrLn $ "Streaming from Lua to mata-ll, we got: " <> show item

export runPartly :: Integer -> IO ()
runPartly c = print $ take c runawayLoopIterator

main :: IO ()
main = putStrLn "You need to compile this example to Lua, than run iterdemo.lua"
