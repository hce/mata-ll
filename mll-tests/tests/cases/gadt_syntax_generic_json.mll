-- expect: True
-- expect: True
-- expect: 2
-- expect: [1,2]
-- VANILLA GADT-syntax constructors (`MkW :: Int -> Int -> W` — no result
-- refinement, no existentials) are ordinary constructors in different
-- clothing (Q74): Generic, the JSON codecs, and LuaDict must accept them
-- through the same registry-backed vanilla check the six structural
-- derives use (gadt_syntax_derives.mll pins those) — the old parser-level
-- `gadt_type.is_some()` gate rejected them outright, and conArity
-- metadata read the parser's field list (empty for GADT syntax) instead
-- of the registry's.
import Data.Generics
import JSON

data W where
  MkW :: Int -> Int -> W
  deriving (Eq, Generic, ToJSON)

data E where
  Aa :: E
  Bb :: E
  deriving (Eq, Generic)

roundTrip :: Generic a => a -> a
roundTrip x = to (from x)

-- Minimal generic arity reflection (the full traversal lives in
-- derive_generic.mll): pins that conArity comes from the REGISTRY.
class GArity f where
  gArity :: f -> Int

instance (GArity a, GArity b) => GArity (a :+: b) where
  gArity (L1 x) = gArity x
  gArity (R1 y) = gArity y

instance Constructor c => GArity (C1 c f) where
  gArity c1 = conArity c1

instance GArity f => GArity (D1 d f) where
  gArity (D1 x) = gArity x

arityOf :: (Generic a, GArity (Rep a)) => a -> Int
arityOf x = gArity (from x)

main :: IO ()
main = do
  print (roundTrip (MkW 1 2) == MkW 1 2)
  print (roundTrip Bb == Bb)
  print (arityOf (MkW 1 2))
  putStrLn (encodeToJSON (MkW 1 2))
