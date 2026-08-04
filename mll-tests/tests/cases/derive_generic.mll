-- deriving (Generic): the compiler synthesises the structural representation
-- `Rep T` (a D1-wrapped sum of C1-wrapped products of S1-wrapped K1 leaves),
-- the from/to conversions, and per-datatype/constructor/field metadata.
-- Covers: from/to round-trips (including the `to (from x)` Rep-unification
-- path), a user-written generic consumer resolved by monomorphization at each
-- concrete type (conIndex via GIx), and metadata reflection — datatypeName,
-- datatypeConCount, conName, conArity, conIsRecord, selName — including
-- `as`-renamed constructors and fields, whose EFFECTIVE names are reflected.
import Data.Generics
import JSON

data Colour = Red | Green | Blue
    deriving (Eq, Generic)

data Person = Person { name :: String, age :: Int }
    deriving (Eq, Generic)

data Shape = Circle { radius :: Number } | Rect Int Int | Pt
    deriving (Eq, Generic)

data WrapI = WrapI Int
    deriving (Eq, Generic)

-- `as` renames: conName/selName reflect the effective external name. A
-- rename requires a codec deriving (ToJSON here) to have something to apply
-- to; the Generic metadata then reflects the same effective names.
data Rn = RnA Int as "renamed" | RnB
    deriving (Eq, ToJSON, Generic)

data Ev = Tick { evAt as "at" :: Int } | Stop
    deriving (Eq, ToJSON, Generic)

-- ============================================================
-- A user generic consumer: constructor index, written once over
-- the representation combinators.
-- ============================================================

class GIx f where
    gix :: f -> Int

instance GIx U1 where
    gix _ = 0

instance GIx (K1 c) where
    gix _ = 0

instance (GIx a, GIx b) => GIx (a :+: b) where
    gix (L1 x) = gix x
    gix (R1 y) = 1 + gix y

instance (GIx a, GIx b) => GIx (a :*: b) where
    gix _ = 0

instance GIx f => GIx (D1 d f) where
    gix (D1 x) = gix x

instance GIx f => GIx (C1 c f) where
    gix (C1 x) = gix x

instance GIx f => GIx (S1 s f) where
    gix (S1 x) = gix x

conIndex :: (Generic a, GIx (Rep a)) => a -> Int
conIndex x = gix (from x)

-- ============================================================
-- Generic metadata readers: the active constructor's name/arity/
-- record-ness, and a constructor's field names, off the wrappers.
-- ============================================================

class GConMeta f where
    gconName :: f -> String
    gconArity :: f -> Int
    gconIsRecord :: f -> Bool

instance (GConMeta a, GConMeta b) => GConMeta (a :+: b) where
    gconName (L1 x) = gconName x
    gconName (R1 y) = gconName y
    gconArity (L1 x) = gconArity x
    gconArity (R1 y) = gconArity y
    gconIsRecord (L1 x) = gconIsRecord x
    gconIsRecord (R1 y) = gconIsRecord y

instance Constructor c => GConMeta (C1 c f) where
    gconName c1 = conName c1
    gconArity c1 = conArity c1
    gconIsRecord c1 = conIsRecord c1

instance GConMeta f => GConMeta (D1 d f) where
    gconName (D1 x) = gconName x
    gconArity (D1 x) = gconArity x
    gconIsRecord (D1 x) = gconIsRecord x

conNameOf :: (Generic a, GConMeta (Rep a)) => a -> String
conNameOf x = gconName (from x)

conArityOf :: (Generic a, GConMeta (Rep a)) => a -> Int
conArityOf x = gconArity (from x)

conIsRecordOf :: (Generic a, GConMeta (Rep a)) => a -> Bool
conIsRecordOf x = gconIsRecord (from x)

class GSelNames f where
    gselNames :: f -> [String]

instance GSelNames U1 where
    gselNames _ = []

instance Selector s => GSelNames (S1 s f) where
    gselNames s1 = selName s1 : []

instance (GSelNames a, GSelNames b) => GSelNames (a :*: b) where
    gselNames (Prod a b) = gselNames a ++ gselNames b

instance (GSelNames a, GSelNames b) => GSelNames (a :+: b) where
    gselNames (L1 x) = gselNames x
    gselNames (R1 y) = gselNames y

instance GSelNames f => GSelNames (D1 d f) where
    gselNames (D1 x) = gselNames x

instance GSelNames f => GSelNames (C1 c f) where
    gselNames (C1 x) = gselNames x

selNamesOf :: (Generic a, GSelNames (Rep a)) => a -> [String]
selNamesOf x = gselNames (from x)

-- ============================================================
-- The from/to round-trip, generically: exercises unifying the
-- fresh `Rep α` of to's argument with the `Rep a` from produces.
-- ============================================================

roundTrip :: Generic a => a -> a
roundTrip x = to (from x)

main :: IO ()
main = do
    -- round-trips through the representation
    assert (roundTrip Red == Red) "roundTrip nullary first"
    assert (roundTrip Blue == Blue) "roundTrip nullary last"
    assert (roundTrip (Person "Ann" 30) == Person "Ann" 30) "roundTrip record"
    assert (roundTrip (Circle 2.5) == Circle 2.5) "roundTrip sum record con"
    assert (roundTrip (Rect 3 4) == Rect 3 4) "roundTrip sum positional con"
    assert (roundTrip Pt == Pt) "roundTrip sum nullary con"
    assert (roundTrip (WrapI 7) == WrapI 7) "roundTrip single positional"
    assert (roundTrip (RnA 1) == RnA 1) "roundTrip renamed con"
    assert (roundTrip (Tick 9) == Tick 9) "roundTrip renamed field"

    -- a user generic function, resolved per concrete type
    assert (conIndex Red == 0) "conIndex first"
    assert (conIndex Green == 1) "conIndex middle"
    assert (conIndex Blue == 2) "conIndex last"
    assert (conIndex (Person "x" 1) == 0) "conIndex single-con"
    assert (conIndex (Circle 1.0) == 0) "conIndex sum first"
    assert (conIndex (Rect 1 2) == 1) "conIndex sum middle"
    assert (conIndex Pt == 2) "conIndex sum last"

    -- datatype metadata
    assert (datatypeName (from (Person "x" 1)) == "Person") "datatypeName"
    assert (datatypeName (from Red) == "Colour") "datatypeName enum"
    assert (datatypeConCount (from (Person "x" 1)) == 1) "conCount single"
    assert (datatypeConCount (from Red) == 3) "conCount enum"
    assert (datatypeConCount (from (Rect 1 2)) == 3) "conCount sum"

    -- constructor metadata
    assert (conNameOf (Person "x" 1) == "Person") "conName record"
    assert (conNameOf (Circle 1.0) == "Circle") "conName sum first"
    assert (conNameOf (Rect 1 2) == "Rect") "conName sum middle"
    assert (conNameOf Pt == "Pt") "conName sum nullary"
    assert (conArityOf (Person "x" 1) == 2) "conArity record"
    assert (conArityOf (Rect 1 2) == 2) "conArity positional"
    assert (conArityOf Pt == 0) "conArity nullary"
    assert (conIsRecordOf (Person "x" 1)) "conIsRecord record"
    assert (not (conIsRecordOf (Rect 1 2))) "conIsRecord positional"
    assert (not (conIsRecordOf Pt)) "conIsRecord nullary"

    -- `as` renames reflect the EFFECTIVE names
    assert (conNameOf (RnA 1) == "renamed") "conName as-rename"
    assert (conNameOf RnB == "RnB") "conName unrenamed sibling"
    assert (conNameOf (Tick 9) == "Tick") "conName with renamed field"

    -- selector metadata: field names in order; positional fields are ""
    assert (selNamesOf (Person "x" 1) == ["name", "age"]) "selNames record"
    assert (selNamesOf (Rect 1 2) == ["", ""]) "selNames positional"
    assert (selNamesOf Pt == []) "selNames nullary"
    assert (selNamesOf (Tick 9) == ["at"]) "selNames as-rename"

    putStrLn "derive_generic ok"
