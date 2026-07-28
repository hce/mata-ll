-- IO self-loop conversion (opt pass 6): a mixed function — a pure
-- terminal branch, a tail forward to ANOTHER IO function, and a
-- self-looping branch with a per-step effect. All three behaviors and
-- the effect ORDER must survive the conversion (the forward must leave
-- the loop as a proper Lua tail call, not become an iteration).

finish :: Int -> IO ()
finish n = putStrLn ("finish " <> show n)

steps :: Int -> IO ()
steps 0 = pure ()
steps 1 = finish 99
steps n = do
    putStrLn ("step " <> show n)
    steps (n - 1)

main :: IO ()
main = do
    steps 3
    steps 0
    putStrLn "done"
