-- Direct-perform IO self-loop conversion (opt pass 7), GHC-parity pin for
-- the pure-suspension terminal: `machine 1 undefined` reaches `pure acc`
-- through one recursion step, and GHC binds the bottom UNFORCED — `r` is
-- never used, so nothing raises. Before the conversion the emitted
-- direct-perform shape re-applied the forwarding runner once per pinned
-- frame on the way out, and the re-application forced the payload thunk:
-- this program raised `Prelude.undefined` where GHC prints `ok`
-- (confirmed against runghc 9.14.1). The converted loop applies each
-- genuine runner application exactly once and keeps the payload's
-- protection closure verbatim, restoring GHC's behavior. (The unconverted
-- shapes that still decline the pass keep the old behavior — recorded as
-- an open item in doc/articles/TODO.md.)

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
