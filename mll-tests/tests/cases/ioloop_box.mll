-- IO self-loop conversion (opt pass 6): the one-pending-box convention.
-- A loop whose terminal is `pure <function>` returns the payload BOXED
-- (__mll_pure) so the binder receives the function as a VALUE; if the
-- conversion stripped (or double-consumed) the box, the runner would
-- mistake the bare function for an action closure and call it.

mkAdd :: Int -> IO (Int -> Int)
mkAdd 0 = pure (\x -> x + 100)
mkAdd n = do
    when (n == 1) (putStrLn "last build step")
    mkAdd (n - 1)

main :: IO ()
main = do
    f <- mkAdd 3
    assert (f 1 == 101) "boxed pure function delivered as a value"
    print (f 41)
