-- IO self-loop conversion (opt pass 6): GHC parity of build-time
-- dispatch. `countdown` matches strictly on its argument, so FORCING the
-- action value `countdown undefined` — before anything runs it — must
-- raise, exactly as GHC's `seq` on the same expression does (the case
-- analysis sits outside the IO closure). The conversion must keep that
-- dispatch at action-build time; deferring it into the returned closure
-- would move the raise from build to run and print "no raise".

countdown :: Int -> IO ()
countdown 0 = pure ()
countdown n = do
  putStrLn ("tick " <> show n)
  countdown (n - 1)

main :: IO ()
main = do
  r <- try (seq (countdown undefined) (pure ()))
  case r of
    Left _  -> putStrLn "raised at build"
    Right _ -> putStrLn "no raise"
  countdown 2
