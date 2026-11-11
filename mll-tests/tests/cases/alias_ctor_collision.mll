-- An import alias that is ALSO a local data constructor (Q67, F10): `tag M`
-- is the application of `tag` to the constructor and `M.size` the qualified
-- reference, both at once — GHC keeps module aliases in their own namespace,
-- and a qualified name is one lexeme (Haskell 2010 section 2.4), so the two
-- spellings never meet. The alias rewrite used to collapse `tag M` into
-- `M.tag` ("Unbound variable: M.tag"), and the constructor meaning then won
-- at the price of the qualified form (a documented deviation, now gone).
import qualified AliasCtor as M

data Mode = M | N

tag :: Mode -> Int
tag M = 1
tag N = 2

main :: IO ()
main = do
  print (tag M)
  print (tag N)
  print M.size
  print (tag M + M.size)
