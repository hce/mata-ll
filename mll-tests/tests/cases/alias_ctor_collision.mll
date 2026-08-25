-- expect: 1
-- expect: 2
-- An import alias that is ALSO a local data constructor (Q67): `tag M`
-- parses to the same shape as a qualified reference `M.tag`, so the alias
-- rewrite used to collapse it and fail with "Unbound variable: M.tag".
-- The constructor meaning must win (the compiler warns that qualified
-- references through this alias will not resolve — see HASKDIFF).
import qualified AliasCtor as M

data Mode = M | N

tag :: Mode -> Int
tag M = 1
tag N = 2

main :: IO ()
main = do
  print (tag M)
  print (tag N)
