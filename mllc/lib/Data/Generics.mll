module Data.Generics
    ( Generic(from, to)
    , Rep
    , U1(..)
    , K1(..)
    , (:+:)(..)
    , (:*:)(..)
    , D1(..)
    , C1(..)
    , S1(..)
    , Datatype(datatypeName, datatypeConCount)
    , Constructor(conName, conArity, conIsRecord)
    , Selector(selName)
    , gProxy
    , pD1, pC1, pS1, pK1
    , pSumL, pSumR
    , pProdL, pProdR
    ) where

-- Datatype-generic programming: a structural representation for types that
-- `deriving (Generic)`. The representation is a sum of products — a D1
-- datatype wrapper around a sum (:+:) of constructors, each a C1 wrapper
-- around a product (:*:) of fields, each field an S1 wrapper around a K1
-- holding the value; U1 is the empty product of a nullary constructor.
-- Deviations from GHC.Generics are documented in HASKDIFF.md: three distinct
-- meta wrappers D1/C1/S1 instead of one tagged M1, no K1 index, combinators
-- of kind Type (no phantom parameter on from/to), no V1.

infixr 5 :+:
infixr 6 :*:

-- The representation family. Closed and compiler-populated: each
-- `deriving (Generic)` adds one equation `Rep T = <rep of T>`; no equations
-- are written here.
type family Rep a where

-- Conversion between a value and its representation. Instances come only
-- from `deriving (Generic)`.
class Generic a where
    from :: a -> Rep a
    to :: Rep a -> a

-- A nullary constructor's (empty) product of fields.
data U1 = U1

-- A field leaf: holds one field's value.
data K1 c = K1 c

-- Choice between constructors, right-nested: the first constructor is L1,
-- each following one is behind another R1.
data (:+:) a b = L1 a | R1 b

-- A product of two or more fields, right-nested.
data (:*:) a b = Prod a b

-- Metadata wrappers. The first parameter is a phantom marker type the
-- compiler synthesises per datatype / constructor / field; the
-- Datatype/Constructor/Selector instances are keyed on it, which is how one
-- generic instance per wrapper reflects per-name metadata under head-keyed
-- instance dispatch.
data D1 d f = D1 f
data C1 c f = C1 f
data S1 s f = S1 f

-- Datatype metadata, reflected off the D1 wrapper.
class Datatype d where
    datatypeName :: D1 d f -> String
    datatypeConCount :: D1 d f -> Int

-- Constructor metadata, reflected off the C1 wrapper. conName is the
-- constructor's effective external name (the `as "…"` rename when present).
class Constructor c where
    conName :: C1 c f -> String
    conArity :: C1 c f -> Int
    conIsRecord :: C1 c f -> Bool

-- Field metadata, reflected off the S1 wrapper. A positional field's
-- selName is "".
class Selector s where
    selName :: S1 s f -> String

-- ================================================================
-- Proxies
--
-- A generic CONSUMER walks a rep value it already has. A generic PRODUCER
-- (a decoder, a parser) must pick instances before any value exists; it
-- navigates the representation with proxy values instead. gProxy is a
-- bottom that is never forced — the metadata methods ignore their argument,
-- and every instance is chosen from the proxy's TYPE — and the p* helpers
-- re-type a proxy for each layer of the representation without ever
-- matching on it (matching would force the bottom).
-- ================================================================

gProxy :: a
gProxy = error "Data.Generics: a metadata proxy value was forced"

pD1 :: D1 d f -> f
pD1 _ = gProxy

pC1 :: C1 c f -> f
pC1 _ = gProxy

pS1 :: S1 s f -> f
pS1 _ = gProxy

pK1 :: K1 c -> c
pK1 _ = gProxy

pSumL :: (a :+: b) -> a
pSumL _ = gProxy

pSumR :: (a :+: b) -> b
pSumR _ = gProxy

pProdL :: (a :*: b) -> a
pProdL _ = gProxy

pProdR :: (a :*: b) -> b
pProdR _ = gProxy
