-- Direct-perform IO self-recursion, GHC-parity pin for the
-- pure-suspension terminal: `machine 1 undefined` reaches `pure acc`
-- through one recursion step, and GHC binds the bottom UNFORCED — `r` is
-- never used, so nothing raises. Originally the emitted direct-perform
-- shape re-applied the forwarding runner once per pinned frame on the way
-- out, and the re-application forced the payload thunk: this program
-- raised `Prelude.undefined` where GHC prints `ok` (confirmed against
-- runghc 9.14.1). The retired performloop pass first fixed it by looping
-- the shape; today the saturated self tail is a bare `return self(…)` that
-- forwards the callee's result unchanged — no runner re-application exists
-- to force anything (see the one-root-application contract at the
-- runtime's runners) — and GHC's behavior holds for every direct-perform
-- tail, self or not.

machine :: Int -> Int -> IO Int
machine n acc =
  case n of
    0 -> pure acc
    1 -> machine 0 acc
    _ -> machine (n - 1) (acc + n)

main :: IO ()
main = do
  r <- machine 1 undefined
  putStrLn "ok"
